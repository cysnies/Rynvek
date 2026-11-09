// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! GPU 集成测试公共设施。
//!
//! 设备获取策略（ERRATA E-014）：
//! - 默认按 `GpuDeviceRequest::from_env()` 请求（`PRIMARY`，无则回退 `GL`；`WGPU_BACKEND` 可覆盖）；
//! - 无可用适配器时打印原因并**跳过**该测试（保持无 GPU 环境的验收绿色）；
//! - 设置 `RYNVEK_REQUIRE_GPU=1` 时无适配器即判定失败（CI 有 GPU / 软件光栅器时启用，杜绝静默跳过）。

#![allow(dead_code)]

use rynvek_wgpu::{GpuDeviceOwner, GpuDeviceRequest};

pub fn gpu() -> Option<GpuDeviceOwner> {
    match GpuDeviceOwner::request_blocking(GpuDeviceRequest::from_env()) {
        Ok(owner) => {
            let info = owner.adapter_info();
            eprintln!(
                "[rynvek-wgpu] 适配器：{} / {:?} / {:?} / 驱动 {} {}；档位 {:?}",
                info.name,
                info.backend,
                info.device_type,
                info.driver,
                info.driver_info,
                owner.profile()
            );
            Some(owner)
        }
        Err(e) => {
            if std::env::var("RYNVEK_REQUIRE_GPU").as_deref() == Ok("1") {
                panic!("RYNVEK_REQUIRE_GPU=1 但无法创建设备：{e}");
            }
            eprintln!("[rynvek-wgpu] 跳过 GPU 测试：{e}");
            None
        }
    }
}
