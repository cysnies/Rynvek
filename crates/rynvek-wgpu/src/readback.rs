// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `3-Slot Readback Ring`（规范 07 §7.1 第 2 条）：`Slot[tick % 3]` + `map_async(MapMode::Read)` 异步回读统计摘要，
//! **主循环内严禁同步阻塞等待 GPU Fence**。
//!
//! 设计：
//! - [`ReadbackRingState`]：纯逻辑状态机（可无 GPU 单测）。每个槽位 `Idle → InFlight(tick) → Ready(tick) → Idle`；
//! - 新 Tick 要使用的槽位仍在 `InFlight`（GPU 尚未完成三帧前的回读）时**不等待**，而是跳过本 Tick 的回读并计数
//!   （`skipped`），保证主循环永不阻塞；`Ready` 但尚未被取走的旧结果被覆盖时计入 `dropped`；
//! - [`ReadbackRing`]：持有 3 个 `MAP_READ | COPY_DST` 暂存缓冲；`map_async` 回调只翻转原子标志，
//!   结果在下一次 [`ReadbackRing::poll`]（`PollType::Poll`，非阻塞）后由 [`ReadbackRing::drain_ready`] 按 Tick 升序取出。

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// 回读环槽位数（规范冻结：3）
pub const READBACK_RING_SLOTS: usize = 3;

/// 槽位状态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadbackSlotState {
    Idle,
    InFlight { tick: u64 },
    Ready { tick: u64 },
}

/// `begin` 的结果
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadbackBegin {
    /// 可使用该槽位
    Slot(usize),
    /// 槽位仍被三帧前的回读占用：跳过本 Tick（不等待）
    SkippedBusy,
}

/// 纯逻辑的回读环状态机
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadbackRingState {
    slots: [ReadbackSlotState; READBACK_RING_SLOTS],
    pub skipped: u64,
    pub dropped: u64,
}

impl Default for ReadbackRingState {
    fn default() -> Self {
        Self {
            slots: [ReadbackSlotState::Idle; READBACK_RING_SLOTS],
            skipped: 0,
            dropped: 0,
        }
    }
}

impl ReadbackRingState {
    pub const fn slot_for_tick(tick: u64) -> usize {
        (tick % READBACK_RING_SLOTS as u64) as usize
    }

    pub fn slot(&self, index: usize) -> ReadbackSlotState {
        self.slots[index]
    }

    /// 为 `tick` 申请槽位
    pub fn begin(&mut self, tick: u64) -> ReadbackBegin {
        let index = Self::slot_for_tick(tick);
        match self.slots[index] {
            ReadbackSlotState::InFlight { .. } => {
                self.skipped += 1;
                ReadbackBegin::SkippedBusy
            }
            ReadbackSlotState::Ready { .. } => {
                self.dropped += 1;
                self.slots[index] = ReadbackSlotState::InFlight { tick };
                ReadbackBegin::Slot(index)
            }
            ReadbackSlotState::Idle => {
                self.slots[index] = ReadbackSlotState::InFlight { tick };
                ReadbackBegin::Slot(index)
            }
        }
    }

    /// GPU 完成映射
    pub fn complete(&mut self, index: usize) {
        if let ReadbackSlotState::InFlight { tick } = self.slots[index] {
            self.slots[index] = ReadbackSlotState::Ready { tick };
        }
    }

    /// 映射失败（例如设备丢失）：放弃该槽位
    pub fn abandon(&mut self, index: usize) {
        self.slots[index] = ReadbackSlotState::Idle;
    }

    /// 取出全部就绪槽位（按 Tick 升序），并把它们复位为 `Idle`
    pub fn take_ready(&mut self) -> Vec<(u64, usize)> {
        let mut ready: Vec<(u64, usize)> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| match s {
                ReadbackSlotState::Ready { tick } => Some((*tick, i)),
                _ => None,
            })
            .collect();
        ready.sort_unstable();
        for &(_, i) in &ready {
            self.slots[i] = ReadbackSlotState::Idle;
        }
        ready
    }

    /// 设备丢失时清空全部槽位
    pub fn reset(&mut self) {
        self.slots = [ReadbackSlotState::Idle; READBACK_RING_SLOTS];
    }
}

const FLAG_PENDING: u8 = 0;
const FLAG_OK: u8 = 1;
const FLAG_FAILED: u8 = 2;

/// GPU 回读环
#[derive(Debug)]
pub struct ReadbackRing {
    bytes_per_slot: u64,
    staging: Vec<wgpu::Buffer>,
    flags: Vec<Arc<AtomicU8>>,
    state: ReadbackRingState,
}

impl ReadbackRing {
    pub fn new(device: &wgpu::Device, label: &str, bytes_per_slot: u64) -> Self {
        let staging = (0..READBACK_RING_SLOTS)
            .map(|i| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("{label}.readback[{i}]")),
                    size: bytes_per_slot,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        Self {
            bytes_per_slot,
            staging,
            flags: (0..READBACK_RING_SLOTS)
                .map(|_| Arc::new(AtomicU8::new(FLAG_PENDING)))
                .collect(),
            state: ReadbackRingState::default(),
        }
    }

    pub fn state(&self) -> &ReadbackRingState {
        &self.state
    }

    /// 录制把 `src[offset..offset+bytes_per_slot]` 拷入 `Slot[tick % 3]` 的命令；槽位忙时返回 `None`（跳过，不等待）
    pub fn encode_copy(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        tick: u64,
        src: &wgpu::Buffer,
        src_offset: u64,
    ) -> Option<usize> {
        match self.state.begin(tick) {
            ReadbackBegin::SkippedBusy => None,
            ReadbackBegin::Slot(i) => {
                encoder.copy_buffer_to_buffer(src, src_offset, &self.staging[i], 0, self.bytes_per_slot);
                Some(i)
            }
        }
    }

