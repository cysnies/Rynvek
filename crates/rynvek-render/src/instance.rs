// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! GPU Instancing 实例记录与 GPU ID 拾取编号。

use bytemuck::{Pod, Zeroable};

use crate::layer::RenderLayer;
use crate::palette::Rgba8;

/// GPU ID 拾取编号：`dense_index + 1`，0 表示「无对象」（R32Uint 拾取目标清屏值）
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PickId(pub u32);

impl PickId {
    pub const NONE: u32 = 0;

    /// 由实体稠密索引构造（`u32::MAX` 无法表示，返回 `None`）
    pub const fn from_dense_index(dense_index: u32) -> Option<Self> {
        match dense_index.checked_add(1) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// 由拾取目标原始值还原
    pub const fn from_raw(raw: u32) -> Option<Self> {
        if raw == Self::NONE { None } else { Some(Self(raw)) }
    }

    pub const fn dense_index(self) -> u32 {
        self.0 - 1
    }
}

/// 图元形状
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceShape {
    /// 方块（建筑、迷雾格、选择框）
    Square = 0,
    /// 圆（生命体、神经元节点）
    Circle = 1,
    /// 空心环（同步脉冲环、拾取高亮）
    Ring = 2,
    /// 轴对齐矩形轮廓；复用32字节标签记录，size为宽、rotation槽为高。
    RectangleOutline = 3,
}

/// 实例记录（32 字节，`VertexStepMode::Instance`）
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct InstanceData {
    /// 中心世界坐标（格）
    pub position: [f32; 2],
    /// 边长 / 直径（格）
    pub size: f32,
    /// 旋转（弧度）；RectangleOutline 标签下为矩形高度（格）
    pub rotation: f32,
    /// `Rgba8::packed()`
    pub color: u32,
    /// 拾取编号原始值（0 = 不可拾取）
    pub pick_id: u32,
    /// 渲染层编号（0–7）
    pub layer: u32,
    /// [`InstanceShape`]
    pub shape: u32,
}

impl InstanceData {
    pub fn new(layer: RenderLayer, position: [f32; 2], size: f32, color: Rgba8, shape: InstanceShape) -> Self {
        Self {
            position,
            size,
            rotation: 0.0,
            color: color.packed(),
            pick_id: PickId::NONE,
            layer: u32::from(layer.index()),
            shape: shape as u32,
        }
    }

    /// 矩形轮廓采用带标签的宽/高记录，不改变实例ABI或新增渲染管线。
    pub fn rectangle_outline(position: [f32; 2], extent: [f32; 2], color: Rgba8) -> Self {
        let mut instance = Self::new(
            RenderLayer::Overlay,
            position,
            extent[0].max(0.0),
            color,
            InstanceShape::RectangleOutline,
        );
        instance.rotation = extent[1].max(0.0);
        instance
    }

    /// 附加拾取编号（不可拾取层忽略）
    pub fn with_pick(mut self, id: PickId) -> Self {
        let pickable = RenderLayer::from_index(self.layer as u8).is_some_and(RenderLayer::is_pickable);
        self.pick_id = if pickable { id.0 } else { PickId::NONE };
        self
    }

    pub fn with_rotation(mut self, radians: f32) -> Self {
        self.rotation = radians;
        self
    }

    pub fn render_layer(&self) -> Option<RenderLayer> {
        RenderLayer::from_index(self.layer as u8)
    }

    /// 顶点布局（location 0–5）
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32,
        2 => Float32,
        3 => Uint32,
        4 => Uint32,
        5 => Uint32x2,
    ];

    pub fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_is_32_bytes_and_pick_ids_reserve_zero() {
        assert_eq!(std::mem::size_of::<InstanceData>(), 32);
        assert_eq!(PickId::from_dense_index(0), Some(PickId(1)));
        assert_eq!(PickId::from_dense_index(u32::MAX), None);
        assert_eq!(PickId::from_raw(0), None);
        assert_eq!(PickId(42).dense_index(), 41);
    }

    #[test]
    fn non_pickable_layers_drop_pick_id() {
        let fog = InstanceData::new(
            RenderLayer::FogOfWar,
            [0.0; 2],
            1.0,
            Rgba8::hex(0),
            InstanceShape::Square,
        )
        .with_pick(PickId(5));
        assert_eq!(fog.pick_id, 0);
        let org = InstanceData::new(
            RenderLayer::Organisms,
            [0.0; 2],
            1.0,
            Rgba8::hex(0),
            InstanceShape::Circle,
        )
        .with_pick(PickId(5));
        assert_eq!(org.pick_id, 5);
    }
    #[test]
    fn rectangle_outline_keeps_tagged_32_byte_abi_and_zero_pick_id() {
        let rect = InstanceData::rectangle_outline([3.0, 5.5], [2.0, 3.0], Rgba8::hex(0xEAD686));
        assert_eq!(std::mem::size_of::<InstanceData>(), 32);
        assert_eq!([rect.size, rect.rotation], [2.0, 3.0]);
        assert_eq!(rect.shape, InstanceShape::RectangleOutline as u32);
        assert_eq!(rect.pick_id, 0);
        assert_eq!(rect.render_layer(), Some(RenderLayer::Overlay));
    }
}
