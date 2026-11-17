// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `rynvek-agent::ToolHost` 在 `ArenaWorld` 上的实现：把 6 个标准工具翻译为感知查询或 [`RtsCommand`]。
//!
//! - `plan` 阶段只读：解析文本 ID、检查实体归属与种类、预校验建筑落点；
//! - `commit` 阶段调用 [`ArenaRuntime::apply_command`]，权威层仍做完整校验（资源、人口、科技前置等），
//!   拒绝原因以 [`RtsError::code`] 原样透传。
//! - `queue_vat_production` 的 "或发起建筑建造请求"（规范 9.1 描述）：当 `unit_archetype` 是 `bld.*` 时，
//!   `vat_entity_id` 解释为占地左下角 Tile `"x,y"`，`count` 必须为 1，`genome_slug` 忽略（ERRATA E-035）。

use rynvek_agent::{AgentError, CrucibleToolAction, ToolCall, ToolContext, ToolHost};
use rynvek_rts::commands::{CrucibleAction, RtsCommand, RtsError, UnitSelector};
use rynvek_rts::evolution::DEFAULT_MUTATION_RATE_PERMILLE;
use rynvek_rts::{ArenaRuntime, BuildingKind, EntitySerial, FactionId, FieldId, OrderKind, TechId, UnitKind, Vec2};
use serde_json::{Value, json};

/// 已通过只读校验的计划
#[derive(Clone, Debug, PartialEq)]
pub enum ArenaPlan {
    /// 感知查询结果（已在 plan 阶段算好）
    Report(Value),
    /// 待提交的权威命令
    Command(RtsCommand),
}

/// `ArenaWorld` 工具宿主
#[derive(Debug)]
pub struct ArenaToolHost<'w> {
    pub world: &'w mut ArenaRuntime,
}

fn rts_err(e: RtsError) -> AgentError {
    AgentError::new(e.code(), e.to_string())
}

fn bad(msg: impl Into<String>) -> AgentError {
    AgentError::invalid_arguments(msg)
}

fn faction_of(ctx: &ToolContext) -> Result<FactionId, AgentError> {
    u8::try_from(ctx.faction)
        .map(FactionId)
        .map_err(|_| bad("阵营编号越界"))
}

fn parse_entity(text: &str) -> Result<EntitySerial, AgentError> {
    EntitySerial::parse(text).ok_or_else(|| bad(format!("实体 ID `{text}` 不是十进制序号")))
}

fn parse_origin(text: &str) -> Result<(u32, u32), AgentError> {
    let (x, y) = text.split_once(',').ok_or_else(|| bad("建筑落点必须写作 \"x,y\""))?;
    let p = |s: &str| s.trim().parse::<u32>().map_err(|_| bad("建筑落点坐标必须是非负整数"));
    Ok((p(x)?, p(y)?))
}

fn chunk_coord(v: i64) -> Result<u32, AgentError> {
    u32::try_from(v).map_err(|_| bad("Chunk 坐标必须是非负整数"))
}

impl ArenaToolHost<'_> {
    /// 只读检查：实体存在、归属本阵营且种类正确
    fn owned(&self, faction: FactionId, id: EntitySerial, kind: BuildingKind) -> Result<(), AgentError> {
        let b = self
            .world
            .buildings
            .get(&id)
            .ok_or(rts_err(RtsError::UnknownEntity { id }))?;
        if b.faction != faction {
            return Err(rts_err(RtsError::NotOwner { id }));
        }
        if b.kind != kind {
            return Err(rts_err(RtsError::WrongEntityKind { id }));
        }
        Ok(())
    }
}

