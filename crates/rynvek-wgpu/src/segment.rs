// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `32 MiB` 分段缓冲池（规范 07 §7.1 第 2 条）。
//!
//! - 严禁分配超过 `128 MiB` 的单体 Storage Buffer；
//! - `GpuOrganismHotState` 与神经突触表等大数组按每段 `33,554,432 B (32 MiB)` 切分。
//!
//! [`SegmentPlan`] 是纯计算的切分方案（可脱离 GPU 单测）：每段容纳整数个元素，元素绝不跨段，
//! 因此每段都可独立绑定为 `array<T>`；[`SegmentedBufferPool`] 按方案真正创建 `wgpu::Buffer`，
//! 并受字节预算约束，超出或设备拒绝时报 `GpuBufferAllocationFailed (0x6003)`。

use crate::error::{GpuError, GpuResult};

/// 每个缓冲段的字节数（规范冻结：32 MiB）
pub const SEGMENT_SIZE_BYTES: u64 = 33_554_432;
/// 单体存储缓冲的硬上限（规范冻结：128 MiB）
pub const MAX_MONOLITHIC_STORAGE_BUFFER_BYTES: u64 = 134_217_728;
/// 存储缓冲元素步长对齐（WGSL `array<T>` 元素至少 4 字节对齐）
pub const STORAGE_ELEMENT_ALIGN: u64 = 4;

/// 切分方案中的一段
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentRange {
    /// 段序号
    pub index: usize,
    /// 本段第一个元素的全局下标
    pub first_element: u64,
    /// 本段元素数
    pub element_count: u64,
    /// 本段字节数
    pub byte_len: u64,
}

/// 大数组的分段切分方案
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentPlan {
    pub element_stride: u64,
    pub element_count: u64,
    pub elements_per_segment: u64,
    pub segments: Vec<SegmentRange>,
}

impl SegmentPlan {
    /// 为 `element_count` 个、每个 `element_stride` 字节的元素生成切分方案
    pub fn new(element_count: u64, element_stride: u64) -> GpuResult<Self> {
        if element_stride == 0 || element_stride % STORAGE_ELEMENT_ALIGN != 0 || element_stride > SEGMENT_SIZE_BYTES {
            return Err(GpuError::allocation(format!(
                "元素步长 {element_stride} 必须为 4 的倍数且不超过单段 {SEGMENT_SIZE_BYTES} 字节"
            )));
        }
        let elements_per_segment = SEGMENT_SIZE_BYTES / element_stride;
        let mut segments = Vec::new();
        let mut first = 0u64;
        while first < element_count {
            let n = elements_per_segment.min(element_count - first);
            segments.push(SegmentRange {
                index: segments.len(),
                first_element: first,
                element_count: n,
                byte_len: n * element_stride,
            });
            first += n;
        }
        Ok(Self {
            element_stride,
            element_count,
            elements_per_segment,
            segments,
        })
    }

    pub fn total_bytes(&self) -> u64 {
        self.segments.iter().map(|s| s.byte_len).sum()
    }

    /// 全局元素下标 → (段序号, 段内元素下标)
    pub fn locate(&self, element: u64) -> Option<(usize, u64)> {
        if element >= self.element_count {
            return None;
        }
        Some((
            (element / self.elements_per_segment) as usize,
            element % self.elements_per_segment,
        ))
    }
}

/// 已分配的分段缓冲
#[derive(Debug)]
pub struct SegmentedBuffer {
    pub label: String,
    pub plan: SegmentPlan,
    pub buffers: Vec<wgpu::Buffer>,
}

impl SegmentedBuffer {
    /// 把全局元素区间 `[first, first + data.len()/stride)` 的数据写入对应各段
    pub fn write_elements(&self, queue: &wgpu::Queue, first_element: u64, data: &[u8]) -> GpuResult<()> {
        let stride = self.plan.element_stride;
        if data.len() as u64 % stride != 0 || first_element + data.len() as u64 / stride > self.plan.element_count {
            return Err(GpuError::allocation("写入区间越界或未按元素步长对齐"));
        }
        let mut element = first_element;
        let mut rest = data;
        while !rest.is_empty() {
            let (seg, local) = self.plan.locate(element).expect("已校验区间");
            let room = (self.plan.segments[seg].element_count - local) * stride;
            let take = room.min(rest.len() as u64) as usize;
            queue.write_buffer(&self.buffers[seg], local * stride, &rest[..take]);
            element += take as u64 / stride;
            rest = &rest[take..];
        }
        Ok(())
    }
}

/// 受字节预算约束的分段缓冲池
#[derive(Debug)]
pub struct SegmentedBufferPool {
    budget_bytes: u64,
    allocated_bytes: u64,
    max_binding_bytes: u64,
}

impl SegmentedBufferPool {
    /// `max_binding_bytes` 取设备 `max_storage_buffer_binding_size`，必须至少容纳一整段
    pub fn new(budget_bytes: u64, max_binding_bytes: u64) -> GpuResult<Self> {
        if max_binding_bytes < SEGMENT_SIZE_BYTES {
            return Err(GpuError::allocation(format!(
                "设备单绑定上限 {max_binding_bytes} 小于 32 MiB 分段"
            )));
        }
        Ok(Self {
            budget_bytes,
            allocated_bytes: 0,
            max_binding_bytes,
        })
    }

