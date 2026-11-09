// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 引擎集成测试公共设施：设备获取（无适配器跳过；`RYNVEK_REQUIRE_GPU=1` 时失败）与确定性伪随机数据。

#![allow(dead_code)]

use rynvek_core::rng::philox4x32_10;
use rynvek_wgpu::{GpuDeviceOwner, GpuDeviceRequest};

pub fn require_gpu() -> bool {
    std::env::var("RYNVEK_REQUIRE_GPU").as_deref() == Ok("1")
}

pub fn gpu() -> Option<GpuDeviceOwner> {
    match GpuDeviceOwner::request_blocking(GpuDeviceRequest::from_env()) {
        Ok(g) => {
            let i = g.adapter_info();
            eprintln!(
                "[rynvek-engine] 适配器：{} / {:?} / {:?}；档位 {:?}",
                i.name,
                i.backend,
                i.device_type,
                g.profile()
            );
            Some(g)
        }
        Err(e) => {
            assert!(!require_gpu(), "RYNVEK_REQUIRE_GPU=1 但无法创建设备：{e}");
            eprintln!("[rynvek-engine] 跳过 GPU 测试：{e}");
            None
        }
    }
}

/// 基于 Philox 的确定性测试数据流
pub struct Rng {
    key: [u32; 2],
    counter: u32,
    buf: [u32; 4],
    used: usize,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self {
            key: [seed, 0x5259_4E56],
            counter: 0,
            buf: [0; 4],
            used: 4,
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.used == 4 {
            self.buf = philox4x32_10([self.counter, 0, 0, 0], self.key);
            self.counter += 1;
            self.used = 0;
        }
        self.used += 1;
        self.buf[self.used - 1]
    }

    /// `[lo, hi]` 闭区间
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let span = (i64::from(hi) - i64::from(lo) + 1) as u64;
        (i64::from(lo) + (u64::from(self.next_u32()) % span) as i64) as i32
    }
}
