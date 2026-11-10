// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 分页列式存储（Paged SoA，规范 04 / ADR-004–005 / 审计报告 §6.1）：
//!
//! - [`DomainPage`]：容量 [`PAGE_CAPACITY`](crate::PAGE_CAPACITY) 的逻辑页（`len: u32` 修复满页 65,536 溢出）；
//! - [`PagedComponentColumn<T>`]：真正按 [`CHUNK_CAPACITY`](crate::CHUNK_CAPACITY)（4,096 行/页）分页分配的 SoA 组件列，避免单 `Vec<T>` 扩容全表拷贝；
//! - [`ComponentColumn<T>`]：与稠密下标对齐的兼容列封装；
//! - [`StructuralMutationBuffer`]：跨阶段延迟结构变更缓冲（收集 Spawn / Despawn 请求并在 Phase 07 确定性批量提交）。

use rynvek_core::error::RynvekErrorCode;
use rynvek_memory::MemoryBudget;

use crate::identity::{EntityHandle, EntityId};
use crate::registry::{CHUNK_CAPACITY, EntityRegistry, PAGE_CAPACITY};

/// 一页实体的元信息（与稀疏表页对应的稠密侧视图）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainPage {
    /// 页编号
    pub page_id: u16,
    /// 本页已占用的稠密槽位数（`0..=PAGE_CAPACITY`，采用 `u32` 容纳满页 `65,536`）
    pub len: u32,
}

impl DomainPage {
    pub const fn new(page_id: u16) -> Self {
        Self { page_id, len: 0 }
    }

    pub const fn with_len(page_id: u16, len: u32) -> Self {
        Self { page_id, len }
    }

    pub const fn capacity(&self) -> usize {
        PAGE_CAPACITY
    }

    pub const fn chunk_capacity(&self) -> usize {
        CHUNK_CAPACITY
    }

    pub const fn is_full(&self) -> bool {
        self.len as usize >= PAGE_CAPACITY
    }
}

/// 真正按 `CHUNK_CAPACITY`（4,096 元素/块）分页的 SoA 组件列
#[derive(Clone, Debug)]
pub struct PagedComponentColumn<T> {
    chunks: Vec<Vec<T>>,
    len: usize,
}

impl<T> Default for PagedComponentColumn<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> PagedComponentColumn<T> {
    pub const fn new() -> Self {
        Self {
            chunks: Vec::new(),
            len: 0,
        }
    }

    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 当前已分配的 4,096 容量分块数
    #[inline]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// 当前已分配分块占用的总字节数
    pub fn allocated_bytes(&self) -> u64 {
        (self.chunks.len() * CHUNK_CAPACITY * core::mem::size_of::<T>()) as u64
    }

    /// 追加一个组件值（若跨入新分块则按 `CHUNK_CAPACITY` 预分配）
    pub fn push(&mut self, value: T) {
        let chunk_idx = self.len / CHUNK_CAPACITY;
        if chunk_idx >= self.chunks.len() {
            let mut chunk = Vec::with_capacity(CHUNK_CAPACITY);
            chunk.push(value);
            self.chunks.push(chunk);
        } else {
            self.chunks[chunk_idx].push(value);
        }
        self.len += 1;
    }

    /// 带内存预算检查的追加：若需新分配 4,096 容量分块则先向 `budget` 申请记账
    pub fn push_with_budget(&mut self, value: T, budget: &mut MemoryBudget) -> Result<(), RynvekErrorCode> {
        let chunk_idx = self.len / CHUNK_CAPACITY;
        if chunk_idx >= self.chunks.len() {
            let chunk_bytes = (CHUNK_CAPACITY * core::mem::size_of::<T>()) as u64;
            if chunk_bytes > 0 {
                budget.commit_allocation(chunk_bytes)?;
            }
            let mut chunk = Vec::with_capacity(CHUNK_CAPACITY);
            chunk.push(value);
            self.chunks.push(chunk);
        } else {
            self.chunks[chunk_idx].push(value);
        }
        self.len += 1;
        Ok(())
    }

