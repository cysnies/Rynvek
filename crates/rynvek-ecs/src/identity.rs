// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 三层实体标识（GAP-04 / ADR-001–003）。
//!
//! | 类型 | 布局 | 用途 |
//! | :--- | :--- | :--- |
//! | [`EntityId`] | `u128`：高 64 位谱系前缀 + 低 64 位单调序号 | 持久身份、跨 Fork/快照稳定 |
//! | [`EntityHandle`] | `{ slot: u32, generation: u32 }` | CPU 侧 O(1) 访问与代数校验 |
//! | [`GpuEntityRef`] | `{ dense_index: u32, generation: u32 }` | GPU 紧凑槽位引用 |

use core::fmt;

use bytemuck::{Pod, Zeroable};

/// 持久实体身份：高 64 位为世界谱系前缀，低 64 位为单调序号
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Pod, Zeroable)]
pub struct EntityId(pub u128);

impl EntityId {
    /// 空/无效身份
    pub const INVALID: Self = Self(0);

    /// 由谱系前缀与单调序号组装
    #[inline]
    pub const fn new(lineage_prefix: u64, monotonic_serial: u64) -> Self {
        Self(((lineage_prefix as u128) << 64) | (monotonic_serial as u128))
    }

    /// 由世界种子与 32 位实体序号构造稳定 128 位 `EntityId`
    #[inline]
    pub const fn from_world_serial(world_lineage_prefix: u64, serial_u32: u32) -> Self {
        Self::new(world_lineage_prefix, serial_u32 as u64)
    }

    /// 谱系前缀（高 64 位）
    #[inline]
    pub const fn lineage_prefix(self) -> u64 {
        (self.0 >> 64) as u64
    }

    /// 单调序号（低 64 位）
    #[inline]
    pub const fn monotonic_serial(self) -> u64 {
        self.0 as u64
    }

    /// 若单调序号在 `u32` 范围内则返回对应的 32 位序号（与 RTS `EntitySerial(u32)` 无损互转）
    #[inline]
    pub const fn to_serial_u32(self) -> Option<u32> {
        let s = self.monotonic_serial();
        if s <= u32::MAX as u64 {
            Some(s as u32)
        } else {
            None
        }
    }

    /// 是否为无效身份
    #[inline]
    pub const fn is_invalid(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Debug for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EntityId(lineage={:#x}, serial={})",
            self.lineage_prefix(),
            self.monotonic_serial()
        )
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

/// CPU 侧代数句柄：`slot` 索引稀疏表，`generation` 防止 ABA
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Pod, Zeroable)]
pub struct EntityHandle {
    pub slot: u32,
    pub generation: u32,
}

impl EntityHandle {
    pub const INVALID: Self = Self {
        slot: u32::MAX,
        generation: 0,
    };

    #[inline]
    pub const fn is_invalid(self) -> bool {
        self.slot == u32::MAX
    }
}

/// GPU 侧紧凑引用：指向稠密 SoA 行
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Pod, Zeroable)]
pub struct GpuEntityRef {
    pub dense_index: u32,
    pub generation: u32,
}

impl GpuEntityRef {
    pub const INVALID: Self = Self {
        dense_index: u32::MAX,
        generation: 0,
    };
}

/// 实体生命周期状态（存放于稀疏表）
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityLifecycleState {
    /// 空闲槽位，可被分配
    #[default]
    Free = 0,
    /// 存活
    Alive = 1,
    /// 已销毁、等待两阶段回收（仍占用稀疏槽，防止同 Epoch 复用）
    Tombstoned = 2,
}

/// 墓碑记录：支撑两阶段回收与回放诊断
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityTombstone {
    /// 销毁时的句柄（含当时的 generation）
    pub handle: EntityHandle,
    /// 被销毁实体的稳定身份
    pub stable_id: EntityId,
    /// 销毁发生的权威 Tick
    pub destroyed_at_tick: u64,
    /// 销毁发生的 Coupling Epoch（`tick / 4`）；回收不得早于下一 Epoch
    pub destroyed_at_epoch: u64,
}
