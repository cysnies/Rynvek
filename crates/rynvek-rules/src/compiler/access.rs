// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Pass 2：读写访问集提取与检查。
//!
//! - 函数体内对未在 `writes` 中声明的组件调用 `EmitComponentWrite` → `RuleUndeclaredWriteAccess (0x5002)`；
//! - 对未声明的场通道调用 `EmitFieldDeposit` → 同上；
//! - 声明的写集超出 Capability Token 授权前缀 → `PluginCapabilityDenied (0x5006)`。

use rynvek_core::error::RynvekErrorCode;

use super::{CompileOptions, DeclTable};
use crate::error::{RuleError, RuleResult};
use crate::ir::{FIELD_CHANNEL_NAMES, FIELD_WRITE_PREFIX, Instr, Opcode};

pub(super) fn check_access_sets(instrs: &[Instr], decls: &DeclTable, options: &CompileOptions) -> RuleResult<()> {
    for (idx, ins) in instrs.iter().enumerate() {
        match ins.op {
            Opcode::EmitComponentWrite => {
                let declared = usize::try_from(ins.imm)
                    .ok()
                    .and_then(|slot| decls.components.get(slot))
                    .is_some_and(|c| c.write);
                if !declared {
                    return Err(RuleError::at(
                        RynvekErrorCode::RuleUndeclaredWriteAccess,
                        idx,
                        "EmitComponentWrite 写入了未在 writes 中声明的组件字段",
                    ));
                }
            }
            Opcode::EmitFieldDeposit => {
                if decls.field_write_mask & (1 << ins.imm) == 0 {
                    return Err(RuleError::at(
                        RynvekErrorCode::RuleUndeclaredWriteAccess,
                        idx,
                        format!(
                            "EmitFieldDeposit 写入了未声明的场通道 `{FIELD_WRITE_PREFIX}{}`",
                            FIELD_CHANNEL_NAMES[ins.imm as usize]
                        ),
                    ));
                }
            }
            _ => {}
        }
    }

    if let Some(prefixes) = &options.allowed_write_prefixes {
        let allowed = |name: &str| prefixes.iter().any(|p| name.starts_with(p.as_str()));
        let field_names = FIELD_CHANNEL_NAMES
            .iter()
            .enumerate()
            .filter(|(i, _)| decls.field_write_mask & (1 << i) != 0)
            .map(|(_, c)| format!("{FIELD_WRITE_PREFIX}{c}"));
        let component_names = decls.components.iter().filter(|c| c.write).map(|c| c.name.clone());
        if let Some(denied) = component_names.chain(field_names).find(|n| !allowed(n)) {
            return Err(RuleError::new(
                RynvekErrorCode::PluginCapabilityDenied,
                format!("Capability Token 未授权写入 `{denied}`"),
            ));
        }
    }
    Ok(())
}