    #[inline]
    pub fn get(&self, dense_index: u32) -> Result<&T, RynvekErrorCode> {
        let idx = dense_index as usize;
        if idx >= self.len {
            return Err(RynvekErrorCode::ArchetypeComponentMissing);
        }
        Ok(&self.chunks[idx / CHUNK_CAPACITY][idx % CHUNK_CAPACITY])
    }

    #[inline]
    pub fn get_mut(&mut self, dense_index: u32) -> Result<&mut T, RynvekErrorCode> {
        let idx = dense_index as usize;
        if idx >= self.len {
            return Err(RynvekErrorCode::ArchetypeComponentMissing);
        }
        Ok(&mut self.chunks[idx / CHUNK_CAPACITY][idx % CHUNK_CAPACITY])
    }

    /// 获取指定分块的连续切片（供 SIMD / CPU 分块内核零拷贝遍历）
    pub fn chunk_slice(&self, chunk_index: usize) -> Option<&[T]> {
        self.chunks.get(chunk_index).map(|c| c.as_slice())
    }

    /// 获取指定分块的可变连续切片
    pub fn chunk_slice_mut(&mut self, chunk_index: usize) -> Option<&mut [T]> {
        self.chunks.get_mut(chunk_index).map(|c| c.as_mut_slice())
    }

    /// 按稠密顺序顺序遍历全部组件
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.chunks.iter().flat_map(|c| c.iter())
    }

    /// 按稠密顺序可变遍历全部组件
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.chunks.iter_mut().flat_map(|c| c.iter_mut())
    }

    /// 与 `EntityRegistry::despawn` 的 swap-remove 严格对齐的 O(1) 删除
    pub fn swap_remove(&mut self, dense_index: u32) -> Result<T, RynvekErrorCode> {
        let idx = dense_index as usize;
        if idx >= self.len {
            return Err(RynvekErrorCode::ArchetypeComponentMissing);
        }
        let last_idx = self.len - 1;
        let last_chunk = last_idx / CHUNK_CAPACITY;
        let last_val = self.chunks[last_chunk]
            .pop()
            .expect("非空分块尾部必有元素");
        if self.chunks[last_chunk].is_empty() {
            self.chunks.pop();
        }
        self.len -= 1;
        if idx == last_idx {
            return Ok(last_val);
        }
        let target_chunk = idx / CHUNK_CAPACITY;
        let target_off = idx % CHUNK_CAPACITY;
        let removed = core::mem::replace(&mut self.chunks[target_chunk][target_off], last_val);
        Ok(removed)
    }
}

/// 与稠密实体下标对齐的组件列（保持连续切片视图的同时支持分页导出）
#[derive(Clone, Debug, Default)]
pub struct ComponentColumn<T> {
    values: Vec<T>,
}

