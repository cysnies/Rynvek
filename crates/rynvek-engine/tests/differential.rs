// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! CPU/GPU 差分测试：同一输入分别交给 CPU Reference 与 WGSL 内核，结果必须**逐位一致**（D0_Strict 0 bit 误差）。
//!
//! - 场：`rynvek-field` 五点拉普拉斯 + CFL 子步（含 D > 64 的多子步、衰减余数、源项、钳制）；
//! - 神经：`rynvek-neural::step_lif_brain_deterministic`（批量脑实例、四因子 STDP、Dale 定律）；
//! - 规则：`@rynvek/rules` 生成的 `forager_metabolism` 规则图，WGSL 后端 vs CPU 字节码解释器。

mod common;

use common::Rng;
use rynvek_core::fixed::Q8_8;
use rynvek_engine::gpu_field::GpuFieldMirror;
use rynvek_engine::gpu_neural::{pack_inputs, unpack_into, unpack_readout, upload_lif_batch};
use rynvek_engine::gpu_rules::GpuRuleKernel;
use rynvek_field::channels::{FieldChannelId, FieldChannelParams, STANDARD_FIELD_CHANNELS};
use rynvek_field::solver::{HALO, STRIDE, TILE_DIM};
use rynvek_field::world::FieldWorld;
use rynvek_neural::{LIF_INPUTS, LIF_OUTPUTS, LifBrain};
use rynvek_rules::cpu::RuleWorldState;
use rynvek_rules::{CompileOptions, compile_rule_graph_json, emit_cpu_simd_bytecode, execute_rule_tick};
use rynvek_wgpu::kernel::GpuDiagnostics;
use rynvek_wgpu::kernels::{FieldKernels, LifKernels};

fn interior(world: &FieldWorld, id: FieldChannelId) -> Vec<i16> {
    let buf = world.channel(id).current_buf();
    (0..TILE_DIM)
        .flat_map(|y| {
            let s = (y + HALO) * STRIDE + HALO;
            buf[s..s + TILE_DIM].to_vec()
        })
        .collect()
}

fn randomize_field(world: &mut FieldWorld, rng: &mut Rng) {
    for id in FieldChannelId::ALL {
        let p = world.channel(id).params;
        let (lo, hi) = (i32::from(p.min_val_q8).max(-4000), i32::from(p.max_val_q8).min(4000));
        for y in 0..TILE_DIM {
            for x in 0..TILE_DIM {
                // 稀疏大值 + 稠密小值，覆盖负数算术右移与钳制
                let v = if rng.next_u32() % 64 == 0 {
                    rng.range(lo, hi)
                } else {
                    rng.range(lo.max(-40), hi.min(40))
                };
                world.set_interior(id, x, y, v as i16);
            }
        }
        let tile = world.channel_mut(id);
        for s in tile.sources.iter_mut() {
            *s = if rng.next_u32() % 128 == 0 {
                rng.range(-200, 600) as i16
            } else {
                0
            };
        }
    }
}

fn run_field_differential(params: [FieldChannelParams; 5], seed: u32, ticks: u32) -> bool {
    let Some(gpu) = common::gpu() else { return false };
    let (device, queue) = (gpu.device(), gpu.queue());
    let diag = GpuDiagnostics::new(device);
    let kernels = FieldKernels::new(device).unwrap();
    let mut cpu = FieldWorld::with_params(params);
    randomize_field(&mut cpu, &mut Rng::new(seed));
    let mut mirror = GpuFieldMirror::new(device, &kernels, &cpu, &diag.buffer).unwrap();
    mirror.upload_from(queue, &cpu).unwrap();
    for tick in 1..=ticks {
        cpu.step_all().unwrap();
        let mut enc = device.create_command_encoder(&Default::default());
        mirror.encode_tick(&kernels, &mut enc, &diag.buffer);
        queue.submit([enc.finish()]);
        for id in FieldChannelId::ALL {
            let got = mirror.tile(id).download_interior_blocking(device, queue).unwrap();
            let want = interior(&cpu, id);
            if got != want {
                let i = got.iter().zip(&want).position(|(a, b)| a != b).unwrap();
                panic!(
                    "tick {tick} 通道 {}：首个差异位于 ({}, {}) GPU={} CPU={}",
                    id.as_str(),
                    i % TILE_DIM,
                    i / TILE_DIM,
                    got[i],
                    want[i]
                );
            }
        }
    }
    assert_eq!(diag.read_blocking(device, queue).unwrap()[0], 0, "INV-01");
    assert!(gpu.take_uncaptured_errors().is_empty());
    true
}

#[test]
fn field_standard_channels_gpu_matches_cpu_bit_exact() {
    if run_field_differential(STANDARD_FIELD_CHANNELS, 1, 12) {
        eprintln!("[差分] 五通道标准参数 12 Tick 逐位一致");
    }
}

#[test]
fn field_multi_substep_and_clamp_gpu_matches_cpu_bit_exact() {
    let mut params = STANDARD_FIELD_CHANNELS;
    // D=129 → 3 子步、衰减 7 → [2, 2, 3]；D=200 λ=255 → 4 子步；窄钳制区间
    params[0].diffusion_q8 = 129;
    params[0].decay_q8 = 7;
    params[1].diffusion_q8 = 200;
    params[1].decay_q8 = 255;
    params[2].min_val_q8 = -100;
    params[2].max_val_q8 = 300;
    params[3].diffusion_q8 = 0;
    params[4].diffusion_q8 = 64;
    params[4].decay_q8 = 256;
    if run_field_differential(params, 2, 8) {
        eprintln!("[差分] 多子步 / 衰减余数 / 钳制 8 Tick 逐位一致");
    }
}

