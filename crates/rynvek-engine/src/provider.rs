// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 引擎内置的可视化提供者：只读消费 `SimulationEngine` 快照。

use rynvek_render::{
    FrameScene, InstanceData, InstanceShape, NeuralLod, PickId, RenderLayer, RenderLayerMask, VisualizationProvider,
    WorldPalette,
};
use rynvek_simulation::engine::SimulationEngine;

/// Q32.32 局部坐标 → 世界格（表现层浮点）
pub fn q32_to_cells(v: u64) -> f32 {
    (v >> 32) as f32 + (v as u32) as f32 / 4_294_967_296.0
}

/// L3 生命体：每个实体一个可拾取圆形实例（`PickId = dense_index + 1`）
#[derive(Clone, Copy, Debug, Default)]
pub struct EntityProvider;

impl VisualizationProvider<SimulationEngine> for EntityProvider {
    fn id(&self) -> &str {
        "rynvek.entities"
    }

    fn layers(&self) -> RenderLayerMask {
        RenderLayerMask::NONE.with(RenderLayer::Organisms)
    }

    fn populate(&self, snapshot: &SimulationEngine, _lod: NeuralLod, scene: &mut FrameScene) {
        for (i, &(x, y)) in snapshot.positions.iter().enumerate() {
            let Some(pick) = PickId::from_dense_index(i as u32) else {
                break;
            };
            scene.push(
                InstanceData::new(
                    RenderLayer::Organisms,
                    [q32_to_cells(x), q32_to_cells(y)],
                    0.8,
                    WorldPalette::faction(i % 7),
                    InstanceShape::Circle,
                )
                .with_pick(pick),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_render::collect_scene;

    #[test]
    fn entities_become_pickable_organisms() {
        assert_eq!(q32_to_cells(5u64 << 32), 5.0);
        assert_eq!(q32_to_cells((2u64 << 32) | (1 << 31)), 2.5);
        let sim = SimulationEngine::default();
        let scene = collect_scene(&sim, 0, NeuralLod::L2, RenderLayerMask::ALL, &[&EntityProvider]);
        let org = scene.layer_instances(RenderLayer::Organisms);
        assert_eq!(org.len(), sim.positions.len());
        assert_eq!(org[0].pick_id, 1);
        assert_eq!(org[0].position, [2.0, 2.0]);
    }
}
