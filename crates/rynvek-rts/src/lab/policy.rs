// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 确定性宏动作与脚本策略（Lab 批量实验与 Gymnasium / PettingZoo 离散动作空间共用）。
//!
//! 规范没有定义科研接口的动作空间，这里按最佳实践给出一组 "一步一个意图" 的宏动作（ERRATA E-046）：
//! 每个宏动作展开为 0..N 条普通 [`RtsCommand`]，经与玩家完全相同的 [`ArenaWorld::apply_command`] 校验，
//! 因此宏动作不会绕过任何规则、资源或迷雾约束。所有选择（采集点、建筑落点、单位集合）都按
//! 固定次序决出，同一状态总是展开为同一组命令。

use crate::commands::{RtsCommand, UnitSelector};
use crate::ids::{BuildingKind, EntitySerial, FactionId, OrderKind, UnitKind};
use crate::map_gen::TerrainTileKind;
use crate::math::Vec2;
use crate::world::ArenaWorld;

/// 离散宏动作（编号即 Gymnasium `Discrete` 动作值，顺序冻结）
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroAction {
    /// 什么也不做
    Noop = 0,
    /// 全部采集工蜂前往距主脑最近的营养沼泽采集
    Harvest = 1,
    /// 在主脑附近建造生物质采集器
    BuildHarvester = 2,
    /// 在主脑附近建造孵化槽
    BuildVat = 3,
    /// 第一个已完工孵化槽排队 1 只采集工蜂
    TrainForager = 4,
    /// 第一个已完工孵化槽排队 1 只突击虫
    TrainStriker = 5,
    /// 全部战斗单位攻击移动至敌方主脑（PvE 为地图中心）
    AttackMove = 6,
    /// 全部战斗单位回防己方主脑
    Defend = 7,
    /// 在主脑附近建造脉冲炮塔
    BuildTurret = 8,
}

impl MacroAction {
    /// 全部宏动作（按编号）
    pub const ALL: [Self; 9] = [
        Self::Noop,
        Self::Harvest,
        Self::BuildHarvester,
        Self::BuildVat,
        Self::TrainForager,
        Self::TrainStriker,
        Self::AttackMove,
        Self::Defend,
        Self::BuildTurret,
    ];

    pub fn from_index(i: u32) -> Option<Self> {
        Self::ALL.get(i as usize).copied()
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Noop => "noop",
            Self::Harvest => "harvest",
            Self::BuildHarvester => "build_harvester",
            Self::BuildVat => "build_vat",
            Self::TrainForager => "train_forager",
            Self::TrainStriker => "train_striker",
            Self::AttackMove => "attack_move",
            Self::Defend => "defend",
            Self::BuildTurret => "build_turret",
        }
    }
}

/// 一次宏动作的执行结果
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MacroOutcome {
    /// 被世界接受的命令数
    pub accepted: u32,
    /// 被拒绝的命令数（资源不足、落点非法等；不视为错误）
    pub rejected: u32,
}

/// 建筑落点搜索半径（切比雪夫环，Tiles）
pub const SITE_SEARCH_RADIUS: u32 = 24;

/// 主脑中心 Tile
pub fn nexus_tile(world: &ArenaWorld, faction: FactionId) -> Option<(u32, u32)> {
    let id = world.faction(faction)?.nexus?;
    world.buildings.get(&id).map(|b| b.center_tile())
}

/// 距 `from` 最近的营养沼泽 Tile（平方距离最小；并列按 `(y, x)` 升序）
pub fn nearest_marsh(world: &ArenaWorld, from: (u32, u32)) -> Option<(u32, u32)> {
    let mut best: Option<(u64, (u32, u32))> = None;
    for y in 0..world.map.height {
        for x in 0..world.map.width {
            if world.map.tile(x, y) != TerrainTileKind::NutrientMarsh {
                continue;
            }
            let dx = u64::from(x.abs_diff(from.0));
            let dy = u64::from(y.abs_diff(from.1));
            let d = dx * dx + dy * dy;
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, (x, y)));
            }
        }
    }
    best.map(|(_, t)| t)
}

