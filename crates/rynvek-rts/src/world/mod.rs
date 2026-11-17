// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `ArenaWorld`：Rynvek Arena 一局对战的宏观权威状态机（D0 确定性）。
//!
//! 每个权威 Tick 按固定顺序推进（ERRATA E-031）：
//! 1. **经济**：主脑与反应堆产出 → 各耗电建筑按实体序号依次供电（电力不足即掉电停工）→
//!    萃取器抽取营养、精炼厂合成、孵化槽 / 研发 / 繁育推进；
//! 2. **施工**：未完工建筑推进进度，生命值由 10% 线性增长到满值；
//! 3. **环境场**（每 3 Tick，10 Hz）：信标注入、反应堆钉住能量场、沼泽再生、五点拉普拉斯扩散；
//! 4. **神经**（每 4 Tick 一个 Coupling Epoch）：组装 16 维感知 + L3 调质 → 单位脑推理 → 6 维动作读出；
//! 5. **运动**：指令方向（流场）与脑运动读出合成期望速度 → ORCA 避障 → 地形速度系数 → 墙体滑移；
//! 6. **采集**、**战斗**、**环境伤害**（危害场 / 裂隙灼烧）、**护盾再生与 Weaver 治疗**；
//! 7. **死亡结算**：残骸回收为营养场、建筑移除并增量重建 HPA*；
//! 8. **迷雾**（每 4 Tick）、**PvE 虫潮**、**胜负裁决**。
//!
//! 所有集合以 `BTreeMap<EntitySerial, _>` 保存并按序号遍历；随机性只来自 Philox。

mod combat_sys;
pub mod ecology;
mod economy;
mod match_sys;
mod persistence;
mod units_sys;

use std::collections::{BTreeMap, VecDeque};

use blake3::Hasher;
use rynvek_core::{Blake3Hash, Q32_32, RynvekErrorCode};
use rynvek_neural::NeurChunk;
use serde::{Deserialize, Serialize};

use crate::arena_map::{ArenaMap, ArenaMapSize};
use crate::brain::UnitBrain;
use crate::buildings::{BUILDING_SPECS, building_spec};
use crate::combat::Defense;
use crate::commands::{CommandAck, CrucibleAction, RtsCommand, RtsError, UnitSelector};
use crate::evolution::{FitnessRecord, MUTATION_RATE_RANGE_PERMILLE};
use crate::fields::FieldGrid;
use crate::fog::{BuildingGhost, FactionFog};
use crate::genome::{Genome, SEED_GENOMES};
use crate::ids::{BuildingKind, EntitySerial, FactionId, FieldId, OrderKind, TechId, UnitKind};
use crate::math::{COS_30, MILLI, SIN_30, Vec2};
use crate::modulators::{BeaconConfig, Modulators};
use crate::nav::{FlowField, HpaGraph, NavGrid, Rect};
use crate::pve::PveDirector;
use crate::resources::{ResourceAmounts, ResourceLedger};
use crate::tech::{TechModifiers, TechSet, tech_spec};
use crate::terrain::{BASALT_BUILDING_ARMOR_BONUS, allows_building};
use crate::units::unit_spec;

/// 比赛时长上限（PvP）：`36,000 Ticks` = 20 分钟
pub const MATCH_TIME_LIMIT_TICKS: u64 = 36_000;
/// 突然死亡加赛时长
pub const SUDDEN_DEATH_TICKS: u64 = 3_600;
/// 突然死亡期间主脑每秒流失
pub const SUDDEN_DEATH_NEXUS_LOSS_MILLI_PER_S: i64 = 20 * MILLI;
/// 拆除返还比例
pub const DEMOLISH_REFUND_PERCENT: i64 = 50;
/// 施工起始生命值比例
pub const CONSTRUCTION_START_HP_PERCENT: i64 = 10;
/// 精英繁育成本（Neurogel 50 + Flux 50）与基础耗时
pub const BREED_COST: ResourceAmounts = ResourceAmounts::whole(0, 50, 50);
pub const BREED_TICKS: u32 = 600;
/// 迷雾刷新间隔
pub const FOG_UPDATE_INTERVAL_TICKS: u64 = rynvek_core::constants::COUPLING_EPOCH_TICKS as u64;
/// 流场缓存上限
pub const FLOW_CACHE_LIMIT: usize = 32;

/// 对局模式
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchMode {
    #[serde(rename = "PvP_Ranked")]
    PvpRanked,
    #[serde(rename = "PvE_Survival")]
    PveSurvival,
}

/// 对局配置
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "MatchConfigWire")]
pub struct MatchConfig {
    pub mode: MatchMode,
    pub map_size: ArenaMapSize,
    pub map_seed: u64,
    /// Philox 世界种子（脑繁育、闪避与虫潮演化）
    pub world_seed: u64,
    /// 玩家阵营数：PvP 固定 2；PvE 为 1 或 2
    pub players: u8,
    /// 显式PvE实验契约：局部摄食、真实计算预算与首次摄食后基础代谢；无隐含数值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ecology: Option<ecology::ForagerEcologyContract>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatchConfigWire {
    mode: MatchMode,
    map_size: ArenaMapSize,
    map_seed: u64,
    world_seed: u64,
    players: u8,
    #[serde(default)]
    ecology: Option<ecology::ForagerEcologyContract>,
}

impl TryFrom<MatchConfigWire> for MatchConfig {
    type Error = &'static str;
    fn try_from(w: MatchConfigWire) -> Result<Self, Self::Error> {
        if w.ecology.is_some() && w.mode != MatchMode::PveSurvival {
            return Err("未认证的能量规则不能进入PvP/排位");
        }
        let config = Self {
            mode: w.mode,
            map_size: w.map_size,
            map_seed: w.map_seed,
            world_seed: w.world_seed,
            players: w.players,
            ecology: w.ecology,
        };
        config.validate_energy_contract()?;
        Ok(config)
    }
}

impl MatchConfig {
    pub fn validate_energy_contract(&self) -> Result<(), &'static str> {
        if let Some(contract) = self.ecology {
            if self.mode != MatchMode::PveSurvival {
                return Err("未认证的能量规则不能进入PvP/排位");
            }
            contract.validate()?;
        }
        Ok(())
    }

    pub const fn duel(map_seed: u64) -> Self {
        Self {
            mode: MatchMode::PvpRanked,
            map_size: ArenaMapSize::Duel1v1,
            map_seed,
            world_seed: map_seed,
            players: 2,
            ecology: None,
        }
    }

    pub const fn pve(map_seed: u64, players: u8) -> Self {
        Self {
            mode: MatchMode::PveSurvival,
            map_size: ArenaMapSize::Duel1v1,
            map_seed,
            world_seed: map_seed,
            players,
            ecology: None,
        }
    }
}

/// 孵化任务
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionJob {
    pub unit: UnitKind,
    pub genome: Genome,
    pub remaining_ticks: u32,
    pub cost: ResourceAmounts,
}

/// 繁育任务
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreedJob {
    pub unit: UnitKind,
    pub child: Genome,
    pub remaining_ticks: u32,
}

