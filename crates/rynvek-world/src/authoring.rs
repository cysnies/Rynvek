// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `WorldPatch` 创作态补丁、四级安全分类与对运行中 `ArenaWorld` 的真实确定性应用闭环（规范 10.2 / 审计报告 §11.1）。
//!
//! 线上 JSON 表示与 `@rynvek/studio` 的 `patch.ts` 严格逐字节对齐：
//! - `LiveEditSafetyClass`：Class 0 (`HotSafePresentation`) / Class 1 (`TickBoundarySafe`) / Class 2 (`EpochBoundarySafe`) / Class 3 (`RequiresForkRestart`)；
//! - `WorldPatchOp`：定义 `SetWorldParam`、`PaintTerrainRect`、`UpsertSeedGenome`、`UpdateRuleModule` 与 `RemoveEntityTemplate`；
//! - `WorldPatch::apply_to_arena`：支持地形和明确列出的资源发放；其余操作在接通前显式拒绝，整包先校验后应用。

use rynvek_core::error::RynvekErrorCode;
use rynvek_rts::nav::Rect;
use rynvek_rts::{ArenaWorld, TerrainTileKind};
use serde::{Deserialize, Serialize};

/// 实时修改的安全分级（与 `@rynvek/studio` 的 `LiveEditSafetyClass` 一一对应）
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LiveEditSafetyClass {
    /// Class 0：视觉颜色、UI 布局、AudioProfile 参数，任意渲染帧立即热生效，不影响权威状态哈希
    HotSafePresentation = 0,
    /// Class 1：环境场参数、建筑造价、地形涂刷，打包为系统命令在下一 Tick 边界生效
    TickBoundarySafe = 1,
    /// Class 2：神经网络拓扑规则、种子基因组更新、规则模块热重载，在下一 Coupling Epoch（`tick % 4 == 0`）原子切换
    EpochBoundarySafe = 2,
    /// Class 3：更改地图尺寸、空间维度或数值 Profile，必须创建新 WorldFork 分支重启
    RequiresForkRestart = 3,
}

/// 混合逻辑时钟时间戳（HLC）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HlcTimestamp {
    pub wall_time_ms: u64,
    pub logical_counter: u32,
    pub author_node_id: u32,
}

/// 世界创作补丁原子操作（外部标记枚举，与 TypeScript 镜像严格对齐）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorldPatchOp {
    SetWorldParam {
        key_path: String,
        new_value_json: String,
    },
    PaintTerrainRect {
        x0: i32,
        y0: i32,
        width: u32,
        height: u32,
        tile_kind: u8,
    },
    UpsertSeedGenome {
        genome_slug: String,
        genome_bytes_b64: String,
    },
    UpdateRuleModule {
        module_id: String,
        ts_source: String,
        compiled_rir_blake3: [u8; 32],
    },
    RemoveEntityTemplate {
        template_id: String,
    },
}

impl WorldPatchOp {
    /// 计算单个操作所需的最低安全分级
    pub fn required_safety_class(&self) -> LiveEditSafetyClass {
        match self {
            Self::SetWorldParam { key_path, .. } => {
                let ns = key_path.split('.').next().unwrap_or("");
                match ns {
                    "presentation" | "visual" | "ui" | "audio" => LiveEditSafetyClass::HotSafePresentation,
                    "field" | "economy" | "buildings" | "units" => LiveEditSafetyClass::TickBoundarySafe,
                    "neural" | "rules" => LiveEditSafetyClass::EpochBoundarySafe,
                    _ => LiveEditSafetyClass::RequiresForkRestart,
                }
            }
            Self::PaintTerrainRect { .. } | Self::RemoveEntityTemplate { .. } => LiveEditSafetyClass::TickBoundarySafe,
            Self::UpsertSeedGenome { .. } | Self::UpdateRuleModule { .. } => LiveEditSafetyClass::EpochBoundarySafe,
        }
    }
}

/// 创作态世界补丁包
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldPatch {
    pub patch_id: [u8; 16],
    pub base_authoring_revision: u64,
    pub hlc: HlcTimestamp,
    pub safety_class: LiveEditSafetyClass,
    pub operations: Vec<WorldPatchOp>,
}

/// 补丁应用结果回执
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchApplyReport {
    pub tiles_modified: u32,
    pub genomes_updated: u32,
    pub params_updated: u32,
    pub rules_updated: u32,
    pub templates_removed: u32,
    pub requires_fork: bool,
}

impl WorldPatch {
    /// 校验补丁安全分级是否不低于其包含操作的最低要求
    pub fn validate_safety(&self) -> Result<(), RynvekErrorCode> {
        if self.operations.is_empty() || self.operations.len() > 4096 {
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }
        let required = self
            .operations
            .iter()
            .map(WorldPatchOp::required_safety_class)
            .max()
            .unwrap_or(LiveEditSafetyClass::HotSafePresentation);
        if self.safety_class < required {
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }
        Ok(())
    }