/// 在 `center` 周围按切比雪夫环由内向外、环内 `(y, x)` 升序搜索第一个合法落点（与 E-036 前端约定一致）
pub fn find_site(world: &ArenaWorld, faction: FactionId, kind: BuildingKind, center: (u32, u32)) -> Option<(u32, u32)> {
    let (cx, cy) = (i64::from(center.0), i64::from(center.1));
    for r in 2..=i64::from(SITE_SEARCH_RADIUS) {
        for y in (cy - r)..=(cy + r) {
            for x in (cx - r)..=(cx + r) {
                if (x - cx).abs().max((y - cy).abs()) != r || x < 0 || y < 0 {
                    continue;
                }
                let origin = (x as u32, y as u32);
                if world.validate_placement(faction, kind, origin).is_ok() {
                    return Some(origin);
                }
            }
        }
    }
    None
}

fn own_units(world: &ArenaWorld, faction: FactionId, pred: impl Fn(UnitKind) -> bool) -> Vec<EntitySerial> {
    world
        .units
        .values()
        .filter(|u| u.faction == faction && pred(u.kind))
        .map(|u| u.id)
        .collect()
}

/// 第一个已完工的孵化槽
pub fn first_vat(world: &ArenaWorld, faction: FactionId) -> Option<EntitySerial> {
    world
        .buildings
        .values()
        .find(|b| b.faction == faction && b.kind == BuildingKind::SpawningVat && b.complete)
        .map(|b| b.id)
}

/// 统计己方某类建筑（含未完工）
pub fn building_count(world: &ArenaWorld, faction: FactionId, kind: BuildingKind) -> usize {
    world
        .buildings
        .values()
        .filter(|b| b.faction == faction && b.kind == kind)
        .count()
}

/// 统计己方某类单位
pub fn unit_count(world: &ArenaWorld, faction: FactionId, kind: UnitKind) -> usize {
    world
        .units
        .values()
        .filter(|u| u.faction == faction && u.kind == kind)
        .count()
}

/// 进攻目标：已知的敌方主脑（双人局）或地图中心（PvE）
fn attack_target(world: &ArenaWorld, faction: FactionId) -> Vec2 {
    world
        .factions
        .iter()
        .filter(|f| f.id != faction)
        .find_map(|f| nexus_tile(world, f.id))
        .map(|(x, y)| Vec2::tile_center(x, y))
        .unwrap_or_else(|| Vec2::tile_center(world.map.width / 2, world.map.height / 2))
}

/// 把宏动作展开为命令并逐条提交
pub fn apply_macro(world: &mut ArenaWorld, faction: FactionId, action: MacroAction) -> MacroOutcome {
    let commands = expand(world, faction, action);
    let mut out = MacroOutcome::default();
    for cmd in &commands {
        match world.apply_command(faction, cmd) {
            Ok(_) => out.accepted += 1,
            Err(_) => out.rejected += 1,
        }
    }
    out
}

/// 只展开不提交（供测试与调试）
pub fn expand(world: &ArenaWorld, faction: FactionId, action: MacroAction) -> Vec<RtsCommand> {
    let Some(home) = nexus_tile(world, faction) else {
        return Vec::new();
    };
    let build = |kind: BuildingKind| {
        find_site(world, faction, kind, home)
            .map(|origin| vec![RtsCommand::PlaceBuilding { kind, origin }])
            .unwrap_or_default()
    };
    let train = |unit: UnitKind, slug: &str| {
        first_vat(world, faction)
            .map(|vat| {
                vec![RtsCommand::QueueUnit {
                    vat,
                    unit,
                    genome_slug: slug.to_owned(),
                    count: 1,
                }]
            })
            .unwrap_or_default()
    };
    let order_combat = |order: OrderKind, target: Vec2| {
        let ids = own_units(world, faction, |k| k != UnitKind::Forager);
        if ids.is_empty() {
            Vec::new()
        } else {
            vec![RtsCommand::IssueOrder {
                units: UnitSelector::Ids(ids),
                order,
                target,
            }]
        }
    };
    match action {
        MacroAction::Noop => Vec::new(),
        MacroAction::Harvest => {
            let ids = own_units(world, faction, |k| k == UnitKind::Forager);
            match nearest_marsh(world, home) {
                Some((x, y)) if !ids.is_empty() => vec![RtsCommand::IssueOrder {
                    units: UnitSelector::Ids(ids),
                    order: OrderKind::Harvest,
                    target: Vec2::tile_center(x, y),
                }],
                _ => Vec::new(),
            }
        }
        MacroAction::BuildHarvester => build(BuildingKind::BiomassHarvester),
        MacroAction::BuildVat => build(BuildingKind::SpawningVat),
        MacroAction::BuildTurret => build(BuildingKind::PulseTurret),
        MacroAction::TrainForager => train(UnitKind::Forager, "seed.forager.v1"),
        MacroAction::TrainStriker => train(UnitKind::Striker, "seed.striker.v1"),
        MacroAction::AttackMove => order_combat(OrderKind::AttackMove, attack_target(world, faction)),
        MacroAction::Defend => order_combat(OrderKind::Move, Vec2::tile_center(home.0, home.1)),
    }
}

