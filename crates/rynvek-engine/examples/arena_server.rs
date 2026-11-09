// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 无头 Arena 会话服务（stdio，JSON Lines）：供 `@rynvek/arena` 的实况闭环测试与夹具录制使用。
//!
//! 用法：`cargo run -p rynvek-engine --example arena_server -- [--seed N] [--pve PLAYERS]`
//!
//! 每行输入 `{"from": "<principal_id>", "message": <ArenaClientMessage>}`，
//! 每行输出该消息产生的全部 `Envelope` 组成的 JSON 数组（可能为空数组）。

use std::io::{BufRead, Write};

use rynvek_agent::SystemClock;
use rynvek_engine::arena::{ArenaClientMessage, ArenaSession};
use rynvek_rts::MatchConfig;
use serde::Deserialize;

#[derive(Deserialize)]
struct Line {
    from: String,
    message: ArenaClientMessage,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let seed = flag("--seed").and_then(|s| s.parse().ok()).unwrap_or(42);
    let config = match flag("--pve").and_then(|s| s.parse::<u8>().ok()) {
        Some(players) => MatchConfig::pve(seed, players),
        None => MatchConfig::duel(seed),
    };
    let mut session = ArenaSession::new(config, SystemClock::default());
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Line>(&line) {
            Ok(l) => serde_json::to_string(&session.handle(&l.from, l.message)).expect("可序列化"),
            Err(e) => serde_json::json!([{ "to": "", "message": { "type": "error", "error": {
                "code": 0x5001, "name": "RuleStaticTypeCheckFailed", "message": e.to_string() } } }])
            .to_string(),
        };
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}
