// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! General原权威观察的最小只读表现投影，不含账户、规则源码、完整快照或本地仿真。
use rynvek_render::{FrameScene, RenderLayerMask};
use rynvek_simulation::{
    SimulationEngine,
    physics::{Body, MAX_BODIES, ONE},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneralCircle {
    pub serial: u32,
    pub position_q32: [i64; 2],
    pub radius_q32: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneralRenderFrame {
    pub format: String,
    pub world_id: String,
    pub tick: u64,
    pub revision: u64,
    pub state_root: String,
    pub bodies: Vec<GeneralCircle>,
}
impl GeneralRenderFrame {
    pub fn capture(w: &SimulationEngine) -> Self {
        let mut bodies: Vec<_> = w
            .physics()
            .iter()
            .map(|(serial, b)| GeneralCircle {
                serial,
                position_q32: b.position_q32,
                radius_q32: b.radius_q32,
            })
            .collect();
        bodies.sort_by_key(|b| b.serial);
        Self {
            format: "rynvek-general-frame/1".into(),
            world_id: crate::protocol::to_hex(&w.world_id),
            tick: w.clock.tick,
            revision: w.clock.world_revision,
            state_root: crate::protocol::to_hex(&w.parent_commit_hash),
            bodies,
        }
    }
    pub fn validate(&self) -> bool {
        self.format == "rynvek-general-frame/1"
            && crate::protocol::parse_world_id(&self.world_id).is_some()
            && self.tick == self.revision
            && self.tick <= 9_007_199_254_740_991
            && self.state_root.len() == 64
            && self
                .state_root
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && self.bodies.len() <= MAX_BODIES
            && self.bodies.windows(2).all(|w| w[0].serial < w[1].serial)
            && self.bodies.iter().all(|b| {
                b.serial > 0
                    && b.serial < u32::MAX
                    && Body {
                        position_q32: b.position_q32,
                        radius_q32: b.radius_q32,
                        velocity_q32: [0; 2],
                        acceleration_q32: [0; 2],
                        mass_q32: ONE,
                        fixed: true,
                    }
                    .validate()
                    .is_ok()
            })
    }
    pub fn scene(&self, mask: RenderLayerMask) -> FrameScene {
        let mut scene = FrameScene::new(self.tick, mask);
        let p = crate::provider::EntityProvider::default();
        for b in &self.bodies {
            p.push_circle(b.serial, b.position_q32, b.radius_q32, &mut scene);
        }
        scene
    }
}