impl<T> ComponentColumn<T> {
    pub fn new() -> Self {
        Self { values: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn as_slice(&self) -> &[T] {
        &self.values
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.values
    }

    pub fn get(&self, dense_index: u32) -> Result<&T, RynvekErrorCode> {
        self.values
            .get(dense_index as usize)
            .ok_or(RynvekErrorCode::ArchetypeComponentMissing)
    }

    pub fn get_mut(&mut self, dense_index: u32) -> Result<&mut T, RynvekErrorCode> {
        self.values
            .get_mut(dense_index as usize)
            .ok_or(RynvekErrorCode::ArchetypeComponentMissing)
    }

    /// 在稠密尾部追加（与 `EntityRegistry::spawn` 同步调用）
    pub fn push(&mut self, value: T) {
        self.values.push(value);
    }

    /// 与注册表 swap-remove 对齐的列删除
    pub fn swap_remove(&mut self, dense_index: u32) -> Result<T, RynvekErrorCode> {
        let idx = dense_index as usize;
        if idx >= self.values.len() {
            return Err(RynvekErrorCode::ArchetypeComponentMissing);
        }
        Ok(self.values.swap_remove(idx))
    }
}

impl<T: Clone> ComponentColumn<T> {
    /// 将列截断或扩展到 `len`，扩展部分填 `fill`
    pub fn resize(&mut self, len: usize, fill: T) {
        self.values.resize(len, fill);
    }
}

/// 延迟结构变更操作（在 Phase 07 `ApplyStructuralMutationsAndDespawns` 统一提交）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructuralMutationOp {
    Spawn { archetype_id: u16 },
    Despawn { handle: EntityHandle },
}

/// 确定性结构变更缓冲器（规范 04 §4.3）
#[derive(Clone, Debug, Default)]
pub struct StructuralMutationBuffer {
    ops: Vec<StructuralMutationOp>,
}

/// 结构变更提交回执
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StructuralMutationFlushReport {
    pub spawned: Vec<(EntityHandle, EntityId)>,
    pub despawned: Vec<EntityId>,
}

impl StructuralMutationBuffer {
    pub const fn new() -> Self {
        Self { ops: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// 排入一条延迟生成请求
    pub fn queue_spawn(&mut self, archetype_id: u16) {
        self.ops.push(StructuralMutationOp::Spawn { archetype_id });
    }

    /// 排入一条延迟销毁请求
    pub fn queue_despawn(&mut self, handle: EntityHandle) {
        self.ops.push(StructuralMutationOp::Despawn { handle });
    }

    /// 按确定性入队顺序在屏障点一次性应用全部结构变更
    pub fn flush(
        &mut self,
        registry: &mut EntityRegistry,
        tick: u64,
    ) -> Result<StructuralMutationFlushReport, RynvekErrorCode> {
        let mut report = StructuralMutationFlushReport::default();
        for op in self.ops.drain(..) {
            match op {
                StructuralMutationOp::Spawn { archetype_id } => {
                    report.spawned.push(registry.spawn(archetype_id)?);
                }
                StructuralMutationOp::Despawn { handle } => {
                    if let Ok(id) = registry.despawn(handle, tick) {
                        report.despawned.push(id);
                    }
                }
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_page_supports_full_65536_capacity_without_u16_overflow() {
        let page = DomainPage::with_len(0, PAGE_CAPACITY as u32);
        assert_eq!(page.len, 65_536);
        assert!(page.is_full());
    }

    #[test]
    fn paged_column_allocates_in_4096_chunks_and_swap_removes() {
        let mut col = PagedComponentColumn::new();
        for i in 0..5_000u32 {
            col.push(i);
        }
        assert_eq!(col.len(), 5_000);
        assert_eq!(col.chunk_count(), 2);
        assert_eq!(*col.get(4_096).unwrap(), 4_096);
        assert_eq!(col.swap_remove(0).unwrap(), 0);
        assert_eq!(*col.get(0).unwrap(), 4_999);
        assert_eq!(col.len(), 4_999);
    }

    #[test]
    fn structural_mutation_buffer_flushes_deterministically() {
        let mut reg = EntityRegistry::new(9);
        let mut buf = StructuralMutationBuffer::new();
        buf.queue_spawn(1);
        buf.queue_spawn(2);
        let r1 = buf.flush(&mut reg, 10).unwrap();
        assert_eq!(r1.spawned.len(), 2);
        assert_eq!(reg.alive_count(), 2);

        buf.queue_despawn(r1.spawned[0].0);
        let r2 = buf.flush(&mut reg, 11).unwrap();
        assert_eq!(r2.despawned, vec![r1.spawned[0].1]);
        assert_eq!(reg.alive_count(), 1);
    }

    #[test]
    fn column_tracks_dense_indices() {
        let mut col = ComponentColumn::new();
        col.push(10u32);
        col.push(20);
        col.push(30);
        assert_eq!(*col.get(1).unwrap(), 20);
        assert_eq!(col.swap_remove(0).unwrap(), 10);
        assert_eq!(col.as_slice(), &[30, 20]);
    }
}
