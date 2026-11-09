// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 规则编译器四阶段管线（规范 08 §8.1）：
//!
//! ```text
//! 1. 规则图 JSON 解析                       → graph.rs
//! 2. Pass 1：类型推断与定点提升              → types.rs
//! 3. Pass 2：读写访问集提取与权限检查        → access.rs
//! 4. Pass 3：循环上界验证与 Gas 估算         → loops.rs
//! ```

mod access;
mod loops;
mod types;

use std::collections::BTreeMap;

use rynvek_core::error::RynvekErrorCode;

use crate::error::{RuleError, RuleResult};
use crate::graph::UntypedRuleGraph;
use crate::ir::{ComponentSlot, FIELD_WRITE_PREFIX, RuleModule, ValueType, field_channel_index};

/// 编译选项
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompileOptions {
    /// Capability Token 授予的可写组件前缀（`None` = 不限制）；越权写入报 `PluginCapabilityDenied`
    pub allowed_write_prefixes: Option<Vec<String>>,
}

/// 声明表：组件槽位（按名称升序）与场写掩码
#[derive(Clone, Debug, Default)]
pub(crate) struct DeclTable {
    pub components: Vec<ComponentSlot>,
    pub field_write_mask: u8,
}

impl DeclTable {
    fn build(graph: &UntypedRuleGraph) -> RuleResult<Self> {
        let mut map: BTreeMap<String, ComponentSlot> = BTreeMap::new();
        let mut field_write_mask = 0u8;
        let mut declare = |name: &str, ty: ValueType, write: bool| -> RuleResult<()> {
            if let Some(channel) = name.strip_prefix(FIELD_WRITE_PREFIX) {
                let idx = field_channel_index(channel).ok_or_else(|| {
                    RuleError::new(
                        RynvekErrorCode::RuleStaticTypeCheckFailed,
                        format!("未知场通道 `{name}`"),
                    )
                })?;
                if !write || ty != ValueType::Q8_8 {
                    return Err(RuleError::new(
                        RynvekErrorCode::RuleStaticTypeCheckFailed,
                        format!("场通道 `{name}` 只能以 Q8_8 类型声明在 writes 中"),
                    ));
                }
                field_write_mask |= 1 << idx;
                return Ok(());
            }
            if !matches!(
                ty,
                ValueType::Bool | ValueType::I32 | ValueType::Q8_8 | ValueType::Q32_32
            ) {
                return Err(RuleError::new(
                    RynvekErrorCode::RuleStaticTypeCheckFailed,
                    format!("组件 `{name}` 类型非法"),
                ));
            }
            let slot = map.entry(name.to_string()).or_insert(ComponentSlot {
                name: name.to_string(),
                ty,
                read: false,
                write: false,
            });
            if slot.ty != ty {
                return Err(RuleError::new(
                    RynvekErrorCode::RuleStaticTypeCheckFailed,
                    format!("组件 `{name}` 在 reads/writes 中声明的类型不一致"),
                ));
            }
            if write {
                slot.write = true;
            } else {
                slot.read = true;
            }
            Ok(())
        };
        for d in &graph.reads {
            declare(&d.component, d.ty, false)?;
        }
        for d in &graph.writes {
            declare(&d.component, d.ty, true)?;
        }
        Ok(Self {
            components: map.into_values().collect(),
            field_write_mask,
        })
    }

    pub fn slot(&self, name: &str) -> Option<usize> {
        self.components.iter().position(|c| c.name == name)
    }
}

/// 编译规则图为已验证的 `RuleModule`
pub fn compile_rule_graph(graph: &UntypedRuleGraph, options: &CompileOptions) -> RuleResult<RuleModule> {
    let decls = DeclTable::build(graph)?;
    let instrs = types::infer_and_promote(graph, &decls)?;
    access::check_access_sets(&instrs, &decls, options)?;
    let gas_estimate = loops::verify_loops_and_estimate_gas(&instrs)?;
    Ok(RuleModule {
        name: graph.name.clone(),
        components: decls.components,
        field_write_mask: decls.field_write_mask,
        instrs,
        gas_estimate,
    })
}

/// 便捷入口：从 JSON 文本编译
pub fn compile_rule_graph_json(json: &str, options: &CompileOptions) -> RuleResult<RuleModule> {
    compile_rule_graph(&UntypedRuleGraph::from_json(json)?, options)
}