    pub fn allocated_bytes(&self) -> u64 {
        self.allocated_bytes
    }

    pub fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    /// 预算校验（纯逻辑，便于在真正分配前拒绝）
    pub fn reserve(&mut self, plan: &SegmentPlan) -> GpuResult<()> {
        let bytes = plan.total_bytes();
        debug_assert!(plan.segments.iter().all(|s| s.byte_len <= self.max_binding_bytes));
        if self.allocated_bytes + bytes > self.budget_bytes {
            return Err(GpuError::allocation(format!(
                "分段缓冲预算不足：已用 {} + 申请 {bytes} > 预算 {}",
                self.allocated_bytes, self.budget_bytes
            )));
        }
        self.allocated_bytes += bytes;
        Ok(())
    }

    /// 归还预算
    pub fn release(&mut self, buffer: SegmentedBuffer) {
        self.allocated_bytes = self.allocated_bytes.saturating_sub(buffer.plan.total_bytes());
        for b in buffer.buffers {
            b.destroy();
        }
    }

    /// 按方案创建各段存储缓冲（`STORAGE | COPY_DST | COPY_SRC`）
    pub fn allocate(
        &mut self,
        device: &wgpu::Device,
        label: &str,
        element_count: u64,
        element_stride: u64,
    ) -> GpuResult<SegmentedBuffer> {
        let plan = SegmentPlan::new(element_count, element_stride)?;
        self.reserve(&plan)?;
        let buffers = plan
            .segments
            .iter()
            .map(|s| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("{label}[{}]", s.index)),
                    size: s.byte_len.max(STORAGE_ELEMENT_ALIGN),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                })
            })
            .collect();
        Ok(SegmentedBuffer {
            label: label.to_string(),
            plan,
            buffers,
        })
    }
}

/// 创建单体存储缓冲前的硬上限检查（`> 128 MiB` 一律拒绝，必须改用分段缓冲）
pub fn check_monolithic_storage_size(bytes: u64) -> GpuResult<()> {
    if bytes > MAX_MONOLITHIC_STORAGE_BUFFER_BYTES {
        return Err(GpuError::allocation(format!(
            "单体存储缓冲 {bytes} 字节超过 128 MiB 上限，必须使用 SegmentedBufferPool"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_keeps_elements_whole_and_segments_at_most_32_mib() {
        // 48 字节的热状态记录：每段 699050 个，末段为余数
        let plan = SegmentPlan::new(2_000_000, 48).unwrap();
        assert_eq!(plan.elements_per_segment, SEGMENT_SIZE_BYTES / 48);
        assert_eq!(plan.segments.len(), 3);
        assert!(
            plan.segments
                .iter()
                .all(|s| s.byte_len <= SEGMENT_SIZE_BYTES && s.byte_len % 48 == 0)
        );
        assert_eq!(plan.segments.iter().map(|s| s.element_count).sum::<u64>(), 2_000_000);
        assert_eq!(plan.total_bytes(), 2_000_000 * 48);
        assert_eq!(plan.locate(699_050), Some((1, 0)));
        assert_eq!(plan.locate(2_000_000), None);
    }

    #[test]
    fn exact_multiple_has_no_empty_tail() {
        let plan = SegmentPlan::new(SEGMENT_SIZE_BYTES / 4 * 2, 4).unwrap();
        assert_eq!(plan.segments.len(), 2);
        assert!(plan.segments.iter().all(|s| s.byte_len == SEGMENT_SIZE_BYTES));
        assert!(SegmentPlan::new(0, 4).unwrap().segments.is_empty());
    }

    #[test]
    fn invalid_strides_and_budgets_report_0x6003() {
        use rynvek_core::error::RynvekErrorCode;
        assert_eq!(
            SegmentPlan::new(1, 6).unwrap_err().code,
            RynvekErrorCode::GpuBufferAllocationFailed
        );
        assert!(SegmentPlan::new(1, SEGMENT_SIZE_BYTES + 4).is_err());
        assert!(SegmentedBufferPool::new(1 << 30, 16 * 1024 * 1024).is_err());
        let mut pool = SegmentedBufferPool::new(64 * 1024 * 1024, 128 * 1024 * 1024).unwrap();
        pool.reserve(&SegmentPlan::new(SEGMENT_SIZE_BYTES / 4, 4).unwrap())
            .unwrap();
        let err = pool
            .reserve(&SegmentPlan::new(SEGMENT_SIZE_BYTES / 4 + 1, 4).unwrap())
            .unwrap_err();
        assert_eq!(err.code, RynvekErrorCode::GpuBufferAllocationFailed);
        assert!(check_monolithic_storage_size(MAX_MONOLITHIC_STORAGE_BUFFER_BYTES).is_ok());
        assert!(check_monolithic_storage_size(MAX_MONOLITHIC_STORAGE_BUFFER_BYTES + 1).is_err());
    }
}