/// 脚本策略
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyKind {
    /// 不下任何命令（策略对照；虫潮生成不是自然选择的证明）
    Idle,
    /// 经济 + 防守开局：采集 → 孵化槽 → 补采集工蜂 → 突击虫回防，对手进入后期再压上
    #[default]
    Economy,
    /// 仅真实采集引导与孵化槽人口设施，不生产单位，隔离自然繁殖与玩家训练。
    Ecology,
}

/// 脚本策略的决策间隔（Ticks，30 Hz 下 1 秒）
pub const POLICY_INTERVAL_TICKS: u64 = 30;
/// 经济策略的采集工蜂目标数
pub const ECONOMY_FORAGERS: usize = 6;
/// 经济策略的突击虫目标数
pub const ECONOMY_STRIKERS: usize = 8;

/// 在当前 Tick 执行一次策略（只在决策间隔边界行动）
pub fn run_policy(world: &mut ArenaWorld, faction: FactionId, policy: PolicyKind) -> MacroOutcome {
    if policy == PolicyKind::Idle || world.tick % POLICY_INTERVAL_TICKS != 0 {
        return MacroOutcome::default();
    }
    let mut plan = vec![MacroAction::Harvest];
    if building_count(world, faction, BuildingKind::SpawningVat) == 0 {
        plan.push(MacroAction::BuildVat);
    } else if policy != PolicyKind::Ecology && unit_count(world, faction, UnitKind::Forager) < ECONOMY_FORAGERS {
        plan.push(MacroAction::TrainForager);
    } else if policy != PolicyKind::Ecology && unit_count(world, faction, UnitKind::Striker) < ECONOMY_STRIKERS {
        plan.push(MacroAction::TrainStriker);
    } else if policy != PolicyKind::Ecology {
        plan.push(MacroAction::BuildHarvester);
    }
    // 每 20 秒一次战术决策：兵力充足时进攻，否则回防
    if policy != PolicyKind::Ecology && world.tick % (POLICY_INTERVAL_TICKS * 20) == 0 {
        let strikers = unit_count(world, faction, UnitKind::Striker);
        plan.push(if strikers >= ECONOMY_STRIKERS {
            MacroAction::AttackMove
        } else {
            MacroAction::Defend
        });
    }
    let mut total = MacroOutcome::default();
    for action in plan {
        let o = apply_macro(world, faction, action);
        total.accepted += o.accepted;
        total.rejected += o.rejected;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::MatchConfig;

    #[test]
    fn macro_expansion_is_deterministic_and_rule_checked() {
        let a = ArenaWorld::new(MatchConfig::pve(7, 1));
        let b = ArenaWorld::new(MatchConfig::pve(7, 1));
        for action in MacroAction::ALL {
            assert_eq!(expand(&a, FactionId(0), action), expand(&b, FactionId(0), action));
        }
        // 开局没有孵化槽：训练动作展开为空
        assert!(expand(&a, FactionId(0), MacroAction::TrainForager).is_empty());
        assert_eq!(expand(&a, FactionId(0), MacroAction::Harvest).len(), 1);
        assert_eq!(MacroAction::from_index(3), Some(MacroAction::BuildVat));
        assert_eq!(MacroAction::from_index(99), None);
    }

    #[test]
    fn economy_policy_builds_a_vat_and_trains_units() {
        let mut w = ArenaWorld::new(MatchConfig::pve(11, 1));
        for _ in 0..2_400 {
            run_policy(&mut w, FactionId(0), PolicyKind::Economy);
            w.step();
        }
        assert!(building_count(&w, FactionId(0), BuildingKind::SpawningVat) >= 1);
        assert!(
            unit_count(&w, FactionId(0), UnitKind::Forager) > 3 || unit_count(&w, FactionId(0), UnitKind::Striker) > 1,
            "经济策略应当训练出新单位"
        );
    }
}