impl ToolHost for ArenaToolHost<'_> {
    type Plan = ArenaPlan;

    fn current_tick(&self) -> u64 {
        self.world.tick
    }

    fn plan(&mut self, ctx: &ToolContext, call: &ToolCall) -> Result<ArenaPlan, AgentError> {
        let faction = faction_of(ctx)?;
        if self.world.faction(faction).is_none() {
            return Err(rts_err(RtsError::MatchOver));
        }
        Ok(match call {
            ToolCall::QueryVisibleClusters {
                region_chunk,
                include_enemy,
            } => {
                let region = match region_chunk {
                    Some((x, y)) => Some((chunk_coord(*x)?, chunk_coord(*y)?)),
                    None => None,
                };
                let report = self.world.query_visible_clusters(faction, region, *include_enemy);
                ArenaPlan::Report(serde_json::to_value(report).expect("可序列化"))
            }
            ToolCall::SampleRegionalFields { chunk_x, chunk_y } => {
                let report = self
                    .world
                    .sample_regional_fields(faction, chunk_coord(*chunk_x)?, chunk_coord(*chunk_y)?)
                    .ok_or_else(|| bad("Chunk 超出地图范围"))?;
                ArenaPlan::Report(serde_json::to_value(report).expect("可序列化"))
            }
            ToolCall::IssueStrategicIntent {
                unit_archetype,
                order_kind,
                target_world_milli: (x, y),
            } => {
                let selector = if unit_archetype == "all" {
                    UnitSelector::Archetype(None)
                } else {
                    UnitSelector::Archetype(Some(
                        UnitKind::parse(unit_archetype).ok_or_else(|| bad(format!("未知原型 `{unit_archetype}`")))?,
                    ))
                };
                let order = OrderKind::parse(order_kind).ok_or_else(|| bad(format!("未知指令 `{order_kind}`")))?;
                let (w, h) = (self.world.map.width as i64 * 1000, self.world.map.height as i64 * 1000);
                if !(0..w).contains(x) || !(0..h).contains(y) {
                    return Err(bad("目标点不在地图内"));
                }
                ArenaPlan::Command(RtsCommand::IssueOrder {
                    units: selector,
                    order,
                    target: Vec2::from_milli(*x, *y),
                })
            }
            ToolCall::ConfigureSignalBeacon {
                beacon_entity_id,
                signal_channel,
                signal_strength_milli,
                neuromodulators_milli,
            } => {
                let beacon = parse_entity(beacon_entity_id)?;
                self.owned(faction, beacon, BuildingKind::SignalBeacon)?;
                let channel = FieldId::parse(signal_channel).ok_or_else(|| bad("未知信号通道"))?;
                ArenaPlan::Command(RtsCommand::ConfigureBeacon {
                    beacon,
                    channel,
                    strength_milli: *signal_strength_milli,
                    modulators_milli: *neuromodulators_milli,
                })
            }
            ToolCall::QueueVatProduction {
                vat_entity_id,
                unit_archetype,
                genome_slug,
                count,
            } => {
                if let Some(kind) = BuildingKind::parse(unit_archetype) {
                    if *count != 1 {
                        return Err(bad("建造请求的 count 必须为 1"));
                    }
                    let origin = parse_origin(vat_entity_id)?;
                    self.world.validate_placement(faction, kind, origin).map_err(rts_err)?;
                    ArenaPlan::Command(RtsCommand::PlaceBuilding { kind, origin })
                } else {
                    let unit =
                        UnitKind::parse(unit_archetype).ok_or_else(|| bad(format!("未知原型 `{unit_archetype}`")))?;
                    let vat = parse_entity(vat_entity_id)?;
                    self.owned(faction, vat, BuildingKind::SpawningVat)?;
                    ArenaPlan::Command(RtsCommand::QueueUnit {
                        vat,
                        unit,
                        genome_slug: genome_slug.clone(),
                        count: *count,
                    })
                }
            }
            ToolCall::TriggerCrucibleBreeding {
                crucible_entity_id,
                action,
                tech_or_archetype_id,
                mutation_rate_permille,
            } => {
                let crucible = parse_entity(crucible_entity_id)?;
                self.owned(faction, crucible, BuildingKind::MutationCrucible)?;
                let action = match action {
                    CrucibleToolAction::ResearchTech => CrucibleAction::ResearchTech(
                        TechId::parse(tech_or_archetype_id).ok_or_else(|| bad("未知科技 ID"))?,
                    ),
                    CrucibleToolAction::BreedEliteGenome => CrucibleAction::BreedEliteGenome {
                        unit: UnitKind::parse(tech_or_archetype_id).ok_or_else(|| bad("未知单位原型"))?,
                        mutation_rate_permille: mutation_rate_permille.unwrap_or(DEFAULT_MUTATION_RATE_PERMILLE),
                    },
                };
                ArenaPlan::Command(RtsCommand::Crucible { crucible, action })
            }
        })
    }

    fn commit(&mut self, ctx: &ToolContext, plan: ArenaPlan) -> Result<Value, AgentError> {
        match plan {
            ArenaPlan::Report(v) => Ok(v),
            ArenaPlan::Command(cmd) => {
                let ack = self
                    .world
                    .apply_command(faction_of(ctx)?, &cmd)
                    .map_err(|e| AgentError::new(e, "工具命令事务已拒绝"))?;
                Ok(json!({ "command": cmd, "ack": ack }))
            }
        }
    }
}
