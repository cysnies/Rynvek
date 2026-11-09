// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Pass 1：类型推断与定点提升（`Q32_32` / `Q8_8`）。
//!
//! 输入是无类型规则图，输出是带类型的 SSA 指令序列：
//!
//! - 混合数值运算按 `I32 < Q8_8 < Q32_32` 向高位提升，并**显式插入** `Cast*` 指令，
//!   使后端无需再做任何隐式转换（CPU 与 WGSL 的语义因此逐条一一对应）；
//! - 写入组件/场时只允许向目标类型提升，禁止隐式窄化（需显式 `Cast*`）；
//! - `Q32_32` 除法不在 Tier-0 指令语义内（WGSL 无 128 位整数除法，无法保证与 CPU 位级一致），直接拒绝；
//! - 校验 SSA 支配与循环作用域：循环体内定义的值只能在该循环体内使用，`Phi` 的最终值可在循环后使用。

use rynvek_core::error::RynvekErrorCode;

use super::DeclTable;
use crate::error::{RuleError, RuleResult};
use crate::graph::{GraphNode, UntypedRuleGraph};
use crate::ir::{Instr, Opcode, ValueType, field_channel_index};

/// 单个模块允许的最大指令数（操作数以 `u16` 编码）
pub const MAX_INSTRS: usize = u16::MAX as usize;

struct LoopFrame {
    /// 循环标识（用于作用域）
    id: u32,
    /// `BoundedForBegin` 的新下标
    begin: usize,
    /// `(phi 新下标, 回边操作数的旧节点号)`
    phis: Vec<(usize, u32)>,
    /// 循环体是否已开始（出现了非 Phi 指令）
    body_started: bool,
}

struct Builder<'a> {
    decls: &'a DeclTable,
    instrs: Vec<Instr>,
    /// 每条新指令所在的循环作用域栈
    scopes: Vec<Vec<u32>>,
    /// 旧节点号 → 新指令下标
    map: Vec<Option<usize>>,
    loops: Vec<LoopFrame>,
    next_loop_id: u32,
}

impl Builder<'_> {
    fn current_scope(&self) -> Vec<u32> {
        self.loops.iter().map(|l| l.id).collect()
    }

    fn push(&mut self, ins: Instr, scope: Vec<u32>, node: usize) -> RuleResult<usize> {
        if self.instrs.len() >= MAX_INSTRS {
            return Err(RuleError::type_error(node, format!("指令数超过上限 {MAX_INSTRS}")));
        }
        self.instrs.push(ins);
        self.scopes.push(scope);
        Ok(self.instrs.len() - 1)
    }

    fn ty(&self, idx: usize) -> ValueType {
        self.instrs[idx].ty
    }

    /// 解析操作数：必须已定义、非 Void，且其作用域对当前位置可见
    fn operand(&self, node: usize, old: u32) -> RuleResult<usize> {
        let new = self
            .map
            .get(old as usize)
            .copied()
            .flatten()
            .filter(|_| (old as usize) < node)
            .ok_or_else(|| RuleError::type_error(node, format!("操作数 #{old} 未定义或违反 SSA 支配次序")))?;
        self.check_visible(node, new)?;
        if self.ty(new) == ValueType::Void {
            return Err(RuleError::type_error(node, format!("操作数 #{old} 没有结果值")));
        }
        Ok(new)
    }

    fn check_visible(&self, node: usize, new: usize) -> RuleResult<()> {
        let def = &self.scopes[new];
        let here = self.current_scope();
        if def.len() <= here.len() && here[..def.len()] == def[..] {
            Ok(())
        } else {
            Err(RuleError::type_error(
                node,
                "引用了循环体外不可见的值（循环内定义的值只能经 Phi 带出）",
            ))
        }
    }

    /// 将值提升到目标类型（必要时插入 Cast）；禁止隐式窄化与 Bool/数值互转
    fn coerce(&mut self, node: usize, value: usize, target: ValueType) -> RuleResult<usize> {
        let from = self.ty(value);
        if from == target {
            return Ok(value);
        }
        if !(from.is_numeric() && target.is_numeric() && from.promotion_rank() <= target.promotion_rank()) {
            return Err(RuleError::type_error(
                node,
                format!("不能将 {from:?} 隐式转换为 {target:?}（需显式 Cast）"),
            ));
        }
        self.cast(node, value, target)
    }

    fn cast(&mut self, node: usize, value: usize, target: ValueType) -> RuleResult<usize> {
        let op = match target {
            ValueType::I32 => Opcode::CastI32,
            ValueType::Q8_8 => Opcode::CastQ8_8,
            ValueType::Q32_32 => Opcode::CastQ32_32,
            _ => return Err(RuleError::type_error(node, "非法的转换目标")),
        };
        let mut ins = Instr::new(op, target);
        ins.a = value as u16;
        let scope = self.current_scope();
        self.push(ins, scope, node)
    }

    fn component_slot(&self, node: usize, n: &GraphNode) -> RuleResult<Option<usize>> {
        let name = n
            .component
            .as_deref()
            .ok_or_else(|| RuleError::type_error(node, format!("{} 缺少 component 字段", n.op)))?;
        Ok(self.decls.slot(name))
    }

    fn channel(&self, node: usize, n: &GraphNode) -> RuleResult<usize> {
        let name = n
            .channel
            .as_deref()
            .ok_or_else(|| RuleError::type_error(node, format!("{} 缺少 channel 字段", n.op)))?;
        field_channel_index(name).ok_or_else(|| RuleError::type_error(node, format!("未知场通道 `{name}`")))
    }
}

