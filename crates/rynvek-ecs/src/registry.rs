// crates/rynvek-ecs/src/registry.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 实体注册表：稀疏代数表 + 紧凑稠密下标（规范 04 §4.1）。
//!
//! ## 两阶段 Tombstone 回收
//!
//! 1. **阶段 A（销毁）**：将稀疏表状态置为 `Tombstoned`，从稠密数组 swap-remove，
//!    写入 [`EntityTombstone`]；**本 Epoch 内不复用该 slot**。
//! 2. **阶段 B（回收）**：在 `recycle_tombstones(current_epoch)` 中，仅回收
//!    `destroyed_at_epoch < current_epoch` 的墓碑：generation 加一、状态改 `Free`、
//!    slot 进入 FIFO 空闲队列。
//!
//! ## 关于 `Box<[SparsePageEntry; PAGE_CAPACITY]>`
//!
//! 规范写法在禁止 `unsafe` 时无法在堆上直接构造巨大定长数组（栈上构造会溢出）。
//! 本实现改为 `Box<[SparsePageEntry]>`，并在构造时断言长度恰为 [`PAGE_CAPACITY`]（见 ERRATA E-003）。

use std::collections::VecDeque;

use rynvek_core::error::RynvekErrorCode;

use crate::identity::{EntityHandle, EntityId, EntityLifecycleState, EntityTombstone, GpuEntityRef};

/// 稀疏表单页容量：65,536 槽
pub const PAGE_CAPACITY: usize = 65_536;
/// SoA 列在页内的分块容量：4,096（规范 04）
pub const CHUNK_CAPACITY: usize = 4_096;

/// 稀疏代数表条目
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SparsePageEntry {
    pub stable_id: EntityId,
    pub dense_page_id: u16,
    pub dense_slot_in_page: u16,
    pub generation: u32,
    pub state: EntityLifecycleState,
    pub archetype_id: u16,
}

impl SparsePageEntry {
    pub const EMPTY: Self = Self {
        stable_id: EntityId::INVALID,
        dense_page_id: 0,
        dense_slot_in_page: 0,
        generation: 0,
        state: EntityLifecycleState::Free,
        archetype_id: 0,
    };

    /// 稠密线性下标（`dense_page_id * PAGE 内编码` 由注册表维护的 dense 数组下标）
    #[inline]
    pub const fn dense_index_hint(self) -> u32 {
        ((self.dense_page_id as u32) << 16) | (self.dense_slot_in_page as u32)
    }
}

/// 实体注册表
#[derive(Debug)]
pub struct EntityRegistry {
    pub world_lineage_prefix: u64,
    pub next_monotonic_serial: u64,
    /// 按 65,536 槽位分页的稀疏代数表
    pub sparse_pages: Vec<Box<[SparsePageEntry]>>,
    /// 紧凑下标 → sparse slot（Swap-Remove 时 O(1) 回修）
    pub dense_to_sparse_slot: Vec<u32>,
    /// 延迟复用空闲队列（FIFO）
    pub free_slots_fifo: VecDeque<u32>,
    pub tombstones: Vec<EntityTombstone>,
    /// 下一个从未使用过的稀疏槽位（高水位）
    next_fresh_slot: u32,
}

impl EntityRegistry {
    /// 创建空注册表
    pub fn new(world_lineage_prefix: u64) -> Self {
        Self {
            world_lineage_prefix,
            // 序号从 1 起，保留 0 作为 Invalid
            next_monotonic_serial: 1,
            sparse_pages: Vec::new(),
            dense_to_sparse_slot: Vec::new(),
            free_slots_fifo: VecDeque::new(),
            tombstones: Vec::new(),
            next_fresh_slot: 0,
        }
    }

    /// 当前存活实体数（= 稠密数组长度）
    pub fn alive_count(&self) -> usize {
        self.dense_to_sparse_slot.len()
    }

    /// 墓碑数量（待回收）
    pub fn tombstone_count(&self) -> usize {
        self.tombstones.len()
    }

