// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 编译器四阶段管线：类型提升、读写集、循环上界与错误码。

mod common;

use common::G;
use rynvek_core::error::RynvekErrorCode;
use rynvek_rules::{CompileOptions, Opcode, ValueType, compile_rule_graph};

fn compile_err(g: G) -> RynvekErrorCode {
    compile_rule_graph(&g.build("t"), &CompileOptions::default())
        .unwrap_err()
        .code
}

#[test]
fn instruction_set_has_exactly_36_unique_opcodes() {
    assert_eq!(Opcode::ALL.len(), 36);
    for (i, op) in Opcode::ALL.iter().enumerate() {
        assert_eq!(*op as usize, i);
        assert_eq!(Opcode::from_u8(i as u8), Some(*op));
        assert_eq!(Opcode::from_name(op.name()), Some(*op));
    }
    assert_eq!(Opcode::from_u8(36), None);
}

#[test]
fn mixed_arithmetic_promotes_with_explicit_casts() {
    let mut g = G::default()
        .read("a.v", ValueType::Q8_8)
        .read("b.n", ValueType::I32)
        .write("a.v", ValueType::Q8_8);
    let a = g.comp("LoadComponent", "a.v", &[]);
    let n = g.comp("LoadComponent", "b.n", &[]);
    let s = g.op("Add", &[a, n]);
    g.comp("EmitComponentWrite", "a.v", &[s]);
    let m = compile_rule_graph(&g.build("promote"), &CompileOptions::default()).unwrap();
    let ops: Vec<Opcode> = m.instrs.iter().map(|i| i.op).collect();
    assert_eq!(
        ops,
        [
            Opcode::LoadComponent,
            Opcode::LoadComponent,
            Opcode::CastQ8_8,
            Opcode::Add,
            Opcode::EmitComponentWrite
        ]
    );
    assert_eq!(m.instrs[3].ty, ValueType::Q8_8);
    // 组件槽位按名称升序
    assert_eq!(
        m.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["a.v", "b.n"]
    );
}

#[test]
fn undeclared_component_write_is_rejected_with_0x5002() {
    let mut g = G::default().read("a.v", ValueType::Q8_8);
    let a = g.comp("LoadComponent", "a.v", &[]);
    g.comp("EmitComponentWrite", "a.v", &[a]);
    assert_eq!(compile_err(g), RynvekErrorCode::RuleUndeclaredWriteAccess);

    let mut g = G::default();
    let c = g.q8(1);
    g.comp("EmitComponentWrite", "never.declared", &[c]);
    assert_eq!(compile_err(g), RynvekErrorCode::RuleUndeclaredWriteAccess);
}

#[test]
fn undeclared_field_deposit_is_rejected_with_0x5002() {
    let mut g = G::default().write("field.nutrient", ValueType::Q8_8);
    let c = g.q8(1);
    g.channel("EmitFieldDeposit", "hazard", &[c]);
    assert_eq!(compile_err(g), RynvekErrorCode::RuleUndeclaredWriteAccess);
}

#[test]
fn loop_bounds_are_enforced_with_0x5003() {
    let mut g = G::default();
    g.loop_begin(65);
    g.loop_end();
    assert_eq!(compile_err(g), RynvekErrorCode::RuleLoopBoundExceeded);

    // 16 × 32 = 512 > 256
    let mut g = G::default();
    g.loop_begin(16);
    g.loop_begin(32);
    g.loop_end();
    g.loop_end();
    assert_eq!(compile_err(g), RynvekErrorCode::RuleLoopBoundExceeded);

    // 16 × 16 = 256 恰好允许
    let mut g = G::default();
    g.loop_begin(16);
    g.loop_begin(16);
    g.loop_end();
    g.loop_end();
    assert!(compile_rule_graph(&g.build("ok"), &CompileOptions::default()).is_ok());
}

#[test]
fn type_errors_are_rejected_with_0x5001() {
    // Q32 除法不在 Tier-0 语义内
    let mut g = G::default();
    let x = g.konst("ConstQ32_32", 1i64 << 32);
    g.op("Div", &[x, x]);
    assert_eq!(compile_err(g), RynvekErrorCode::RuleStaticTypeCheckFailed);

    // Bool 参与算术
    let mut g = G::default();
    let t = g.konst("ConstBool", "true");
    g.op("Add", &[t, t]);
    assert_eq!(compile_err(g), RynvekErrorCode::RuleStaticTypeCheckFailed);

    // 隐式窄化写入
    let mut g = G::default().write("n.i", ValueType::I32);
    let q = g.q8(256);
    g.comp("EmitComponentWrite", "n.i", &[q]);
    assert_eq!(compile_err(g), RynvekErrorCode::RuleStaticTypeCheckFailed);

    // 前向引用（非 Phi）
    let mut g = G::default();
    g.op("Neg", &[1]);
    g.q8(1);
    assert_eq!(compile_err(g), RynvekErrorCode::RuleStaticTypeCheckFailed);

    // 循环未闭合
    let mut g = G::default();
    g.loop_begin(2);
    assert_eq!(compile_err(g), RynvekErrorCode::RuleStaticTypeCheckFailed);
}

#[test]
fn loop_body_values_do_not_escape_except_through_phi() {
    let mut g = G::default();
    let zero = g.q8(0);
    g.loop_begin(4);
    let acc = g.phi(zero);
    let one = g.q8(256);
    let next = g.op("Add", &[acc, one]);
    g.set_phi_next(acc, next);
    g.loop_end();
    let _ok = g.op("Neg", &[acc]); // Phi 最终值可用
    g.op("Neg", &[next]); // 循环体内的值不可用
    assert_eq!(compile_err(g), RynvekErrorCode::RuleStaticTypeCheckFailed);
}

#[test]
fn capability_token_restricts_write_set_with_0x5006() {
    let mut g = G::default()
        .write("energy.value", ValueType::Q8_8)
        .write("field.hazard", ValueType::Q8_8);
    let c = g.q8(1);
    g.comp("EmitComponentWrite", "energy.value", &[c]);
    let graph = g.build("cap");
    let options = CompileOptions {
        allowed_write_prefixes: Some(vec!["energy.".into()]),
    };
    assert_eq!(
        compile_rule_graph(&graph, &options).unwrap_err().code,
        RynvekErrorCode::PluginCapabilityDenied
    );
    let options = CompileOptions {
        allowed_write_prefixes: Some(vec!["energy.".into(), "field.".into()]),
    };
    assert!(compile_rule_graph(&graph, &options).is_ok());
}

#[test]
fn gas_estimate_multiplies_loop_bodies() {
    let mut g = G::default();
    g.q8(1); // 1
    g.loop_begin(8); // 1
    g.q8(2); // 1 × 8
    g.loop_end(); // 1 × 8
    let m = compile_rule_graph(&g.build("gas"), &CompileOptions::default()).unwrap();
    assert_eq!(m.gas_estimate, 1 + 1 + 8 + 8);
}
