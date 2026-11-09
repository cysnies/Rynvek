// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 双后端：覆盖全部 36 条指令的规则 → WGSL（naga 校验）/ CPU 字节码（精确语义）/ `.rir` 往返。

mod common;

use common::G;
use rynvek_core::error::RynvekErrorCode;
use rynvek_core::fixed::{Q8_8, Q32_32};
use rynvek_core::rng::RandomKey;
use rynvek_rules::cpu::RULE_PHASE_ID;
use rynvek_rules::wgsl::{RULE_ENTRY_POINT, RULE_WORKGROUP_SIZE};
use rynvek_rules::{
    CompileOptions, Opcode, RuleModule, RuleWorldState, ValueType, compile_rule_graph, decode_rir,
    emit_cpu_simd_bytecode, emit_wgsl_compute_shader, encode_rir, execute_rule_tick,
};

const Q32_ONE: i64 = 1 << 32;

/// 构造一条覆盖全部 36 条指令的规则
fn opcode_coverage_module() -> RuleModule {
    let mut g = G::default()
        .read("a.q8", ValueType::Q8_8)
        .read("b.i32", ValueType::I32)
        .read("c.q32", ValueType::Q32_32)
        .read("d.flag", ValueType::Bool)
        .write("a.q8", ValueType::Q8_8)
        .write("b.i32", ValueType::I32)
        .write("c.q32", ValueType::Q32_32)
        .write("d.flag", ValueType::Bool)
        .write("field.hazard", ValueType::Q8_8);
    let a = g.comp("LoadComponent", "a.q8", &[]);
    let b = g.comp("LoadComponent", "b.i32", &[]);
    let c = g.comp("LoadComponent", "c.q32", &[]);
    let d = g.comp("LoadComponent", "d.flag", &[]);
    let two = g.q8(512);
    let p_mul = g.op("Mul", &[a, two]); // a*2
    let p_add = g.op("Add", &[a, b]); // a + b（I32 → Q8 隐式提升）
    let p_div = g.op("Div", &[p_add, two]); // (a+b)/2
    let p_sub = g.op("Sub", &[p_div, p_mul]);
    let p_neg = g.op("Neg", &[p_sub]);
    let p_abs = g.op("Abs", &[p_neg]);
    let p_min = g.op("Min", &[p_neg, p_abs]);
    let p_max = g.op("Max", &[p_neg, p_abs]);
    let lo = g.i32(-3);
    let hi = g.i32(5);
    let b_cl = g.op("Clamp", &[b, lo, hi]);
    let eq = g.op("CmpEq", &[b_cl, hi]);
    let ne = g.op("CmpNe", &[b_cl, lo]);
    let lt = g.op("CmpLt", &[p_min, p_max]);
    let le = g.op("CmpLe", &[p_max, p_max]);
    let gt = g.op("CmpGt", &[p_min, p_max]);
    let ge = g.op("CmpGe", &[p_min, p_max]);
    let and = g.op("And", &[eq, ne]);
    let or = g.op("Or", &[gt, ge]);
    let not = g.op("Not", &[or]);
    let cb = g.konst("ConstBool", "true");
    let and2 = g.op("And", &[lt, le]);
    let and3 = g.op("And", &[and2, cb]);
    let and4 = g.op("And", &[and3, not]);
    let flag = g.op("And", &[and4, and]);
    let flag = g.op("Or", &[flag, d]);
    let flag = g.op("Not", &[flag]); // 期望 false
    let sel = g.op("Select", &[flag, p_min, p_max]);
    let as_i = g.op("CastI32", &[sel]);
    let as_q8 = g.op("CastQ8_8", &[b]);
    let as_q32 = g.op("CastQ32_32", &[as_q8]);
    let q32k = g.konst("ConstQ32_32", Q32_ONE / 2);
    let c_mul = g.op("Mul", &[c, q32k]);
    let c_sum = g.op("Add", &[c_mul, as_q32]);
    // 邻居能量求和（BoundedFor + Phi）
    let zero = g.q8(0);
    let nc = g.op("NeighborCount", &[]);
    let begin = g.loop_begin(4);
    let acc = g.phi(zero);
    let v = g.comp("LoadNeighborComponent", "a.q8", &[begin]);
    let next = g.op("Add", &[acc, v]);
    g.set_phi_next(acc, next);
    g.loop_end();
    let haz = g.channel("SampleField", "hazard", &[]);
    let r = g.push(rynvek_rules::GraphNode {
        op: "RandomQ8_8".into(),
        stream: Some(3),
        ..Default::default()
    });
    let dep = g.op("Add", &[haz, r]);
    g.channel("EmitFieldDeposit", "hazard", &[dep]);
    let new_a = g.op("Add", &[sel, acc]);
    g.comp("EmitComponentWrite", "a.q8", &[new_a]);
    let new_b = g.op("Add", &[as_i, nc]);
    g.comp("EmitComponentWrite", "b.i32", &[new_b]);
    g.comp("EmitComponentWrite", "c.q32", &[c_sum]);
    g.comp("EmitComponentWrite", "d.flag", &[flag]);
    compile_rule_graph(&g.build("rules.opcode_coverage"), &CompileOptions::default()).unwrap()
}