    /// 分配一个新实体，返回句柄与稳定 ID
    pub fn spawn(&mut self, archetype_id: u16) -> Result<(EntityHandle, EntityId), RynvekErrorCode> {
        let slot = self.allocate_slot()?;
        let serial = self.next_monotonic_serial;
        self.next_monotonic_serial = self
            .next_monotonic_serial
            .checked_add(1)
            .ok_or(RynvekErrorCode::WorldMemoryBudgetExceeded)?;
        let stable_id = EntityId::new(self.world_lineage_prefix, serial);

        let dense_index = self.dense_to_sparse_slot.len() as u32;
        self.dense_to_sparse_slot.push(slot);

        let entry = self.sparse_entry_mut(slot)?;
        // 复用槽位时 generation 已在回收阶段递增；新槽位 generation 从 1 开始
        if entry.generation == 0 {
            entry.generation = 1;
        }
        entry.stable_id = stable_id;
        entry.state = EntityLifecycleState::Alive;
        entry.archetype_id = archetype_id;
        entry.dense_page_id = (dense_index >> 16) as u16;
        entry.dense_slot_in_page = dense_index as u16;

        let handle = EntityHandle {
            slot,
            generation: entry.generation,
        };
        Ok((handle, stable_id))
    }

    /// 校验句柄并返回稳定 ID
    pub fn resolve(&self, handle: EntityHandle) -> Result<EntityId, RynvekErrorCode> {
        let entry = self.sparse_entry(handle.slot)?;
        if entry.state != EntityLifecycleState::Alive || entry.generation != handle.generation {
            return Err(if entry.generation != handle.generation {
                RynvekErrorCode::StaleEntityHandleGeneration
            } else {
                RynvekErrorCode::EntityNotFoundOrTombstoned
            });
        }
        Ok(entry.stable_id)
    }

    /// 由句柄得到 GPU 紧凑引用
    pub fn gpu_ref(&self, handle: EntityHandle) -> Result<GpuEntityRef, RynvekErrorCode> {
        let entry = self.sparse_entry(handle.slot)?;
        if entry.state != EntityLifecycleState::Alive || entry.generation != handle.generation {
            return Err(RynvekErrorCode::StaleEntityHandleGeneration);
        }
        Ok(GpuEntityRef {
            dense_index: Self::pack_dense(entry.dense_page_id, entry.dense_slot_in_page),
            generation: entry.generation,
        })
    }

