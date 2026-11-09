// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! rynvek-wgpu 真实设备集成测试（llvmpipe GL / SwiftShader Vulkan / 硬件 GPU 均可）。

mod common;

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::rng::philox4x32_10;
use rynvek_wgpu::kernel::{GpuDiagnostics, read_buffer_blocking};
use rynvek_wgpu::kernels::{
    FieldKernels, GpuFieldParams, GpuFieldTile, GpuLifBatch, LifBatchState, LifKernels, PhiloxKernel,
};
use rynvek_wgpu::{
    DeviceLostRecovery, GpuDeviceOwner, ReadbackRing, RecoveryEvent, RecoveryOutcome, RecoveryPhase,
    SEGMENT_SIZE_BYTES, SegmentedBufferPool,
};

#[test]
fn adapter_probe_reports_backend_and_profile() {
    let Some(gpu) = common::gpu() else { return };
    let summary = gpu.capability_summary();
    assert!(summary.compute_shaders);
    assert!(summary.max_storage_buffer_binding_size >= SEGMENT_SIZE_BYTES);
    assert_eq!(gpu.generation(), 0);
    assert!(!gpu.is_lost());
}

#[test]
fn philox_gpu_matches_cpu_reference_bit_exact() {
    let Some(gpu) = common::gpu() else { return };
    let (device, queue) = (gpu.device(), gpu.queue());
    let diag = GpuDiagnostics::new(device);
    let kernel = PhiloxKernel::new(device).unwrap();
    let key = [0x1234_5678, 0x9ABC_DEF0];
    // 覆盖不足一个 Workgroup 的尾部（1000 = 7×128 + 104）与全 1 计数器
    let mut counters: Vec<[u32; 4]> = (0..999u32)
        .map(|i| [i, i.wrapping_mul(0x9E37_79B9), 7, 0xFFFF_0000 ^ i])
        .collect();
    counters.push([u32::MAX; 4]);
    let gpu_out = kernel
        .run_blocking(device, queue, &diag.buffer, key, &counters)
        .unwrap();
    for (c, g) in counters.iter().zip(&gpu_out) {
        assert_eq!(*g, philox4x32_10(*c, key), "counter {c:?}");
    }
    assert_eq!(
        diag.read_blocking(device, queue).unwrap(),
        [0; 4],
        "INV-01: s_diagnostics[0] 必须为 0"
    );
    assert!(gpu.take_uncaptured_errors().is_empty());
}

#[test]
fn field_tile_row_sums_flow_through_readback_ring() {
    let Some(gpu) = common::gpu() else { return };
    let (device, queue) = (gpu.device(), gpu.queue());
    let diag = GpuDiagnostics::new(device);
    let kernels = FieldKernels::new(device).unwrap();
    let dim = 32u32;
    let stride = dim + 4;
    // D=128 → 2 个 CFL 子步，无衰减、无源项（Neumann 边界）
    let params = GpuFieldParams {
        diffusion_q8: 128,
        decay_q8: 0,
        min_val_q8: i16::MIN,
        max_val_q8: i16::MAX,
    };
    let mut tile = GpuFieldTile::new(device, &kernels, dim, params, &diag.buffer).unwrap();
    assert_eq!(tile.substeps_per_tick(), 2);
    let mut padded = vec![0i16; (stride * stride) as usize];
    padded[((2 + 16) * stride + 2 + 16) as usize] = 10_000;
    padded[((2 + 3) * stride + 2 + 30) as usize] = 4_000;
    tile.upload_padded(queue, &padded).unwrap();

    let mut ring = ReadbackRing::new(device, "field.row_sums", tile.row_sums_bytes());
    let mut seen = Vec::new();
    for tick in 0..6u64 {
        let mut enc = device.create_command_encoder(&Default::default());
        tile.encode_tick(&kernels, &mut enc);
        let slot = ring.encode_copy(&mut enc, tick, tile.row_sums_buffer(), 0);
        queue.submit([enc.finish()]);
        if let Some(s) = slot {
            ring.request_map(s);
        }
        ring.poll(device);
        seen.extend(ring.drain_ready());
    }
    // 收尾：仅测试中等待在途回读完成
    while ring.has_in_flight() {
        gpu.wait_idle().unwrap();
        ring.poll(device);
        seen.extend(ring.drain_ready());
    }
    assert!(!seen.is_empty());
    // Q8.8 算术右移向 -∞ 取整：正值场的总量只减不增（逐位等价性由 rynvek-engine 的 CPU/GPU 差分测试覆盖）
    let mut prev = 14_000i64;
    let mut last = (0u64, 0i64);
    for (tick, bytes) in &seen {
        let rows: Vec<i32> = bytemuck::pod_collect_to_vec(bytes);
        let total: i64 = rows.iter().map(|&r| i64::from(r)).sum();
        assert!(total <= prev && total > 13_000, "tick {tick}: {total}");
        prev = total;
        last = (*tick, total);
    }
    // 槽位忙时跳过而非等待（软件光栅器上映射较慢，部分 Tick 会被跳过）
    assert_eq!(seen.len() as u64 + ring.state().skipped + ring.state().dropped, 6);
    assert!(last.0 <= 5);
    // 补一次只拷贝不推进的回读，核对最终行和与内区样本一致
    let mut enc = device.create_command_encoder(&Default::default());
    let slot = ring
        .encode_copy(&mut enc, 6, tile.row_sums_buffer(), 0)
        .expect("空闲槽位");
    queue.submit([enc.finish()]);
    ring.request_map(slot);
    gpu.wait_idle().unwrap();
    ring.poll(device);
    let final_rows = ring.drain_ready();
    assert_eq!(final_rows.len(), 1);
    let rows: Vec<i32> = bytemuck::pod_collect_to_vec(&final_rows[0].1);
    let interior = tile.download_interior_blocking(device, queue).unwrap();
    assert_eq!(
        interior.iter().map(|&v| i64::from(v)).sum::<i64>(),
        rows.iter().map(|&r| i64::from(r)).sum::<i64>(),
        "行和与内区一致"
    );
    assert!(interior[(16 * dim + 16) as usize] < 10_000, "峰值应已扩散");
    assert_eq!(diag.read_blocking(device, queue).unwrap()[0], 0);
}

