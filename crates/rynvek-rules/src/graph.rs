// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `Untyped Rule Graph JSON`：`@rynvek/rules` 的 `Kernel.define` 输出、编译器管线的输入（规范 08 §8.1 第 1 步）。
//!
//! 规范未冻结 JSON 结构；按最佳实践定义为（`format = "rynvek.rule-graph.v1"`）：
//!
//! ```json
//! {
//!   "format": "rynvek.rule-graph.v1",
//!   "name": "forager.metabolism",
//!   "reads":  [{ "component": "energy.value", "ty": "Q8_8" }],
//!   "writes": [{ "component": "energy.value", "ty": "Q8_8" }],
//!   "nodes":  [{ "op": "LoadComponent", "component": "energy.value" }, ...]
//! }
//! ```
//!
//! 节点下标即 SSA 值编号；`imm` 一律用十进制字符串承载，避免 JS `number` 丢失 64 位精度。

use serde::{Deserialize, Serialize};

use crate::error::{RuleError, RuleResult};
use crate::ir::{FIELD_CHANNEL_NAMES, FIELD_WRITE_PREFIX, Opcode, RuleModule, ValueType};
use rynvek_core::error::RynvekErrorCode;

/// 规则图格式标识
pub const RULE_GRAPH_FORMAT: &str = "rynvek.rule-graph.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentDecl {
    pub component: String,
    pub ty: ValueType,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GraphNode {
    pub op: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iters: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UntypedRuleGraph {
    pub format: String,
    pub name: String,
    #[serde(default)]
    pub reads: Vec<ComponentDecl>,
    #[serde(default)]
    pub writes: Vec<ComponentDecl>,
    pub nodes: Vec<GraphNode>,
}

impl UntypedRuleGraph {
    pub fn from_json(text: &str) -> RuleResult<Self> {
        let graph: Self = serde_json::from_str(text).map_err(|e| {
            RuleError::new(
                RynvekErrorCode::RuleStaticTypeCheckFailed,
                format!("规则图 JSON 解析失败：{e}"),
            )
        })?;
        if graph.format != RULE_GRAPH_FORMAT {
            return Err(RuleError::new(
                RynvekErrorCode::RuleStaticTypeCheckFailed,
                format!("未知规则图格式 `{}`", graph.format),
            ));
        }
        Ok(graph)
    }
}

impl RuleModule {
    /// 还原为等价的规则图（显式保留 Pass 1 插入的 Cast），用于 `.rir` 重新验证与调试
    pub fn to_graph(&self) -> UntypedRuleGraph {
        let comp = |imm: i64| self.components.get(imm as usize).map(|c| c.name.clone());
        let nodes = self
            .instrs
            .iter()
            .map(|ins| {
                let mut node = GraphNode {
                    op: ins.op.name().to_string(),
                    args: ins.operands().into_iter().map(u32::from).collect(),
                    ..GraphNode::default()
                };
                match ins.op {
                    Opcode::ConstBool | Opcode::ConstI32 | Opcode::ConstQ8_8 | Opcode::ConstQ32_32 => {
                        node.imm = Some(ins.imm.to_string());
                    }
                    Opcode::LoadComponent | Opcode::EmitComponentWrite | Opcode::LoadNeighborComponent => {
                        node.component = comp(ins.imm);
                    }
                    Opcode::SampleField | Opcode::EmitFieldDeposit => {
                        node.channel = FIELD_CHANNEL_NAMES.get(ins.imm as usize).map(|c| c.to_string());
                    }
                    Opcode::RandomQ8_8 => node.stream = u16::try_from(ins.imm).ok(),
                    Opcode::BoundedForBegin => node.max_iters = u32::try_from(ins.imm).ok(),
                    _ => {}
                }
                node
            })
            .collect();
        let decl = |c: &crate::ir::ComponentSlot| ComponentDecl {
            component: c.name.clone(),
            ty: c.ty,
        };
        let mut writes: Vec<ComponentDecl> = self.components.iter().filter(|c| c.write).map(decl).collect();
        for (i, ch) in FIELD_CHANNEL_NAMES.iter().enumerate() {
            if self.field_write_mask & (1 << i) != 0 {
                writes.push(ComponentDecl {
                    component: format!("{FIELD_WRITE_PREFIX}{ch}"),
                    ty: ValueType::Q8_8,
                });
            }
        }
        UntypedRuleGraph {
            format: RULE_GRAPH_FORMAT.to_string(),
            name: self.name.clone(),
            reads: self.components.iter().filter(|c| c.read).map(decl).collect(),
            writes,
            nodes,
        }
    }
}
