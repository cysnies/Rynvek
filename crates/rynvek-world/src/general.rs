// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 原 RYWL 容器的非 RTS 世界入口；仅此明确物理剖面，不是任意 World 插件执行器。
use rynvek_core::package::{PackageError, PackageInput, PackageInputEntry, PackageKind, read_package, write_package};
use rynvek_simulation::{
    engine::SimulationEngine,
    life::{LifeProfile, NutrientDeposit},
    persistence::PHYSICS_PROFILE,
    physics::{Body, MAX_BODIES},
};
use serde::{Deserialize, Serialize};
const ERROR: rynvek_core::RynvekErrorCode = rynvek_core::RynvekErrorCode::InvalidFileMagicOrVersion;
pub const MAX_GENERAL_PACKAGE_BYTES: usize = 128 * 1024;
pub const GENERAL_ENGINE_COMPAT: &str = ">=0.1.0 <0.2.0";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralWorldConfig {
    pub schema: String,
    pub profile: String,
    pub bodies: Vec<Body>,
    /// 缺省为纯物理世界；启用时必须给出有来源场沉淀，不能只写初始内能。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub life: Option<GeneralLifeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<rynvek_simulation::rules::GeneralRulesConfig>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralLifeConfig {
    pub profile: LifeProfile,
    #[serde(default)]
    pub nutrient_deposits: Vec<NutrientDeposit>,
}
impl GeneralWorldConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != "rynvek.general-world.v1" || self.profile != PHYSICS_PROFILE || self.bodies.len() > MAX_BODIES
        {
            return Err("通用 World schema、物理剖面或实体上限非法".into());
        }
        for body in &self.bodies {
            body.validate().map_err(|e| format!("刚体超出物理剖面：{e:?}"))?;
        }
        if let Some(life) = &self.life {
            life.profile.validate().map_err(|e| format!("生命剖面非法：{e:?}"))?;
            rynvek_simulation::life::validate_deposits(&life.nutrient_deposits)
                .map_err(|e| format!("场沉淀非法：{e:?}"))?;
            if life.nutrient_deposits.is_empty()
                && life.profile.maintenance_q_per_tick > 0
                && self.bodies.iter().any(|b| !b.fixed)
            {
                return Err("有维护热耗的活体必须给出明确营养沉淀，不能凭空供能".into());
            }
        }
        if let Some(rules) = &self.components {
            rules.compile().map_err(|e| format!("声明组件非法：{e:?}"))?;
            rules
                .validate_genesis_host(&self.bodies, self.life.as_ref().map(|l| &l.profile))
                .map_err(|e| format!("声明组件与创世身体/生命剖面不兼容：{e:?}"))?;
        }
        Ok(())
    }
    pub fn instantiate(&self, world_id: [u8; 16]) -> Result<SimulationEngine, String> {
        self.validate()?;
        let mut world =
            SimulationEngine::with_bodies(world_id, &self.bodies).map_err(|e| format!("通用 World 创世失败：{e:?}"))?;
        if let Some(life) = &self.life {
            world = world
                .with_sourced_life(life.profile.clone(), &life.nutrient_deposits)
                .map_err(|e| format!("有来源生命创世失败：{e:?}"))?;
        }
        if let Some(rules) = &self.components {
            world = world
                .with_component_rules(rules.clone())
                .map_err(|e| format!("声明组件创世失败：{e:?}"))?;
        }
        Ok(world)
    }
    /// 使用唯一的原包写入器，不自建另一种 .rworld 格式。
    pub fn build_package(&self, id: &str, version: &str) -> Result<Vec<u8>, PackageError> {
        self.validate().map_err(invalid)?;
        let bytes = write_package(&PackageInput {
            kind: PackageKind::Rworld,
            id: id.into(),
            version: version.into(),
            name: id.into(),
            license: "CC0-1.0".into(),
            engine_compat: GENERAL_ENGINE_COMPAT.into(),
            entries: vec![PackageInputEntry {
                path: "world.json".into(),
                kind: "world_config".into(),
                media_type: "application/json".into(),
                data: serde_json::to_vec(self).map_err(|e| invalid(e.to_string()))?,
            }],
        })?;
        if bytes.len() > MAX_GENERAL_PACKAGE_BYTES {
            return Err(invalid("通用 World 包超限".into()));
        }
        Ok(bytes)
    }
    pub fn from_package_bytes(bytes: &[u8]) -> Result<Self, PackageError> {
        if bytes.len() > MAX_GENERAL_PACKAGE_BYTES {
            return Err(invalid("通用 World 包超限".into()));
        }
        let p = read_package(bytes, Some(PackageKind::Rworld))?;
        if p.manifest.engine_compat != GENERAL_ENGINE_COMPAT || p.manifest.entries.len() != 1 {
            return Err(invalid("通用 World 不支持此引擎范围、附加依赖或脚本条目".into()));
        }
        let entries: Vec<_> = p.entries_of("world_config").collect();
        let [(entry, data)] = entries.as_slice() else {
            return Err(invalid("必须仅有 world_config".into()));
        };
        if entry.path != "world.json" {
            return Err(invalid("配置路径必须为 world.json".into()));
        }
        let config: Self = serde_json::from_slice(data).map_err(|e| invalid(e.to_string()))?;
        config.validate().map_err(invalid)?;
        Ok(config)
    }
}
fn invalid(message: String) -> PackageError {
    PackageError {
        code: ERROR,
        detail: message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn general_rywl_roundtrip_is_not_arena_template() {
        let config = GeneralWorldConfig {
            schema: "rynvek.general-world.v1".into(),
            profile: PHYSICS_PROFILE.into(),
            bodies: vec![],
            life: None,
            components: None,
        };
        let bytes = config.build_package("world.general.empty", "1.0.0").unwrap();
        assert_eq!(GeneralWorldConfig::from_package_bytes(&bytes).unwrap(), config);
        assert!(crate::WorldBundle::from_bytes(&bytes).is_err());
        let world = config.instantiate([5; 16]).unwrap();
        assert_eq!(world.clock.tick, 0);
        assert_eq!(world.physics().iter().count(), 0);
        let mut broken = bytes;
        *broken.last_mut().unwrap() ^= 1;
        assert!(GeneralWorldConfig::from_package_bytes(&broken).is_err());
    }
    #[test]
    fn general_strict_schema_rejects_scripts_and_unsupported_profile() {
        assert!(serde_json::from_str::<GeneralWorldConfig>(r#"{"schema":"rynvek.general-world.v1","profile":"rynvek.general-circle-ecs.v1","bodies":[],"script":"while(true){}"}"#).is_err());
        let config = GeneralWorldConfig {
            schema: "rynvek.general-world.v1".into(),
            profile: "arbitrary-world".into(),
            bodies: vec![],
            life: None,
            components: None,
        };
        assert!(config.build_package("world.general.bad", "1.0.0").is_err());
    }
    #[test]
    fn general_components_build_rejects_cross_profile_pricing_and_stale_serial() {
        let original: GeneralWorldConfig =
            serde_json::from_str(include_str!("../../../examples/worlds/general-rules.world.json")).unwrap();
        let bytes = original.build_package("world.general.rules", "0.1.0").unwrap();
        assert_eq!(GeneralWorldConfig::from_package_bytes(&bytes).unwrap(), original);
        for kind in 0..3 {
            let mut bad = original.clone();
            let rules = bad.components.as_mut().unwrap();
            match kind {
                0 => rules.rule_cost_micro_per_gas = None,
                1 => rules.body_controllers[0].entity_serial = 2,
                _ => bad.bodies[0].fixed = true,
            }
            assert!(bad.build_package("world.general.rules", "0.1.0").is_err());
        }
    }

    #[test]
    fn sourced_life_rejects_maintenance_without_deposits() {
        use rynvek_simulation::life::{LIFE_PROFILE, LifeProfile};
        use rynvek_simulation::physics::{Body, ONE};
        let body = Body {
            position_q32: [10 * ONE, 64 * ONE],
            velocity_q32: [0, 0],
            acceleration_q32: [0, 0],
            radius_q32: ONE / 2,
            mass_q32: ONE,
            fixed: false,
        };
        let config = GeneralWorldConfig {
            schema: "rynvek.general-world.v1".into(),
            profile: PHYSICS_PROFILE.into(),
            bodies: vec![body],
            components: None,
            life: Some(GeneralLifeConfig {
                profile: LifeProfile {
                    profile: LIFE_PROFILE.into(),
                    max_ingest_raw_per_tick: 8,
                    maintenance_q_per_tick: 1,
                    recycle_percent: 80,
                    neural: None,
                },
                nutrient_deposits: vec![],
            }),
        };
        assert!(config.validate().is_err());
    }
}