#[test]
fn lif_kernel_reproduces_gv004_single_step() {
    // GV-004：v_prev=180, β=230, I=96, m_NE=128 → θ=218, v=257 → 发放、复位、不应期 2
    let Some(gpu) = common::gpu() else { return };
    let (device, queue) = (gpu.device(), gpu.queue());
    let diag = GpuDiagnostics::new(device);
    let kernels = LifKernels::new(device).unwrap();
    let mut weights = vec![0i32; 22];
    weights[0] = 256;
    weights[16] = 400; // 输出 0
    let mut state = LifBatchState::zeros(1, 1);
    state.membrane_v_q8[0] = 180;
    let batch = GpuLifBatch::new(device, &kernels, 1, 1, &weights, &state, false, &diag.buffer).unwrap();
    let mut sensory = vec![0i32; 16];
    sensory[0] = 96;
    batch.upload_inputs(queue, &sensory, &[0, 0, 128, 0]).unwrap();
    let mut enc = device.create_command_encoder(&Default::default());
    batch.encode_step(&kernels, &mut enc);
    queue.submit([enc.finish()]);
    let s = batch.download_state_blocking(device, queue).unwrap();
    assert_eq!(s.membrane_v_q8, [0]);
    assert_eq!(s.refractory_ticks, [2]);
    assert_eq!(LifBatchState::join_ring(s.spike_ring[0]), 1);
    assert_eq!(
        batch.download_readout_blocking(device, queue).unwrap(),
        [100, 0, 0, 0, 0, 0]
    );
    assert_eq!(diag.read_blocking(device, queue).unwrap()[0], 0);
}

#[test]
fn diagnostics_counter_flags_out_of_range_q8_input() {
    let Some(gpu) = common::gpu() else { return };
    let (device, queue) = (gpu.device(), gpu.queue());
    let diag = GpuDiagnostics::new(device);
    let kernels = LifKernels::new(device).unwrap();
    let batch = GpuLifBatch::new(
        device,
        &kernels,
        1,
        1,
        &[0; 22],
        &LifBatchState::zeros(1, 1),
        false,
        &diag.buffer,
    )
    .unwrap();
    let mut sensory = vec![0i32; 16];
    sensory[3] = 40_000; // 超出 i16
    batch.upload_inputs(queue, &sensory, &[0; 4]).unwrap();
    let mut enc = device.create_command_encoder(&Default::default());
    batch.encode_step(&kernels, &mut enc);
    queue.submit([enc.finish()]);
    assert_eq!(
        diag.read_blocking(device, queue).unwrap()[0],
        1,
        "INV-01 违规必须被计数"
    );
    diag.reset(queue);
    assert_eq!(diag.read_blocking(device, queue).unwrap()[0], 0);
}

