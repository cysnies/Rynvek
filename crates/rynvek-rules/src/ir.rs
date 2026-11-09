// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `Typed SSA Rule IR`：36 条指令、值类型与已验证规则模块（规范 08 §8.1）。
//!
//! 规范冻结了「36 条指令 / `.rir` 魔数 `RYIR` / 四阶段管线」，但未列出指令表本身；
//! 本模块按最佳实践冻结如下指令集（见 ERRATA E-010），编号即 `.rir` 中的 `u8` 操作码：
//!
//! | 编号 | 类别 | 指令 |
//! | :-- | :-- | :-- |
//! | 0–3 | 常量 | `ConstBool` `ConstI32` `ConstQ8_8` `ConstQ32_32` |
//! | 4–12 | 算术 | `Add` `Sub` `Mul` `Div` `Neg` `Abs` `Min` `Max` `Clamp` |
//! | 13–18 | 比较 | `CmpEq` `CmpNe` `CmpLt` `CmpLe` `CmpGt` `CmpGe` |
//! | 19–22 | 逻辑 | `And` `Or` `Not` `Select` |
//! | 23–25 | 转换 | `CastI32` `CastQ8_8` `CastQ32_32` |
//! | 26–31 | 世界访问 | `LoadComponent` `EmitComponentWrite` `SampleField` `EmitFieldDeposit` `NeighborCount` `LoadNeighborComponent` |
//! | 32 | 随机 | `RandomQ8_8` |
//! | 33–35 | 控制流 | `BoundedForBegin` `BoundedForEnd` `Phi` |
//!
//! SSA 约定：每条指令的下标即其结果值编号；操作数只能引用更早的指令，
//! 唯一例外是 `Phi` 的回边操作数（`b`）可引用同一循环体内更晚的指令。

use serde::{Deserialize, Serialize};

/// IR 值类型
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ValueType {
    /// 无结果（写入 / 循环结束）
    Void = 0,
    Bool = 1,
    I32 = 2,
    /// 原始值为 `i16`
    #[serde(rename = "Q8_8")]
    Q8_8 = 3,
    /// 原始值为 `i64`
    #[serde(rename = "Q32_32")]
    Q32_32 = 4,
}

impl ValueType {
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Void),
            1 => Some(Self::Bool),
            2 => Some(Self::I32),
            3 => Some(Self::Q8_8),
            4 => Some(Self::Q32_32),
            _ => None,
        }
    }

    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::I32 | Self::Q8_8 | Self::Q32_32)
    }

    /// 定点提升次序：`I32 < Q8_8 < Q32_32`（混合运算向更高者提升）
    pub const fn promotion_rank(self) -> u8 {
        match self {
            Self::I32 => 1,
            Self::Q8_8 => 2,
            Self::Q32_32 => 3,
            Self::Void | Self::Bool => 0,
        }
    }
}

macro_rules! define_opcodes {
    ($($name:ident = $val:expr),* $(,)?) => {
        /// 36 条 `Typed SSA Rule IR` 指令
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Opcode {
            $($name = $val,)*
        }

        impl Opcode {
            /// 全部指令（按编号升序）
            pub const ALL: [Opcode; 36] = [$(Opcode::$name,)*];

            pub const fn from_u8(v: u8) -> Option<Self> {
                match v {
                    $($val => Some(Opcode::$name),)*
                    _ => None,
                }
            }

            /// 规范名（同时是规则图 JSON 中 `op` 字段的取值）
            pub const fn name(self) -> &'static str {
                match self {
                    $(Opcode::$name => stringify!($name),)*
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $(stringify!($name) => Some(Opcode::$name),)*
                    _ => None,
                }
            }
        }
    };
}

define_opcodes! {
    ConstBool = 0,
    ConstI32 = 1,
    ConstQ8_8 = 2,
    ConstQ32_32 = 3,
    Add = 4,
    Sub = 5,
    Mul = 6,
    Div = 7,
    Neg = 8,
    Abs = 9,
    Min = 10,
    Max = 11,
    Clamp = 12,
    CmpEq = 13,
    CmpNe = 14,
    CmpLt = 15,
    CmpLe = 16,
    CmpGt = 17,
    CmpGe = 18,
    And = 19,
    Or = 20,
    Not = 21,
    Select = 22,
    CastI32 = 23,
    CastQ8_8 = 24,
    CastQ32_32 = 25,
    LoadComponent = 26,
    EmitComponentWrite = 27,
    SampleField = 28,
    EmitFieldDeposit = 29,
    NeighborCount = 30,
    LoadNeighborComponent = 31,
    RandomQ8_8 = 32,
    BoundedForBegin = 33,
    BoundedForEnd = 34,
    Phi = 35,
}

