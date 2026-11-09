// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 非阻塞 GPU ID 拾取：把 R32Uint 拾取目标中的单个纹素拷入映射缓冲，`map_async` 完成后取回。
//!
//! 与 3-Slot 回读环同样遵守「主循环禁止同步等待 GPU」：同一时刻最多一个在途请求，
//! 新请求在旧请求完成前到达时直接替换待发请求（只关心最新的鼠标位置）。

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::instance::PickId;

/// 纹素拷贝的行对齐（WebGPU `COPY_BYTES_PER_ROW_ALIGNMENT`）
const ROW_ALIGN: u64 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64;

const PENDING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// 拾取结果
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickResult {
    /// 请求方关联号（`@rynvek/engine` 的 `PICK_REQUEST.requestId`）
    pub request_id: u32,
    pub x: u32,
    pub y: u32,
    pub id: Option<PickId>,
}

#[derive(Clone, Copy, Debug)]
struct Request {
    request_id: u32,
    x: u32,
    y: u32,
}

/// 拾取回读器
#[derive(Debug)]
pub struct PickReadback {
    staging: wgpu::Buffer,
    flag: Arc<AtomicU8>,
    in_flight: Option<Request>,
    queued: Option<Request>,
    mapped: bool,
}

impl PickReadback {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            staging: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rynvek.pick.readback"),
                size: ROW_ALIGN,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            flag: Arc::new(AtomicU8::new(PENDING)),
            in_flight: None,
            queued: None,
            mapped: false,
        }
    }

    /// 登记一个拾取请求（同帧多次请求只保留最后一个）
    pub fn request(&mut self, request_id: u32, x: u32, y: u32) {
        self.queued = Some(Request { request_id, x, y });
    }

    pub fn is_busy(&self) -> bool {
        self.in_flight.is_some()
    }

    /// 在渲染编码末尾调用：若空闲且有待发请求，录制纹素拷贝；返回是否录制
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder, pick_texture: &wgpu::Texture) -> bool {
        if self.in_flight.is_some() {
            return false;
        }
        let Some(req) = self.queued.take() else {
            return false;
        };
        let size = pick_texture.size();
        let x = req.x.min(size.width.saturating_sub(1));
        let y = req.y.min(size.height.saturating_sub(1));
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: pick_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ROW_ALIGN as u32),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        self.in_flight = Some(Request { x, y, ..req });
        self.mapped = false;
        true
    }

    /// `queue.submit` 之后调用：发起异步映射
    pub fn after_submit(&mut self) {
        if self.in_flight.is_some() && !self.mapped {
            self.mapped = true;
            let flag = Arc::clone(&self.flag);
            flag.store(PENDING, Ordering::Release);
            self.staging.slice(..4).map_async(wgpu::MapMode::Read, move |r| {
                flag.store(if r.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
            });
        }
    }

    /// 非阻塞取回结果（调用前应已 `device.poll(Poll)`）
    pub fn try_take(&mut self) -> Option<PickResult> {
        let req = self.in_flight?;
        match self.flag.load(Ordering::Acquire) {
            MAPPED => {
                let raw = self
                    .staging
                    .slice(..4)
                    .get_mapped_range()
                    .map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
                    .unwrap_or(0);
                self.staging.unmap();
                self.flag.store(PENDING, Ordering::Release);
                self.in_flight = None;
                Some(PickResult {
                    request_id: req.request_id,
                    x: req.x,
                    y: req.y,
                    id: PickId::from_raw(raw),
                })
            }
            FAILED => {
                self.flag.store(PENDING, Ordering::Release);
                self.in_flight = None;
                Some(PickResult {
                    request_id: req.request_id,
                    x: req.x,
                    y: req.y,
                    id: None,
                })
            }
            _ => None,
        }
    }

    /// 设备丢失后丢弃在途状态
    pub fn reset(&mut self) {
        self.in_flight = None;
        self.queued = None;
        self.mapped = false;
        self.flag.store(PENDING, Ordering::Release);
    }
}