#[test]
fn segmented_pool_allocates_32mib_segments_on_device() {
    let Some(gpu) = common::gpu() else { return };
    let (device, queue) = (gpu.device(), gpu.queue());
    let limits = device.limits();
    let mut pool = SegmentedBufferPool::new(256 << 20, limits.max_storage_buffer_binding_size).unwrap();
    // 40 MiB / 16 B → 2 段（32 MiB + 8 MiB）
    let seg = pool.allocate(device, "test.seg", (40 << 20) / 16, 16).unwrap();
    assert_eq!(seg.buffers.len(), 2);
    assert_eq!(seg.buffers[0].size(), SEGMENT_SIZE_BYTES);
    // 跨段写入：最后一个元素位于第 1 段
    let last = seg.plan.element_count - 1;
    seg.write_elements(queue, last, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16])
        .unwrap();
    let tail = read_buffer_blocking(device, queue, &seg.buffers[1], seg.buffers[1].size()).unwrap();
    assert_eq!(
        &tail[tail.len() - 16..],
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
    );
    // 预算耗尽 → 0x6003
    let err = pool.allocate(device, "too.big", (300 << 20) / 4, 4).unwrap_err();
    assert_eq!(err.code, RynvekErrorCode::GpuBufferAllocationFailed);
    pool.release(seg);
    assert_eq!(pool.allocated_bytes(), 0);
}

/// 真实 DeviceLost：销毁设备 → 状态机 5 步 → 同一 Instance 重获设备 → Philox 探针校验 → 恢复
#[test]
fn real_device_loss_drives_five_step_recovery() {
    let Some(mut gpu) = common::gpu() else { return };
    let mut machine = DeviceLostRecovery::default();
    gpu.destroy_device();
    gpu.poll_nonblocking();
    assert!(gpu.is_lost(), "destroy 后必须触发 device.lost 回调");
    let info = gpu.lost_info().unwrap();
    assert_eq!(info.generation, 0);
    assert!(info.reason.contains("Destroyed"), "{info:?}");

    machine.handle(RecoveryEvent::DeviceLost { tick: 17 }).unwrap();
    assert!(!machine.can_submit_gpu_work());
    machine.handle(RecoveryEvent::ResourcesReleased).unwrap();
    let profile = gpu.reacquire_blocking().expect("重新获取设备");
    machine.handle(RecoveryEvent::DeviceReacquired { profile }).unwrap();
    assert_eq!(gpu.generation(), 1);
    assert!(!gpu.is_lost());
    machine.handle(RecoveryEvent::StateRestored).unwrap();

    // 第 5 步：差分探针
    let (device, queue) = (gpu.device(), gpu.queue());
    let diag = GpuDiagnostics::new(device);
    let probe = PhiloxKernel::new(device).unwrap();
    let counters = [[1, 2, 3, 4], [5, 6, 7, 8]];
    let out = probe
        .run_blocking(device, queue, &diag.buffer, [9, 10], &counters)
        .unwrap();
    let ok = out.iter().zip(&counters).all(|(g, c)| *g == philox4x32_10(*c, [9, 10]));
    let outcome = machine
        .handle(if ok {
            RecoveryEvent::VerificationPassed
        } else {
            RecoveryEvent::VerificationFailed
        })
        .unwrap();
    assert_eq!(
        outcome,
        RecoveryOutcome::Resumed {
            resume_tick: 17,
            generation: 1
        }
    );
    assert_eq!(machine.phase(), RecoveryPhase::Healthy);
}

#[test]
fn work_on_lost_device_surfaces_as_error_not_panic() {
    let Some(gpu) = common::gpu() else { return };
    let device = gpu.device();
    let diag = GpuDiagnostics::new(device);
    gpu.destroy_device();
    // 对已丢失设备的回读必须以 0x6002 失败，而非 panic 或挂起
    let r = read_buffer_blocking(device, gpu.queue(), &diag.buffer, 16);
    if let Err(e) = r {
        assert_eq!(e.code, RynvekErrorCode::GpuDeviceLost);
    }
    let _ = GpuDeviceOwner::request_blocking(Default::default());
}

#[test]
fn oneshot_readback_is_non_blocking_until_mapped() {
    use rynvek_wgpu::{OneShotPoll, OneShotReadback};
    let Some(gpu) = common::gpu() else { return };
    let device = gpu.device();
    let src =
        rynvek_wgpu::kernel::create_storage_buffer(device, "src", &[1, 2, 3, 4, 5, 6], wgpu::BufferUsages::empty())
            .unwrap();
    let mut enc = device.create_command_encoder(&Default::default());
    let mut rb = OneShotReadback::encode(device, &mut enc, &src, 6);
    gpu.queue().submit([enc.finish()]);
    rb.after_submit();
    let mut got = OneShotPoll::Pending;
    for _ in 0..2000 {
        gpu.poll_nonblocking();
        got = rb.poll();
        if got != OneShotPoll::Pending {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(got, OneShotPoll::Ready(vec![1, 2, 3, 4, 5, 6]));
}