fn world() -> RuleWorldState {
    // 3 个实体；邻居：0→{1,2}，1→{0}，2→{}
    let n = 3;
    let mut field = vec![0i64; 5 * n];
    let hazard = 4;
    field[hazard * n] = 128;
    field[hazard * n + 1] = -64;
    field[hazard * n + 2] = 1000;
    RuleWorldState {
        tick: 42,
        world_seed: 0x5EED,
        entity_serials: vec![10, 11, 12],
        components: vec![
            vec![384, -256, 100],                     // a.q8
            vec![7, -9, 2],                           // b.i32
            vec![3 * Q32_ONE, -Q32_ONE, Q32_ONE / 4], // c.q32
            vec![0, 0, 1],                            // d.flag
        ],
        field_samples: field,
        neighbor_offsets: vec![0, 2, 3, 3],
        neighbor_indices: vec![1, 2, 0],
    }
}

/// 用 rynvek-core 的定点类型与朴素整数运算独立推导期望值（不经过 rynvek-rules 的语义模块）
fn oracle(w: &RuleWorldState) -> (Vec<Vec<i64>>, Vec<i64>) {
    let n = w.entity_serials.len();
    let sat = |v: i64| v.clamp(i16::MIN as i64, i16::MAX as i64);
    let q8_mul = |a: i64, b: i64| Q8_8(a as i16).mul(Q8_8(b as i16)).0 as i64;
    let mut out = w.components.clone();
    let mut deposits = vec![0i64; 5 * n];
    for e in 0..n {
        let a = w.components[0][e];
        let b = w.components[1][e];
        let c = Q32_32(w.components[2][e]);
        let d = w.components[3][e] != 0;
        let b_q8 = b.clamp(-128, 127) << 8;
        let p_mul = q8_mul(a, 512);
        let p_add = sat(a + b_q8);
        let p_div = sat((p_add * 256) / 512);
        let p_sub = sat(p_div - p_mul);
        let p_neg = sat(-p_sub);
        let p_abs = sat(p_neg.abs());
        let p_min = p_neg.min(p_abs);
        let p_max = p_neg.max(p_abs);
        let b_cl = b.clamp(-3, 5);
        let eq = b_cl == 5;
        let ne = b_cl != -3;
        let lt = p_min < p_max;
        let gt = p_min > p_max;
        let ge = p_min >= p_max;
        let not = !(gt || ge);
        let flag = !((lt && not && eq && ne) || d);
        let sel = if flag { p_min } else { p_max };
        let as_i = sel >> 8; // Q8 → I32：算术右移（向负无穷取整）
        let as_q32 = Q32_32(b_q8 << 24);
        let c_sum = c.mul(Q32_32(Q32_ONE / 2)).saturating_add(as_q32);
        let (start, end) = (w.neighbor_offsets[e] as usize, w.neighbor_offsets[e + 1] as usize);
        let mut acc = 0i64;
        for i in 0..4 {
            let v = if start + i < end {
                w.components[0][w.neighbor_indices[start + i] as usize]
            } else {
                0
            };
            acc = sat(acc + v);
        }
        let key = RandomKey {
            world_seed: w.world_seed,
            tick: w.tick,
            entity_serial: w.entity_serials[e],
            stream_and_phase: RandomKey::pack_stream_and_phase(3, RULE_PHASE_ID),
            sub_counter: 0,
        };
        let r = (key.generate_u32x4()[0] >> 24) as i64;
        deposits[4 * n + e] = sat(w.field_samples[4 * n + e] + r);
        out[0][e] = sat(sel + acc);
        out[1][e] = as_i + (end - start) as i64;
        out[2][e] = c_sum.0;
        out[3][e] = flag as i64;
    }
    (out, deposits)
}

