// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 分页列式存储（Paged SoA，规范 04 / ADR-004–005）。
//!
//! - [`DomainPage`]：容量 [`PAGE_CAPACITY`](crate::PAGE_CAPACITY) 的逻辑页；
//! - 页内按 [`CHUNK_CAPACITY`](crate::CHUNK_CAPACITY) 切块，便于与 GPU 分段缓冲对齐；
//! - [`ComponentColumn<T>`]：单一组件类型的稠密列，与 [`EntityRegistry`](crate::EntityRegistry)
//!   的 `dense_to_sparse_slot` 下标一一对应。

use rynvek_core::error::RynvekErrorCode;

use crate::registry::{CHUNK_CAPACITY, PAGE_CAPACITY};

/// 一页实体的元信息（与稀疏表页对应的稠密侧视图）
#[derive(Clone, Debug)]
pub struct DomainPage {
    /// 页编号
    pub page_id: u16,
    /// 本页已占用的稠密槽位数（≤ PAGE_CAPACITY）
    pub len: u16,
}

impl DomainPage {
    pub fn new(page_id: u16) -> Self {
        Self { page_id, len: 0 }
    }

    pub const fn capacity(&self) -> usize {
        PAGE_CAPACITY
    }

    pub const fn chunk_capacity(&self) -> usize {
        CHUNK_CAPACITY
    }
}

/// 与稠密实体下标对齐的组件列
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

#[cfg(test)]
mod tests {
    use super::*;

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
