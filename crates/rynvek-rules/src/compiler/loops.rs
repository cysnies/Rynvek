// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Pass 3：循环上界验证与 Gas 估算。
//!
//! - 任一 `BoundedForBegin` 的 `max_iters > 64`（或为 0）→ `RuleLoopBoundExceeded (0x5003)`；
//! - 嵌套循环总展开次数（各层上界之积）`> 256` → 同上；
//! - Gas 估算：每条指令成本 × 所在嵌套层上界之积（循环无提前退出，故与 CPU 实测 Gas 精确相等）。

use rynvek_core::error::RynvekErrorCode;

use crate::error::{RuleError, RuleResult};
use crate::ir::{Instr, Opcode};

/// 单个循环的迭代上界
pub const MAX_LOOP_ITERS: u32 = 64;
/// 嵌套循环总展开次数上界
pub const MAX_NESTED_UNROLL: u64 = 256;

pub(super) fn verify_loops_and_estimate_gas(instrs: &[Instr]) -> RuleResult<u64> {
    let mut multiplier_stack: Vec<u64> = vec![1];
    let mut gas = 0u64;
    for (idx, ins) in instrs.iter().enumerate() {
        let current = *multiplier_stack.last().unwrap_or(&1);
        match ins.op {
            Opcode::BoundedForBegin => {
                let iters = ins.imm;
                if !(1..=MAX_LOOP_ITERS as i64).contains(&iters) {
                    return Err(RuleError::at(
                        RynvekErrorCode::RuleLoopBoundExceeded,
                        idx,
                        format!("BoundedForBegin max_iters={iters} 超出 [1, {MAX_LOOP_ITERS}]"),
                    ));
                }
                gas += ins.op.gas_cost() * current;
                let nested = current * iters as u64;
                if nested > MAX_NESTED_UNROLL {
                    return Err(RuleError::at(
                        RynvekErrorCode::RuleLoopBoundExceeded,
                        idx,
                        format!("嵌套循环总展开次数 {nested} > {MAX_NESTED_UNROLL}"),
                    ));
                }
                multiplier_stack.push(nested);
            }
            Opcode::BoundedForEnd => {
                gas += ins.op.gas_cost() * current;
                multiplier_stack.pop();
            }
            _ => gas += ins.op.gas_cost() * current,
        }
    }
    Ok(gas)
}
