// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 跨语言协议夹具：`tests/engine/protocol_messages_v2.json` 中每条命令 / 事件经 serde 往返后逐字段不变。
//! `@rynvek/engine` 的 `protocol.test.ts` 对同一文件做对称断言。

use rynvek_engine::{EngineCommand, EngineEvent};
use serde_json::Value;

const FIXTURE: &str = include_str!("../../../tests/engine/protocol_messages_v2.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("夹具是合法 JSON")
}

#[test]
fn every_fixture_command_roundtrips_field_by_field() {
    let f = fixture();
    let cmds = f["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 17, "17 种当前命令各一条");
    for raw in cmds {
        let cmd: EngineCommand = serde_json::from_value(raw.clone()).unwrap_or_else(|e| panic!("{raw}: {e}"));
        assert_eq!(&serde_json::to_value(&cmd).unwrap(), raw);
    }
}

#[test]
fn every_fixture_event_roundtrips_field_by_field() {
    let f = fixture();
    let events = f["events"].as_array().unwrap();
    let mut kinds: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds.len(), 11, "11 种当前事件全部覆盖");
    for raw in events {
        let ev: EngineEvent = serde_json::from_value(raw.clone()).unwrap_or_else(|e| panic!("{raw}: {e}"));
        assert_eq!(&serde_json::to_value(&ev).unwrap(), raw);
    }
}
