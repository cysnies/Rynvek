// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 固定 32 MiB 的 Slab 分配器（规范 01「GPU Buffer 分段大小 32 MiB」在 CPU 侧的对称结构）。
//!
//! 每个 Slab 是一整块连续字节缓冲，按「游标式」顺序切出子区间给调用方记账；
//! 释放采用「整块 Slab 引用计数归零后回收」的策略，避免在禁止 `unsafe` 时做精细的
//! 堆内分裂合并。

use rynvek_core::error::RynvekErrorCode;

use crate::budget::MemoryBudget;

/// 单个 Slab 的字节数：32 MiB
pub const SLAB_SIZE_BYTES: usize = 32 * 1024 * 1024;

/// 一次从 Slab 切出的逻辑区间
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlabAllocation {
    /// 所属 Slab 编号
    pub slab_id: u32,
    /// 区间在 Slab 内的起始偏移
    pub offset: u32,
    /// 区间长度
    pub len: u32,
}

#[derive(Debug)]
struct Slab {
    bytes: Vec<u8>,
    cursor: usize,
    live_allocations: u32,
}

impl Slab {
    fn new() -> Self {
        Self {
            bytes: vec![0u8; SLAB_SIZE_BYTES],
            cursor: 0,
            live_allocations: 0,
        }
    }

    fn remaining(&self) -> usize {
        SLAB_SIZE_BYTES.saturating_sub(self.cursor)
    }
}

/// 32 MiB Slab 分配器
#[derive(Debug, Default)]
pub struct SlabAllocator {
    slabs: Vec<Slab>,
    /// 可复用的空闲 Slab 下标
    free_slab_ids: Vec<u32>,
}

impl SlabAllocator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn slab_count(&self) -> usize {
        self.slabs.len()
    }

    /// 从某个未满 Slab（或新建 Slab）切出 `len` 字节；`len` 不得超过单 Slab 大小
    pub fn allocate(&mut self, len: usize, budget: &mut MemoryBudget) -> Result<SlabAllocation, RynvekErrorCode> {
        if len == 0 || len > SLAB_SIZE_BYTES {
            return Err(RynvekErrorCode::DimensionMismatch);
        }

        if let Some((slab_id, slab)) = self
            .slabs
            .iter_mut()
            .enumerate()
            .find(|(_, slab)| slab.remaining() >= len)
        {
            let offset = slab.cursor as u32;
            slab.cursor += len;
            slab.live_allocations += 1;
            return Ok(SlabAllocation {
                slab_id: slab_id as u32,
                offset,
                len: len as u32,
            });
        }

        let slab_id = if let Some(id) = self.free_slab_ids.pop() {
            id
        } else {
            budget.commit_allocation(SLAB_SIZE_BYTES as u64)?;
            let id = self.slabs.len() as u32;
            self.slabs.push(Slab::new());
            id
        };

        let slab = &mut self.slabs[slab_id as usize];
        // 复用空闲 Slab 时重置游标
        if slab.live_allocations == 0 {
            slab.cursor = 0;
            slab.bytes.fill(0);
        }
        if slab.remaining() < len {
            return Err(RynvekErrorCode::WorldMemoryBudgetExceeded);
        }
        let offset = slab.cursor as u32;
        slab.cursor += len;
        slab.live_allocations += 1;
        Ok(SlabAllocation {
            slab_id,
            offset,
            len: len as u32,
        })
    }

    /// 只读访问某次分配对应的字节
    pub fn get(&self, alloc: SlabAllocation) -> Result<&[u8], RynvekErrorCode> {
        let slab = self
            .slabs
            .get(alloc.slab_id as usize)
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)?;
        let start = alloc.offset as usize;
        let end = start + alloc.len as usize;
        slab.bytes
            .get(start..end)
            .ok_or(RynvekErrorCode::CoordinateOutOfCanonicalRange)
    }

    /// 可变访问某次分配对应的字节
    pub fn get_mut(&mut self, alloc: SlabAllocation) -> Result<&mut [u8], RynvekErrorCode> {
        let slab = self
            .slabs
            .get_mut(alloc.slab_id as usize)
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)?;
        let start = alloc.offset as usize;
        let end = start + alloc.len as usize;
        slab.bytes
            .get_mut(start..end)
            .ok_or(RynvekErrorCode::CoordinateOutOfCanonicalRange)
    }

    /// 释放一次分配；当 Slab 上无存活分配时回收整块以供复用
    pub fn free(&mut self, alloc: SlabAllocation, budget: &mut MemoryBudget) -> Result<(), RynvekErrorCode> {
        let slab = self
            .slabs
            .get_mut(alloc.slab_id as usize)
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)?;
        if slab.live_allocations == 0 {
            return Err(RynvekErrorCode::StructuralMutationConflict);
        }
        slab.live_allocations -= 1;
        if slab.live_allocations == 0 {
            slab.cursor = 0;
            slab.bytes.fill(0);
            if !self.free_slab_ids.contains(&alloc.slab_id) {
                self.free_slab_ids.push(alloc.slab_id);
            }
            // 预算仍按「持有的 Slab 数」计；真正归还给 OS 需调用 shrink
            let _ = budget;
        }
        Ok(())
    }

    /// 释放所有空闲 Slab 占用的预算与内存
    pub fn shrink_to_fit(&mut self, budget: &mut MemoryBudget) {
        // 只收缩下标最大的空闲尾部，避免移动仍存活的 Slab
        self.free_slab_ids.sort_unstable();
        while let Some(&id) = self.free_slab_ids.last() {
            if id as usize + 1 != self.slabs.len() {
                break;
            }
            self.free_slab_ids.pop();
            self.slabs.pop();
            budget.reclaim(SLAB_SIZE_BYTES as u64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::MemoryBudget;

    #[test]
    fn slab_allocates_and_recycles() {
        let mut budget = MemoryBudget::new(SLAB_SIZE_BYTES as u64 * 4);
        let mut allocator = SlabAllocator::new();
        let a = allocator.allocate(1024, &mut budget).unwrap();
        let b = allocator.allocate(2048, &mut budget).unwrap();
        assert_eq!(a.slab_id, b.slab_id);
        allocator.get_mut(a).unwrap().fill(1);
        allocator.free(a, &mut budget).unwrap();
        allocator.free(b, &mut budget).unwrap();
        assert_eq!(allocator.free_slab_ids.len(), 1);
        let c = allocator.allocate(16, &mut budget).unwrap();
        assert_eq!(c.slab_id, a.slab_id);
        assert_eq!(allocator.get(c).unwrap()[0], 0);
    }
}