/// 建筑实体
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Building {
    pub id: EntitySerial,
    pub faction: FactionId,
    pub kind: BuildingKind,
    pub origin: (u32, u32),
    pub defense: Defense,
    pub max_hp_milli: i64,
    /// 已施工 Tick 数；`>= build_ticks` 即完工
    pub build_progress: u32,
    pub complete: bool,
    /// 本 Tick 是否获得供电
    pub powered: bool,
    pub weapon_cooldown: u32,
    pub queue: VecDeque<ProductionJob>,
    pub research: Option<(TechId, u32)>,
    pub breeding: Option<BreedJob>,
    pub refinery_progress: u32,
    pub beacon: BeaconConfig,
    /// 萃取器营养→生物质换算余数（`raw × 125` 的 /32 余数）
    pub harvest_remainder: i64,
    pub on_fissure: bool,
}

impl Building {
    pub fn footprint(&self) -> u32 {
        building_spec(self.kind).footprint
    }

    /// 占地矩形
    pub fn rect(&self) -> Rect {
        let f = self.footprint();
        Rect {
            x0: self.origin.0,
            y0: self.origin.1,
            x1: self.origin.0 + f,
            y1: self.origin.1 + f,
        }
    }

    /// 几何中心
    pub fn center(&self) -> Vec2 {
        let f = self.footprint() as i64;
        Vec2::new(
            Q32_32(((self.origin.0 as i64) << 32) + (f << 31)),
            Q32_32(((self.origin.1 as i64) << 32) + (f << 31)),
        )
    }

    /// 中心 Tile
    pub fn center_tile(&self) -> (u32, u32) {
        let f = self.footprint();
        (self.origin.0 + f / 2, self.origin.1 + f / 2)
    }

    /// 点到占地矩形的最近点
    pub fn nearest_point(&self, p: Vec2) -> Vec2 {
        let r = self.rect();
        let clamp = |v: Q32_32, lo: u32, hi: u32| Q32_32(v.0.clamp((lo as i64) << 32, ((hi as i64) << 32) - 1));
        Vec2::new(clamp(p.x, r.x0, r.x1), clamp(p.y, r.y0, r.y1))
    }
}

/// 单位当前指令
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub kind: OrderKind,
    pub target: Vec2,
    /// 巡逻起点（Patrol 往返）
    pub origin: Vec2,
}

/// 单位实体
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub id: EntitySerial,
    pub faction: FactionId,
    pub kind: UnitKind,
    pub genome: Genome,
    pub brain: UnitBrain,
    pub position: Vec2,
    pub velocity: Vec2,
    pub defense: Defense,
    pub max_hp_milli: i64,
    pub max_shield_milli: i64,
    pub weapon_cooldown: u32,
    pub order: Order,
    pub born_tick: u64,
    pub fitness: FitnessRecord,
    pub modulators: Modulators,
    /// 最近一次 Coupling Epoch 的动作读出（Q8.8）
    pub actions: [i16; 6],
    pub harvest_remainder: i64,
}

impl Unit {
    pub fn tile(&self) -> (u32, u32) {
        self.position.tile().unwrap_or((0, 0))
    }

    pub fn radius(&self) -> Q32_32 {
        Q32_32::from_milli(unit_spec(self.kind).radius_milli)
    }
}

/// 阵营状态
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Faction {
    pub id: FactionId,
    pub ledger: ResourceLedger,
    pub techs: TechSet,
    pub fog: FactionFog,
    pub ghosts: BTreeMap<EntitySerial, BuildingGhost>,
    /// 可用基因组库（6 个种子 + 繁育后代），按 slug 排序
    pub genomes: BTreeMap<String, Genome>,
    pub bred_count: u32,
    pub surrendered: bool,
    pub eliminated: bool,
    pub nexus: Option<EntitySerial>,
}

impl Faction {
    pub fn modifiers(&self) -> TechModifiers {
        self.techs.modifiers()
    }
}

/// 对局结果
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum MatchOutcome {
    Victory {
        winner: FactionId,
        reason: VictoryReason,
        tick: u64,
    },
    Draw {
        tick: u64,
    },
    /// PvE：全部玩家主脑被摧毁
    Defeat {
        tick: u64,
        waves_cleared: u32,
    },
}

/// 胜利原因
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VictoryReason {
    Elimination,
    Surrender,
    TieBreakerScore,
    SuddenDeath,
    WavesCleared,
}

/// 世界事件（供客户端、Agent 记忆与回放索引消费）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RtsEvent {
    BuildingPlaced {
        id: EntitySerial,
        faction: FactionId,
        kind: BuildingKind,
    },
    BuildingCompleted {
        id: EntitySerial,
        faction: FactionId,
        kind: BuildingKind,
    },
    BuildingDestroyed {
        id: EntitySerial,
        faction: FactionId,
        kind: BuildingKind,
    },
    UnitSpawned {
        id: EntitySerial,
        faction: FactionId,
        kind: UnitKind,
    },
    UnitDied {
        id: EntitySerial,
        faction: FactionId,
        kind: UnitKind,
    },
    ResearchCompleted {
        faction: FactionId,
        tech: TechId,
    },
    GenomeBred {
        faction: FactionId,
        slug: String,
        unit: UnitKind,
    },
    WaveSpawned {
        wave: u32,
        count: u32,
    },
    SuddenDeathStarted {
        tick: u64,
    },
    MatchEnded {
        outcome: MatchOutcome,
    },
}

/// 权威世界规范化分层状态根清单（覆盖全部十类权威子状态，消灭审计报告 §3 的全部哈希盲区）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalWorldState {
    pub tick: u64,
    /// 世界配置与地形（MatchConfig + map.tiles_hash）
    pub config_root: Blake3Hash,
    /// 建筑与单位全部组件字段（含建造队列、研发、繁育、精炼、信标、指令、适应度与余数）
    pub entity_root: Blake3Hash,
    /// 全部存活单位的四模型脑内部动态态（LIF / GatedMLP / Izhikevich / CTRNN）与动作读出
    pub neural_root: Blake3Hash,
    /// 五通道连续反应扩散环境场
    pub field_root: Blake3Hash,
    /// 阵营资源账本（余额、容量上限、累计采集、溢出丢弃）、科技集合、主脑锚点、投降/淘汰状态
    pub economy_root: Blake3Hash,
    /// 阵营基因池、繁育计数、演化坩埚繁育任务、PvE 导演基因池与个体适应度归档
    pub evolution_root: Blake3Hash,
    /// 阵营战争迷雾（可见位图 + 已探索位图）与敌方建筑静态幽灵快照
    pub visibility_root: Blake3Hash,
    /// 随机流与实体序号发生器状态（world_seed, tick, next_serial）
    pub rng_root: Blake3Hash,
    /// 调度事件、胜负裁决结果与突然死亡倒计时
    pub scheduled_event_root: Blake3Hash,
    /// 前驱提交根（可选，单步世界状态根缺省为全零）
    pub parent_commit_root: Blake3Hash,
}

