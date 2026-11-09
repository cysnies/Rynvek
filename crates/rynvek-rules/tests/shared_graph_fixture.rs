// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 跨语言夹具：`@rynvek/rules` 的 `Kernel.define` 生成的规则图（`tests/rules/forager_metabolism.graph.json`）
//! 必须能被本编译器接受、规范序列化逐字节往返，并在 CPU / WGSL 后端得到预期结果。

use rynvek_core::rng::RandomKey;
use rynvek_rules::cpu::RULE_PHASE_ID;
use rynvek_rules::{
    CompileOptions, RuleWorldState, UntypedRuleGraph, compile_rule_graph_json, decode_rir, emit_cpu_simd_bytecode,
    emit_wgsl_compute_shader, encode_rir, execute_rule_tick,
};

const FIXTURE: &str = include_str!("../../../tests/rules/forager_metabolism.graph.json");

#[test]
fn ts_generated_graph_is_canonical_json() {
    let graph = UntypedRuleGraph::from_json(FIXTURE).unwrap();
    assert_eq!(format!("{}\n", serde_json::to_string_pretty(&graph).unwrap()), FIXTURE);
}

#[test]
fn ts_generated_graph_compiles_and_runs_on_both_backends() {
    let module = compile_rule_graph_json(FIXTURE, &CompileOptions::default()).unwrap();
    assert_eq!(module.name, "forager.metabolism");
    assert_eq!(decode_rir(&encode_rir(&module)).unwrap(), module);

    let wgsl = emit_wgsl_compute_shader(&module);
    let naga_module = naga::front::wgsl::parse_str(&wgsl).unwrap();
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&naga_module)
        .unwrap();

    // 槽位按名称升序：0 = energy.value，1 = genome.metabolic_rate
    let n = 4;
    let energy = vec![2560i64, 25500, 10, 0];
    let rate = vec![64i64, 32, 128, 0];
    let nutrient = vec![1024i64, 0, 300, -50];
    let mut field = vec![0i64; 5 * n];
    field[..n].copy_from_slice(&nutrient);
    let world = RuleWorldState {
        tick: 7,
        world_seed: 99,
        entity_serials: vec![1, 2, 3, 4],
        components: vec![energy.clone(), rate.clone()],
        field_samples: field,
        // 0→{1,2,3}，1→{0}，2→{}，3→{0,1,2,0,1,2,0,1,2}（超过 8 个时只累加前 8 个）
        neighbor_offsets: vec![0, 3, 4, 4, 13],
        neighbor_indices: vec![1, 2, 3, 0, 0, 1, 2, 0, 1, 2, 0, 1, 2],
    };
    let out = execute_rule_tick(&emit_cpu_simd_bytecode(&module), &world, u64::MAX).unwrap();

    let sat = |v: i64| v.clamp(-32768, 32767);
    for e in 0..n {
        let (start, end) = (
            world.neighbor_offsets[e] as usize,
            world.neighbor_offsets[e + 1] as usize,
        );
        let nc = (end - start) as i64;
        let uptake = sat((nutrient[e] * 64) >> 8);
        let nc_q8 = nc.clamp(-128, 127) << 8;
        let crowd = sat((nc_q8 * 16) >> 8);
        let mut sum = 0i64;
        for i in 0..8 {
            if start + i < end {
                sum = sat(sum + energy[world.neighbor_indices[start + i] as usize]);
            }
        }
        let diffusion = if nc > 0 {
            sat((sat(((sum << 8) / nc_q8).clamp(-32768, 32767) - energy[e]) * 32) >> 8)
        } else {
            0
        };
        let key = RandomKey {
            world_seed: 99,
            tick: 7,
            entity_serial: world.entity_serials[e],
            stream_and_phase: RandomKey::pack_stream_and_phase(1, RULE_PHASE_ID),
            sub_counter: 0,
        };
        let jitter = if (key.generate_u32x4()[0] >> 24) as i64 > 128 {
            4
        } else {
            0
        };
        let next = sat(sat(sat(energy[e] + uptake) + diffusion) - sat(sat(rate[e] + crowd) + jitter));
        assert_eq!(out.components[0][e], next.clamp(0, 25600), "实体 {e} 能量");
        assert_eq!(out.components[1][e], rate[e], "只读组件保持不变");
        assert_eq!(out.field_deposits[e], sat(-uptake), "实体 {e} 营养沉积");
    }
}
