// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 阵营观测：服务端按战争迷雾裁剪后的世界视图，以及 ToolBroker 感知工具的查询实现。
//!
//! - 己方单位 / 建筑全量可见；敌方单位只在所在 Tile 当前可见时出现；敌方建筑在任一占地格可见时出现，
//!   否则只以 [`BuildingGhost`] 静态快照出现（GAP-12）；
//! - `sample_regional_fields` 只统计本阵营**已探索**的格，避免借环境场反推迷雾中的敌方信标（ERRATA E-034）。

use serde::{Deserialize, Serialize};

use crate::buildings::building_spec;
use crate::fog::BuildingGhost;
use crate::ids::{BuildingKind, EntitySerial, FactionId, FieldId, OrderKind, TechId, UnitKind};
use crate::math::Vec2;
use crate::modulators::{BeaconConfig, Modulators};
use crate::resources::ResourceAmounts;
use crate::world::{ArenaWorld, MatchOutcome};

/// 单位视图
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitView {
    pub id: EntitySerial,
    pub faction: FactionId,
    pub kind: UnitKind,
    pub position: Vec2,
    pub hp_milli: i64,
    pub max_hp_milli: i64,
    pub shield_milli: i64,
    /// 敌方单位不暴露指令
    pub order: Option<OrderKind>,
    pub modulators: Option<Modulators>,
}

/// 建筑视图
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingView {
    pub id: EntitySerial,
    pub faction: FactionId,
    pub kind: BuildingKind,
    pub origin: (u32, u32),
    pub hp_milli: i64,
    pub max_hp_milli: i64,
    pub complete: bool,
    pub powered: bool,
    pub build_progress: u32,
    pub queue: Vec<UnitKind>,
    pub research: Option<TechId>,
    /// 当前科研剩余 tick（仅己方可见）。未完成科技不得视为已解锁。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub research_remaining_ticks: Option<u32>,
    /// 定向繁育进度（仅己方可见）。只暴露兵种与剩余 tick，不序列化子代基因组。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breeding: Option<BreedingView>,
    pub beacon: Option<BeaconConfig>,
}

/// 定向繁育的公开观测投影。故意不含 `BreedJob.child`。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BreedingView {
    pub unit: UnitKind,
    pub remaining_ticks: u32,
}

/// 资源账本视图
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerView {
    pub balance: ResourceAmounts,
    pub capacity: ResourceAmounts,
    pub biomass_harvested_total: i64,
}

/// 一个阵营在某 Tick 的完整观测
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactionObservation {
    pub tick: u64,
    pub faction: FactionId,
    pub ledger: LedgerView,
    pub supply: (u32, u32),
    pub techs: Vec<TechId>,
    pub genomes: Vec<String>,
    pub units: Vec<UnitView>,
    pub buildings: Vec<BuildingView>,
    pub ghosts: Vec<BuildingGhost>,
    /// `4×4` 聚合可见性格（`1` 可见、`2` 已探索、`0` 未探索）
    pub visibility: Vec<u8>,
    pub visibility_width: u32,
    pub score_milli: i64,
    pub pve_wave: Option<u32>,
    pub outcome: Option<MatchOutcome>,
}

/// 单位集群摘要（`query_visible_clusters`）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterSummary {
    pub faction: FactionId,
    pub archetype: UnitKind,
    pub count: u32,
    /// 质心（milli Tile）
    pub centroid_milli: (i64, i64),
    /// 平均生命比例（‰）
    pub mean_hp_permille: i64,
}

/// 主脑状态
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NexusStatus {
    pub faction: FactionId,
    pub id: EntitySerial,
    pub hp_milli: i64,
    pub max_hp_milli: i64,
}

/// `query_visible_clusters` 返回
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterReport {
    pub tick: u64,
    pub clusters: Vec<ClusterSummary>,
    pub nexus: Vec<NexusStatus>,
}

/// 单通道采样
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldChannelSample {
    pub field: FieldId,
    /// 均值（milli 场单位）
    pub mean_milli: i64,
    /// 峰值（milli 场单位）与坐标（Tile）
    pub peak_milli: i64,
    pub peak_tile: (u32, u32),
}

/// `sample_regional_fields` 返回
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldSampleReport {
    pub chunk: (u32, u32),
    pub explored_tiles: u32,
    pub channels: Vec<FieldChannelSample>,
}

/// Q8.8 场原始值 → milli 场单位
const fn q8_to_milli(v: i64) -> i64 {
    v * 1000 / 256
}

impl ArenaWorld {
    fn unit_visible_to(&self, viewer: FactionId, faction: FactionId, p: Vec2) -> bool {
        if viewer == faction {
            return true;
        }
        let Some(f) = self.faction(viewer) else { return false };
        p.tile()
            .is_some_and(|(x, y)| x < self.map.width && y < self.map.height && f.fog.is_visible(x, y))
    }