impl CanonicalWorldState {
    /// 聚合全部分层子根计算最终 256-bit BLAKE3 世界状态根
    pub fn state_root(&self) -> Blake3Hash {
        Blake3Hash(rynvek_journal::CommitManifest::canonical_state_root(
            self.tick,
            self.parent_commit_root.0,
            self.commit_sub_roots(),
            self.commit_domains(),
        ))
    }
    pub(crate) fn commit_sub_roots(&self) -> rynvek_journal::CommitSubRoots {
        rynvek_journal::CommitSubRoots {
            entity_root_hash: self.entity_root.0,
            field_root_hash: self.field_root.0,
            neural_root_hash: self.neural_root.0,
            economy_root_hash: self.economy_root.0,
            rng_root_hash: self.rng_root.0,
        }
    }

    pub(crate) fn commit_domains(&self) -> rynvek_journal::CanonicalDomains {
        rynvek_journal::CanonicalDomains {
            config: self.config_root.0,
            evolution: self.evolution_root.0,
            visibility: self.visibility_root.0,
            scheduled_events: self.scheduled_event_root.0,
        }
    }
}

/// 竞技场世界
#[derive(Clone, Debug)]
pub struct ArenaWorld {
    pub config: MatchConfig,
    pub map: ArenaMap,
    pub nav: NavGrid,
    pub hpa: HpaGraph,
    pub fields: FieldGrid,
    pub tick: u64,
    next_serial: u32,
    pub factions: Vec<Faction>,
    pub buildings: BTreeMap<EntitySerial, Building>,
    pub units: BTreeMap<EntitySerial, Unit>,
    pub ecology: Option<ecology::EcologyState>,
    flow_cache: BTreeMap<(u32, u32), FlowField>,
    flow_order: VecDeque<(u32, u32)>,
    pub pve: Option<PveDirector>,
    pub pve_records: Vec<(u32, Genome, FitnessRecord)>,
    pub outcome: Option<MatchOutcome>,
    pub sudden_death_until: Option<u64>,
    events: Vec<RtsEvent>,
    /// 已被接受的命令日志（`.rreplay` 录制；默认关闭，不参与状态根）
    command_log: Option<Vec<RecordedCommand>>,
}

/// 一条被权威层接受的命令：在 `tick` 的 Tick 边界（`step` 之前）对 `faction` 生效
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedCommand {
    pub tick: u64,
    pub faction: FactionId,
    pub command: RtsCommand,
}

impl ArenaWorld {
    /// 按配置生成地图并放置 Starter Kit
    pub fn new(config: MatchConfig) -> Self {
        let map = ArenaMap::generate(config.map_size, config.map_seed);
        Self::with_map(config, map)
    }

    /// 用给定地图开局（测试与自定义房间）
    pub fn with_map(config: MatchConfig, map: ArenaMap) -> Self {
        assert!((1..=2).contains(&config.players), "仅存在 2 个对称出生锚点");
        assert!(
            config.mode == MatchMode::PveSurvival || config.players == 2,
            "PvP_Ranked 需要 2 个阵营"
        );
        assert!(
            config.ecology.is_none() || config.mode == MatchMode::PveSurvival,
            "能量规则尚未认证，不允许PvP"
        );
        let nav = NavGrid::from_terrain(map.width, map.height, &map.tiles);
        let hpa = HpaGraph::build(&nav);
        let fields = FieldGrid::for_terrain(map.width, map.height, &map.tiles);
        config
            .validate_energy_contract()
            .expect("世界能量/生存契约必须通过验证");
        let mut world = Self {
            ecology: config.ecology.map(|_| ecology::EcologyState::default()),
            pve: (config.mode == MatchMode::PveSurvival).then(|| PveDirector::new(config.world_seed)),
            config,
            nav,
            hpa,
            fields,
            tick: 0,
            next_serial: 1,
            factions: Vec::new(),
            buildings: BTreeMap::new(),
            units: BTreeMap::new(),
            flow_cache: BTreeMap::new(),
            flow_order: VecDeque::new(),
            pve_records: Vec::new(),
            outcome: None,
            sudden_death_until: None,
            events: Vec::new(),
            command_log: None,
            map,
        };
        let genomes: BTreeMap<String, Genome> = SEED_GENOMES
            .iter()
            .map(|s| (s.slug.to_owned(), Genome::seed(s.unit)))
            .collect();
        let anchors = world.map.spawn_anchors();
        for f in 0..world.config.players {
            world.factions.push(Faction {
                id: FactionId(f),
                ledger: ResourceLedger::starter(),
                techs: TechSet::default(),
                fog: FactionFog::new(world.map.width, world.map.height),
                ghosts: BTreeMap::new(),
                genomes: genomes.clone(),
                bred_count: 0,
                surrendered: false,
                eliminated: false,
                nexus: None,
            });
            world.place_starter_kit(FactionId(f), anchors[f as usize]);
        }
        world.update_fog();
        world
    }

    fn place_starter_kit(&mut self, faction: FactionId, anchor: Vec2) {
        let (ax, ay) = (anchor.x.to_int_floor() as u32, anchor.y.to_int_floor() as u32);
        let nexus = self.insert_building(faction, BuildingKind::NexusCore, (ax - 2, ay - 2), true);
        self.faction_mut(faction).expect("阵营存在").nexus = Some(nexus);
        // 朝地图中心的单位方向
        let center = Vec2::new(
            Q32_32((self.map.width as i64) << 31),
            Q32_32((self.map.height as i64) << 31),
        );
        let dir = (center - anchor).normalized();
        let rotate = |v: Vec2, cos: Q32_32, sin: Q32_32| {
            Vec2::new(
                v.x.mul(cos).saturating_sub(v.y.mul(sin)),
                v.x.mul(sin).saturating_add(v.y.mul(cos)),
            )
        };
        let neg_sin = Q32_32(-SIN_30.0);
        for d in [rotate(dir, COS_30, neg_sin), dir, rotate(dir, COS_30, SIN_30)] {
            let p = anchor + d.scale(Q32_32::from_milli(3_500));
            self.spawn_unit(faction, UnitKind::Forager, Genome::seed(UnitKind::Forager), p);
        }
        let p = anchor + dir.scale(Q32_32::from_milli(4_500));
        self.spawn_unit(faction, UnitKind::Striker, Genome::seed(UnitKind::Striker), p);
    }

    /// 分配下一个实体序号（带 `checked_add` 溢出保护）
    pub fn try_alloc_serial(&mut self) -> Result<EntitySerial, RynvekErrorCode> {
        let id = EntitySerial(self.next_serial);
        self.next_serial = self
            .next_serial
            .checked_add(1)
            .ok_or(RynvekErrorCode::DimensionMismatch)?;
        Ok(id)
    }

    fn alloc_serial(&mut self) -> EntitySerial {
        self.try_alloc_serial()
            .expect("实体序号 EntitySerial 耗尽 u32::MAX 上限")
    }

    /// 获取下一个待分配的实体序号
    pub const fn next_serial(&self) -> u32 {
        self.next_serial
    }

    /// 设置下一个待分配的实体序号（供快照恢复与测试使用）
    pub fn set_next_serial(&mut self, next_serial: u32) {
        self.next_serial = next_serial;
    }