fn parse_imm<T: std::str::FromStr>(node: usize, n: &GraphNode) -> RuleResult<T> {
    n.imm
        .as_deref()
        .and_then(|s| s.parse::<T>().ok())
        .ok_or_else(|| RuleError::type_error(node, format!("{} 的 imm 缺失或超出类型范围", n.op)))
}

fn binary_numeric_type(node: usize, a: ValueType, b: ValueType) -> RuleResult<ValueType> {
    if a.is_numeric() && b.is_numeric() {
        Ok(if a.promotion_rank() >= b.promotion_rank() { a } else { b })
    } else {
        Err(RuleError::type_error(
            node,
            format!("数值运算的操作数类型非法：{a:?}, {b:?}"),
        ))
    }
}

pub(super) fn infer_and_promote(graph: &UntypedRuleGraph, decls: &DeclTable) -> RuleResult<Vec<Instr>> {
    let mut b = Builder {
        decls,
        instrs: Vec::with_capacity(graph.nodes.len()),
        scopes: Vec::with_capacity(graph.nodes.len()),
        map: vec![None; graph.nodes.len()],
        loops: Vec::new(),
        next_loop_id: 0,
    };

    for (node, n) in graph.nodes.iter().enumerate() {
        let op = Opcode::from_name(&n.op).ok_or_else(|| RuleError::type_error(node, format!("未知指令 `{}`", n.op)))?;
        if n.args.len() != op.arity() {
            return Err(RuleError::type_error(
                node,
                format!("{} 需要 {} 个操作数，实际 {}", op.name(), op.arity(), n.args.len()),
            ));
        }
        // Phi 只能紧跟在 BoundedForBegin（或同循环的其他 Phi）之后
        match (op, b.loops.last_mut()) {
            (Opcode::Phi, Some(frame)) if !frame.body_started => {}
            (Opcode::Phi, _) => return Err(RuleError::type_error(node, "Phi 只能紧跟在 BoundedForBegin 之后")),
            (_, Some(frame)) => frame.body_started = true,
            (_, None) => {}
        }

        let new_idx = build_node(&mut b, node, op, n)?;
        b.map[node] = new_idx;
    }

    if !b.loops.is_empty() {
        return Err(RuleError::new(
            RynvekErrorCode::RuleStaticTypeCheckFailed,
            "BoundedForBegin 缺少配对的 BoundedForEnd",
        ));
    }
    Ok(b.instrs)
}

