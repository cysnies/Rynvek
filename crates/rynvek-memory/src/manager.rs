// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! [`MemoryManager`]：对齐池 + Slab + 预算状态机的统一门面。

use rynvek_core::error::RynvekErrorCode;

use crate::budget::{MemoryBudget, MemoryBudgetLevel};
use crate::pool::{ALIGN_64, ALIGN_256, AlignedBlock, AlignedPool};
use crate::slab::{SLAB_SIZE_BYTES, SlabAllocation, SlabAllocator};

/// 默认世界内存预算：512 MiB（可通过 [`MemoryManager::with_budget`] 覆盖）
pub const DEFAULT_WORLD_MEMORY_BUDGET_BYTES: u64 = 512 * 1024 * 1024;

/// 对齐池默认块容量：64 KiB
const POOL_BLOCK_CAPACITY: usize = 64 * 1024;

/// 内存管理器
#[derive(Debug)]
pub struct MemoryManager {
    budget: MemoryBudget,
    pool_64: AlignedPool,
    pool_256: AlignedPool,
    slabs: SlabAllocator,
}

impl MemoryManager {
    /// 使用默认 512 MiB 预算创建
    pub fn new() -> Self {
        Self::with_budget(DEFAULT_WORLD_MEMORY_BUDGET_BYTES)
    }

    /// 使用自定义预算上限创建
    pub fn with_budget(limit_bytes: u64) -> Self {
        Self {
            budget: MemoryBudget::new(limit_bytes),
            pool_64: AlignedPool::new(POOL_BLOCK_CAPACITY, ALIGN_64),
            pool_256: AlignedPool::new(POOL_BLOCK_CAPACITY, ALIGN_256),
            slabs: SlabAllocator::new(),
        }
    }

    pub fn budget_level(&self) -> MemoryBudgetLevel {
        self.budget.level()
    }

    pub fn used_bytes(&self) -> u64 {
        self.budget.used_bytes()
    }

    pub fn limit_bytes(&self) -> u64 {
        self.budget.limit_bytes()
    }

    /// 从 64B 对齐池取一块
    pub fn acquire_64(&mut self) -> Result<AlignedBlock, RynvekErrorCode> {
        self.pool_64.acquire(&mut self.budget)
    }

    /// 归还 64B 对齐块
    pub fn release_64(&mut self, block: AlignedBlock) {
        self.pool_64.release(block);
    }

    /// 从 256B 对齐池取一块
    pub fn acquire_256(&mut self) -> Result<AlignedBlock, RynvekErrorCode> {
        self.pool_256.acquire(&mut self.budget)
    }

    /// 归还 256B 对齐块
    pub fn release_256(&mut self, block: AlignedBlock) {
        self.pool_256.release(block);
    }

    /// 从 32 MiB Slab 切出区间
    pub fn slab_allocate(&mut self, len: usize) -> Result<SlabAllocation, RynvekErrorCode> {
        self.slabs.allocate(len, &mut self.budget)
    }

    pub fn slab_get(&self, alloc: SlabAllocation) -> Result<&[u8], RynvekErrorCode> {
        self.slabs.get(alloc)
    }

    pub fn slab_get_mut(&mut self, alloc: SlabAllocation) -> Result<&mut [u8], RynvekErrorCode> {
        self.slabs.get_mut(alloc)
    }

    pub fn slab_free(&mut self, alloc: SlabAllocation) -> Result<(), RynvekErrorCode> {
        self.slabs.free(alloc, &mut self.budget)
    }

    /// 压缩空闲池与空闲 Slab
    pub fn shrink_to_fit(&mut self) {
        self.pool_64.shrink_to_fit(&mut self.budget);
        self.pool_256.shrink_to_fit(&mut self.budget);
        self.slabs.shrink_to_fit(&mut self.budget);
    }

    /// 单 Slab 大小（供上层规划分段缓冲）
    pub const fn slab_size() -> usize {
        SLAB_SIZE_BYTES
    }
}

impl Default for MemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_routes_pools_and_slabs() {
        let mut mm = MemoryManager::with_budget(128 * 1024 * 1024);
        let a = mm.acquire_64().unwrap();
        assert!(a.is_well_aligned());
        mm.release_64(a);
        let b = mm.acquire_256().unwrap();
        assert!(b.is_well_aligned());
        mm.release_256(b);
        let s = mm.slab_allocate(128).unwrap();
        mm.slab_get_mut(s).unwrap()[0] = 9;
        assert_eq!(mm.slab_get(s).unwrap()[0], 9);
        mm.slab_free(s).unwrap();
        mm.shrink_to_fit();
        assert!(mm.used_bytes() <= 128 * 1024 * 1024);
    }
}
