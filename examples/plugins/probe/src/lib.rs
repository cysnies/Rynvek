// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

//! 示例 / 探针插件：正常模式下每 30 Tick 令采集工蜂原地驻守并写一条日志；
//! 其余模式故意越界，用于验证宿主的燃料、内存与能力隔离（配置 `{"mode": "..."}`）：
//! - `probe`（缺省）：正常行为；
//! - `spin`：on-tick 死循环 → 宿主 0x5004；
//! - `bomb`：on-tick 持续申请内存 → 宿主 0x5005；
//! - `sneaky-log`：不声明 `log` 能力却写日志 → 宿主 0x5006；
//! - `sneaky-command`：不声明 `command` 能力却返回命令 → 宿主 0x5006；
//! - `panic`：on-tick 触发 Rust panic（`unreachable` 陷阱）。

wit_bindgen::generate!({
    path: "../../../protocols/wit/rynvek-plugin.wit",
    world: "rynvek-plugin",
});

use std::sync::OnceLock;

use exports::rynvek::plugin::plugin::Guest;
use rynvek::plugin::host::{self, LogLevel};
use rynvek::plugin::types::{Capability, Command, FactionView, PluginError, PluginManifest};

static MODE: OnceLock<String> = OnceLock::new();

/// 从配置 JSON 中取 `mode` 字段（不引入 JSON 依赖，示例插件保持最小体积）
fn parse_mode(config: &str) -> String {
    let Some(at) = config.find("\"mode\"") else {
        return "probe".into();
    };
    let rest = &config[at + 6..];
    let Some(q1) = rest.find('"') else {
        return "probe".into();
    };
    let rest = &rest[q1 + 1..];
    rest.find('"').map_or_else(|| "probe".into(), |q2| rest[..q2].to_owned())
}

fn mode() -> &'static str {
    MODE.get().map_or("probe", String::as_str)
}

struct Probe;

impl Guest for Probe {
    fn init(config_json: String) -> Result<PluginManifest, PluginError> {
        let m = parse_mode(&config_json);
        let caps = match m.as_str() {
            "probe" | "spin" | "bomb" | "panic" => vec![Capability::Observe, Capability::Command, Capability::Log],
            "sneaky-log" => vec![Capability::Observe, Capability::Command],
            "sneaky-command" => vec![Capability::Observe, Capability::Log],
            other => {
                return Err(PluginError {
                    code: 0x5006,
                    message: format!("未知模式 {other}"),
                });
            }
        };
        let _ = MODE.set(m);
        Ok(PluginManifest {
            id: "community.probe".into(),
            version: "1.0.0".into(),
            capabilities: caps,
        })
    }

    fn on_tick(view: FactionView) -> Result<Vec<Command>, PluginError> {
        match mode() {
            "spin" => {
                let mut x: u64 = view.tick;
                loop {
                    x = core::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
                }
            }
            "bomb" => {
                let mut hoard: Vec<Vec<u8>> = Vec::new();
                loop {
                    hoard.push(vec![0xA5; 1 << 20]);
                    core::hint::black_box(&hoard);
                }
            }
            "panic" => panic!("探针插件按配置触发 panic"),
            "sneaky-log" => {
                host::log(LogLevel::Info, "未声明 log 能力的日志")?;
                return Ok(Vec::new());
            }
            _ => {}
        }
        if view.tick % 30 != 0 {
            return Ok(Vec::new());
        }
        let own = view.units.iter().find(|u| u.faction == view.faction);
        let Some(anchor) = own else { return Ok(Vec::new()) };
        if mode() == "probe" {
            let r = host::random(view.tick);
            host::log(LogLevel::Debug, &format!("tick={} rand={r}", view.tick))?;
        }
        Ok(vec![Command {
            payload_json: format!(
                "{{\"type\":\"issue_order\",\"units\":{{\"kind\":\"archetype\",\"value\":\"unit.forager\"}},\"order\":\"HoldPosition\",\"target\":[{},{}]}}",
                anchor.x_raw, anchor.y_raw
            ),
        }])
    }
}

export!(Probe);
