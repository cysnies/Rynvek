// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Tier-1 QuickJS-NG 沙箱：确定性、Gas / 内存上限与内建裁剪。

#![cfg(feature = "quickjs")]

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::rng::philox4x32_10;
use rynvek_rules::sandbox::GAS_PER_INTERRUPT_POLL;
use rynvek_rules::{QuickJsSandbox, QuickJsSandboxConfig};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/rules");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURE_DIR}/{name}")).unwrap()
}

fn sandbox(source: &str) -> QuickJsSandbox {
    QuickJsSandbox::new(source, QuickJsSandboxConfig::default()).unwrap()
}

#[test]
fn shared_fixture_matches_expected_and_host_philox() {
    let sb = sandbox(&fixture("tier1_metabolism.js"));
    let input = fixture("tier1_metabolism.input.json");
    let out = sb.run_tick(input.trim()).unwrap();
    assert_eq!(out.output_json, fixture("tier1_metabolism.expected.json").trim());

    // 独立用 rynvek-core 的 Philox 复算抖动值
    let v: serde_json::Value = serde_json::from_str(&out.output_json).unwrap();
    for (serial, e) in [10u32, 11, 12].iter().zip(v["entities"].as_array().unwrap()) {
        let r = philox4x32_10([*serial, 7, 42, 0], [24301, 0]);
        assert_eq!(e["jitter"].as_u64().unwrap(), (r[0] >> 24) as u64);
    }
}

#[test]
fn gas_metering_is_deterministic_across_ticks() {
    let src = "function tick(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * 7) | 0; } return s; }";
    let sb = sandbox(src);
    let a = sb.run_tick("100000").unwrap();
    let b = sb.run_tick("100000").unwrap();
    assert_eq!(a, b);
    assert!(a.gas_used > 0 && a.gas_used % GAS_PER_INTERRUPT_POLL == 0);
    // 更多工作 → 更多 Gas
    assert!(sb.run_tick("200000").unwrap().gas_used > a.gas_used);
}

#[test]
fn infinite_loop_exhausts_gas_with_0x5004() {
    let sb = sandbox("function tick() { for (;;) {} }");
    let err = sb.run_tick("null").unwrap_err();
    assert_eq!(err.code, RynvekErrorCode::SandboxGasBudgetExhausted);
    // 沙箱可继续使用（下一 Tick 计数器复位）
    let err = sb.run_tick("null").unwrap_err();
    assert_eq!(err.code, RynvekErrorCode::SandboxGasBudgetExhausted);

    // 用户 try/catch 无法吞掉 Gas 中断
    let sb = sandbox("function tick() { try { for (;;) {} } catch (e) { return 1; } }");
    assert_eq!(
        sb.run_tick("null").unwrap_err().code,
        RynvekErrorCode::SandboxGasBudgetExhausted
    );
}

#[test]
fn heap_bomb_hits_memory_limit_with_0x5005() {
    let config = QuickJsSandboxConfig {
        heap_limit_bytes: 8 * 1024 * 1024,
        ..QuickJsSandboxConfig::default()
    };
    let src = "function tick() { return 'x'.repeat(1 << 24).length; }";
    let sb = QuickJsSandbox::new(src, config).unwrap();
    let err = sb.run_tick("null").unwrap_err();
    assert_eq!(err.code, RynvekErrorCode::SandboxMemoryLimitExceeded, "{err:?}");
}

#[test]
fn nondeterministic_builtins_are_unavailable() {
    let sb = sandbox(
        "function tick() { let r = 'ok'; try { Math.random(); r = 'random'; } catch (e) {} \
         return [r, typeof Date, typeof Promise, typeof WeakRef, typeof performance, typeof Rynvek.philox4x32_10]; }",
    );
    assert_eq!(
        sb.run_tick("null").unwrap().output_json,
        r#"["ok","undefined","undefined","undefined","undefined","function"]"#
    );
    // Math.random 不可被重新赋值绕过
    let sb = sandbox(
        "function tick() { try { Math.random = () => 0.5; } catch (e) { return 'frozen'; } return 'mutable'; }",
    );
    assert_eq!(sb.run_tick("null").unwrap().output_json, r#""frozen""#);
}

#[test]
fn script_without_tick_or_with_errors_is_rejected() {
    let err = QuickJsSandbox::new("var x = 1;", QuickJsSandboxConfig::default()).unwrap_err();
    assert_eq!(err.code, RynvekErrorCode::RuleStaticTypeCheckFailed);
    let err = QuickJsSandbox::new("function tick( {", QuickJsSandboxConfig::default()).unwrap_err();
    assert_eq!(err.code, RynvekErrorCode::RuleStaticTypeCheckFailed);
    let sb = sandbox("function tick() { throw new TypeError('boom'); }");
    let err = sb.run_tick("null").unwrap_err();
    assert_eq!(err.code, RynvekErrorCode::RuleStaticTypeCheckFailed);
    assert!(err.message.contains("boom"));
}

#[test]
fn ticks_do_not_share_global_state() {
    let sb = sandbox("var counter = 0; function tick() { counter += 1; return counter; }");
    assert_eq!(sb.run_tick("null").unwrap().output_json, "1");
    assert_eq!(sb.run_tick("null").unwrap().output_json, "1");
}