    /// 插入建筑（不校验、不扣费）；`complete` 为真时直接完工
    fn insert_building(
        &mut self,
        faction: FactionId,
        kind: BuildingKind,
        origin: (u32, u32),
        complete: bool,
    ) -> EntitySerial {
        let spec = building_spec(kind);
        let id = self.alloc_serial();
        let f = spec.footprint;
        let mut all_basalt = true;
        let mut on_fissure = false;
        for y in origin.1..origin.1 + f {
            for x in origin.0..origin.0 + f {
                let t = self.map.tile(x, y);
                all_basalt &= t == crate::TerrainTileKind::ConductiveBasalt;
                on_fissure |= t == crate::TerrainTileKind::AcidicFissure;
                self.nav.set_occupied(x, y, true);
            }
        }
        let armor = spec.armor + if all_basalt { BASALT_BUILDING_ARMOR_BONUS } else { 0 };
        let hp = if complete {
            spec.hp_milli
        } else {
            spec.hp_milli * CONSTRUCTION_START_HP_PERCENT / 100
        };
        let b = Building {
            id,
            faction,
            kind,
            origin,
            defense: Defense {
                hp_milli: hp,
                shield_milli: 0,
                armor,
            },
            max_hp_milli: spec.hp_milli,
            build_progress: if complete { spec.build_ticks } else { 0 },
            complete,
            powered: false,
            weapon_cooldown: 0,
            queue: VecDeque::new(),
            research: None,
            breeding: None,
            refinery_progress: 0,
            beacon: BeaconConfig::default(),
            harvest_remainder: 0,
            on_fissure,
        };
        let rect = b.rect();
        self.buildings.insert(id, b);
        self.hpa.update_region(&self.nav, rect);
        self.invalidate_flow();
        self.events.push(RtsEvent::BuildingPlaced { id, faction, kind });
        if complete {
            self.recompute_capacity(faction);
        }
        id
    }

    /// 生成单位
    pub(crate) fn spawn_unit(
        &mut self,
        faction: FactionId,
        kind: UnitKind,
        genome: Genome,
        position: Vec2,
    ) -> EntitySerial {
        let spec = unit_spec(kind);
        let id = self.alloc_serial();
        let brain = UnitBrain::from_genome(&genome).expect("种子与繁育基因组必须可表达");
        let armor_bonus = self
            .faction(faction)
            .map(|f| f.modifiers().unit_armor_bonus)
            .unwrap_or(0);
        let u = Unit {
            id,
            faction,
            kind,
            genome,
            brain,
            position,
            velocity: Vec2::ZERO,
            defense: Defense {
                hp_milli: spec.hp_milli,
                shield_milli: spec.shield_milli,
                armor: spec.armor + armor_bonus,
            },
            max_hp_milli: spec.hp_milli,
            max_shield_milli: spec.shield_milli,
            weapon_cooldown: 0,
            order: Order {
                kind: OrderKind::HoldPosition,
                target: position,
                origin: position,
            },
            born_tick: self.tick,
            fitness: FitnessRecord::default(),
            modulators: Modulators::ZERO,
            actions: [0; 6],
            harvest_remainder: 0,
        };
        if let Some(ecology) = &mut self.ecology {
            ecology.bodies.insert(id, ecology::BodyEnergy::default());
        }
        self.units.insert(id, u);
        self.events.push(RtsEvent::UnitSpawned { id, faction, kind });
        id
    }

    pub fn faction(&self, id: FactionId) -> Option<&Faction> {
        self.factions.get(id.0 as usize).filter(|_| !id.is_wild())
    }

    pub fn faction_mut(&mut self, id: FactionId) -> Option<&mut Faction> {
        if id.is_wild() {
            return None;
        }
        self.factions.get_mut(id.0 as usize)
    }

    /// 取出并清空事件队列
    pub fn drain_events(&mut self) -> Vec<RtsEvent> {
        std::mem::take(&mut self.events)
    }

    /// 是否已分出胜负
    pub fn is_over(&self) -> bool {
        self.outcome.is_some()
    }

    /// 两阵营是否敌对（野生阵营与所有玩家敌对）
    pub const fn hostile(a: FactionId, b: FactionId) -> bool {
        a.0 != b.0
    }

    /// 阵营当前神经人口（含队列）与上限
    pub fn supply(&self, faction: FactionId) -> (u32, u32) {
        let used_units: u32 = self
            .units
            .values()
            .filter(|u| u.faction == faction)
            .map(|u| unit_spec(u.kind).supply)
            .sum();
        let mut queued = 0;
        let mut cap = 0;
        for b in self.buildings.values().filter(|b| b.faction == faction) {
            queued += b.queue.iter().map(|j| unit_spec(j.unit).supply).sum::<u32>();
            if b.kind == BuildingKind::SpawningVat && b.complete {
                cap += crate::buildings::VAT_SUPPLY_BONUS;
            }
        }
        (used_units + queued, cap)
    }

    /// 重新计算阵营存储上限
    pub(crate) fn recompute_capacity(&mut self, faction: FactionId) {
        let mut cap = ResourceAmounts::ZERO;
        let mut has_nexus = false;
        for b in self.buildings.values().filter(|b| b.faction == faction && b.complete) {
            match b.kind {
                BuildingKind::NexusCore => {
                    has_nexus = true;
                    cap = cap.plus(crate::resources::NEXUS_STORAGE_CAP);
                }
                BuildingKind::FluxReactor => cap.flux += crate::buildings::REACTOR_FLUX_CAPACITY_BONUS_MILLI,
                BuildingKind::SynapseRefinery => {
                    cap.neurogel += crate::buildings::REFINERY_NEUROGEL_CAPACITY_BONUS_MILLI
                }
                _ => {}
            }
        }
        if let Some(f) = self.faction_mut(faction) {
            if has_nexus {
                cap.flux += f.modifiers().flux_capacity_bonus_milli;
            }
            f.ledger.capacity = cap;
        }
    }

    /// 地形或占地变化后使派生流场缓存失效，不修改权威状态。
    pub fn invalidate_flow(&mut self) {
        self.flow_cache.clear();
        self.flow_order.clear();
    }

    /// 在目标 Tile 的共享流场（LRU 缓存）中采样 `here` 处的方向
    pub(crate) fn flow_direction(&mut self, goal: (u32, u32), here: (u32, u32)) -> Vec2 {
        if !self.flow_cache.contains_key(&goal) {
            if self.flow_order.len() >= FLOW_CACHE_LIMIT {
                if let Some(old) = self.flow_order.pop_front() {
                    self.flow_cache.remove(&old);
                }
            }
            let f = FlowField::solve(&self.nav, goal, None);
            self.flow_cache.insert(goal, f);
            self.flow_order.push_back(goal);
        }
        self.flow_cache[&goal].direction(&self.nav, here.0, here.1)
    }

    // ------------------------------------------------------------------ 命令

    /// 开启命令录制（之后每条成功的命令都追加到日志）。
    pub fn enable_command_log(&mut self) {
        self.command_log.get_or_insert_with(Vec::new);
    }