    /// 在 `queue.submit` 之后调用：发起异步映射
    pub fn request_map(&mut self, slot: usize) {
        let flag = Arc::clone(&self.flags[slot]);
        flag.store(FLAG_PENDING, Ordering::Release);
        self.staging[slot]
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                flag.store(if result.is_ok() { FLAG_OK } else { FLAG_FAILED }, Ordering::Release);
            });
    }

    /// 非阻塞推进：驱动映射回调并更新槽位状态
    pub fn poll(&mut self, device: &wgpu::Device) {
        let _ = device.poll(wgpu::PollType::Poll);
        for i in 0..READBACK_RING_SLOTS {
            if let ReadbackSlotState::InFlight { .. } = self.state.slot(i) {
                match self.flags[i].load(Ordering::Acquire) {
                    FLAG_OK => self.state.complete(i),
                    FLAG_FAILED => self.state.abandon(i),
                    _ => {}
                }
            }
        }
    }

    /// 取出全部就绪结果：`(tick, 字节)`，按 Tick 升序
    pub fn drain_ready(&mut self) -> Vec<(u64, Vec<u8>)> {
        self.state
            .take_ready()
            .into_iter()
            .filter_map(|(tick, i)| {
                let bytes = self.staging[i].slice(..).get_mapped_range().map(|v| v.to_vec()).ok();
                self.staging[i].unmap();
                self.flags[i].store(FLAG_PENDING, Ordering::Release);
                bytes.map(|b| (tick, b))
            })
            .collect()
    }

    /// 是否仍有在途回读
    pub fn has_in_flight(&self) -> bool {
        (0..READBACK_RING_SLOTS).any(|i| matches!(self.state.slot(i), ReadbackSlotState::InFlight { .. }))
    }
}

/// 一次性非阻塞回读（恢复校验探针等低频场景）：录制拷贝 → 提交后 `map_async` → 轮询取回
#[derive(Debug)]
pub struct OneShotReadback {
    staging: wgpu::Buffer,
    bytes: u64,
    flag: Arc<AtomicU8>,
    requested: bool,
}

/// 一次性回读的轮询结果
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OneShotPoll {
    Pending,
    Ready(Vec<u8>),
    Failed,
}

impl OneShotReadback {
    /// 录制从 `src[0..bytes]` 到新暂存缓冲的拷贝
    pub fn encode(device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, src: &wgpu::Buffer, bytes: u64) -> Self {
        let size = bytes.div_ceil(wgpu::COPY_BUFFER_ALIGNMENT).max(1) * wgpu::COPY_BUFFER_ALIGNMENT;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rynvek.readback.oneshot"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(src, 0, &staging, 0, size);
        Self {
            staging,
            bytes,
            flag: Arc::new(AtomicU8::new(FLAG_PENDING)),
            requested: false,
        }
    }

    /// `queue.submit` 之后调用
    pub fn after_submit(&mut self) {
        if !self.requested {
            self.requested = true;
            let flag = Arc::clone(&self.flag);
            self.staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                flag.store(if r.is_ok() { FLAG_OK } else { FLAG_FAILED }, Ordering::Release);
            });
        }
    }

    /// 非阻塞轮询（调用方负责 `device.poll(PollType::Poll)`）
    pub fn poll(&self) -> OneShotPoll {
        match self.flag.load(Ordering::Acquire) {
            FLAG_OK => {
                let data = self
                    .staging
                    .slice(..)
                    .get_mapped_range()
                    .map(|v| v[..self.bytes as usize].to_vec());
                self.staging.unmap();
                self.flag.store(FLAG_FAILED, Ordering::Release);
                data.map_or(OneShotPoll::Failed, OneShotPoll::Ready)
            }
            FLAG_FAILED => OneShotPoll::Failed,
            _ => OneShotPoll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_rotate_by_tick_mod_three() {
        assert_eq!(
            [0, 1, 2, 3, 4, 5].map(ReadbackRingState::slot_for_tick),
            [0, 1, 2, 0, 1, 2]
        );
    }

    #[test]
    fn busy_slot_is_skipped_never_waited_on() {
        let mut ring = ReadbackRingState::default();
        assert_eq!(ring.begin(0), ReadbackBegin::Slot(0));
        assert_eq!(ring.begin(1), ReadbackBegin::Slot(1));
        assert_eq!(ring.begin(2), ReadbackBegin::Slot(2));
        // Tick 3 复用 Slot 0，但 Tick 0 的回读尚未完成：跳过而非等待
        assert_eq!(ring.begin(3), ReadbackBegin::SkippedBusy);
        assert_eq!(ring.skipped, 1);
        ring.complete(0);
        ring.complete(2);
        assert_eq!(ring.take_ready(), vec![(0, 0), (2, 2)]);
        assert_eq!(ring.begin(3), ReadbackBegin::Slot(0));
    }

    #[test]
    fn unconsumed_ready_results_are_overwritten_and_counted() {
        let mut ring = ReadbackRingState::default();
        ring.begin(0);
        ring.complete(0);
        assert_eq!(ring.begin(3), ReadbackBegin::Slot(0));
        assert_eq!(ring.dropped, 1);
        assert_eq!(ring.slot(0), ReadbackSlotState::InFlight { tick: 3 });
        ring.abandon(0);
        assert_eq!(ring.slot(0), ReadbackSlotState::Idle);
        ring.begin(1);
        ring.reset();
        assert!(ring.take_ready().is_empty());
    }
}
