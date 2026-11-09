// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 由 `adapter.limits()` 自动判定 `ComputeCapabilityProfile`（规范 07 §7.1）。
//!
//! 规范只冻结了档位名称（Tier A / B / C / D），未给出阈值；按最佳实践冻结如下（ERRATA E-014），
//! 判定只依赖可序列化的 [`AdapterCapabilitySummary`]，因此无需 GPU 即可单测：
//!
//! | 档位 | 条件（自上而下首个满足者） |
//! | :--- | :--- |
//! | Tier D | 不满足任一「计算最低要求」：计算着色器可用、Workgroup ≥ 128×1×1 且 ≥ 8×8×1、每阶段 ≥ 8 个存储缓冲、≥ 4 个 BindGroup、单绑定 ≥ 32 MiB |
//! | Tier C | 满足最低要求，但为软件 / CPU 适配器，或 `max_buffer_size < 256 MiB` |
//! | Tier A | 独立显卡，`max_buffer_size ≥ 1 GiB`，单绑定 ≥ 1 GiB，每 Workgroup ≥ 1024 调用 |
//! | Tier B | 其余硬件适配器（集显 / 虚拟 GPU / 未达 A 档的独显） |

use rynvek_compute::CapabilityProfile;

use crate::segment::SEGMENT_SIZE_BYTES;

/// 规范 07 的档位类型名（与 `rynvek-compute` 的 [`CapabilityProfile`] 为同一类型）
pub type ComputeCapabilityProfile = CapabilityProfile;

const MIB: u64 = 1024 * 1024;

/// 档位判定所需的适配器能力摘要
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterCapabilitySummary {
    pub device_type: wgpu::DeviceType,
    pub compute_shaders: bool,
    pub max_compute_invocations_per_workgroup: u32,
    pub max_compute_workgroup_size_x: u32,
    pub max_compute_workgroup_size_y: u32,
    pub max_storage_buffers_per_shader_stage: u32,
    pub max_bind_groups: u32,
    pub max_storage_buffer_binding_size: u64,
    pub max_buffer_size: u64,
}

impl AdapterCapabilitySummary {
    /// 从真实适配器读取
    pub fn from_adapter(adapter: &wgpu::Adapter) -> Self {
        let info = adapter.get_info();
        let downlevel = adapter.get_downlevel_capabilities();
        Self::from_parts(
            info.device_type,
            downlevel.flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS),
            &adapter.limits(),
        )
    }

    pub fn from_parts(device_type: wgpu::DeviceType, compute_shaders: bool, limits: &wgpu::Limits) -> Self {
        Self {
            device_type,
            compute_shaders,
            max_compute_invocations_per_workgroup: limits.max_compute_invocations_per_workgroup,
            max_compute_workgroup_size_x: limits.max_compute_workgroup_size_x,
            max_compute_workgroup_size_y: limits.max_compute_workgroup_size_y,
            max_storage_buffers_per_shader_stage: limits.max_storage_buffers_per_shader_stage,
            max_bind_groups: limits.max_bind_groups,
            max_storage_buffer_binding_size: limits.max_storage_buffer_binding_size,
            max_buffer_size: limits.max_buffer_size,
        }
    }

    /// 是否满足 Rynvek GPU 计算的最低要求
    pub fn meets_compute_minimum(&self) -> bool {
        self.compute_shaders
            && self.max_compute_invocations_per_workgroup >= 128
            && self.max_compute_workgroup_size_x >= 128
            && self.max_compute_workgroup_size_y >= 8
            && self.max_storage_buffers_per_shader_stage >= 8
            && self.max_bind_groups >= 4
            && self.max_storage_buffer_binding_size >= SEGMENT_SIZE_BYTES
    }
}

/// 判定计算能力档位
pub fn classify_compute_capability(s: &AdapterCapabilitySummary) -> ComputeCapabilityProfile {
    if !s.meets_compute_minimum() {
        return CapabilityProfile::TierD;
    }
    let software = matches!(s.device_type, wgpu::DeviceType::Cpu | wgpu::DeviceType::Other);
    if software || s.max_buffer_size < 256 * MIB {
        return CapabilityProfile::TierC;
    }
    if s.device_type == wgpu::DeviceType::DiscreteGpu
        && s.max_buffer_size >= 1024 * MIB
        && s.max_storage_buffer_binding_size >= 1024 * MIB
        && s.max_compute_invocations_per_workgroup >= 1024
    {
        return CapabilityProfile::TierA;
    }
    CapabilityProfile::TierB
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(device_type: wgpu::DeviceType) -> AdapterCapabilitySummary {
        AdapterCapabilitySummary::from_parts(device_type, true, &wgpu::Limits::default())
    }

    #[test]
    fn webgpu_default_limits_on_integrated_gpu_are_tier_b() {
        // WebGPU 默认限制：单绑定 128 MiB、缓冲 256 MiB、8 个存储缓冲、4 个 BindGroup
        assert_eq!(
            classify_compute_capability(&summary(wgpu::DeviceType::IntegratedGpu)),
            CapabilityProfile::TierB
        );
    }

    #[test]
    fn software_adapters_are_capped_at_tier_c() {
        assert_eq!(
            classify_compute_capability(&summary(wgpu::DeviceType::Cpu)),
            CapabilityProfile::TierC
        );
    }

    #[test]
    fn missing_compute_or_small_limits_fall_to_tier_d() {
        let mut s = summary(wgpu::DeviceType::DiscreteGpu);
        s.compute_shaders = false;
        assert_eq!(classify_compute_capability(&s), CapabilityProfile::TierD);
        let mut s = summary(wgpu::DeviceType::DiscreteGpu);
        s.max_storage_buffer_binding_size = SEGMENT_SIZE_BYTES - 4;
        assert_eq!(classify_compute_capability(&s), CapabilityProfile::TierD);
        let s = AdapterCapabilitySummary::from_parts(
            wgpu::DeviceType::DiscreteGpu,
            true,
            &wgpu::Limits::downlevel_webgl2_defaults(),
        );
        assert_eq!(classify_compute_capability(&s), CapabilityProfile::TierD);
    }

    #[test]
    fn large_discrete_gpu_is_tier_a() {
        let mut s = summary(wgpu::DeviceType::DiscreteGpu);
        s.max_buffer_size = 4096 * MIB;
        s.max_storage_buffer_binding_size = 2048 * MIB;
        s.max_compute_invocations_per_workgroup = 1024;
        assert_eq!(classify_compute_capability(&s), CapabilityProfile::TierA);
        s.max_compute_invocations_per_workgroup = 256;
        assert_eq!(classify_compute_capability(&s), CapabilityProfile::TierB);
    }
}
