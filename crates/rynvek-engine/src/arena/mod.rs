// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Rynvek Arena 对局会话：把 `rynvek-rts` 权威世界、`rynvek-agent` 控制协议与客户端消息协议组合成可玩闭环。
//!
//! 线上协议 `rynvek-arena/1`（JSON，`type` 标签、snake_case，ERRATA E-035）：
//!
//! | 客户端 → 服务端 | 服务端 → 客户端 |
//! |---|---|
//! | `hello`：入座（Principal + 阵营） | `welcome`：阵营、模式、地图（4 bit 打包 Base64）、工具清单 |
//! | `acquire_lease` / `release_lease` | `lease` / `lease_revoked` |
//! | `command`：权威 [`RtsCommand`] + Fencing Token | `ack`：[`CommandAck`] 或错误体 |
//! | `tool_call`：LLM 工具调用 | `tool_result`：[`ToolResponse`] |
//! | `advance`：推进 N Tick（本地练习 / PvE / 测试；排位服务器自行计时） | `observation`：迷雾裁剪观测 + 本阵营事件 |
//!
//! 人类、脚本与 Agent 走同一套 L4 契约：每条 `command` 都要求有效租约与足够的 [`ControlLevel`]，并计入阵营 600 APM。
//! 人类申请租约即无感接管 Agent（旧 Token 立即失效并收到 `lease_revoked`）；人类执行动作 27
//! `rts.release_human_lease` 交还后，被接管的 Agent 自动按原等级恢复租约。

pub mod tool_host;

use std::collections::{BTreeMap, VecDeque};

use rynvek_agent::error::ErrorBody;
use rynvek_agent::{
    AgentError, ControlLease, ControlLevel, MonotonicClock, Principal, PrincipalKind, ToolBroker, ToolRequest,
    ToolResponse, ToolSchema,
};
use rynvek_rts::commands::{CommandAck, RtsCommand};
use rynvek_rts::observation::FactionObservation;
use rynvek_rts::world::{MATCH_TIME_LIMIT_TICKS, SUDDEN_DEATH_TICKS};
use rynvek_rts::{ArenaMapSize, ArenaWorld, FactionId, MatchConfig, MatchMode, RtsEvent};
use serde::{Deserialize, Serialize};

pub use tool_host::{ArenaPlan, ArenaToolHost};

/// 协议版本标识
pub const ARENA_PROTOCOL: &str = "rynvek-arena/1";
/// 单条 `advance` 最多推进的 Tick 数（30 s）
pub const MAX_ADVANCE_TICKS: u32 = 900;
/// 租约时长：覆盖整局（时限 + 突然死亡），中途可续期
pub const ARENA_LEASE_TICKS: u64 = MATCH_TIME_LIMIT_TICKS + SUDDEN_DEATH_TICKS;

/// 命令所需的控制等级（与工具等级表一致，ERRATA E-030）
pub const fn command_level(cmd: &RtsCommand) -> ControlLevel {
    match cmd {
        RtsCommand::IssueOrder { .. } | RtsCommand::ConfigureBeacon { .. } => ControlLevel::Modulate,
        RtsCommand::PlaceBuilding { .. }
        | RtsCommand::QueueUnit { .. }
        | RtsCommand::CancelQueue { .. }
        | RtsCommand::ResearchTech { .. }
        | RtsCommand::Crucible { .. } => ControlLevel::Produce,
        RtsCommand::Demolish { .. } | RtsCommand::Surrender => ControlLevel::Sovereign,
    }
}

/// 客户端消息
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ArenaClientMessage {
    Hello {
        protocol: String,
        principal_id: String,
        kind: PrincipalKind,
        faction: u32,
    },
    AcquireLease {
        seq: u64,
        level: ControlLevel,
    },
    ReleaseLease {
        seq: u64,
        fencing_token: u64,
    },
    Command {
        seq: u64,
        fencing_token: u64,
        command: RtsCommand,
    },
    ToolCall {
        seq: u64,
        fencing_token: u64,
        tool: String,
        arguments: serde_json::Value,
    },
    Advance {
        ticks: u32,
    },
}

/// 地图信息
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapInfo {
    pub size: ArenaMapSize,
    pub width: u32,
    pub height: u32,
    pub seed: u64,
    /// 地形 BLAKE3（十六进制），客户端解码后自校验
    pub tiles_hash: String,
    /// 地形 4 bit 打包（偶数下标在低半字节）后的标准 Base64
    pub terrain_b64: String,
    pub spawn_tiles: [(u32, u32); 2],
}