    /// 已录制的命令（未开启时为空）
    pub fn command_log(&self) -> &[RecordedCommand] {
        self.command_log.as_deref().unwrap_or(&[])
    }

    /// 按录制日志从同一配置重放到 `until_tick`（或对局提前结束）。
    ///
    /// D0 确定性保证：同一 `MatchConfig` + 同一命令序列 ⇒ 逐位相同的 [`Self::state_hash`]。
    /// `on_tick` 在每个 Tick 边界（命令生效之前）回调，供回放校验器比对检查点。
    pub fn replay(
        config: MatchConfig,
        log: &[RecordedCommand],
        until_tick: u64,
        mut on_tick: impl FnMut(&ArenaWorld),
    ) -> Result<ArenaWorld, (usize, RtsError)> {
        let mut world = ArenaWorld::new(config);
        world.enable_command_log();
        let mut next = 0;
        loop {
            on_tick(&world);
            while let Some(rec) = log.get(next).filter(|r| r.tick == world.tick) {
                world.apply_command(rec.faction, &rec.command).map_err(|e| (next, e))?;
                next += 1;
            }
            if world.tick >= until_tick || world.outcome.is_some() {
                break;
            }
            world.step();
        }
        Ok(world)
    }

    /// 在 Tick 边界执行一条命令（成功时若开启录制则写入命令日志）
    pub fn apply_command(&mut self, faction: FactionId, cmd: &RtsCommand) -> Result<CommandAck, RtsError> {
        let ack = self.execute_command(faction, cmd)?;
        if let Some(log) = self.command_log.as_mut() {
            log.push(RecordedCommand {
                tick: self.tick,
                faction,
                command: cmd.clone(),
            });
        }
        Ok(ack)
    }

    fn execute_command(&mut self, faction: FactionId, cmd: &RtsCommand) -> Result<CommandAck, RtsError> {
        if self.outcome.is_some() {
            return Err(RtsError::MatchOver);
        }
        if self.faction(faction).is_none_or(|f| f.eliminated || f.surrendered) {
            return Err(RtsError::MatchOver);
        }
        match cmd {
            RtsCommand::PlaceBuilding { kind, origin } => self.cmd_place(faction, *kind, *origin),
            RtsCommand::Demolish { building } => self.cmd_demolish(faction, *building),
            RtsCommand::QueueUnit {
                vat,
                unit,
                genome_slug,
                count,
            } => self.cmd_queue(faction, *vat, *unit, genome_slug, *count),
            RtsCommand::CancelQueue { vat } => self.cmd_cancel(faction, *vat),
            RtsCommand::IssueOrder { units, order, target } => self.cmd_order(faction, units, *order, *target),
            RtsCommand::ConfigureBeacon {
                beacon,
                channel,
                strength_milli,
                modulators_milli,
            } => self.cmd_beacon(faction, *beacon, *channel, *strength_milli, *modulators_milli),
            RtsCommand::ResearchTech { site, tech } => self.cmd_research(faction, *site, *tech),
            RtsCommand::Crucible { crucible, action } => match action {
                CrucibleAction::ResearchTech(tech) => self.cmd_research(faction, *crucible, *tech),
                CrucibleAction::BreedEliteGenome {
                    unit,
                    mutation_rate_permille,
                } => self.cmd_breed(faction, *crucible, *unit, *mutation_rate_permille),
            },
            RtsCommand::Surrender => {
                self.faction_mut(faction).expect("已校验").surrendered = true;
                Ok(CommandAck::Surrendered)
            }
        }
    }

    fn owned_building(&self, faction: FactionId, id: EntitySerial) -> Result<&Building, RtsError> {
        let b = self.buildings.get(&id).ok_or(RtsError::UnknownEntity { id })?;
        if b.faction != faction {
            return Err(RtsError::NotOwner { id });
        }
        Ok(b)
    }

    /// 校验建筑落点（不扣费）
    pub fn validate_placement(
        &self,
        faction: FactionId,
        kind: BuildingKind,
        origin: (u32, u32),
    ) -> Result<(), RtsError> {
        let f = building_spec(kind).footprint;
        let invalid = |detail: &str| {
            Err(RtsError::InvalidPlacement {
                detail: detail.to_owned(),
            })
        };
        if origin.0 + f > self.map.width || origin.1 + f > self.map.height {
            return invalid("越界");
        }
        let fog = &self.faction(faction).ok_or(RtsError::MatchOver)?.fog;
        for y in origin.1..origin.1 + f {
            for x in origin.0..origin.0 + f {
                if !allows_building(self.map.tile(x, y), kind) {
                    return invalid("地形不允许");
                }
                if self.nav.cost(x, y) == crate::nav::grid::BLOCKED {
                    return invalid("与既有建筑重叠");
                }
                if !fog.is_visible(x, y) {
                    return invalid("落点不在当前视野内");
                }
            }
        }
        Ok(())
    }

    fn cmd_place(
        &mut self,
        faction: FactionId,
        kind: BuildingKind,
        origin: (u32, u32),
    ) -> Result<CommandAck, RtsError> {
        if kind == BuildingKind::NexusCore {
            return Err(RtsError::NotBuildable { kind });
        }
        self.validate_placement(faction, kind, origin)?;
        let cost = building_spec(kind).cost;
        self.faction_mut(faction)
            .expect("已校验")
            .ledger
            .spend(&cost)
            .map_err(|_| RtsError::InsufficientResources)?;
        let id = self.insert_building(faction, kind, origin, false);
        Ok(CommandAck::BuildingPlaced { id })
    }

    fn cmd_demolish(&mut self, faction: FactionId, id: EntitySerial) -> Result<CommandAck, RtsError> {
        let b = self.owned_building(faction, id)?;
        if b.kind == BuildingKind::NexusCore {
            return Err(RtsError::NotBuildable { kind: b.kind });
        }
        let refund = building_spec(b.kind).cost.scaled(DEMOLISH_REFUND_PERCENT, 100);
        let queued: Vec<ResourceAmounts> = b.queue.iter().map(|j| j.cost).collect();
        self.remove_building(id, false);
        let f = self.faction_mut(faction).expect("已校验");
        f.ledger.refund(&refund);
        for c in queued {
            f.ledger.refund(&c);
        }
        Ok(CommandAck::Demolished { id })
    }