    /// 将已支持的操作原子应用到 Arena：地形涂刷和正整数生物质发放。
    /// 基因、规则、模板及其他参数尚未接通，返回 `PluginCapabilityDenied`，不伪报成功。
    /// 此入口不会替代服务器的排队、权限、修订与 Tick 边界调度。
    pub fn apply_to_arena(&self, world: &mut ArenaWorld) -> Result<PatchApplyReport, RynvekErrorCode> {
        self.validate_safety()?;
        if self.safety_class == LiveEditSafetyClass::EpochBoundarySafe
            && world.tick % (rynvek_core::constants::COUPLING_EPOCH_TICKS as u64) != 0
        {
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }
        let mut report = PatchApplyReport {
            requires_fork: self.safety_class == LiveEditSafetyClass::RequiresForkRestart,
            ..Default::default()
        };
        if report.requires_fork {
            return Ok(report);
        }

        // 预检整包：所有会失败的输入检查必须先于任何权威状态修改。
        let mut total_tiles = 0u32;
        for op in &self.operations {
            match op {
                WorldPatchOp::PaintTerrainRect {
                    x0,
                    y0,
                    width,
                    height,
                    tile_kind,
                } => {
                    if *x0 < 0
                        || *y0 < 0
                        || *width == 0
                        || *height == 0
                        || (*x0 as u32).checked_add(*width).is_none_or(|x| x > world.map.width)
                        || (*y0 as u32).checked_add(*height).is_none_or(|y| y > world.map.height)
                    {
                        return Err(RynvekErrorCode::DimensionMismatch);
                    }
                    if *tile_kind > 4 {
                        return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
                    }
                    total_tiles = total_tiles
                        .checked_add(width.checked_mul(*height).ok_or(RynvekErrorCode::DimensionMismatch)?)
                        .ok_or(RynvekErrorCode::DimensionMismatch)?;
                }
                WorldPatchOp::SetWorldParam {
                    key_path,
                    new_value_json,
                } if key_path == "economy.grant_biomass_milli" => {
                    let amount: i64 =
                        serde_json::from_str(new_value_json).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
                    if amount <= 0 {
                        return Err(RynvekErrorCode::DimensionMismatch);
                    }
                }
                _ => return Err(RynvekErrorCode::PluginCapabilityDenied),
            }
        }

        for op in &self.operations {
            match op {
                WorldPatchOp::PaintTerrainRect {
                    x0,
                    y0,
                    width,
                    height,
                    tile_kind,
                } => {
                    if *x0 < 0 || *y0 < 0 || *width == 0 || *height == 0 {
                        return Err(RynvekErrorCode::DimensionMismatch);
                    }
                    let (ux0, uy0) = (*x0 as u32, *y0 as u32);
                    let (ux1, uy1) = (ux0.saturating_add(*width), uy0.saturating_add(*height));
                    if ux1 > world.map.width || uy1 > world.map.height {
                        return Err(RynvekErrorCode::DimensionMismatch);
                    }
                    let kind = match tile_kind {
                        0 => TerrainTileKind::OpenSubstrate,
                        1 => TerrainTileKind::ConductiveBasalt,
                        2 => TerrainTileKind::NutrientMarsh,
                        3 => TerrainTileKind::AcidicFissure,
                        4 => TerrainTileKind::ImperviousWall,
                        _ => return Err(RynvekErrorCode::InvalidFileMagicOrVersion),
                    };
                    for y in uy0..uy1 {
                        for x in ux0..ux1 {
                            let idx = world.map.index(x, y);
                            world.map.tiles[idx] = kind;
                            report.tiles_modified += 1;
                        }
                    }
                    // 重建受影响区域的导航网格并保留现有建筑占地阻挡
                    let mut nav =
                        rynvek_rts::nav::NavGrid::from_terrain(world.map.width, world.map.height, &world.map.tiles);
                    for b in world.buildings.values() {
                        let r = b.rect();
                        for by in r.y0..r.y1 {
                            for bx in r.x0..r.x1 {
                                nav.set_occupied(bx, by, true);
                            }
                        }
                    }
                    world.nav = nav;
                    world.hpa.update_region(
                        &world.nav,
                        Rect {
                            x0: ux0,
                            y0: uy0,
                            x1: ux1,
                            y1: uy1,
                        },
                    );
                    world.invalidate_flow();
                }
                WorldPatchOp::SetWorldParam {
                    key_path,
                    new_value_json,
                } => {
                    if key_path == "economy.grant_biomass_milli" {
                        if let Ok(amount) = serde_json::from_str::<i64>(new_value_json) {
                            if amount > 0 {
                                for f in &mut world.factions {
                                    f.ledger.deposit(rynvek_rts::ResourceKind::Biomass, amount);
                                }
                            }
                        }
                    }
                    report.params_updated += 1;
                }
                WorldPatchOp::UpsertSeedGenome { .. }
                | WorldPatchOp::UpdateRuleModule { .. }
                | WorldPatchOp::RemoveEntityTemplate { .. } => return Err(RynvekErrorCode::PluginCapabilityDenied),
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_rts::MatchConfig;

    #[test]
    fn world_patch_paints_terrain_and_updates_arena_state_hash() {
        let mut world = ArenaWorld::new(MatchConfig::duel(42));
        let before = world.state_hash();
        let patch = WorldPatch {
            patch_id: [1u8; 16],
            base_authoring_revision: 0,
            hlc: HlcTimestamp {
                wall_time_ms: 1_000,
                logical_counter: 0,
                author_node_id: 1,
            },
            safety_class: LiveEditSafetyClass::TickBoundarySafe,
            operations: vec![WorldPatchOp::PaintTerrainRect {
                x0: 100,
                y0: 100,
                width: 4,
                height: 4,
                tile_kind: TerrainTileKind::ConductiveBasalt as u8,
            }],
        };
        let report = patch.apply_to_arena(&mut world).unwrap();
        assert_eq!(report.tiles_modified, 16);
        assert_eq!(world.map.tile(100, 100), TerrainTileKind::ConductiveBasalt);
        assert_ne!(world.state_hash(), before, "修改地形后世界状态根必须立即反映变更");
    }
    fn patch_with(operations: Vec<WorldPatchOp>) -> WorldPatch {
        WorldPatch {
            patch_id: [2; 16],
            base_authoring_revision: 0,
            hlc: HlcTimestamp::default(),
            safety_class: LiveEditSafetyClass::EpochBoundarySafe,
            operations,
        }
    }

    fn paint() -> WorldPatchOp {
        WorldPatchOp::PaintTerrainRect {
            x0: 100,
            y0: 100,
            width: 1,
            height: 1,
            tile_kind: 4,
        }
    }

    #[test]
    fn unsupported_patch_operations_must_not_report_success_or_partially_apply() {
        let unsupported = [
            WorldPatchOp::SetWorldParam {
                key_path: "field.diffusion".into(),
                new_value_json: "123".into(),
            },
            WorldPatchOp::UpsertSeedGenome {
                genome_slug: "test".into(),
                genome_bytes_b64: "e30=".into(),
            },
            WorldPatchOp::UpdateRuleModule {
                module_id: "test".into(),
                ts_source: "export {}".into(),
                compiled_rir_blake3: [0; 32],
            },
            WorldPatchOp::RemoveEntityTemplate {
                template_id: "test".into(),
            },
        ];
        for op in unsupported {
            let mut world = ArenaWorld::new(MatchConfig::duel(42));
            let before = world.state_hash();
            assert!(
                patch_with(vec![paint(), op]).apply_to_arena(&mut world).is_err(),
                "未接通操作不能伪报成功"
            );
            assert_eq!(world.state_hash(), before, "补丁失败必须保留原始世界");
        }
    }

    #[test]
    fn invalid_later_terrain_operation_rolls_back_the_whole_patch() {
        let mut world = ArenaWorld::new(MatchConfig::duel(42));
        let before = world.state_hash();
        let invalid = WorldPatchOp::PaintTerrainRect {
            x0: -1,
            y0: 0,
            width: 1,
            height: 1,
            tile_kind: 0,
        };
        assert!(patch_with(vec![paint(), invalid]).apply_to_arena(&mut world).is_err());
        assert_eq!(world.state_hash(), before, "不得留下前半段地形修改");
    }

    #[test]
    fn invalid_biomass_grants_fail_before_mutation() {
        for value in ["oops", "-1", "0", "1.5"] {
            let mut world = ArenaWorld::new(MatchConfig::duel(42));
            let before = world.state_hash();
            let grant = WorldPatchOp::SetWorldParam {
                key_path: "economy.grant_biomass_milli".into(),
                new_value_json: value.into(),
            };
            assert!(patch_with(vec![paint(), grant]).apply_to_arena(&mut world).is_err());
            assert_eq!(world.state_hash(), before);
        }
    }

    #[test]
    fn real_patch_survives_snapshot_restore_and_continuation() {
        let mut world = ArenaWorld::new(MatchConfig::duel(42));
        let grant = WorldPatchOp::SetWorldParam {
            key_path: "economy.grant_biomass_milli".into(),
            new_value_json: "1000".into(),
        };
        let before = world.state_hash();
        let report = patch_with(vec![paint(), grant]).apply_to_arena(&mut world).unwrap();
        assert_eq!(report.tiles_modified, 1);
        assert_eq!(report.params_updated, 1);
        assert_ne!(world.state_hash(), before);
        let (bytes, _) = world.encode_snapshot([2; 16], 1, [0; 32]).unwrap();
        let mut restored = ArenaWorld::decode_snapshot(&bytes).unwrap();
        for _ in 0..12 {
            assert_eq!(restored.state_hash(), world.state_hash());
            world.step();
            restored.step();
        }
        assert_eq!(restored.state_hash(), world.state_hash());
    }
}
