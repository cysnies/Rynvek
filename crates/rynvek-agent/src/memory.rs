// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `AgentEpisodicMemory`：Agent 外部情景记忆归档（GAP-26）。
//!
//! 每个 Principal 一条独立的 64 条环形缓冲（D216–D232 "Agent 记忆隔离"），记录每次工具调用的
//! Tick、工具、规范化参数摘要（BLAKE3，键按字典序序列化）与结果。归档形态为 JSON，供 LLM 上下文回灌与赛后复盘。

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::ToolName;

/// 每个 Principal 的记忆容量
pub const AGENT_MEMORY_CAPACITY: usize = 64;

/// 调用结果
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EpisodeOutcome {
    Ok { summary: String },
    Rejected { code: u16, message: String },
}

/// 一条情景记录
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeRecord {
    /// 该 Principal 的单调序号（环形覆盖后仍连续）
    pub seq: u64,
    pub tick: u64,
    pub tool: Option<ToolName>,
    /// 规范化参数 JSON 的 BLAKE3 十六进制摘要
    pub arguments_digest: String,
    pub outcome: EpisodeOutcome,
}

/// 环形情景记忆
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEpisodicMemory {
    capacity: usize,
    next_seq: u64,
    records: VecDeque<EpisodeRecord>,
}

impl Default for AgentEpisodicMemory {
    fn default() -> Self {
        Self::with_capacity(AGENT_MEMORY_CAPACITY)
    }
}

impl AgentEpisodicMemory {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            next_seq: 0,
            records: VecDeque::new(),
        }
    }

    /// 参数摘要：serde_json 的 `Map` 为有序映射，`to_string` 即规范化序列化
    pub fn digest(arguments: &Value) -> String {
        blake3::hash(arguments.to_string().as_bytes()).to_hex().to_string()
    }

    pub fn push(&mut self, tick: u64, tool: Option<ToolName>, arguments: &Value, outcome: EpisodeOutcome) {
        if self.records.len() == self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(EpisodeRecord {
            seq: self.next_seq,
            tick,
            tool,
            arguments_digest: Self::digest(arguments),
            outcome,
        });
        self.next_seq += 1;
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 由旧到新遍历
    pub fn iter(&self) -> impl Iterator<Item = &EpisodeRecord> {
        self.records.iter()
    }

    /// 最近 `n` 条（由旧到新）
    pub fn recent(&self, n: usize) -> Vec<EpisodeRecord> {
        self.records
            .iter()
            .skip(self.records.len().saturating_sub(n))
            .cloned()
            .collect()
    }

    /// JSON 归档
    pub fn archive(&self) -> Value {
        serde_json::to_value(self).expect("情景记忆可序列化")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn ring_keeps_latest_64_with_contiguous_seq() {
        let mut m = AgentEpisodicMemory::default();
        for i in 0..100u64 {
            m.push(
                i,
                Some(ToolName::SampleRegionalFields),
                &json!({"chunk_x": i, "chunk_y": 0}),
                EpisodeOutcome::Ok { summary: String::new() },
            );
        }
        assert_eq!(m.len(), 64);
        assert_eq!(m.iter().next().unwrap().seq, 36);
        assert_eq!(m.recent(2).iter().map(|r| r.seq).collect::<Vec<_>>(), [98, 99]);
        let back: AgentEpisodicMemory = serde_json::from_value(m.archive()).unwrap();
        assert_eq!(back, m);
        // 键顺序不影响摘要
        let a: Value = serde_json::from_str(r#"{"b":1,"a":2}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":2,"b":1}"#).unwrap();
        assert_eq!(AgentEpisodicMemory::digest(&a), AgentEpisodicMemory::digest(&b));
    }
}