    fn cmd_queue(
        &mut self,
        faction: FactionId,
        vat: EntitySerial,
        unit: UnitKind,
        slug: &str,
        count: u32,
    ) -> Result<CommandAck, RtsError> {
        if !(1..=5).contains(&count) {
            return Err(RtsError::InvalidArgument {
                detail: "count 必须在 [1, 5]".into(),
            });
        }
        let b = self.owned_building(faction, vat)?;
        if b.kind != BuildingKind::SpawningVat {
            return Err(RtsError::WrongEntityKind { id: vat });
        }
        if !b.complete {
            return Err(RtsError::NotComplete { id: vat });
        }
        if b.queue.len() + count as usize > crate::buildings::VAT_QUEUE_LIMIT {
            return Err(RtsError::QueueFull { id: vat });
        }
        let fac = self.faction(faction).expect("已校验");
        if !fac.techs.unit_unlocked(unit) {
            return Err(RtsError::TechLocked { unit });
        }
        let genome = fac
            .genomes
            .get(slug)
            .ok_or_else(|| RtsError::UnknownGenome { slug: slug.to_owned() })?
            .clone();
        if genome.unit != unit {
            return Err(RtsError::GenomeArchetypeMismatch { slug: slug.to_owned() });
        }
        let spec = unit_spec(unit);
        let (used, cap) = self.supply(faction);
        if used + spec.supply * count > cap {
            return Err(RtsError::SupplyCapReached);
        }
        let total = spec.cost.scaled(count as i64, 1);
        self.faction_mut(faction)
            .expect("已校验")
            .ledger
            .spend(&total)
            .map_err(|_| RtsError::InsufficientResources)?;
        let b = self.buildings.get_mut(&vat).expect("已校验");
        for _ in 0..count {
            b.queue.push_back(ProductionJob {
                unit,
                genome: genome.clone(),
                remaining_ticks: spec.hatch_ticks,
                cost: spec.cost,
            });
        }
        Ok(CommandAck::Queued { vat, count })
    }

    fn cmd_cancel(&mut self, faction: FactionId, vat: EntitySerial) -> Result<CommandAck, RtsError> {
        self.owned_building(faction, vat)?;
        let job = self.buildings.get_mut(&vat).expect("已校验").queue.pop_back();
        if let Some(j) = job {
            self.faction_mut(faction).expect("已校验").ledger.refund(&j.cost);
        }
        Ok(CommandAck::Cancelled { vat })
    }

    fn cmd_order(
        &mut self,
        faction: FactionId,
        sel: &UnitSelector,
        kind: OrderKind,
        target: Vec2,
    ) -> Result<CommandAck, RtsError> {
        let max = Vec2::from_int(self.map.width as i32, self.map.height as i32);
        if target.x.0 < 0 || target.y.0 < 0 || target.x >= max.x || target.y >= max.y {
            return Err(RtsError::InvalidArgument {
                detail: "目标坐标越界".into(),
            });
        }
        let ids: Vec<EntitySerial> = match sel {
            UnitSelector::Ids(ids) => {
                for id in ids {
                    let u = self.units.get(id).ok_or(RtsError::UnknownEntity { id: *id })?;
                    if u.faction != faction {
                        return Err(RtsError::NotOwner { id: *id });
                    }
                }
                ids.clone()
            }
            UnitSelector::Archetype(a) => self
                .units
                .values()
                .filter(|u| u.faction == faction && a.is_none_or(|k| k == u.kind))
                .map(|u| u.id)
                .collect(),
        };
        for id in &ids {
            let u = self.units.get_mut(id).expect("已校验");
            u.order = Order {
                kind,
                target,
                origin: u.position,
            };
        }
        Ok(CommandAck::OrderIssued {
            units: ids.len() as u32,
        })
    }

    fn cmd_beacon(
        &mut self,
        faction: FactionId,
        id: EntitySerial,
        channel: FieldId,
        strength_milli: i64,
        modulators_milli: [i64; 4],
    ) -> Result<CommandAck, RtsError> {
        let b = self.owned_building(faction, id)?;
        if b.kind != BuildingKind::SignalBeacon {
            return Err(RtsError::WrongEntityKind { id });
        }
        if !matches!(channel, FieldId::SignalA | FieldId::SignalB) {
            return Err(RtsError::InvalidArgument {
                detail: "信标只能注入 field.signal_a / field.signal_b".into(),
            });
        }
        if !(-50_000..=50_000).contains(&strength_milli) || modulators_milli.iter().any(|m| !(-1000..=1000).contains(m))
        {
            return Err(RtsError::InvalidArgument {
                detail: "强度须在 [-50, 50]，调质分量须在 [-1, 1]".into(),
            });
        }
        let b = self.buildings.get_mut(&id).expect("已校验");
        b.beacon = BeaconConfig {
            channel,
            strength_q8: BeaconConfig::strength_from_milli(strength_milli),
            modulators: Modulators::from_milli(modulators_milli),
        };
        Ok(CommandAck::BeaconConfigured { id })
    }

    fn cmd_research(&mut self, faction: FactionId, site: EntitySerial, tech: TechId) -> Result<CommandAck, RtsError> {
        let spec = tech_spec(tech);
        let b = self.owned_building(faction, site)?;
        if b.kind != spec.research_site() {
            return Err(RtsError::WrongEntityKind { id: site });
        }
        if !b.complete {
            return Err(RtsError::NotComplete { id: site });
        }
        if b.research.is_some() || b.breeding.is_some() {
            return Err(RtsError::SiteBusy { id: site });
        }
        let fac = self.faction(faction).expect("已校验");
        if fac.techs.contains(tech) {
            return Err(RtsError::AlreadyResearched { tech });
        }
        let in_progress = self
            .buildings
            .values()
            .any(|o| o.faction == faction && o.research.is_some_and(|(t, _)| t == tech));
        if in_progress {
            return Err(RtsError::SiteBusy { id: site });
        }
        let has_buildings = spec.requires_buildings.iter().all(|k| {
            self.buildings
                .values()
                .any(|o| o.faction == faction && o.kind == *k && o.complete)
        });
        if !has_buildings || !fac.techs.techs_satisfied(spec) {
            return Err(RtsError::PrerequisitesUnmet { tech });
        }
        self.faction_mut(faction)
            .expect("已校验")
            .ledger
            .spend(&spec.cost)
            .map_err(|_| RtsError::InsufficientResources)?;
        self.buildings.get_mut(&site).expect("已校验").research = Some((tech, spec.research_ticks));
        Ok(CommandAck::ResearchStarted { site, tech })
    }

    fn cmd_breed(
        &mut self,
        faction: FactionId,
        crucible: EntitySerial,
        unit: UnitKind,
        rate: u32,
    ) -> Result<CommandAck, RtsError> {
        if !(MUTATION_RATE_RANGE_PERMILLE.0..=MUTATION_RATE_RANGE_PERMILLE.1).contains(&rate) {
            return Err(RtsError::InvalidArgument {
                detail: "mutation_rate 必须在 [0.01, 0.25]".into(),
            });
        }
        let b = self.owned_building(faction, crucible)?;
        if b.kind != BuildingKind::MutationCrucible {
            return Err(RtsError::WrongEntityKind { id: crucible });
        }
        if !b.complete {
            return Err(RtsError::NotComplete { id: crucible });
        }
        if b.research.is_some() || b.breeding.is_some() {
            return Err(RtsError::SiteBusy { id: crucible });
        }
        let child = self.prepare_breeding(faction, unit, rate)?;
        self.faction_mut(faction)
            .expect("已校验")
            .ledger
            .spend(&BREED_COST)
            .map_err(|_| RtsError::InsufficientResources)?;
        let mods = self.faction(faction).expect("已校验").modifiers();
        let ticks = (BREED_TICKS as i64 * mods.breeding_time_percent / 100) as u32;
        self.buildings.get_mut(&crucible).expect("已校验").breeding = Some(BreedJob {
            unit,
            child,
            remaining_ticks: ticks,
        });
        Ok(CommandAck::BreedingStarted { crucible, unit })
    }

