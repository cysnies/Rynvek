// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Backend B：CPU Reference 字节码与解释器（规范 08 §8.1 `emit_cpu_simd_bytecode`）。
//!
//! 字节码每条指令固定 16 字节且按 16 字节对齐（[`BytecodeWord`]），便于宿主以 SIMD 宽度批量加载；
//! 解释器按实体逐一执行，语义见 [`crate::semantics`]，与 WGSL 后端逐条对应。
//!
//! 执行模型（双缓冲）：所有 `LoadComponent` / `LoadNeighborComponent` 读取 Tick 开始时的快照，
//! `EmitComponentWrite` 写入输出列（初值 = 输入列，同一实体多次写入以最后一次为准），
//! 场沉积写入每实体独立的槽位，因此结果与实体执行顺序无关。

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::rng::RandomKey;

use crate::error::{RuleError, RuleResult};
use crate::ir::{FIELD_CHANNEL_NAMES, Instr, Opcode, RuleModule, ValueType};
use crate::semantics;

/// Tier-1 单 Tick Gas 预算（规范 01 §1.1）；CPU 执行 Tier-0 规则时沿用同一预算
pub const DEFAULT_TICK_GAS_BUDGET: u64 = 500_000;
/// 规则所在阶段：`CanonicalPhaseId::ControllerAndRuleEval = 7`（Tier 3 枚举不可被 Tier 2 引用，故冻结数值）
pub const RULE_PHASE_ID: u16 = 7;

/// 16 字节对齐的字节码字
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BytecodeWord(pub [u8; Instr::ENCODED_LEN]);

/// CPU 字节码程序
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignedBytecode {
    pub words: Vec<BytecodeWord>,
    /// 按槽位的组件类型
    pub component_types: Vec<ValueType>,
    pub gas_estimate: u64,
}

/// 由已验证模块生成 CPU 字节码
pub fn emit_cpu_simd_bytecode(module: &RuleModule) -> AlignedBytecode {
    AlignedBytecode {
        words: module.instrs.iter().map(|i| BytecodeWord(i.encode())).collect(),
        component_types: module.components.iter().map(|c| c.ty).collect(),
        gas_estimate: module.gas_estimate,
    }
}

/// 规则执行所需的世界视图（宿主在 Phase 07 前采集）
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleWorldState {
    pub tick: u64,
    pub world_seed: u64,
    /// 每实体的稳定序号（RNG 计数器输入）
    pub entity_serials: Vec<u64>,
    /// 按模块槽位的组件列（原始值），每列长度 = 实体数
    pub components: Vec<Vec<i64>>,
    /// `5 × 实体数`：`[channel * count + entity]` 的 Q8.8 场采样；未使用场时可为空
    pub field_samples: Vec<i64>,
    /// CSR 邻居表：长度 = 实体数 + 1；未使用邻居时可为空
    pub neighbor_offsets: Vec<u32>,
    pub neighbor_indices: Vec<u32>,
}

/// 一个 Tick 的规则输出
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleTickOutput {
    pub components: Vec<Vec<i64>>,
    /// `5 × 实体数` 的 Q8.8 场沉积
    pub field_deposits: Vec<i64>,
    pub gas_used: u64,
}

fn dim_error(msg: &str) -> RuleError {
    RuleError::new(RynvekErrorCode::DimensionMismatch, msg)
}

fn validate(program: &AlignedBytecode, world: &RuleWorldState) -> RuleResult<usize> {
    let count = world.entity_serials.len();
    if world.components.len() != program.component_types.len() || world.components.iter().any(|c| c.len() != count) {
        return Err(dim_error("组件列数量或长度与实体数不一致"));
    }
    let uses = |op: Opcode| program.words.iter().any(|w| w.0[0] == op as u8);
    if (uses(Opcode::SampleField)) && world.field_samples.len() != FIELD_CHANNEL_NAMES.len() * count {
        return Err(dim_error("field_samples 长度必须为 5 × 实体数"));
    }
    if uses(Opcode::NeighborCount) || uses(Opcode::LoadNeighborComponent) {
        if world.neighbor_offsets.len() != count + 1 {
            return Err(dim_error("neighbor_offsets 长度必须为实体数 + 1"));
        }
        let ok = world.neighbor_offsets.windows(2).all(|w| w[0] <= w[1])
            && world
                .neighbor_offsets
                .last()
                .is_some_and(|&e| e as usize <= world.neighbor_indices.len())
            && world.neighbor_indices.iter().all(|&n| (n as usize) < count);
        if !ok {
            return Err(dim_error("CSR 邻居表非法"));
        }
    }
    Ok(count)
}