    /// 生成阵营观测（迷雾裁剪）
    pub fn observe(&self, viewer: FactionId) -> Option<FactionObservation> {
        let f = self.faction(viewer)?;
        let units = self
            .units
            .values()
            .filter(|u| self.unit_visible_to(viewer, u.faction, u.position))
            .map(|u| {
                let own = u.faction == viewer;
                UnitView {
                    id: u.id,
                    faction: u.faction,
                    kind: u.kind,
                    position: u.position,
                    hp_milli: u.defense.hp_milli,
                    max_hp_milli: u.max_hp_milli,
                    shield_milli: u.defense.shield_milli,
                    order: own.then_some(u.order.kind),
                    modulators: own.then_some(u.modulators),
                }
            })
            .collect();
        let buildings = self
            .buildings
            .values()
            .filter(|b| {
                let r = b.rect();
                b.faction == viewer || f.fog.any_visible(r.x0, r.y0, r.x1 - r.x0, r.y1 - r.y0)
            })
            .map(|b| {
                let own = b.faction == viewer;
                BuildingView {
                    id: b.id,
                    faction: b.faction,
                    kind: b.kind,
                    origin: b.origin,
                    hp_milli: b.defense.hp_milli,
                    max_hp_milli: b.max_hp_milli,
                    complete: b.complete,
                    powered: own && b.powered,
                    build_progress: b.build_progress,
                    queue: if own {
                        b.queue.iter().map(|j| j.unit).collect()
                    } else {
                        Vec::new()
                    },
                    research: if own { b.research.map(|(t, _)| t) } else { None },
                    research_remaining_ticks: if own { b.research.map(|(_, rem)| rem) } else { None },
                    breeding: if own {
                        b.breeding.as_ref().map(|j| BreedingView {
                            unit: j.unit,
                            remaining_ticks: j.remaining_ticks,
                        })
                    } else {
                        None
                    },
                    beacon: (own && b.kind == BuildingKind::SignalBeacon).then_some(b.beacon),
                }
            })
            .collect();
        let visible_ids: std::collections::BTreeSet<EntitySerial> = self
            .buildings
            .values()
            .filter(|b| b.faction != viewer)
            .map(|b| b.id)
            .collect();
        let ghosts = f
            .ghosts
            .values()
            .filter(|g| {
                // 当前仍可见的建筑以实体视图出现，不重复下发幽灵
                let fp = building_spec(g.kind).footprint;
                !(visible_ids.contains(&g.entity_id) && f.fog.any_visible(g.origin.0, g.origin.1, fp, fp))
            })
            .cloned()
            .collect();
        Some(FactionObservation {
            tick: self.tick,
            faction: viewer,
            ledger: LedgerView {
                balance: f.ledger.balance,
                capacity: f.ledger.capacity,
                biomass_harvested_total: f.ledger.biomass_harvested_total,
            },
            supply: self.supply(viewer),
            techs: f.techs.iter().collect(),
            genomes: f.genomes.keys().cloned().collect(),
            units,
            buildings,
            ghosts,
            visibility: f.fog.visibility_cells(),
            visibility_width: self.map.width.div_ceil(crate::fog::VISIBILITY_CELL_TILES),
            score_milli: self.score_milli(viewer),
            pve_wave: self.pve.as_ref().map(|d| d.wave),
            outcome: self.outcome.clone(),
        })
    }

    /// `query_visible_clusters`：按 (阵营, 原型) 聚合可见单位；`region_chunk` 限定 256×256 Chunk
    pub fn query_visible_clusters(
        &self,
        viewer: FactionId,
        region_chunk: Option<(u32, u32)>,
        include_enemy: bool,
    ) -> ClusterReport {
        let chunk = rynvek_core::constants::CHUNK_SIZE_UNITS as i64;
        let in_region = |p: Vec2| {
            region_chunk.is_none_or(|(cx, cy)| {
                let (x, y) = (p.x.0 >> 32, p.y.0 >> 32);
                x / chunk == cx as i64 && y / chunk == cy as i64
            })
        };
        let mut groups: std::collections::BTreeMap<(FactionId, UnitKind), (u32, i64, i64, i64)> = Default::default();
        for u in self.units.values() {
            if !include_enemy && u.faction != viewer {
                continue;
            }
            if !self.unit_visible_to(viewer, u.faction, u.position) || !in_region(u.position) {
                continue;
            }
            let e = groups.entry((u.faction, u.kind)).or_default();
            e.0 += 1;
            e.1 += (u.position.x.0 * 1000) >> 32;
            e.2 += (u.position.y.0 * 1000) >> 32;
            e.3 += u.defense.hp_milli * 1000 / u.max_hp_milli.max(1);
        }
        let clusters = groups
            .into_iter()
            .map(|((faction, archetype), (n, sx, sy, hp))| ClusterSummary {
                faction,
                archetype,
                count: n,
                centroid_milli: (sx / n as i64, sy / n as i64),
                mean_hp_permille: hp / n as i64,
            })
            .collect();
        let nexus = self
            .factions
            .iter()
            .filter_map(|f| f.nexus.and_then(|id| self.buildings.get(&id)))
            .filter(|b| {
                let r = b.rect();
                b.faction == viewer
                    || (include_enemy
                        && self
                            .faction(viewer)
                            .is_some_and(|f| f.fog.any_visible(r.x0, r.y0, 4, 4)))
            })
            .map(|b| NexusStatus {
                faction: b.faction,
                id: b.id,
                hp_milli: b.defense.hp_milli,
                max_hp_milli: b.max_hp_milli,
            })
            .collect();
        ClusterReport {
            tick: self.tick,
            clusters,
            nexus,
        }
    }