    // ------------------------------------------------------------------ Tick

    /// 推进一个权威 Tick
    pub fn step(&mut self) {
        if self.outcome.is_some() {
            return;
        }
        self.step_economy();
        self.step_construction();
        if self.tick % crate::fields::FIELD_STEP_TICKS == 0 {
            self.step_fields();
        }
        self.step_forager_uptake();
        if self.tick % rynvek_core::constants::COUPLING_EPOCH_TICKS as u64 == 0 {
            self.step_brains();
        }
        self.step_movement();
        self.step_harvest();
        self.step_metabolism();
        self.step_combat();
        self.step_environment();
        self.step_deaths();
        self.step_ecology_returns();
        self.tick += 1;
        if self.tick % FOG_UPDATE_INTERVAL_TICKS == 0 {
            self.update_fog();
        }
        self.step_pve();
        self.step_victory();
    }

    /// 连续推进 `n` 个 Tick（提前结束时停止）
    pub fn run(&mut self, n: u64) {
        for _ in 0..n {
            if self.outcome.is_some() {
                break;
            }
            self.step();
        }
    }

    /// 移除建筑（死亡或拆除）
    pub(crate) fn remove_building(&mut self, id: EntitySerial, destroyed: bool) {
        let Some(b) = self.buildings.remove(&id) else { return };
        let rect = b.rect();
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                self.nav.set_occupied(x, y, false);
            }
        }
        self.hpa.update_region(&self.nav, rect);
        self.invalidate_flow();
        self.recompute_capacity(b.faction);
        if destroyed {
            self.events.push(RtsEvent::BuildingDestroyed {
                id,
                faction: b.faction,
                kind: b.kind,
            });
        }
        if let Some(f) = self.faction_mut(b.faction) {
            if f.nexus == Some(id) {
                f.nexus = None;
                f.eliminated = true;
            }
        }
    }

    /// 导出全部存活单位脑的 `NEUR` 稀疏神经分块（以实体序号 `u.id.0` 作为 `dense_index`）
    pub fn to_neur_chunk(&self) -> NeurChunk {
        let mut rows = Vec::new();
        for u in self.units.values() {
            rows.extend(u.brain.to_neur_rows(u.id.0));
        }
        NeurChunk { rows }
    }

    /// 从 `NEUR` 稀疏神经分块恢复全部存活单位脑的内部动态状态
    pub fn restore_from_neur(&mut self, chunk: &NeurChunk) -> Result<(), RynvekErrorCode> {
        for u in self.units.values_mut() {
            u.brain.restore_from_neur(chunk, u.id.0)?;
        }
        Ok(())
    }

    /// 计算当前世界的完整分层规范化状态根 [`CanonicalWorldState`]
    pub fn canonical_state(&self) -> CanonicalWorldState {
        self.canonical_state_with_parent(Blake3Hash::ZERO)
    }

    /// 带指定前驱提交根计算 [`CanonicalWorldState`]
    pub fn canonical_state_with_parent(&self, parent_commit_root: Blake3Hash) -> CanonicalWorldState {
        // 1. config_root：MatchConfig + 地图尺寸与地形内容摘要
        let config_root = {
            let mut h = Hasher::new();
            // 当前格式绑定整个配置，删除历史模式共用标签与二次身份来源。
            h.update(b"RYNVEK_ARENA_CONFIG_CURRENT_3");
            h.update(&serde_json::to_vec(&self.config).expect("配置仅含确定性枚举和整数"));
            h.update(&self.map.width.to_le_bytes());
            h.update(&self.map.height.to_le_bytes());
            h.update(&self.map.seed.to_le_bytes());
            h.update(&self.map.tiles_hash().0);
            Blake3Hash(*h.finalize().as_bytes())
        };

        // 2. entity_root：全部建筑与全部单位的完整权威属性（不含单位脑内部动态态，后者归入 neural_root）
        let entity_root = {
            let mut h = Hasher::new();
            h.update(b"RYNVEK_SUBROOT_ENTITY");
            h.update(&(self.buildings.len() as u32).to_le_bytes());
            for b in self.buildings.values() {
                h.update(&b.id.0.to_le_bytes());
                h.update(&[
                    b.faction.0,
                    b.kind as u8,
                    b.complete as u8,
                    b.powered as u8,
                    b.on_fissure as u8,
                ]);
                h.update(&b.origin.0.to_le_bytes());
                h.update(&b.origin.1.to_le_bytes());
                h.update(&b.defense.hp_milli.to_le_bytes());
                h.update(&b.defense.shield_milli.to_le_bytes());
                h.update(&b.defense.armor.to_le_bytes());
                h.update(&b.max_hp_milli.to_le_bytes());
                h.update(&b.build_progress.to_le_bytes());
                h.update(&b.weapon_cooldown.to_le_bytes());
                h.update(&b.refinery_progress.to_le_bytes());
                h.update(&b.harvest_remainder.to_le_bytes());
                h.update(&[b.beacon.channel as u8]);
                h.update(&b.beacon.strength_q8.to_le_bytes());
                for m in b.beacon.modulators.0 {
                    h.update(&m.to_le_bytes());
                }
                h.update(&(b.queue.len() as u32).to_le_bytes());
                for job in &b.queue {
                    h.update(&[job.unit as u8]);
                    h.update(&job.genome.digest().0);
                    h.update(&job.remaining_ticks.to_le_bytes());
                    h.update(&job.cost.biomass.to_le_bytes());
                    h.update(&job.cost.flux.to_le_bytes());
                    h.update(&job.cost.neurogel.to_le_bytes());
                }
                match b.research {
                    Some((tech, rem)) => {
                        h.update(&[1u8, tech as u8]);
                        h.update(&rem.to_le_bytes());
                    }
                    None => {
                        h.update(&[0u8]);
                    }
                }
                match &b.breeding {
                    Some(job) => {
                        h.update(&[1u8, job.unit as u8]);
                        h.update(&job.child.digest().0);
                        h.update(&job.remaining_ticks.to_le_bytes());
                    }
                    None => {
                        h.update(&[0u8]);
                    }
                }
            }

            h.update(&(self.units.len() as u32).to_le_bytes());
            for u in self.units.values() {
                h.update(&u.id.0.to_le_bytes());
                h.update(&[u.faction.0, u.kind as u8, u.order.kind as u8]);
                h.update(&u.genome.digest().0);
                for v in [
                    u.position.x.0,
                    u.position.y.0,
                    u.velocity.x.0,
                    u.velocity.y.0,
                    u.defense.hp_milli,
                    u.defense.shield_milli,
                    u.defense.armor,
                    u.max_hp_milli,
                    u.max_shield_milli,
                    u.order.target.x.0,
                    u.order.target.y.0,
                    u.order.origin.x.0,
                    u.order.origin.y.0,
                    u.fitness.damage_dealt_milli,
                    u.harvest_remainder,
                ] {
                    h.update(&v.to_le_bytes());
                }
                h.update(&u.weapon_cooldown.to_le_bytes());
                h.update(&u.born_tick.to_le_bytes());
                h.update(&u.fitness.survival_ticks.to_le_bytes());
                for m in u.modulators.0 {
                    h.update(&m.to_le_bytes());
                }
                for a in u.actions {
                    h.update(&a.to_le_bytes());
                }
            }
            if let Some(ecology) = &self.ecology {
                h.update(b"energy_exchange_v1");
                h.update(&serde_json::to_vec(ecology).expect("能量账本仅含整数与有序键"));
            }
            Blake3Hash(*h.finalize().as_bytes())
        };

        // 3. neural_root：全部存活单位的四模型脑内部动态状态（LIF / GatedMLP / Izhikevich / CTRNN）与读出
        let neural_root = {
            let mut h = Hasher::new();
            h.update(b"RYNVEK_SUBROOT_NEURAL");
            h.update(&(self.units.len() as u32).to_le_bytes());
            for u in self.units.values() {
                h.update(&u.id.0.to_le_bytes());
                h.update(&u.brain.state_root().0);
                for a in u.actions {
                    h.update(&a.to_le_bytes());
                }
                for m in u.modulators.0 {
                    h.update(&m.to_le_bytes());
                }
            }
            Blake3Hash(*h.finalize().as_bytes())
        };

        // 4. field_root：五通道连续反应扩散环境场
        let field_root = self.fields.state_root();

        // 5. economy_root：阵营账本（余额、容量上限、累计采集、溢出丢弃）、科技树、主脑锚点、投降与淘汰标记
        let economy_root = {
            let mut h = Hasher::new();
            h.update(b"RYNVEK_SUBROOT_ECONOMY");
            h.update(&(self.factions.len() as u32).to_le_bytes());
            for f in &self.factions {
                h.update(&[f.id.0, f.surrendered as u8, f.eliminated as u8]);
                let l = &f.ledger;
                for v in [
                    l.balance.biomass,
                    l.balance.flux,
                    l.balance.neurogel,
                    l.capacity.biomass,
                    l.capacity.flux,
                    l.capacity.neurogel,
                    l.biomass_harvested_total,
                    l.overflow_discarded.biomass,
                    l.overflow_discarded.flux,
                    l.overflow_discarded.neurogel,
                ] {
                    h.update(&v.to_le_bytes());
                }
                h.update(&f.techs.0.to_le_bytes());
                match f.nexus {
                    Some(id) => {
                        h.update(&[1u8]);
                        h.update(&id.0.to_le_bytes());
                    }
                    None => {
                        h.update(&[0u8]);
                    }
                }
            }
            Blake3Hash(*h.finalize().as_bytes())
        };

        // 6. evolution_root：阵营基因池、繁育计数、坩埚在途繁育、PvE 导演基因池与个体适应度归档
        let evolution_root = {
            let mut h = Hasher::new();
            h.update(b"RYNVEK_SUBROOT_EVOLUTION");
            for f in &self.factions {
                h.update(&[f.id.0]);
                h.update(&f.bred_count.to_le_bytes());
                h.update(&(f.genomes.len() as u32).to_le_bytes());
                for (slug, g) in &f.genomes {
                    h.update(slug.as_bytes());
                    h.update(&[0u8]);
                    h.update(&g.digest().0);
                }
            }
            match &self.pve {
                Some(d) => {
                    h.update(&[1u8]);
                    h.update(&d.state_root().0);
                }
                None => {
                    h.update(&[0u8]);
                }
            }
            h.update(&(self.pve_records.len() as u32).to_le_bytes());
            for (serial, g, fit) in &self.pve_records {
                h.update(&serial.to_le_bytes());
                h.update(&g.digest().0);
                h.update(&fit.damage_dealt_milli.to_le_bytes());
                h.update(&fit.survival_ticks.to_le_bytes());
            }
            Blake3Hash(*h.finalize().as_bytes())
        };

        // 7. visibility_root：各阵营战争迷雾（visible + explored 位图）与敌方建筑幽灵快照
        let visibility_root = {
            let mut h = Hasher::new();
            h.update(b"RYNVEK_SUBROOT_VISIBILITY");
            for f in &self.factions {
                h.update(&[f.id.0]);
                h.update(&f.fog.state_root().0);
                h.update(&(f.ghosts.len() as u32).to_le_bytes());
                for (id, g) in &f.ghosts {
                    h.update(&id.0.to_le_bytes());
                    h.update(&g.entity_id.0.to_le_bytes());
                    h.update(&[g.faction.0, g.kind as u8]);
                    h.update(&g.origin.0.to_le_bytes());
                    h.update(&g.origin.1.to_le_bytes());
                    h.update(&g.last_seen_tick.to_le_bytes());
                }
            }
            Blake3Hash(*h.finalize().as_bytes())
        };

        // 8. rng_root：世界随机种子、当前 Tick 与实体序号发生器状态
        let rng_root = {
            let mut h = Hasher::new();
            h.update(b"RYNVEK_SUBROOT_RNG");
            h.update(&self.config.world_seed.to_le_bytes());
            h.update(&self.tick.to_le_bytes());
            h.update(&self.next_serial.to_le_bytes());
            Blake3Hash(*h.finalize().as_bytes())
        };

        // 9. scheduled_event_root：突然死亡倒计时与胜负裁决结果
        let scheduled_event_root = {
            let mut h = Hasher::new();
            h.update(b"RYNVEK_SUBROOT_SCHEDULED_EVENT");
            match self.sudden_death_until {
                Some(t) => {
                    h.update(&[1u8]);
                    h.update(&t.to_le_bytes());
                }
                None => {
                    h.update(&[0u8]);
                }
            }
            match &self.outcome {
                Some(MatchOutcome::Victory { winner, reason, tick }) => {
                    h.update(&[1u8, winner.0, *reason as u8]);
                    h.update(&tick.to_le_bytes());
                }
                Some(MatchOutcome::Draw { tick }) => {
                    h.update(&[2u8]);
                    h.update(&tick.to_le_bytes());
                }
                Some(MatchOutcome::Defeat { tick, waves_cleared }) => {
                    h.update(&[3u8]);
                    h.update(&tick.to_le_bytes());
                    h.update(&waves_cleared.to_le_bytes());
                }
                None => {
                    h.update(&[0u8]);
                }
            }
            Blake3Hash(*h.finalize().as_bytes())
        };

        CanonicalWorldState {
            tick: self.tick,
            config_root,
            entity_root,
            neural_root,
            field_root,
            economy_root,
            evolution_root,
            visibility_root,
            rng_root,
            scheduled_event_root,
            parent_commit_root,
        }
    }

    /// 世界状态根：由 [`CanonicalWorldState`] 聚合全部十类权威子状态根得到
    pub fn state_hash(&self) -> Blake3Hash {
        self.canonical_state().state_root()
    }

    /// 建筑平衡表遍历（UI 建造菜单）
    pub fn building_catalog() -> &'static [crate::buildings::BuildingSpec] {
        &BUILDING_SPECS
    }
}