/// 处理单个节点，返回其结果的新下标（Void 指令也返回下标，供引用检查报错）
fn build_node(b: &mut Builder<'_>, node: usize, op: Opcode, n: &GraphNode) -> RuleResult<Option<usize>> {
    use Opcode::*;
    let scope = b.current_scope();
    let arg = |b: &Builder<'_>, i: usize| b.operand(node, n.args[i]);

    let ins = match op {
        ConstBool => {
            let v = match n.imm.as_deref() {
                Some("true") | Some("1") => 1,
                Some("false") | Some("0") => 0,
                _ => return Err(RuleError::type_error(node, "ConstBool 的 imm 必须为 true/false")),
            };
            Instr {
                imm: v,
                ..Instr::new(op, ValueType::Bool)
            }
        }
        ConstI32 => Instr {
            imm: parse_imm::<i32>(node, n)? as i64,
            ..Instr::new(op, ValueType::I32)
        },
        ConstQ8_8 => Instr {
            imm: parse_imm::<i16>(node, n)? as i64,
            ..Instr::new(op, ValueType::Q8_8)
        },
        ConstQ32_32 => Instr {
            imm: parse_imm::<i64>(node, n)?,
            ..Instr::new(op, ValueType::Q32_32)
        },

        Add | Sub | Mul | Div | Min | Max => {
            let (x, y) = (arg(b, 0)?, arg(b, 1)?);
            let t = binary_numeric_type(node, b.ty(x), b.ty(y))?;
            if op == Div && t == ValueType::Q32_32 {
                return Err(RuleError::type_error(
                    node,
                    "Tier-0 规则不支持 Q32_32 除法（请改用 Q8_8 或 Tier-1 沙箱）",
                ));
            }
            let (x, y) = (b.coerce(node, x, t)?, b.coerce(node, y, t)?);
            Instr {
                a: x as u16,
                b: y as u16,
                ..Instr::new(op, t)
            }
        }
        Neg | Abs => {
            let x = arg(b, 0)?;
            let t = b.ty(x);
            if !t.is_numeric() {
                return Err(RuleError::type_error(node, format!("{} 需要数值操作数", op.name())));
            }
            Instr {
                a: x as u16,
                ..Instr::new(op, t)
            }
        }
        Clamp => {
            let (x, lo, hi) = (arg(b, 0)?, arg(b, 1)?, arg(b, 2)?);
            let t = binary_numeric_type(node, b.ty(x), b.ty(lo))?;
            let t = binary_numeric_type(node, t, b.ty(hi))?;
            let (x, lo, hi) = (b.coerce(node, x, t)?, b.coerce(node, lo, t)?, b.coerce(node, hi, t)?);
            Instr {
                a: x as u16,
                b: lo as u16,
                c: hi as u16,
                ..Instr::new(op, t)
            }
        }
        CmpEq | CmpNe | CmpLt | CmpLe | CmpGt | CmpGe => {
            let (x, y) = (arg(b, 0)?, arg(b, 1)?);
            let (tx, ty) = (b.ty(x), b.ty(y));
            let both_bool = tx == ValueType::Bool && ty == ValueType::Bool;
            let (x, y) = if both_bool && matches!(op, CmpEq | CmpNe) {
                (x, y)
            } else {
                let t = binary_numeric_type(node, tx, ty)?;
                (b.coerce(node, x, t)?, b.coerce(node, y, t)?)
            };
            Instr {
                a: x as u16,
                b: y as u16,
                ..Instr::new(op, ValueType::Bool)
            }
        }
        And | Or => {
            let (x, y) = (arg(b, 0)?, arg(b, 1)?);
            if b.ty(x) != ValueType::Bool || b.ty(y) != ValueType::Bool {
                return Err(RuleError::type_error(node, format!("{} 需要 Bool 操作数", op.name())));
            }
            Instr {
                a: x as u16,
                b: y as u16,
                ..Instr::new(op, ValueType::Bool)
            }
        }
        Not => {
            let x = arg(b, 0)?;
            if b.ty(x) != ValueType::Bool {
                return Err(RuleError::type_error(node, "Not 需要 Bool 操作数"));
            }
            Instr {
                a: x as u16,
                ..Instr::new(op, ValueType::Bool)
            }
        }
        Select => {
            let (c, x, y) = (arg(b, 0)?, arg(b, 1)?, arg(b, 2)?);
            if b.ty(c) != ValueType::Bool {
                return Err(RuleError::type_error(node, "Select 的条件必须为 Bool"));
            }
            let (x, y, t) = if b.ty(x) == ValueType::Bool && b.ty(y) == ValueType::Bool {
                (x, y, ValueType::Bool)
            } else {
                let t = binary_numeric_type(node, b.ty(x), b.ty(y))?;
                (b.coerce(node, x, t)?, b.coerce(node, y, t)?, t)
            };
            Instr {
                a: c as u16,
                b: x as u16,
                c: y as u16,
                ..Instr::new(op, t)
            }
        }
        CastI32 | CastQ8_8 | CastQ32_32 => {
            let x = arg(b, 0)?;
            let target = match op {
                CastI32 => ValueType::I32,
                CastQ8_8 => ValueType::Q8_8,
                _ => ValueType::Q32_32,
            };
            let from = b.ty(x);
            let legal = from.is_numeric() || (from == ValueType::Bool && target == ValueType::I32);
            if !legal {
                return Err(RuleError::type_error(
                    node,
                    format!("不能将 {from:?} 转换为 {target:?}"),
                ));
            }
            Instr {
                a: x as u16,
                ..Instr::new(op, target)
            }
        }

        LoadComponent => {
            let slot = b
                .component_slot(node, n)?
                .ok_or_else(|| RuleError::type_error(node, "LoadComponent 读取了未在 reads 中声明的组件"))?;
            let c = &b.decls.components[slot];
            if !c.read {
                return Err(RuleError::type_error(
                    node,
                    format!("组件 `{}` 未在 reads 中声明", c.name),
                ));
            }
            Instr {
                imm: slot as i64,
                ..Instr::new(op, c.ty)
            }
        }
        LoadNeighborComponent => {
            let k = arg(b, 0)?;
            if b.ty(k) != ValueType::I32 {
                return Err(RuleError::type_error(
                    node,
                    "LoadNeighborComponent 的邻居序号必须为 I32",
                ));
            }
            let slot = b
                .component_slot(node, n)?
                .ok_or_else(|| RuleError::type_error(node, "LoadNeighborComponent 读取了未声明的组件"))?;
            let c = &b.decls.components[slot];
            if !c.read {
                return Err(RuleError::type_error(
                    node,
                    format!("组件 `{}` 未在 reads 中声明", c.name),
                ));
            }
            Instr {
                a: k as u16,
                imm: slot as i64,
                ..Instr::new(op, c.ty)
            }
        }
        EmitComponentWrite => {
            let mut v = arg(b, 0)?;
            // 未声明的写入保留 imm = -1，由 Pass 2 报 RuleUndeclaredWriteAccess
            let slot = b.component_slot(node, n)?;
            if let Some(slot) = slot {
                let ty = b.decls.components[slot].ty;
                v = b.coerce(node, v, ty)?;
            }
            Instr {
                a: v as u16,
                imm: slot.map_or(-1, |s| s as i64),
                ..Instr::new(op, ValueType::Void)
            }
        }
        SampleField => Instr {
            imm: b.channel(node, n)? as i64,
            ..Instr::new(op, ValueType::Q8_8)
        },
        EmitFieldDeposit => {
            let channel = b.channel(node, n)?;
            let v = arg(b, 0)?;
            let v = b.coerce(node, v, ValueType::Q8_8)?;
            Instr {
                a: v as u16,
                imm: channel as i64,
                ..Instr::new(op, ValueType::Void)
            }
        }
        NeighborCount => Instr::new(op, ValueType::I32),
        RandomQ8_8 => Instr {
            imm: n.stream.unwrap_or(0) as i64,
            ..Instr::new(op, ValueType::Q8_8)
        },

        BoundedForBegin => {
            let iters = n
                .max_iters
                .ok_or_else(|| RuleError::type_error(node, "BoundedForBegin 缺少 maxIters"))?;
            let id = b.next_loop_id;
            b.next_loop_id += 1;
            let idx = b.instrs.len();
            b.loops.push(LoopFrame {
                id,
                begin: idx,
                phis: Vec::new(),
                body_started: false,
            });
            // 循环下标属于循环体作用域
            let body_scope = b.current_scope();
            let ins = Instr {
                imm: iters as i64,
                ..Instr::new(op, ValueType::I32)
            };
            return b.push(ins, body_scope, node).map(Some);
        }
        Phi => {
            let init = arg(b, 0)?;
            let t = b.ty(init);
            let frame = b
                .loops
                .last()
                .ok_or_else(|| RuleError::type_error(node, "Phi 不在循环内"))?;
            let begin = frame.begin;
            // Phi 自身属于循环的外层作用域：循环结束后其值即最终值
            let outer_scope = scope[..scope.len() - 1].to_vec();
            let ins = Instr {
                a: init as u16,
                imm: begin as i64,
                ..Instr::new(op, t)
            };
            let idx = b.push(ins, outer_scope, node)?;
            if let Some(frame) = b.loops.last_mut() {
                frame.phis.push((idx, n.args[1]));
            }
            return Ok(Some(idx));
        }
        BoundedForEnd => {
            let frame = b
                .loops
                .last()
                .ok_or_else(|| RuleError::type_error(node, "BoundedForEnd 没有配对的 BoundedForBegin"))?;
            let (begin, phis) = (frame.begin, frame.phis.clone());
            // 解析回边：next 必须已定义且在循环体内可见，并提升到 Phi 类型
            for (phi_idx, next_old) in phis {
                let next = b.operand(node, next_old)?;
                let next = b.coerce(node, next, b.ty(phi_idx))?;
                b.instrs[phi_idx].b = next as u16;
            }
            let ins = Instr {
                a: begin as u16,
                ..Instr::new(op, ValueType::Void)
            };
            let idx = b.push(ins, scope, node)?;
            b.loops.pop();
            return Ok(Some(idx));
        }
    };
    b.push(ins, scope, node).map(Some)
}