    /// 读取稠密下标对应的句柄
    pub fn handle_at_dense(&self, dense_index: u32) -> Result<EntityHandle, RynvekErrorCode> {
        let slot = *self
            .dense_to_sparse_slot
            .get(dense_index as usize)
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)?;
        let entry = self.sparse_entry(slot)?;
        Ok(EntityHandle {
            slot,
            generation: entry.generation,
        })
    }

    /// 阶段 A：销毁实体（tombstone），立即从稠密数组移除
    pub fn despawn(&mut self, handle: EntityHandle, tick: u64) -> Result<EntityId, RynvekErrorCode> {
        let entry = *self.sparse_entry(handle.slot)?;
        if entry.state != EntityLifecycleState::Alive || entry.generation != handle.generation {
            return Err(RynvekErrorCode::StaleEntityHandleGeneration);
        }

        let dense_index = Self::pack_dense(entry.dense_page_id, entry.dense_slot_in_page) as usize;
        self.swap_remove_dense(dense_index)?;

        let sparse = self.sparse_entry_mut(handle.slot)?;
        sparse.state = EntityLifecycleState::Tombstoned;
        sparse.dense_page_id = 0;
        sparse.dense_slot_in_page = 0;
        // 保留 generation 与 stable_id，直到阶段 B

        let epoch = tick / 4; // Coupling Epoch：与规范 COUPLING_EPOCH_TICKS=4 对齐
        self.tombstones.push(EntityTombstone {
            handle,
            stable_id: entry.stable_id,
            destroyed_at_tick: tick,
            destroyed_at_epoch: epoch,
        });
        Ok(entry.stable_id)
    }

    /// 阶段 B：回收所有 `destroyed_at_epoch < current_epoch` 的墓碑
    pub fn recycle_tombstones(&mut self, current_epoch: u64) -> usize {
        let mut recycled = 0usize;
        let mut remaining = Vec::with_capacity(self.tombstones.len());
        let mut freed_slots = Vec::new();
        let pending = std::mem::take(&mut self.tombstones);
        for tomb in pending {
            if tomb.destroyed_at_epoch < current_epoch {
                let slot = tomb.handle.slot;
                let generation = tomb.handle.generation;
                if let Ok(entry) = self.sparse_entry_mut(slot) {
                    if entry.state == EntityLifecycleState::Tombstoned && entry.generation == generation {
                        entry.state = EntityLifecycleState::Free;
                        entry.stable_id = EntityId::INVALID;
                        entry.archetype_id = 0;
                        entry.generation = entry.generation.wrapping_add(1);
                        if entry.generation == 0 {
                            // 跳过 0，保留「未初始化」哨兵
                            entry.generation = 1;
                        }
                        freed_slots.push(slot);
                        recycled += 1;
                        continue;
                    }
                }
            }
            remaining.push(tomb);
        }
        self.tombstones = remaining;
        self.free_slots_fifo.extend(freed_slots);
        recycled
    }

    /// 遍历所有存活实体的 (dense_index, handle, EntityId)
    pub fn iter_alive(&self) -> impl Iterator<Item = (u32, EntityHandle, EntityId)> + '_ {
        self.dense_to_sparse_slot
            .iter()
            .enumerate()
            .filter_map(|(dense, &slot)| {
                let entry = self.sparse_entry(slot).ok()?;
                if entry.state != EntityLifecycleState::Alive {
                    return None;
                }
                Some((
                    dense as u32,
                    EntityHandle {
                        slot,
                        generation: entry.generation,
                    },
                    entry.stable_id,
                ))
            })
    }

    /// 计算存活实体集合的确定性状态根（按 EntityId 排序后 BLAKE3）
    pub fn state_root_blake3(&self) -> [u8; 32] {
        let mut rows: Vec<(EntityId, u32, u16)> = self
            .iter_alive()
            .map(|(_, handle, id)| {
                let arch = self.sparse_entry(handle.slot).map(|e| e.archetype_id).unwrap_or(0);
                (id, handle.generation, arch)
            })
            .collect();
        rows.sort_by_key(|(id, _, _)| *id);
        let mut hasher = blake3::Hasher::new();
        for (id, generation, archetype_id) in rows {
            hasher.update(&id.0.to_le_bytes());
            hasher.update(&generation.to_le_bytes());
            hasher.update(&archetype_id.to_le_bytes());
        }
        *hasher.finalize().as_bytes()
    }

    fn allocate_slot(&mut self) -> Result<u32, RynvekErrorCode> {
        if let Some(slot) = self.free_slots_fifo.pop_front() {
            return Ok(slot);
        }
        let slot = self.next_fresh_slot;
        self.next_fresh_slot = self
            .next_fresh_slot
            .checked_add(1)
            .ok_or(RynvekErrorCode::WorldMemoryBudgetExceeded)?;
        self.ensure_sparse_page(slot)?;
        Ok(slot)
    }

    fn ensure_sparse_page(&mut self, slot: u32) -> Result<(), RynvekErrorCode> {
        let page_index = (slot as usize) / PAGE_CAPACITY;
        while self.sparse_pages.len() <= page_index {
            let page = vec![SparsePageEntry::EMPTY; PAGE_CAPACITY].into_boxed_slice();
            debug_assert_eq!(page.len(), PAGE_CAPACITY);
            self.sparse_pages.push(page);
        }
        Ok(())
    }

    fn sparse_entry(&self, slot: u32) -> Result<&SparsePageEntry, RynvekErrorCode> {
        let page_index = (slot as usize) / PAGE_CAPACITY;
        let offset = (slot as usize) % PAGE_CAPACITY;
        self.sparse_pages
            .get(page_index)
            .and_then(|page| page.get(offset))
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)
    }

    fn sparse_entry_mut(&mut self, slot: u32) -> Result<&mut SparsePageEntry, RynvekErrorCode> {
        let page_index = (slot as usize) / PAGE_CAPACITY;
        let offset = (slot as usize) % PAGE_CAPACITY;
        self.sparse_pages
            .get_mut(page_index)
            .and_then(|page| page.get_mut(offset))
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)
    }

    fn swap_remove_dense(&mut self, dense_index: usize) -> Result<(), RynvekErrorCode> {
        let last = self.dense_to_sparse_slot.len().saturating_sub(1);
        if dense_index > last {
            return Err(RynvekErrorCode::EntityNotFoundOrTombstoned);
        }
        let removed_slot = self.dense_to_sparse_slot.swap_remove(dense_index);
        let _ = removed_slot;
        if dense_index < self.dense_to_sparse_slot.len() {
            // 被换入的尾元素需要回写其稀疏表中的稠密坐标
            let moved_slot = self.dense_to_sparse_slot[dense_index];
            let entry = self.sparse_entry_mut(moved_slot)?;
            entry.dense_page_id = (dense_index as u32 >> 16) as u16;
            entry.dense_slot_in_page = dense_index as u16;
        }
        Ok(())
    }

    #[inline]
    const fn pack_dense(page: u16, slot_in_page: u16) -> u32 {
        ((page as u32) << 16) | (slot_in_page as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_resolve_despawn_recycle_cycle() {
        let mut reg = EntityRegistry::new(0xA11CE);
        let (h1, id1) = reg.spawn(7).unwrap();
        assert_eq!(reg.resolve(h1).unwrap(), id1);
        assert_eq!(reg.alive_count(), 1);

        reg.despawn(h1, 4).unwrap(); // epoch = 1
        assert_eq!(reg.alive_count(), 0);
        assert_eq!(reg.tombstone_count(), 1);
        assert_eq!(reg.resolve(h1), Err(RynvekErrorCode::EntityNotFoundOrTombstoned));

        // 同一 epoch 不得回收
        assert_eq!(reg.recycle_tombstones(1), 0);
        assert_eq!(reg.tombstone_count(), 1);

        // 下一 epoch 回收
        assert_eq!(reg.recycle_tombstones(2), 1);
        assert_eq!(reg.free_slots_fifo.len(), 1);

        let (h2, _) = reg.spawn(1).unwrap();
        assert_eq!(h2.slot, h1.slot);
        assert_ne!(h2.generation, h1.generation);
        assert_eq!(reg.resolve(h1), Err(RynvekErrorCode::StaleEntityHandleGeneration));
    }

    #[test]
    fn swap_remove_repairs_dense_index() {
        let mut reg = EntityRegistry::new(1);
        let (h0, _) = reg.spawn(0).unwrap();
        let (h1, _) = reg.spawn(0).unwrap();
        let (h2, _) = reg.spawn(0).unwrap();
        reg.despawn(h0, 0).unwrap();
        // h2 应被换到 dense 0
        let gpu = reg.gpu_ref(h2).unwrap();
        assert_eq!(gpu.dense_index, 0);
        assert_eq!(reg.handle_at_dense(0).unwrap(), h2);
        assert_eq!(reg.handle_at_dense(1).unwrap(), h1);
    }

    #[test]
    fn one_hundred_thousand_entity_churn() {
        let mut reg = EntityRegistry::new(42);
        let mut handles = Vec::with_capacity(100_000);
        for i in 0..100_000u32 {
            let (h, _) = reg.spawn((i % 16) as u16).unwrap();
            handles.push(h);
        }
        assert_eq!(reg.alive_count(), 100_000);
        let root_before = reg.state_root_blake3();

        // 删除偶数，修改奇数 archetype（通过 despawn+spawn 模拟「改」）
        for (i, h) in handles.iter().enumerate() {
            if i % 2 == 0 {
                reg.despawn(*h, 8).unwrap(); // epoch 2
            }
        }
        assert_eq!(reg.alive_count(), 50_000);
        assert_eq!(reg.recycle_tombstones(3), 50_000);

        // 再生成 50_000，总数回到 100_000
        for i in 0..50_000u32 {
            reg.spawn((i % 8) as u16).unwrap();
        }
        assert_eq!(reg.alive_count(), 100_000);
        let root_after = reg.state_root_blake3();
        assert_ne!(root_before, root_after);
        // 状态根应稳定可复算
        assert_eq!(root_after, reg.state_root_blake3());
    }
}