    /// `sample_regional_fields`：Chunk 内已探索格的五通道均值与峰值；Chunk 越界返回 `None`
    pub fn sample_regional_fields(&self, viewer: FactionId, chunk_x: u32, chunk_y: u32) -> Option<FieldSampleReport> {
        let chunk = rynvek_core::constants::CHUNK_SIZE_UNITS;
        let (x0, y0) = (chunk_x.checked_mul(chunk)?, chunk_y.checked_mul(chunk)?);
        if x0 >= self.map.width || y0 >= self.map.height {
            return None;
        }
        let fog = &self.faction(viewer)?.fog;
        let (x1, y1) = ((x0 + chunk).min(self.map.width), (y0 + chunk).min(self.map.height));
        let mut explored = 0u32;
        let mut stats: Vec<(i64, i64, (u32, u32))> = vec![(0, i64::MIN, (x0, y0)); 5];
        for y in y0..y1 {
            for x in x0..x1 {
                if !fog.is_explored(x, y) {
                    continue;
                }
                explored += 1;
                for (i, field) in FieldId::ALL.iter().enumerate() {
                    let v = self.fields.get(*field, x as i64, y as i64) as i64;
                    let s = &mut stats[i];
                    s.0 += v;
                    if v > s.1 {
                        s.1 = v;
                        s.2 = (x, y);
                    }
                }
            }
        }
        let channels = FieldId::ALL
            .iter()
            .zip(stats)
            .map(|(field, (sum, peak, at))| FieldChannelSample {
                field: *field,
                mean_milli: if explored == 0 {
                    0
                } else {
                    q8_to_milli(sum) / explored as i64
                },
                peak_milli: if explored == 0 { 0 } else { q8_to_milli(peak) },
                peak_tile: at,
            })
            .collect();
        Some(FieldSampleReport {
            chunk: (chunk_x, chunk_y),
            explored_tiles: explored,
            channels,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::world::{ArenaWorld, MatchConfig};
    use crate::{FactionId, UnitKind};

    #[test]
    fn observation_is_fog_filtered_and_serializable() {
        let w = ArenaWorld::new(MatchConfig::duel(42));
        let o = w.observe(FactionId(0)).unwrap();
        assert!(o.units.iter().all(|u| u.faction == FactionId(0)), "开局看不到敌方");
        assert_eq!(o.units.len(), 4);
        assert_eq!(o.buildings.len(), 1);
        assert_eq!(o.visibility.len(), 64 * 64);
        let json = serde_json::to_string(&o).unwrap();
        let back: super::FactionObservation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, o);
        assert!(w.observe(FactionId::WILD).is_none());
    }

    #[test]
    fn cluster_and_field_queries() {
        let w = ArenaWorld::new(MatchConfig::duel(42));
        let r = w.query_visible_clusters(FactionId(0), None, true);
        assert_eq!(r.clusters.len(), 2);
        let foragers = r.clusters.iter().find(|c| c.archetype == UnitKind::Forager).unwrap();
        assert_eq!((foragers.count, foragers.mean_hp_permille), (3, 1000));
        assert_eq!(r.nexus.len(), 1, "敌方主脑不可见");
        assert!(
            w.query_visible_clusters(FactionId(0), Some((0, 0)), false)
                .clusters
                .len()
                == 2
        );
        let s = w.sample_regional_fields(FactionId(0), 0, 0).unwrap();
        assert_eq!(s.channels.len(), 5);
        assert!(s.explored_tiles > 1000);
        assert!(w.sample_regional_fields(FactionId(0), 1, 0).is_none());
    }
}
