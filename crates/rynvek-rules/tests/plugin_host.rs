// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! WASM 插件宿主集成测试：使用 `cargo xtask build-plugin-fixtures` 生成的探针插件夹具。

#![cfg(feature = "wasmtime")]

use rynvek_core::error::RynvekErrorCode;
use rynvek_rules::plugin::{
    Capability, FactionView, PLUGIN_GAS_PER_CALL, PluginFaultKind, PluginInstance, PluginRuntime, UnitView,
};

const PROBE: &[u8] = include_bytes!("fixtures/plugin/probe.component.wasm");

fn view(tick: u64) -> FactionView {
    FactionView {
        tick,
        faction: 0,
        biomass_milli: 1_000,
        flux_milli: 0,
        neurogel_milli: 0,
        supply_used: 4,
        supply_cap: 10,
        score_milli: 0,
        units: vec![UnitView {
            id: 7,
            faction: 0,
            kind: 0,
            x_raw: 10 << 32,
            y_raw: 12 << 32,
            hp_milli: 50_000,
        }],
        buildings: Vec::new(),
    }
}

fn instance(mode: &str) -> Result<PluginInstance, rynvek_rules::plugin::PluginFault> {
    let rt = PluginRuntime::new().unwrap();
    let m = rt.compile(PROBE).unwrap();
    PluginInstance::instantiate(&rt, &m, &format!("{{\"mode\":\"{mode}\"}}"), 42)
}

#[test]
fn probe_plugin_runs_deterministically_within_budget() {
    let run = || {
        let mut p = instance("probe").unwrap();
        assert_eq!(p.manifest().id, "community.probe");
        assert!(p.manifest().has(Capability::Command));
        let mut out = Vec::new();
        for t in 0..120 {
            out.push(p.on_tick(view(t)).unwrap());
        }
        (out, p.drain_logs(), p.stats())
    };
    let (a, logs, stats) = run();
    let (b, logs_b, _) = run();
    assert_eq!(a, b, "同一输入必须得到同一输出");
    assert_eq!(logs, logs_b, "宿主随机数确定性");
    assert_eq!(a.iter().filter(|c| !c.is_empty()).count(), 4, "每 30 Tick 一条命令");
    assert!(a[30][0].contains("\"type\":\"issue_order\""));
    assert_eq!(logs.len(), 4);
    assert!(stats.gas_max_call > 0 && stats.gas_max_call < PLUGIN_GAS_PER_CALL);
}

#[test]
fn infinite_loop_exhausts_gas_and_isolates() {
    let mut p = instance("spin").unwrap();
    let f = p.on_tick(view(1)).unwrap_err();
    assert_eq!(f.kind, PluginFaultKind::GasExhausted);
    assert_eq!(f.code(), RynvekErrorCode::SandboxGasBudgetExhausted);
    assert!(p.isolated().is_some());
    // 隔离后不再执行
    assert_eq!(p.on_tick(view(2)).unwrap_err(), f);
}

#[test]
fn memory_bomb_hits_64_mib_limit() {
    let mut p = instance("bomb").unwrap();
    let f = p.on_tick(view(1)).unwrap_err();
    assert_eq!(f.code(), RynvekErrorCode::SandboxMemoryLimitExceeded, "{f}");
}

#[test]
fn undeclared_capabilities_are_denied() {
    let mut p = instance("sneaky-log").unwrap();
    assert_eq!(
        p.on_tick(view(1)).unwrap_err().code(),
        RynvekErrorCode::PluginCapabilityDenied
    );
    let mut p = instance("sneaky-command").unwrap();
    assert!(p.on_tick(view(1)).unwrap().is_empty());
    let f = p.on_tick(view(30)).unwrap_err();
    assert_eq!(f.kind, PluginFaultKind::CapabilityDenied);
}

#[test]
fn panics_trap_and_isolate() {
    let mut p = instance("panic").unwrap();
    let f = p.on_tick(view(1)).unwrap_err();
    assert_eq!(f.kind, PluginFaultKind::Trap);
    assert!(f.isolates());
}

#[test]
fn guest_init_error_is_reported() {
    let f = instance("nonsense").unwrap_err();
    assert_eq!(f.kind, PluginFaultKind::GuestError);
    assert_eq!(f.code(), RynvekErrorCode::PluginCapabilityDenied);
}

#[test]
fn static_checks_reject_core_modules_and_wasi_imports() {
    let rt = PluginRuntime::new().unwrap();
    // 核心模块（非 Component）
    let core = wat::parse_str("(module (func (export \"f\")))").unwrap();
    assert_eq!(rt.compile(&core).unwrap_err().kind, PluginFaultKind::InvalidComponent);
    // 导入 WASI 文件系统的组件
    let wasi = wat::parse_str(r#"(component (import "wasi:filesystem/types@0.2.0" (instance)))"#).unwrap();
    let f = rt.compile(&wasi).unwrap_err();
    assert_eq!(f.kind, PluginFaultKind::CapabilityDenied);
    assert!(f.message.contains("wasi:filesystem"));
    // 未导出 plugin 接口
    let empty = wat::parse_str("(component)").unwrap();
    assert_eq!(rt.compile(&empty).unwrap_err().kind, PluginFaultKind::InvalidComponent);
    // 垃圾字节
    assert!(rt.compile(b"\0asm garbage").is_err());
    // 探针插件只导入 types / host
    let m = rt.compile(PROBE).unwrap();
    assert!(m.imports.iter().all(|i| i.starts_with("rynvek:plugin/")));
}
