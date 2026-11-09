// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Arena 可玩闭环夹具：`tests/arena/opening.transcript.json` 中每一步的客户端消息喂给真实 [`ArenaSession`]，
//! 产出的服务端消息必须与夹具逐字段相同。`@rynvek/arena` 的 `loop.test.ts` 以同一夹具驱动前端
//! （人类键鼠输入 + 两个受控工具 Agent），断言前端发出的每条消息与夹具一致——两侧合起来闭合整个回路。

use rynvek_agent::ManualClock;
use rynvek_engine::arena::{ArenaClientMessage, ArenaSession};
use rynvek_rts::MatchConfig;
use serde_json::Value;

const FIXTURE: &str = include_str!("../../../tests/arena/opening.transcript.json");

/// JSON 数值语义归一：夹具由 JS 序列化，`1.0` 会写成 `1`；整值浮点统一为整数后再比较。
fn normalize(v: Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 9.0e15 => Value::from(f as i64),
            _ => Value::Number(n),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(normalize).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, normalize(v))).collect()),
        other => other,
    }
}

#[test]
fn transcript_replays_byte_for_byte_on_the_authoritative_session() {
    let steps: Vec<Value> = serde_json::from_str(FIXTURE).expect("夹具是合法 JSON");
    assert!(steps.len() >= 20);
    let mut session = ArenaSession::new(MatchConfig::duel(42), ManualClock::default());
    for (i, step) in steps.iter().enumerate() {
        let from = step["from"].as_str().unwrap();
        let msg: ArenaClientMessage =
            serde_json::from_value(step["send"].clone()).unwrap_or_else(|e| panic!("第 {i} 步无法解析：{e}"));
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            step["send"],
            "第 {i} 步客户端消息 serde 往返不变"
        );
        let out = normalize(serde_json::to_value(session.handle(from, msg)).unwrap());
        if out != normalize(step["recv"].clone()) {
            if let Ok(dir) = std::env::var("RYNVEK_DIFF_DIR") {
                std::fs::write(format!("{dir}/got.json"), serde_json::to_string_pretty(&out).unwrap()).unwrap();
                std::fs::write(
                    format!("{dir}/want.json"),
                    serde_json::to_string_pretty(&step["recv"]).unwrap(),
                )
                .unwrap();
            }
        }
        assert_eq!(
            out,
            normalize(step["recv"].clone()),
            "第 {i} 步（{from}）服务端输出与夹具不一致"
        );
    }
    assert_eq!(session.world.tick, 2431);
    assert!(session.world.map.is_point_symmetric());
}