impl Opcode {
    /// 操作数个数
    pub const fn arity(self) -> usize {
        use Opcode::*;
        match self {
            ConstBool | ConstI32 | ConstQ8_8 | ConstQ32_32 | LoadComponent | SampleField | NeighborCount
            | RandomQ8_8 | BoundedForBegin | BoundedForEnd => 0,
            Neg
            | Abs
            | Not
            | CastI32
            | CastQ8_8
            | CastQ32_32
            | EmitComponentWrite
            | EmitFieldDeposit
            | LoadNeighborComponent => 1,
            Add | Sub | Mul | Div | Min | Max | CmpEq | CmpNe | CmpLt | CmpLe | CmpGt | CmpGe | And | Or | Phi => 2,
            Clamp | Select => 3,
        }
    }

    /// 单次执行的 Gas 成本（CPU 计量与静态估算共用）
    pub const fn gas_cost(self) -> u64 {
        use Opcode::*;
        match self {
            Phi => 0,
            ConstBool | ConstI32 | ConstQ8_8 | ConstQ32_32 | Add | Sub | Neg | Abs | Min | Max | CmpEq | CmpNe
            | CmpLt | CmpLe | CmpGt | CmpGe | And | Or | Not | Select | CastI32 | CastQ8_8 | CastQ32_32
            | BoundedForBegin | BoundedForEnd => 1,
            Mul | Clamp | LoadComponent | SampleField | NeighborCount => 2,
            EmitComponentWrite | EmitFieldDeposit | LoadNeighborComponent => 3,
            Div => 4,
            RandomQ8_8 => 8,
        }
    }
}

/// 一条已类型化的 IR 指令（`.rir` 与 CPU 字节码中固定编码为 16 字节）
///
/// `imm` 的含义随指令而定：常量原始值 / 组件槽位 / 场通道 / 随机流 / 循环上界 / 所属循环起点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instr {
    pub op: Opcode,
    pub ty: ValueType,
    pub a: u16,
    pub b: u16,
    pub c: u16,
    pub imm: i64,
}

impl Instr {
    pub const ENCODED_LEN: usize = 16;

    pub const fn new(op: Opcode, ty: ValueType) -> Self {
        Self {
            op,
            ty,
            a: 0,
            b: 0,
            c: 0,
            imm: 0,
        }
    }

    /// 按元数返回操作数
    pub fn operands(&self) -> Vec<u16> {
        [self.a, self.b, self.c][..self.op.arity()].to_vec()
    }

    pub fn encode(&self) -> [u8; Self::ENCODED_LEN] {
        let mut out = [0u8; Self::ENCODED_LEN];
        out[0] = self.op as u8;
        out[1] = self.ty as u8;
        out[2..4].copy_from_slice(&self.a.to_le_bytes());
        out[4..6].copy_from_slice(&self.b.to_le_bytes());
        out[6..8].copy_from_slice(&self.c.to_le_bytes());
        out[8..16].copy_from_slice(&self.imm.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8; Self::ENCODED_LEN]) -> Option<Self> {
        Some(Self {
            op: Opcode::from_u8(bytes[0])?,
            ty: ValueType::from_u8(bytes[1])?,
            a: u16::from_le_bytes([bytes[2], bytes[3]]),
            b: u16::from_le_bytes([bytes[4], bytes[5]]),
            c: u16::from_le_bytes([bytes[6], bytes[7]]),
            imm: i64::from_le_bytes(bytes[8..16].try_into().ok()?),
        })
    }
}

/// 组件字段的访问权限
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentSlot {
    /// 形如 `energy.value`
    pub name: String,
    pub ty: ValueType,
    pub read: bool,
    pub write: bool,
}

/// 标准五通道环境场名称（与 `rynvek-field` 的 `FieldChannelId` 顺序一致；Tier 2 间不互相依赖，故在此重复冻结）
pub const FIELD_CHANNEL_NAMES: [&str; 5] = ["nutrient", "energy", "signal_a", "signal_b", "hazard"];

/// 规则图中声明场写入时使用的组件前缀
pub const FIELD_WRITE_PREFIX: &str = "field.";

pub fn field_channel_index(name: &str) -> Option<usize> {
    FIELD_CHANNEL_NAMES.iter().position(|&c| c == name)
}

/// 经四阶段管线验证后的规则模块
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleModule {
    pub name: String,
    /// 按名称升序排列；槽位号即下标
    pub components: Vec<ComponentSlot>,
    /// 声明了写权限的场通道位掩码（bit i = `FIELD_CHANNEL_NAMES[i]`）
    pub field_write_mask: u8,
    pub instrs: Vec<Instr>,
    /// 单实体单次调用的静态 Gas 上界（循环按 `max_iters` 展开计）
    pub gas_estimate: u64,
}

impl RuleModule {
    pub fn component_slot(&self, name: &str) -> Option<usize> {
        self.components.iter().position(|c| c.name == name)
    }

    /// 是否用到了某类指令
    pub fn uses(&self, op: Opcode) -> bool {
        self.instrs.iter().any(|i| i.op == op)
    }
}