/// 服务端消息
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ArenaServerMessage {
    Welcome {
        protocol: String,
        faction: u32,
        mode: MatchMode,
        tick: u64,
        map: MapInfo,
        tools: Vec<ToolSchema>,
    },
    Lease {
        seq: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        lease: Option<ControlLease>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    LeaseRevoked {
        faction: u32,
        by: String,
    },
    Ack {
        seq: u64,
        tick: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        ack: Option<CommandAck>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    ToolResult {
        seq: u64,
        response: ToolResponse,
    },
    Observation {
        observation: Box<FactionObservation>,
        events: Vec<RtsEvent>,
    },
    Error {
        error: ErrorBody,
    },
}

/// 带收件人的服务端消息
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub to: String,
    pub message: ArenaServerMessage,
}

#[derive(Clone, Debug)]
struct Seat {
    principal: Principal,
    faction: u32,
}

/// 标准 Base64（RFC 4648，带填充）
pub fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// 地图 → [`MapInfo`]
pub fn map_info(world: &ArenaWorld) -> MapInfo {
    let bytes = world.map.tile_bytes();
    let packed: Vec<u8> = bytes
        .chunks(2)
        .map(|p| p[0] | p.get(1).copied().unwrap_or(0) << 4)
        .collect();
    MapInfo {
        size: world.config.map_size,
        width: world.map.width,
        height: world.map.height,
        seed: world.config.map_seed,
        tiles_hash: world.map.tiles_hash().to_hex(),
        terrain_b64: base64_encode(&packed),
        spawn_tiles: world.map.spawn_tiles(),
    }
}

/// 事件是否对某阵营可见：本阵营事件与全局事件（虫潮、突然死亡、终局）
fn event_visible_to(e: &RtsEvent, faction: FactionId) -> bool {
    match e {
        RtsEvent::BuildingPlaced { faction: f, .. }
        | RtsEvent::BuildingCompleted { faction: f, .. }
        | RtsEvent::BuildingDestroyed { faction: f, .. }
        | RtsEvent::UnitSpawned { faction: f, .. }
        | RtsEvent::UnitDied { faction: f, .. }
        | RtsEvent::ResearchCompleted { faction: f, .. }
        | RtsEvent::GenomeBred { faction: f, .. } => *f == faction,
        RtsEvent::WaveSpawned { .. } | RtsEvent::SuddenDeathStarted { .. } | RtsEvent::MatchEnded { .. } => true,
    }
}

/// 竞技场会话
#[derive(Debug)]
pub struct ArenaSession<C: MonotonicClock> {
    pub world: ArenaWorld,
    broker: ToolBroker<C>,
    seats: BTreeMap<String, Seat>,
    /// 被人类接管的 Agent（阵营 → 原持有者与等级），人类交还后自动恢复
    standby: BTreeMap<u32, (Principal, ControlLevel)>,
    /// 各阵营尚未下发的事件
    pending_events: BTreeMap<u32, VecDeque<RtsEvent>>,
}

impl<C: MonotonicClock> ArenaSession<C> {
    pub fn new(config: MatchConfig, clock: C) -> Self {
        Self {
            world: ArenaWorld::new(config),
            broker: ToolBroker::new(clock),
            seats: BTreeMap::new(),
            standby: BTreeMap::new(),
            pending_events: BTreeMap::new(),
        }
    }

    pub fn broker(&self) -> &ToolBroker<C> {
        &self.broker
    }

    fn reply(to: &str, message: ArenaServerMessage) -> Envelope {
        Envelope {
            to: to.to_owned(),
            message,
        }
    }

    fn error(to: &str, e: AgentError) -> Vec<Envelope> {
        vec![Self::reply(to, ArenaServerMessage::Error { error: e.body() })]
    }

    /// 处理来自 `from` 的一条消息，返回需要投递的全部消息（按投递顺序）
    pub fn handle(&mut self, from: &str, message: ArenaClientMessage) -> Vec<Envelope> {
        if let ArenaClientMessage::Hello {
            protocol,
            principal_id,
            kind,
            faction,
        } = message
        {
            return self.hello(from, protocol, principal_id, kind, faction);
        }
        let Some(seat) = self.seats.get(from).cloned() else {
            return Self::error(from, AgentError::unauthorized("尚未入座：请先发送 hello"));
        };
        let tick = self.world.tick;
        match message {
            ArenaClientMessage::Hello { .. } => unreachable!("已在上方处理"),
            ArenaClientMessage::AcquireLease { seq, level } => self.acquire(&seat, seq, level),
            ArenaClientMessage::ReleaseLease { seq, fencing_token } => self.release(&seat, seq, fencing_token),
            ArenaClientMessage::Command {
                seq,
                fencing_token,
                command,
            } => {
                let result = self.command(&seat, fencing_token, &command);
                let (ack, error) = match result {
                    Ok(a) => (Some(a), None),
                    Err(e) => (None, Some(e.body())),
                };
                vec![Self::reply(from, ArenaServerMessage::Ack { seq, tick, ack, error })]
            }
            ArenaClientMessage::ToolCall {
                seq,
                fencing_token,
                tool,
                arguments,
            } => {
                let request = ToolRequest {
                    principal_id: seat.principal.id.clone(),
                    faction: seat.faction,
                    fencing_token,
                    tool,
                    arguments,
                };
                let mut host = ArenaToolHost { world: &mut self.world };
                let response = self.broker.invoke(&mut host, &request);
                vec![Self::reply(from, ArenaServerMessage::ToolResult { seq, response })]
            }
            ArenaClientMessage::Advance { ticks } => {
                if ticks == 0 || ticks > MAX_ADVANCE_TICKS {
                    return Self::error(
                        from,
                        AgentError::invalid_arguments(format!("advance 必须在 [1, {MAX_ADVANCE_TICKS}]")),
                    );
                }
                self.advance(ticks)
            }
        }
    }

    fn hello(&mut self, from: &str, protocol: String, id: String, kind: PrincipalKind, faction: u32) -> Vec<Envelope> {
        if protocol != ARENA_PROTOCOL {
            return Self::error(
                from,
                AgentError::invalid_arguments(format!("不支持的协议 `{protocol}`")),
            );
        }
        if id != from {
            return Self::error(from, AgentError::unauthorized("principal_id 与连接身份不一致"));
        }
        let valid_faction = u8::try_from(faction)
            .ok()
            .and_then(|f| self.world.faction(FactionId(f)))
            .is_some();
        if !valid_faction {
            return Self::error(from, AgentError::invalid_arguments(format!("阵营 {faction} 不存在")));
        }
        self.seats.insert(
            id.clone(),
            Seat {
                principal: Principal::new(id, kind),
                faction,
            },
        );
        vec![Self::reply(
            from,
            ArenaServerMessage::Welcome {
                protocol: ARENA_PROTOCOL.to_owned(),
                faction,
                mode: self.world.config.mode,
                tick: self.world.tick,
                map: map_info(&self.world),
                tools: self.broker.registry().tools().to_vec(),
            },
        )]
    }

    fn acquire(&mut self, seat: &Seat, seq: u64, level: ControlLevel) -> Vec<Envelope> {
        let tick = self.world.tick;
        let previous = self
            .broker
            .leases()
            .current(seat.faction)
            .filter(|l| l.is_active(tick))
            .cloned();
        match self
            .broker
            .acquire_lease(seat.principal.clone(), seat.faction, level, tick, ARENA_LEASE_TICKS)
        {
            Ok(lease) => {
                let mut out = Vec::new();
                if let Some(prev) = previous.filter(|p| p.holder.id != seat.principal.id) {
                    // 人类无感接管：记下被接管者，交还时恢复
                    self.standby.insert(seat.faction, (prev.holder.clone(), prev.level));
                    out.push(Self::reply(
                        &prev.holder.id,
                        ArenaServerMessage::LeaseRevoked {
                            faction: seat.faction,
                            by: seat.principal.id.clone(),
                        },
                    ));
                }
                out.insert(
                    0,
                    Self::reply(
                        &seat.principal.id,
                        ArenaServerMessage::Lease {
                            seq: Some(seq),
                            lease: Some(lease),
                            error: None,
                        },
                    ),
                );
                out
            }
            Err(e) => vec![Self::reply(
                &seat.principal.id,
                ArenaServerMessage::Lease {
                    seq: Some(seq),
                    lease: None,
                    error: Some(e.body()),
                },
            )],
        }
    }

    fn release(&mut self, seat: &Seat, seq: u64, token: u64) -> Vec<Envelope> {
        let held_by_caller = self
            .broker
            .leases()
            .current(seat.faction)
            .is_some_and(|l| l.holder.id == seat.principal.id);
        let result = if held_by_caller {
            self.broker.leases_mut().release(seat.faction, token)
        } else {
            Err(AgentError::unauthorized("调用方未持有该阵营租约"))
        };
        let mut out = vec![Self::reply(
            &seat.principal.id,
            ArenaServerMessage::Lease {
                seq: Some(seq),
                lease: None,
                error: result.as_ref().err().map(AgentError::body),
            },
        )];
        if result.is_ok() {
            if let Some((agent, level)) = self.standby.remove(&seat.faction) {
                if let Ok(lease) =
                    self.broker
                        .acquire_lease(agent.clone(), seat.faction, level, self.world.tick, ARENA_LEASE_TICKS)
                {
                    out.push(Self::reply(
                        &agent.id,
                        ArenaServerMessage::Lease {
                            seq: None,
                            lease: Some(lease),
                            error: None,
                        },
                    ));
                }
            }
        }
        out
    }

    fn command(&mut self, seat: &Seat, token: u64, cmd: &RtsCommand) -> Result<CommandAck, AgentError> {
        let tick = self.world.tick;
        let lease = self
            .broker
            .leases()
            .authorize(seat.faction, &seat.principal.id, token, tick)?
            .clone();
        let need = command_level(cmd);
        if lease.level < need {
            return Err(AgentError::capability_denied(format!(
                "该命令需要控制等级 {need:?}，当前租约为 {:?}",
                lease.level
            )));
        }
        self.broker.apm_mut(seat.faction).check(tick)?;
        let ack = self
            .world
            .apply_command(FactionId(seat.faction as u8), cmd)
            .map_err(|e| AgentError::new(e.code(), e.to_string()))?;
        self.broker.apm_mut(seat.faction).try_acquire(tick)?;
        Ok(ack)
    }

    /// 推进权威世界 `ticks` 个 Tick，返回各阵营可见的事件（按阵营编号升序；全局事件每个阵营各一份）。
    ///
    /// 由 `advance` 消息（本地 / PvE / 测试）与 `rynvek-server` 的实时 Tick 驱动共用；
    /// 调用方负责把事件与观测按自己的线上格式下发。
    pub fn step_world(&mut self, ticks: u32) -> BTreeMap<u32, Vec<RtsEvent>> {
        self.world.run(ticks as u64);
        let events = self.world.drain_events();
        (0..self.world.factions.len() as u32)
            .map(|f| {
                let visible = events
                    .iter()
                    .filter(|e| event_visible_to(e, FactionId(f as u8)))
                    .cloned()
                    .collect();
                (f, visible)
            })
            .collect()
    }

    /// 某连接身份入座的阵营
    pub fn seat_faction(&self, id: &str) -> Option<u32> {
        self.seats.get(id).map(|s| s.faction)
    }

    /// 离座（连接关闭）；其持有的租约保留到过期，以便断线重连（`ResumeToken`）后继续使用
    pub fn leave(&mut self, id: &str) -> bool {
        self.seats.remove(id).is_some()
    }

    fn advance(&mut self, ticks: u32) -> Vec<Envelope> {
        for (f, events) in self.step_world(ticks) {
            self.pending_events.entry(f).or_default().extend(events);
        }
        let mut out = Vec::new();
        for (id, seat) in &self.seats {
            let Some(observation) = self.world.observe(FactionId(seat.faction as u8)) else {
                continue;
            };
            // 同阵营多席位（人类 + 待命 Agent）共享同一事件流
            let events: Vec<RtsEvent> = self
                .pending_events
                .get(&seat.faction)
                .into_iter()
                .flatten()
                .cloned()
                .collect();
            out.push(Self::reply(
                id,
                ArenaServerMessage::Observation {
                    observation: Box::new(observation),
                    events,
                },
            ));
        }
        self.pending_events.clear();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        let v = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ];
        for (i, o) in v {
            assert_eq!(base64_encode(i.as_bytes()), o);
        }
    }

    #[test]
    fn command_levels_cover_every_command() {
        assert_eq!(command_level(&RtsCommand::Surrender), ControlLevel::Sovereign);
        assert_eq!(
            command_level(&RtsCommand::CancelQueue {
                vat: rynvek_rts::EntitySerial(1)
            }),
            ControlLevel::Produce
        );
    }
}
