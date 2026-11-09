// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! BindGroup 0–3 冻结约定（目录树「BindGroup 0-3」；规范未给出各组职责，按最佳实践冻结，ERRATA E-014）。
//!
//! | 组 | 名称 | 内容 |
//! | :--- | :--- | :--- |
//! | 0 | `Frame` | 本次调度的参数 Uniform（binding 0）；帧常量只读数据（binding 1..）；`s_diagnostics`（binding 15） |
//! | 1 | `StateIn` | 只读输入状态（`Committed[t-1]` 视图） |
//! | 2 | `StateOut` | 读写输出状态（`CandidateState[t]`） |
//! | 3 | `Auxiliary` | 辅助数据：场采样/沉积、CSR 邻居表、统计摘要 |
//!
//! 该约定与 `rynvek-rules` 生成的规则 WGSL（M3）一致；同一 `StateIn/StateOut` 划分使「读旧写新」的双缓冲语义
//! 在着色器接口层面即可审计。

/// 绑定组槽位
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindGroupSlot {
    Frame = 0,
    StateIn = 1,
    StateOut = 2,
    Auxiliary = 3,
}

impl BindGroupSlot {
    pub const ALL: [Self; 4] = [Self::Frame, Self::StateIn, Self::StateOut, Self::Auxiliary];

    pub const fn index(self) -> u32 {
        self as u32
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Frame => "rynvek.bind_group.0.frame",
            Self::StateIn => "rynvek.bind_group.1.state_in",
            Self::StateOut => "rynvek.bind_group.2.state_out",
            Self::Auxiliary => "rynvek.bind_group.3.auxiliary",
        }
    }
}

/// `s_diagnostics` 诊断计数器的位置：`@group(0) @binding(15)`，4 × `atomic<u32>`
pub const DIAGNOSTICS_BINDING: (BindGroupSlot, u32) = (BindGroupSlot::Frame, 15);
/// 诊断计数器缓冲字节数
pub const DIAGNOSTICS_BYTES: u64 = 16;

/// 绑定资源种类
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    Uniform,
    StorageReadOnly,
    StorageReadWrite,
}

/// 单个绑定的声明
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindingSpec {
    pub binding: u32,
    pub kind: BindingKind,
}

impl BindingSpec {
    pub const fn uniform(binding: u32) -> Self {
        Self {
            binding,
            kind: BindingKind::Uniform,
        }
    }

    pub const fn read(binding: u32) -> Self {
        Self {
            binding,
            kind: BindingKind::StorageReadOnly,
        }
    }

    pub const fn read_write(binding: u32) -> Self {
        Self {
            binding,
            kind: BindingKind::StorageReadWrite,
        }
    }

    /// 校验所在组是否符合约定：`StateIn` 只能只读，`StateOut` 不得是 Uniform
    pub const fn allowed_in(self, slot: BindGroupSlot) -> bool {
        match (slot, self.kind) {
            (BindGroupSlot::StateIn, BindingKind::StorageReadOnly) => true,
            (BindGroupSlot::StateIn, _) => false,
            (BindGroupSlot::StateOut, BindingKind::Uniform) => false,
            _ => true,
        }
    }

    pub fn layout_entry(self, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
        let ty = match self.kind {
            BindingKind::Uniform => wgpu::BufferBindingType::Uniform,
            BindingKind::StorageReadOnly => wgpu::BufferBindingType::Storage { read_only: true },
            BindingKind::StorageReadWrite => wgpu::BufferBindingType::Storage { read_only: false },
        };
        wgpu::BindGroupLayoutEntry {
            binding: self.binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_numbered_zero_to_three() {
        assert_eq!(BindGroupSlot::ALL.map(BindGroupSlot::index), [0, 1, 2, 3]);
        assert_eq!(DIAGNOSTICS_BINDING, (BindGroupSlot::Frame, 15));
    }

    #[test]
    fn state_in_is_read_only() {
        assert!(BindingSpec::read(0).allowed_in(BindGroupSlot::StateIn));
        assert!(!BindingSpec::read_write(0).allowed_in(BindGroupSlot::StateIn));
        assert!(!BindingSpec::uniform(0).allowed_in(BindGroupSlot::StateOut));
        assert!(BindingSpec::read_write(0).allowed_in(BindGroupSlot::StateOut));
    }
}
