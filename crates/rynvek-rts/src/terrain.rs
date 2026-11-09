// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 五种标准地形 Tile 的物理属性（项目文档 §5.5.1 第 2 条）。
//!
//! | Tile | 移速系数 | 建造 | 其他 |
//! | --- | --- | --- | --- |
//! | `OpenSubstrate` | 1.00x | 可自由建造 | — |
//! | `ConductiveBasalt` | 1.10x | 可建造，建筑护甲 `+2` | — |
//! | `NutrientMarsh` | 0.85x | 可建造 | 每 Tick 再生 `+0.05 field.nutrient`（至 `80.0`） |
//! | `AcidicFissure` | 规范未给出（取 1.00x） | 仅 `Flux Reactor`（发电 `+50%`） | 地面单位每 Tick 受 `1.5` 灼烧伤害 |
//! | `ImperviousWall` | 不可通行 | 不可建造 | 完全阻挡视线与直线弹道 |

use crate::ids::BuildingKind;
pub use crate::map_gen::TerrainTileKind;

/// 移速系数（千分比）；墙体为 0（不可通行）
pub const fn speed_factor_milli(kind: TerrainTileKind) -> i64 {
    match kind {
        TerrainTileKind::OpenSubstrate => 1000,
        TerrainTileKind::ConductiveBasalt => 1100,
        TerrainTileKind::NutrientMarsh => 850,
        // 规范只规定灼烧伤害，未规定移速；按最佳实践取标准地面 1.00x（ERRATA E-022）
        TerrainTileKind::AcidicFissure => 1000,
        TerrainTileKind::ImperviousWall => 0,
    }
}

/// 是否可通行
pub const fn is_passable(kind: TerrainTileKind) -> bool {
    !matches!(kind, TerrainTileKind::ImperviousWall)
}

/// 是否阻挡视线与直线弹道
pub const fn blocks_line_of_sight(kind: TerrainTileKind) -> bool {
    matches!(kind, TerrainTileKind::ImperviousWall)
}

/// 指定建筑能否覆盖该地形 Tile
pub const fn allows_building(kind: TerrainTileKind, building: BuildingKind) -> bool {
    match kind {
        TerrainTileKind::ImperviousWall => false,
        TerrainTileKind::AcidicFissure => matches!(building, BuildingKind::FluxReactor),
        _ => true,
    }
}

/// 导电玄武岩上建造的建筑护甲加成 `+2`
pub const BASALT_BUILDING_ARMOR_BONUS: i64 = 2;

/// 热液裂隙上的通量反应堆发电加成 `+50%`（`60 → 90 Flux/s`）
pub const FISSURE_REACTOR_BONUS_PERCENT: i64 = 50;

/// 裂隙灼烧：普通地面单位每 Tick 受 `1.5` 点伤害（毫单位）
pub const FISSURE_BURN_MILLI_PER_TICK: i64 = 1_500;

/// 富营养沼泽每 Tick 自然再生 `+0.05 field.nutrient`
pub const MARSH_NUTRIENT_REGEN_MILLI_PER_TICK: i64 = 50;

/// 沼泽营养再生上限 `80.0`
pub const MARSH_NUTRIENT_REGEN_CAP_MILLI: i64 = 80_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_properties_match_design_bible() {
        assert_eq!(speed_factor_milli(TerrainTileKind::OpenSubstrate), 1000);
        assert_eq!(speed_factor_milli(TerrainTileKind::ConductiveBasalt), 1100);
        assert_eq!(speed_factor_milli(TerrainTileKind::NutrientMarsh), 850);
        assert!(!is_passable(TerrainTileKind::ImperviousWall));
        assert!(blocks_line_of_sight(TerrainTileKind::ImperviousWall));
        assert!(!blocks_line_of_sight(TerrainTileKind::AcidicFissure));
        assert!(allows_building(TerrainTileKind::AcidicFissure, BuildingKind::FluxReactor));
        assert!(!allows_building(TerrainTileKind::AcidicFissure, BuildingKind::BiomassHarvester));
        assert!(!allows_building(TerrainTileKind::ImperviousWall, BuildingKind::FluxReactor));
        assert!(allows_building(TerrainTileKind::NutrientMarsh, BuildingKind::BiomassHarvester));
    }
}