/// 执行一个 Tick；累计 Gas 超出预算时返回 `SandboxGasBudgetExhausted`，且不产生任何输出
pub fn execute_rule_tick(
    program: &AlignedBytecode,
    world: &RuleWorldState,
    gas_budget: u64,
) -> RuleResult<RuleTickOutput> {
    let count = validate(program, world)?;
    let instrs: Vec<Instr> = program
        .words
        .iter()
        .map(|w| {
            Instr::decode(&w.0).ok_or_else(|| RuleError::new(RynvekErrorCode::InvalidFileMagicOrVersion, "非法字节码"))
        })
        .collect::<RuleResult<_>>()?;

    let mut out_components = world.components.clone();
    for (col, ty) in out_components.iter_mut().zip(&program.component_types) {
        col.iter_mut().for_each(|v| *v = semantics::normalize(*ty, *v));
    }
    let mut deposits = vec![0i64; FIELD_CHANNEL_NAMES.len() * count];
    let mut gas = 0u64;
    let mut values = vec![0i64; instrs.len()];

    for e in 0..count {
        run_entity(
            &instrs,
            program,
            world,
            e,
            &mut values,
            &mut out_components,
            &mut deposits,
            &mut gas,
            gas_budget,
        )?;
    }
    Ok(RuleTickOutput {
        components: out_components,
        field_deposits: deposits,
        gas_used: gas,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_entity(
    instrs: &[Instr],
    program: &AlignedBytecode,
    world: &RuleWorldState,
    e: usize,
    values: &mut [i64],
    out_components: &mut [Vec<i64>],
    deposits: &mut [i64],
    gas: &mut u64,
    gas_budget: u64,
) -> RuleResult<()> {
    use Opcode::*;
    let count = world.entity_serials.len();
    let load = |slot: usize, entity: usize| {
        semantics::normalize(program.component_types[slot], world.components[slot][entity])
    };
    let mut rng_counter = 0u32;
    let mut pc = 0usize;

    while pc < instrs.len() {
        let ins = instrs[pc];
        *gas += ins.op.gas_cost();
        if *gas > gas_budget {
            return Err(RuleError::at(
                RynvekErrorCode::SandboxGasBudgetExhausted,
                pc,
                format!("单 Tick Gas 预算 {gas_budget} 已耗尽"),
            ));
        }
        let (a, b, c) = (values[ins.a as usize], values[ins.b as usize], values[ins.c as usize]);
        let result = match ins.op {
            ConstBool | ConstI32 | ConstQ8_8 | ConstQ32_32 => ins.imm,
            Add | Sub | Mul | Div | Min | Max => semantics::binary(ins.op, ins.ty, a, b),
            Neg | Abs => semantics::unary(ins.op, ins.ty, a),
            Clamp => semantics::clamp(a, b, c),
            CmpEq | CmpNe | CmpLt | CmpLe | CmpGt | CmpGe => semantics::compare(ins.op, a, b),
            And => a & b,
            Or => a | b,
            Not => 1 - a,
            Select => {
                if a != 0 {
                    b
                } else {
                    c
                }
            }
            CastI32 | CastQ8_8 | CastQ32_32 => semantics::cast(instrs[ins.a as usize].ty, ins.ty, a),
            LoadComponent => load(ins.imm as usize, e),
            EmitComponentWrite => {
                out_components[ins.imm as usize][e] = a;
                0
            }
            SampleField => semantics::normalize(ValueType::Q8_8, world.field_samples[ins.imm as usize * count + e]),
            EmitFieldDeposit => {
                let slot = ins.imm as usize * count + e;
                deposits[slot] = semantics::deposit_add(deposits[slot], a);
                0
            }
            NeighborCount => (world.neighbor_offsets[e + 1] - world.neighbor_offsets[e]) as i64,
            LoadNeighborComponent => {
                let start = world.neighbor_offsets[e] as i64;
                let n = world.neighbor_offsets[e + 1] as i64 - start;
                if (0..n).contains(&a) {
                    load(ins.imm as usize, world.neighbor_indices[(start + a) as usize] as usize)
                } else {
                    0
                }
            }
            RandomQ8_8 => {
                let key = RandomKey {
                    world_seed: world.world_seed,
                    tick: world.tick,
                    entity_serial: world.entity_serials[e],
                    stream_and_phase: RandomKey::pack_stream_and_phase(ins.imm as u16, RULE_PHASE_ID),
                    sub_counter: rng_counter,
                };
                rng_counter = rng_counter.wrapping_add(1);
                semantics::random_q8_from_u32(key.generate_u32x4()[0])
            }
            BoundedForBegin => 0,
            Phi => a,
            BoundedForEnd => {
                let begin = ins.a as usize;
                let phis: Vec<usize> = (begin + 1..pc).take_while(|&i| instrs[i].op == Phi).collect();
                // 先读出全部回边值再统一赋值，避免 Phi 之间互相引用时的顺序依赖
                let nexts: Vec<i64> = phis.iter().map(|&p| values[instrs[p].b as usize]).collect();
                for (&p, v) in phis.iter().zip(nexts) {
                    values[p] = v;
                }
                values[begin] += 1;
                if values[begin] < instrs[begin].imm {
                    pc = begin + 1 + phis.len();
                    continue;
                }
                0
            }
        };
        values[pc] = result;
        pc += 1;
    }
    Ok(())
}
