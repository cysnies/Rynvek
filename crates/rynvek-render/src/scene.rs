// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `VisualizationProvider`：只读快照 → 帧场景的解耦契约（W7 表现与分析解耦层）。
//!
//! Provider 只拿到 `&S`（`Phase 15` 发布的只读快照），在类型签名上无法修改权威状态；
//! 多个 Provider（地形、场、单位、神经叠加、`.rview` 视觉包……）各自负责若干渲染层，
//! 由 [`collect_scene`] 按 LOD 与层开关汇总。

use crate::instance::InstanceData;
use crate::layer::{RenderLayer, RenderLayerMask};
use crate::lod::NeuralLod;

/// 一帧待绘制的场景
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameScene {
    /// 快照对应的权威 Tick
    pub tick: u64,
    /// 启用的渲染层
    pub layers: RenderLayerMask,
    /// 各层实例（按层编号分桶，保证绘制顺序与层顺序一致）
    instances: [Vec<InstanceData>; 8],
}

impl FrameScene {
    pub fn new(tick: u64, layers: RenderLayerMask) -> Self {
        Self {
            tick,
            layers,
            instances: Default::default(),
        }
    }

    /// 追加实例；所属层被关闭时直接丢弃
    pub fn push(&mut self, instance: InstanceData) {
        let accepted = instance
            .render_layer()
            // 静态地图复用 L0 实例；L1 热力图仍只读取既有场缓冲。
            .filter(|l| *l != RenderLayer::FieldHeatmap && self.layers.contains(*l));
        if let Some(layer) = accepted {
            self.instances[layer.index() as usize].push(instance);
        }
    }

    pub fn layer_instances(&self, layer: RenderLayer) -> &[InstanceData] {
        &self.instances[layer.index() as usize]
    }

    /// 按 L0→L7 顺序展平（GPU 实例缓冲的上传顺序）
    pub fn ordered_instances(&self) -> Vec<InstanceData> {
        self.instances.iter().flatten().copied().collect()
    }

    pub fn instance_count(&self) -> usize {
        self.instances.iter().map(Vec::len).sum()
    }
}

/// 可视化提供者
pub trait VisualizationProvider<S: ?Sized> {
    /// 稳定标识（日志与 `.rview` 绑定）
    fn id(&self) -> &str;
    /// 负责的渲染层
    fn layers(&self) -> RenderLayerMask;
    /// 读取只读快照，向场景追加图元
    fn populate(&self, snapshot: &S, lod: NeuralLod, scene: &mut FrameScene);
}

/// 汇总全部 Provider：只调用与当前启用层有交集的 Provider
pub fn collect_scene<S: ?Sized>(
    snapshot: &S,
    tick: u64,
    lod: NeuralLod,
    enabled: RenderLayerMask,
    providers: &[&dyn VisualizationProvider<S>],
) -> FrameScene {
    let layers = RenderLayerMask(enabled.0 & lod.visible_layers().0);
    let mut scene = FrameScene::new(tick, layers);
    for p in providers {
        if p.layers().0 & layers.0 != 0 {
            p.populate(snapshot, lod, &mut scene);
        }
    }
    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::{InstanceShape, PickId};
    use crate::palette::WorldPalette;

    struct Dots;

    impl VisualizationProvider<[[f32; 2]]> for Dots {
        fn id(&self) -> &str {
            "dots"
        }
        fn layers(&self) -> RenderLayerMask {
            RenderLayerMask::NONE.with(RenderLayer::Organisms)
        }
        fn populate(&self, snapshot: &[[f32; 2]], _lod: NeuralLod, scene: &mut FrameScene) {
            for (i, p) in snapshot.iter().enumerate() {
                scene.push(
                    InstanceData::new(
                        RenderLayer::Organisms,
                        *p,
                        1.0,
                        WorldPalette::faction(0),
                        InstanceShape::Circle,
                    )
                    .with_pick(PickId::from_dense_index(i as u32).unwrap()),
                );
            }
        }
    }

    #[test]
    fn provider_output_respects_lod_and_mask() {
        let snapshot = [[1.0, 1.0], [2.0, 2.0]];
        let dots = Dots;
        let near = collect_scene(&snapshot[..], 9, NeuralLod::L2, RenderLayerMask::ALL, &[&dots]);
        assert_eq!(near.tick, 9);
        assert_eq!(near.layer_instances(RenderLayer::Organisms).len(), 2);
        // L0 远景不逐个绘制个体
        let far = collect_scene(&snapshot[..], 9, NeuralLod::L0, RenderLayerMask::ALL, &[&dots]);
        assert_eq!(far.instance_count(), 0);
        let masked = collect_scene(
            &snapshot[..],
            9,
            NeuralLod::L2,
            RenderLayerMask::ALL.without(RenderLayer::Organisms),
            &[&dots],
        );
        assert_eq!(masked.instance_count(), 0);
    }

    #[test]
    fn ordered_instances_follow_layer_order() {
        let mut scene = FrameScene::new(0, RenderLayerMask::ALL);
        let c = WorldPalette::faction(1);
        scene.push(InstanceData::new(
            RenderLayer::Overlay,
            [0.0; 2],
            1.0,
            c,
            InstanceShape::Ring,
        ));
        scene.push(InstanceData::new(
            RenderLayer::Structures,
            [0.0; 2],
            1.0,
            c,
            InstanceShape::Square,
        ));
        // L0 同时支持网格底色和真实地形实例，仍按层顺序绘制。
        scene.push(InstanceData::new(
            RenderLayer::Terrain,
            [0.0; 2],
            1.0,
            c,
            InstanceShape::Square,
        ));
        let layers: Vec<u32> = scene.ordered_instances().iter().map(|i| i.layer).collect();
        assert_eq!(layers, [0, 2, 7]);
        scene.push(InstanceData::new(
            RenderLayer::FieldHeatmap,
            [0.0; 2],
            1.0,
            c,
            InstanceShape::Square,
        ));
        assert!(scene.layer_instances(RenderLayer::FieldHeatmap).is_empty());
        let mut masked = FrameScene::new(0, RenderLayerMask::ALL.without(RenderLayer::Terrain));
        masked.push(InstanceData::new(
            RenderLayer::Terrain,
            [0.0; 2],
            1.0,
            c,
            InstanceShape::Square,
        ));
        assert!(masked.layer_instances(RenderLayer::Terrain).is_empty());
    }
}
