// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! WGSL 核心着色器库与 naga 预校验。
//!
//! 所有核心计算着色器随 crate 一起编译进二进制（`include_str!`），组合方式固定为：
//! `common/bindings.wgsl`（BindGroup 0 诊断计数器）、按需的 `common/philox_rng.wgsl`（规范 02 冻结路径）
//! 与内核本体依次拼接。提交给 `wgpu` 之前先用 naga 30 解析与校验，失败即报
//! `WgslShaderValidationFailed (0x6004)`，避免把非法着色器交给浏览器驱动后才在 `device.lost` 中暴露。

use crate::error::{GpuError, GpuResult};

/// 规范 02 冻结的 Philox4x32-10 WGSL 源（`crates/rynvek-wgpu/shaders/common/philox_rng.wgsl`）
pub const PHILOX_RNG_WGSL: &str = include_str!("../shaders/common/philox_rng.wgsl");
/// BindGroup 公共约定与诊断计数器
pub const BINDINGS_WGSL: &str = include_str!("../shaders/common/bindings.wgsl");

/// 核心着色器标识
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderSource {
    /// 批量 Philox4x32-10（差分测试与随机流预生成）
    PhiloxFill,
    /// 场扩散一个 CFL 子步（规范 06 §6.1）
    FieldDiffuse,
    /// Neumann 零梯度 Halo 拷边
    FieldFillHalo,
    /// 场内区逐行求和（守恒与差分校验）
    FieldRowSums,
    /// LIF 神经元步进与四因子可塑性（规范 05）
    LifStep,
    /// LIF 6 维动作读出
    LifReadout,
}

impl ShaderSource {
    pub const ALL: [Self; 6] = [
        Self::PhiloxFill,
        Self::FieldDiffuse,
        Self::FieldFillHalo,
        Self::FieldRowSums,
        Self::LifStep,
        Self::LifReadout,
    ];

    /// 调试标签
    pub const fn label(self) -> &'static str {
        match self {
            Self::PhiloxFill => "rynvek.compute.philox_fill",
            Self::FieldDiffuse => "rynvek.compute.field_diffuse",
            Self::FieldFillHalo => "rynvek.compute.field_fill_halo",
            Self::FieldRowSums => "rynvek.compute.field_row_sums",
            Self::LifStep => "rynvek.compute.lif_step",
            Self::LifReadout => "rynvek.compute.lif_readout",
        }
    }

    /// 计算入口函数名
    pub const fn entry_point(self) -> &'static str {
        match self {
            Self::PhiloxFill => "philox_fill",
            Self::FieldDiffuse => "field_diffuse",
            Self::FieldFillHalo => "field_fill_halo",
            Self::FieldRowSums => "field_row_sums",
            Self::LifStep => "lif_step",
            Self::LifReadout => "lif_readout",
        }
    }

    /// 内核本体源码
    pub const fn body(self) -> &'static str {
        match self {
            Self::PhiloxFill => include_str!("../shaders/compute/philox_fill.wgsl"),
            Self::FieldDiffuse => include_str!("../shaders/compute/field_diffusion.wgsl"),
            Self::FieldFillHalo => include_str!("../shaders/compute/field_halo.wgsl"),
            Self::FieldRowSums => include_str!("../shaders/compute/field_row_sums.wgsl"),
            Self::LifStep => include_str!("../shaders/compute/lif_step.wgsl"),
            Self::LifReadout => include_str!("../shaders/compute/lif_readout.wgsl"),
        }
    }

    /// 是否依赖 Philox 公共函数
    pub const fn needs_philox(self) -> bool {
        matches!(self, Self::PhiloxFill)
    }

    /// Workgroup 尺寸（规范 01 §1.1：线性 128×1×1，二维 8×8×1）
    pub const fn workgroup_size(self) -> [u32; 3] {
        match self {
            Self::FieldDiffuse | Self::FieldFillHalo => [8, 8, 1],
            _ => [128, 1, 1],
        }
    }
}

/// 着色器库：负责组合与校验
#[derive(Clone, Copy, Debug, Default)]
pub struct ShaderLibrary;

impl ShaderLibrary {
    /// 组合出完整的 WGSL 模块源码
    pub fn compose(source: ShaderSource) -> String {
        let mut out = String::with_capacity(BINDINGS_WGSL.len() + source.body().len() + 4096);
        out.push_str(BINDINGS_WGSL);
        out.push('\n');
        if source.needs_philox() {
            out.push_str(PHILOX_RNG_WGSL);
            out.push('\n');
        }
        out.push_str(source.body());
        out
    }

    /// 预校验并在设备上创建着色器模块
    pub fn create_module(device: &wgpu::Device, source: ShaderSource) -> GpuResult<wgpu::ShaderModule> {
        let code = Self::compose(source);
        Self::create_module_from_wgsl(device, source.label(), &code)
    }

    /// 对任意 WGSL（如规则编译器产物）预校验后创建模块
    pub fn create_module_from_wgsl(device: &wgpu::Device, label: &str, code: &str) -> GpuResult<wgpu::ShaderModule> {
        validate_wgsl(label, code)?;
        Ok(device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(code.into()),
        }))
    }
}

/// 用 naga 30 解析并校验 WGSL；失败返回 `WgslShaderValidationFailed (0x6004)`
pub fn validate_wgsl(label: &str, code: &str) -> GpuResult<naga::Module> {
    let module = naga::front::wgsl::parse_str(code)
        .map_err(|e| GpuError::shader(format!("{label}: WGSL 解析失败：{}", e.emit_to_string(code))))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map_err(|e| GpuError::shader(format!("{label}: WGSL 校验失败：{}", e.emit_to_string(code))))?;
    Ok(module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_core::error::RynvekErrorCode;

    #[test]
    fn all_core_shaders_validate_and_declare_spec_workgroup_sizes() {
        for s in ShaderSource::ALL {
            let module = validate_wgsl(s.label(), &ShaderLibrary::compose(s)).expect("核心着色器必须通过 naga 校验");
            let ep = module
                .entry_points
                .iter()
                .find(|e| e.name == s.entry_point())
                .expect("入口存在");
            assert_eq!(ep.stage, naga::ShaderStage::Compute);
            assert_eq!(ep.workgroup_size, s.workgroup_size(), "{}", s.label());
        }
    }

    #[test]
    fn philox_source_is_the_frozen_spec_file() {
        assert!(PHILOX_RNG_WGSL.contains("fn philox4x32_10(ctr_in: vec4<u32>, key_in: vec2<u32>) -> vec4<u32>"));
        assert!(PHILOX_RNG_WGSL.contains("0xD2511F53u"));
    }

    #[test]
    fn invalid_wgsl_maps_to_0x6004() {
        let err = validate_wgsl("bad", "fn main( { }").unwrap_err();
        assert_eq!(err.code, RynvekErrorCode::WgslShaderValidationFailed);
        assert_eq!(err.code as u16, 0x6004);
        // 类型错误（解析通过但校验失败）同样归入 0x6004
        let err = validate_wgsl("bad-type", "fn f() -> u32 { return 1.5; }").unwrap_err();
        assert_eq!(err.code, RynvekErrorCode::WgslShaderValidationFailed);
    }
}
