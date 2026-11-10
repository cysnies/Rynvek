// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 引擎内置的可视化提供者与双世界统一运行时门面（审计报告 §2）：
//! - [`EntityProvider`]：只读消费 `SimulationEngine` 快照；
//! - [`ArenaVisualizationProvider`]：只读消费 `ArenaWorld` 快照，将全部存活单位与工业建筑投影为 GPU 渲染 [`EntityInstance`]；
//! - [`WorldRuntime`]：统一封装 `SimulationEngine`（16-Phase 内核验收世界）与 `ArenaWorld`（RTS 宏观权威世界），消除双世界栈分裂。

use rynvek_core::constants::Q32_32_ONE;
use rynvek_render::{EntityInstance, FrameScene, NeuralLod, VisualizationProvider};
use rynvek_rts::{ArenaWorld, MatchConfig};
use rynvek_simulation::engine::{SimulationEngine, SimulationError};

/// 把 `SimulationEngine::positions`（Q32.32 局部坐标）投影为 [`EntityInstance`]
#[derive(Clone, Debug)]
pub struct EntityProvider {
    pub radius: f32,
    pub color: [f32; 4],
}

impl Default for EntityProvider {
    fn default() -> Self {
        Self {
            radius: 1.5,
            color: [0.36, 0.88, 0.90, 1.0],
        }
    }
}

impl VisualizationProvider<SimulationEngine> for EntityProvider {
    fn id(&self) -> &'static str {
        "rynvek.engine.entities"
    }

    fn min_lod(&self) -> NeuralLod {
        NeuralLod::Lod3MacroHeatmap
    }

    fn populate(&self, snapshot: &SimulationEngine, _lod: NeuralLod, scene: &mut FrameScene) {
        let scale = 1.0 / (Q32_32_ONE as f64);
        for (i, &(lx, ly)) in snapshot.positions.iter().enumerate() {
            let x = (lx as f64 * scale) as f32;
            let y = (ly as f64 * scale) as f32;
            scene.entities.push(EntityInstance {
                position: [x, y],
                radius: self.radius,
                archetype: 0,
                color: self.color,
                entity_id: (i as u32) + 1,
                generation: 1,
            });
        }
    }
}

/// 将 `ArenaWorld` 的全部存活工业建筑与神经原生单位投影为 [`FrameScene`] 渲染实例
#[derive(Clone, Debug, Default)]
pub struct ArenaVisualizationProvider;

impl ArenaVisualizationProvider {
    /// 按阵营返回标准 RGBA 视觉色值
    pub const fn faction_color(faction_id: u8, is_building: bool) -> [f32; 4] {
        let alpha = if is_building { 0.95 } else { 0.88 };
        match faction_id {
            0 => [0.36, 0.88, 0.90, alpha],
            1 => [0.95, 0.42, 0.38, alpha],
            u8::MAX => [0.76, 0.32, 0.88, alpha],
            _ => [0.64, 0.84, 0.36, alpha],
        }
    }
}

impl VisualizationProvider<ArenaWorld> for ArenaVisualizationProvider {
    fn id(&self) -> &'static str {
        "rynvek.engine.arena_entities"
    }

    fn min_lod(&self) -> NeuralLod {
        NeuralLod::Lod3MacroHeatmap
    }

    fn populate(&self, snapshot: &ArenaWorld, _lod: NeuralLod, scene: &mut FrameScene) {
        let scale = 1.0 / (Q32_32_ONE as f64);
        for b in snapshot.buildings.values() {
            let c = b.center();
            let x = (c.x.0 as f64 * scale) as f32;
            let y = (c.y.0 as f64 * scale) as f32;
            let radius = (b.footprint() as f32) * 0.5;
            scene.entities.push(EntityInstance {
                position: [x, y],
                radius,
                archetype: 100 + (b.kind as u32),
                color: Self::faction_color(b.faction.0, true),
                entity_id: b.id.0,
                generation: 1,
            });
        }
        for u in snapshot.units.values() {
            let x = (u.position.x.0 as f64 * scale) as f32;
            let y = (u.position.y.0 as f64 * scale) as f32;
            let r = (u.radius().0 as f64 * scale) as f32;
            scene.entities.push(EntityInstance {
                position: [x, y],
                radius: r.max(0.35),
                archetype: u.kind as u32,
                color: Self::faction_color(u.faction.0, false),
                entity_id: u.id.0,
                generation: 1,
            });
        }
    }
}

/// 统一世界运行时：将 `SimulationEngine`（16-Phase 内核）与 `ArenaWorld`（RTS 完整对局世界）收敛到单一引擎调度与场景提取接口
#[derive(Debug)]
pub enum WorldRuntime {
    Simulation(Box<SimulationEngine>),
    Arena(Box<ArenaWorld>),
}

impl WorldRuntime {
    pub fn new_simulation(world_id: [u8; 16]) -> Self {
        Self::Simulation(Box::new(SimulationEngine::new(world_id)))
    }

    pub fn new_arena(config: MatchConfig) -> Self {
        Self::Arena(Box::new(ArenaWorld::new(config)))
    }

    /// 推进一个权威 Tick 并返回最新 32 字节世界状态根
    pub fn step_tick(&mut self) -> Result<[u8; 32], SimulationError> {
        match self {
            Self::Simulation(sim) => Ok(sim.tick()?.state_root_hash),
            Self::Arena(arena) => {
                arena.step();
                Ok(arena.state_hash().0)
            }
        }
    }

    /// 当前权威 Tick
    pub fn tick(&self) -> u64 {
        match self {
            Self::Simulation(sim) => sim.clock.tick,
            Self::Arena(arena) => arena.tick,
        }
    }

    /// 当前 32 字节世界状态根
    pub fn state_root_bytes(&self) -> [u8; 32] {
        match self {
            Self::Simulation(sim) => sim
                .last_manifest
                .as_ref()
                .map(|m| m.state_root_hash)
                .unwrap_or(sim.parent_commit_hash),
            Self::Arena(arena) => arena.state_hash().0,
        }
    }

    /// 向渲染帧场景填充当前世界的所有可视实体
    pub fn populate_scene(&self, lod: NeuralLod, scene: &mut FrameScene) {
        match self {
            Self::Simulation(sim) => {
                EntityProvider::default().populate(sim, lod, scene);
            }
            Self::Arena(arena) => {
                ArenaVisualizationProvider.populate(arena, lod, scene);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_render::ProviderRegistry;

    #[test]
    fn populates_four_initial_entities_from_simulation() {
        let sim = SimulationEngine::default();
        let mut reg = ProviderRegistry::new();
        reg.register(EntityProvider::default());
        let mut scene = FrameScene::default();
        reg.collect(&sim, NeuralLod::Lod0SingleSynapse, &mut scene);
        assert_eq!(scene.entities.len(), 4);
        assert_eq!(scene.entities[0].position, [2.0, 2.0]);
        assert_eq!(scene.entities[0].entity_id, 1);
    }

    #[test]
    fn unified_world_runtime_populates_arena_entities_and_steps() {
        let mut rt = WorldRuntime::new_arena(MatchConfig::duel(42));
        let mut scene = FrameScene::default();
        rt.populate_scene(NeuralLod::Lod1ClusterAggregate, &mut scene);
        // 2 个阵营 × (1 主脑 + 3 Forager + 1 Striker) = 10 个初始实体
        assert_eq!(scene.entities.len(), 10);
        let root0 = rt.state_root_bytes();
        let root1 = rt.step_tick().unwrap();
        assert_eq!(rt.tick(), 1);
        assert_ne!(root0, root1);
    }
}
