// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `protocols/schemas/agent-tools.v1.json` 必须与开发规范 9.1 的 JSON 代码块逐字节一致。

use rynvek_agent::{AGENT_TOOLS_V1_JSON, ToolName, ToolRegistry};

const SPEC: &str = include_str!("../../../docs/Rynvek_Development_Specification_v1.0.md");

/// 取出规范 9.1 小节内的第一个 ```json 代码块
fn spec_block() -> &'static str {
    let section = &SPEC[SPEC.find("### 9.1 ").expect("规范 9.1 小节")..];
    let start = section.find("```json\n").expect("json 代码块") + "```json\n".len();
    let end = start + section[start..].find("```").expect("代码块结尾");
    &section[start..end]
}

#[test]
fn schema_file_matches_spec_verbatim() {
    assert_eq!(AGENT_TOOLS_V1_JSON, spec_block());
}

#[test]
fn every_tool_in_schema_has_a_typed_counterpart() {
    let r = ToolRegistry::v1();
    assert_eq!(r.tools().len(), 6);
    for (t, name) in r.tools().iter().zip(ToolName::ALL) {
        assert_eq!(t.name, name.as_str());
        assert!(!t.description.is_empty());
        assert_eq!(t.parameters["type"], "object");
    }
}
