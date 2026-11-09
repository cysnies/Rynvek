// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 固定对齐块池（64B / 256B，D291–D296 / D541–D560）。
//!
//! 在禁止 `unsafe` 的约束下，块内容以 `Vec<u8>` 保存；对齐通过在底层缓冲前插入
//! `align - (addr % align)` 字节的填充实现，并把用户可见切片的起始地址约束到对齐边界。
//! 对外只暴露已对齐的可变字节视图与块句柄，供 SoA 列与临时缓冲复用。

use std::collections::VecDeque;

use rynvek_core::error::RynvekErrorCode;

use crate::budget::MemoryBudget;

/// 64 字节对齐（缓存行友好的通用热数据）
pub const ALIGN_64: usize = 64;
/// 256 字节对齐（GPU / SIMD 友好的宽列块）
pub const ALIGN_256: usize = 256;

/// 一块已对齐的可复用缓冲
#[derive(Debug)]
pub struct AlignedBlock {
    /// 对齐要求
    pub align: usize,
    /// 用户可用容量（不含对齐填充）
    pub capacity: usize,
    /// 底层拥有的字节（含对齐前缀）
    storage: Vec<u8>,
    /// 用户视图在 `storage` 中的起始下标
    offset: usize,
}

impl AlignedBlock {
    /// 分配一块至少 `capacity` 字节、对齐到 `align` 的缓冲
    pub fn allocate(capacity: usize, align: usize) -> Result<Self, RynvekErrorCode> {
        if align == 0 || !align.is_power_of_two() {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        // 额外预留 `align - 1` 字节以便找到对齐起点
        let mut storage = vec![0u8; capacity.saturating_add(align)];
        let base = storage.as_ptr() as usize;
        let aligned = (base + (align - 1)) & !(align - 1);
        let offset = aligned - base;
        if offset + capacity > storage.len() {
            storage.resize(offset + capacity, 0);
        }
        // 再次确认：Vec 扩容可能搬家，需要在最终缓冲上重算偏移
        let base = storage.as_ptr() as usize;
        let aligned = (base + (align - 1)) & !(align - 1);
        let offset = aligned - base;
        if offset + capacity > storage.len() {
            return Err(RynvekErrorCode::WorldMemoryBudgetExceeded);
        }
        Ok(Self {
            align,
            capacity,
            storage,
            offset,
        })
    }

    /// 用户可见的可变字节切片（保证起始地址对齐到 `align`）
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.storage[self.offset..self.offset + self.capacity]
    }

    /// 用户可见的只读字节切片
    pub fn bytes(&self) -> &[u8] {
        &self.storage[self.offset..self.offset + self.capacity]
    }

    /// 底层占用字节数（用于预算记账）
    pub fn footprint(&self) -> usize {
        self.storage.capacity()
    }

    /// 起始地址是否满足对齐（测试与调试用）
    pub fn is_well_aligned(&self) -> bool {
        self.bytes().as_ptr() as usize % self.align == 0
    }
}

/// 单一对齐规格的对象池
#[derive(Debug)]
pub struct AlignedPool {
    align: usize,
    block_capacity: usize,
    free: VecDeque<AlignedBlock>,
    live_blocks: usize,
    live_bytes: u64,
}

impl AlignedPool {
    /// 创建池：每块用户容量为 `block_capacity`，对齐到 `align`
    pub fn new(block_capacity: usize, align: usize) -> Self {
        Self {
            align,
            block_capacity,
            free: VecDeque::new(),
            live_blocks: 0,
            live_bytes: 0,
        }
    }

    pub const fn align(&self) -> usize {
        self.align
    }

    pub const fn block_capacity(&self) -> usize {
        self.block_capacity
    }

    pub const fn live_blocks(&self) -> usize {
        self.live_blocks
    }

    pub const fn live_bytes(&self) -> u64 {
        self.live_bytes
    }

    pub fn free_len(&self) -> usize {
        self.free.len()
    }

    /// 取出一块；池空时向预算申请新块
    pub fn acquire(&mut self, budget: &mut MemoryBudget) -> Result<AlignedBlock, RynvekErrorCode> {
        if let Some(block) = self.free.pop_front() {
            self.live_blocks += 1;
            return Ok(block);
        }
        let block = AlignedBlock::allocate(self.block_capacity, self.align)?;
        let footprint = block.footprint() as u64;
        budget.commit_allocation(footprint)?;
        self.live_bytes = self.live_bytes.saturating_add(footprint);
        self.live_blocks += 1;
        Ok(block)
    }

    /// 归还一块到空闲队列（不立刻释放给 OS，便于复用）
    pub fn release(&mut self, mut block: AlignedBlock) {
        assert_eq!(block.align, self.align, "归还了错误对齐规格的块");
        assert_eq!(block.capacity, self.block_capacity, "归还了错误容量的块");
        block.bytes_mut().fill(0);
        self.live_blocks = self.live_blocks.saturating_sub(1);
        self.free.push_back(block);
    }

    /// 把空闲队列彻底释放，并回收预算
    pub fn shrink_to_fit(&mut self, budget: &mut MemoryBudget) {
        while let Some(block) = self.free.pop_front() {
            let footprint = block.footprint() as u64;
            self.live_bytes = self.live_bytes.saturating_sub(footprint);
            budget.reclaim(footprint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::MemoryBudget;

    #[test]
    fn blocks_are_aligned_and_reusable() {
        let mut budget = MemoryBudget::new(64 * 1024 * 1024);
        let mut pool = AlignedPool::new(4096, ALIGN_256);
        let mut block = pool.acquire(&mut budget).unwrap();
        assert!(block.is_well_aligned());
        block.bytes_mut()[0] = 0xAB;
        pool.release(block);
        let again = pool.acquire(&mut budget).unwrap();
        assert_eq!(again.bytes()[0], 0);
        assert_eq!(pool.free_len(), 0);
        pool.release(again);
        assert_eq!(pool.free_len(), 1);
        pool.shrink_to_fit(&mut budget);
        assert_eq!(pool.free_len(), 0);
    }
}