#[test]
fn coverage_rule_uses_every_opcode() {
    let m = opcode_coverage_module();
    for op in Opcode::ALL {
        assert!(m.uses(op), "未覆盖指令 {}", op.name());
    }
}

#[test]
fn cpu_backend_matches_independent_fixed_point_oracle() {
    let m = opcode_coverage_module();
    let program = emit_cpu_simd_bytecode(&m);
    assert_eq!(program.words.as_ptr() as usize % 16, 0, "字节码必须 16 字节对齐");
    let w = world();
    let out = execute_rule_tick(&program, &w, u64::MAX).unwrap();
    let (components, deposits) = oracle(&w);
    assert_eq!(out.components, components);
    assert_eq!(out.field_deposits, deposits);
    // 循环无提前退出，静态 Gas 估算与实测精确相等
    assert_eq!(out.gas_used, m.gas_estimate * w.entity_serials.len() as u64);
    // 确定性：同输入同输出
    assert_eq!(execute_rule_tick(&program, &w, u64::MAX).unwrap(), out);
}

#[test]
fn cpu_backend_enforces_gas_budget_with_0x5004() {
    let m = opcode_coverage_module();
    let program = emit_cpu_simd_bytecode(&m);
    let w = world();
    let need = m.gas_estimate * 3;
    assert!(execute_rule_tick(&program, &w, need).is_ok());
    let err = execute_rule_tick(&program, &w, need - 1).unwrap_err();
    assert_eq!(err.code, RynvekErrorCode::SandboxGasBudgetExhausted);
}

#[test]
fn cpu_backend_rejects_mismatched_world_dimensions() {
    let m = opcode_coverage_module();
    let program = emit_cpu_simd_bytecode(&m);
    let mut w = world();
    w.neighbor_offsets.pop();
    assert_eq!(
        execute_rule_tick(&program, &w, u64::MAX).unwrap_err().code,
        RynvekErrorCode::DimensionMismatch
    );
}

fn validate_wgsl(source: &str) -> naga::Module {
    let module =
        naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}\n{source}", e.emit_to_string(source)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}\n{source}"));
    module
}

#[test]
fn wgsl_backend_emits_valid_compute_shader() {
    let m = opcode_coverage_module();
    let source = emit_wgsl_compute_shader(&m);
    assert!(
        source
            .lines()
            .take(3)
            .any(|l| l == "// SPDX-License-Identifier: AGPL-3.0-only")
    );
    let module = validate_wgsl(&source);
    let entry = module
        .entry_points
        .iter()
        .find(|e| e.name == RULE_ENTRY_POINT)
        .expect("缺少入口点");
    assert_eq!(entry.stage, naga::ShaderStage::Compute);
    assert_eq!(entry.workgroup_size, [RULE_WORKGROUP_SIZE, 1, 1]);
    // 确定性：同模块同源码
    assert_eq!(emit_wgsl_compute_shader(&m), source);
}

#[test]
fn wgsl_backend_validates_minimal_rule() {
    let mut g = G::default().read("e.v", ValueType::Q8_8).write("e.v", ValueType::Q8_8);
    let v = g.comp("LoadComponent", "e.v", &[]);
    let k = g.q8(-64);
    let s = g.op("Add", &[v, k]);
    g.comp("EmitComponentWrite", "e.v", &[s]);
    let m = compile_rule_graph(&g.build("minimal"), &CompileOptions::default()).unwrap();
    validate_wgsl(&emit_wgsl_compute_shader(&m));
}

#[test]
fn rir_roundtrip_and_integrity() {
    let m = opcode_coverage_module();
    let bytes = encode_rir(&m);
    assert_eq!(&bytes[..4], b"RYIR");
    assert_eq!(decode_rir(&bytes).unwrap(), m);
    assert_eq!(encode_rir(&decode_rir(&bytes).unwrap()), bytes);

    // 任意单字节翻转都必须被拒绝
    for i in [0, 5, bytes.len() / 2, bytes.len() - 1] {
        let mut bad = bytes.clone();
        bad[i] ^= 0x01;
        assert!(decode_rir(&bad).is_err(), "篡改第 {i} 字节未被检测");
    }
    assert!(decode_rir(&bytes[..bytes.len() - 1]).is_err());
}