#[test]
fn lif_batch_gpu_matches_cpu_bit_exact() {
    let Some(gpu) = common::gpu() else { return };
    let (device, queue) = (gpu.device(), gpu.queue());
    let diag = GpuDiagnostics::new(device);
    let kernels = LifKernels::new(device).unwrap();
    let mut rng = Rng::new(3);
    let (brain_count, n_hid) = (16usize, 24usize);
    let mut cpu: Vec<LifBrain> = (0..brain_count)
        .map(|_| {
            let mut w: Vec<i16> = (0..n_hid * (LIF_INPUTS + LIF_OUTPUTS))
                .map(|_| rng.range(-700, 900) as i16)
                .collect();
            w[0] = i16::MIN; // 极值权重
            w[1] = i16::MAX;
            LifBrain::new(n_hid, w).unwrap()
        })
        .collect();
    let batch = upload_lif_batch(device, &kernels, &cpu, true, &diag.buffer).unwrap();
    let mut gpu_view = cpu.clone();
    let mut cpu_energy = vec![rynvek_neural::metabolism::EnergyAccount::default(); brain_count];
    let mut gpu_energy = cpu_energy.clone();
    for step in 0..40 {
        let sensory: Vec<[Q8_8; LIF_INPUTS]> = (0..brain_count)
            .map(|_| std::array::from_fn(|_| Q8_8(rng.range(-512, 512) as i16)))
            .collect();
        let modulators: Vec<[i16; 4]> = (0..brain_count)
            .map(|_| std::array::from_fn(|_| rng.range(-256, 256) as i16))
            .collect();
        let want: Vec<[Q8_8; LIF_OUTPUTS]> = cpu
            .iter_mut()
            .enumerate()
            .map(|(b, brain)| brain.step(&sensory[b], modulators[b], true))
            .collect();
        let (s, m) = pack_inputs(&sensory, &modulators);
        batch.upload_inputs(queue, &s, &m).unwrap();
        let mut enc = device.create_command_encoder(&Default::default());
        batch.encode_step(&kernels, &mut enc);
        queue.submit([enc.finish()]);
        let got = unpack_readout(&batch.download_readout_blocking(device, queue).unwrap());
        assert_eq!(got, want, "step {step} 读出不一致");
        unpack_into(&mut gpu_view, &batch.download_state_blocking(device, queue).unwrap());
        assert_eq!(gpu_view, cpu, "step {step} 神经状态不一致");
        for i in 0..brain_count {
            // 以真实下载的 GPU 发放环独立计量；两档冻结科技成本都必须逐位一致。
            let cost = if step < 20 { 800 } else { 480 };
            cpu_energy[i].receive_field_raw(1);
            gpu_energy[i].receive_field_raw(1);
            let a = cpu[i].spike_ring.iter().filter(|r| **r & 1 != 0).count() as u32;
            let b = gpu_view[i].spike_ring.iter().filter(|r| **r & 1 != 0).count() as u32;
            assert_eq!(
                cpu_energy[i].charge_spikes(a, cost),
                gpu_energy[i].charge_spikes(b, cost)
            );
            assert!(cpu_energy[i].is_conserved());
        }
        assert_eq!(cpu_energy, gpu_energy, "step {step} 实际发放的精确能量账本不一致");
    }
    assert!(
        cpu.iter().any(|b| b.weights_delta_q8.iter().any(|&d| d != 0)),
        "可塑性必须实际生效"
    );
    assert!(cpu.iter().any(|b| b.spike_ring.iter().any(|&r| r != 0)), "必须出现发放");
    assert_eq!(diag.read_blocking(device, queue).unwrap()[0], 0, "INV-01");
}

const FORAGER: &str = include_str!("../../../tests/rules/forager_metabolism.graph.json");

#[test]
fn forager_rule_wgsl_backend_matches_cpu_interpreter() {
    let Some(gpu) = common::gpu() else { return };
    let (device, queue) = (gpu.device(), gpu.queue());
    let diag = GpuDiagnostics::new(device);
    let module = compile_rule_graph_json(FORAGER, &CompileOptions::default()).unwrap();
    let kernel = GpuRuleKernel::new(device, &module).unwrap();
    let mut rng = Rng::new(4);
    let n = 1000usize;
    let mut offsets = vec![0u32];
    let mut indices = Vec::new();
    for _ in 0..n {
        for _ in 0..rng.range(0, 12) {
            indices.push(rng.range(0, n as i32 - 1) as u32);
        }
        offsets.push(indices.len() as u32);
    }
    let components = module
        .components
        .iter()
        .map(|_| (0..n).map(|_| i64::from(rng.range(-2000, 26000))).collect())
        .collect();
    let world = RuleWorldState {
        tick: 7,
        world_seed: 99,
        entity_serials: (0..n as u64).map(|i| i * 7 + 1).collect(),
        components,
        field_samples: (0..5 * n).map(|_| i64::from(rng.range(-300, 1200))).collect(),
        neighbor_offsets: offsets,
        neighbor_indices: indices,
    };
    let cpu = execute_rule_tick(&emit_cpu_simd_bytecode(&module), &world, u64::MAX).unwrap();
    let got = kernel.run_blocking(device, queue, &diag.buffer, &world).unwrap();
    assert_eq!(got.components, cpu.components, "组件输出不一致");
    assert_eq!(got.field_deposits, cpu.field_deposits, "场沉积不一致");
    assert!(
        cpu.components.iter().zip(&world.components).any(|(a, b)| a != b),
        "规则必须产生写入"
    );
    assert!(gpu.take_uncaptured_errors().is_empty());
}
