// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 权威对局宿主（D202–D208、D645–D657）：每局一个 Tokio 任务（Actor），独占 [`ArenaSession`] 与全部会话状态。
//!
//! ## 通道负载约定（`rynvek-wire/1`，ERRATA 记录）
//!
//! | 通道 | 上行（客户端 → 服务端） | 下行（服务端 → 客户端） |
//! |---|---|---|
//! | `0 Control` | Protobuf `ClientHello` / `Heartbeat` / `KeyframeRequest` / `Disconnect` | `ServerHello` / `HeartbeatAck` / `Disconnect` |
//! | `1 Command` | JSON `CommandEnvelope<ArenaClientMessage>`（`rynvek-arena/1` 负载） | JSON [`CommandReply`]（`ArenaServerMessage`） |
//! | `2 Query` | JSON [`QueryRequest`] | JSON [`QueryReply`] |
//! | `3 Event` | —（上行即违例） | JSON `RtsEvent` 数组（已按阵营过滤） |
//! | `4 StateStream` | —（上行即违例） | 二进制 [`StatePacket`]（迷雾物理裁剪，关键帧置 `KEYFRAME` 标志） |
//! | `5 Telemetry` | 任意（仅计数，不解析） | — |
//!
//! ## 防线
//!
//! 1. 握手：`CapabilityToken` 验签 + `world.connect` + ABAC（`stream` 模块完成）；
//! 2. 帧：16 字节帧头、序号连续、LZ4 上限（`rynvek-network`）；
//! 3. 命令闸门：身份绑定 → Tick 时间窗 → 幂等键 → 突发速率（`rynvek-security::CommandGate`）；
//! 4. 规则：租约 / 控制等级 / APM / 资源与落点（`ArenaSession` + `rynvek-rts`）；
//! 5. 审计：行为监测可疑项写入 `audit_logs`；终局生成签名 `.rreplay` 供裁判复核。
//!
//! 客户端 `advance` 一律拒绝：排位对局只由服务器计时（[`TickDriver`]）。

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::transactions::{PatchApplied, TickCommitter, TickInput, TickRecord, conflict};
use rynvek_agent::{AgentError, PrincipalKind, SystemClock};
use rynvek_core::error::RynvekErrorCode;
use rynvek_engine::arena::{ARENA_PROTOCOL, ArenaClientMessage, ArenaServerMessage, ArenaSession, map_info};
use rynvek_network::control::{ControlBody, ControlMessage, Disconnect, HeartbeatAck, ServerHello};
use rynvek_network::link::LinkSender;
use rynvek_network::replication::{Replicator, visible_records};
use rynvek_network::visibility::VisibilityGrid;
use rynvek_network::{Frame, FrameEncoder, NetError};
use rynvek_protocol::{CommandEnvelope, WireChannelId, WireUuid};
use rynvek_rts::{ArenaRuntime, ArenaWorld, FactionId, MatchConfig, MatchOutcome, RtsEvent, RuntimeCheckpoint};
use rynvek_security::{BehaviorMonitor, CommandGate, CommandGateConfig, League, PrincipalKindCode, ServerSigningKey};
use rynvek_world::{LiveEditSafetyClass, WorldPatch};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, watch};

use crate::error::ApiError;
use crate::replay::{self, ReplayParticipant};
use crate::store::{MatchState, NewAudit};

/// 标准实时 Tick 频率（30 Hz）
pub const DEFAULT_TICK_RATE_HZ: u32 = 30;
/// 每局 Actor 收件箱容量
const INBOX_CAPACITY: usize = 1024;
/// 排程输入同时受字节预算约束，不能用少量巨型载荷绕过计数上限。
const MAX_QUEUED_BYTES: usize = 4 << 20;
const MAX_PATCH_WAITERS: usize = 64;
/// 连接被取代 / 对局结束时 Disconnect 的错误码（0 = 正常关闭）
const NORMAL_CLOSE: u32 = 0;

/// Tick 驱动方式
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickDriver {
    /// 只由宿主 API（[`MatchHandle::advance`]）推进：测试、离线裁判、单步调试
    Manual,
    /// 按固定频率实时推进
    RealTime { hz: u32 },
}

impl TickDriver {
    pub const fn hz(self) -> u32 {
        match self {
            Self::Manual => 0,
            Self::RealTime { hz } => hz,
        }
    }
}

/// 宿主配置
#[derive(Clone, Debug)]
pub struct HostConfig {
    pub driver: TickDriver,
    pub keyframe_interval_ticks: u64,
    /// 大厅等待超时：超时未全员入座则 `Aborted`（`None` 不超时）
    pub lobby_timeout: Option<Duration>,
    pub gate: CommandGateConfig,
    /// ResumeToken MAC 密钥
    pub resume_secret: [u8; 32],
    /// `.rreplay` 签名密钥
    pub signer: ServerSigningKey,
}

/// 一个席位
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeatSpec {
    pub faction: u8,
    pub principal_id: u64,
    pub kind: PrincipalKindCode,
}

/// 对局规格
#[derive(Clone, Debug)]
pub struct MatchSpec {
    pub match_id: WireUuid,
    pub world_id: WireUuid,
    pub league: League,
    pub ruleset_hash: [u8; 32],
    pub config: MatchConfig,
    pub seats: Vec<SeatSpec>,
}

/// 终局结果（交给持久化任务）
#[derive(Clone, Debug)]
pub struct CompletedMatch {
    pub match_id: WireUuid,
    pub world_id: WireUuid,
    pub league: League,
    pub outcome: Option<MatchOutcome>,
    pub total_ticks: u64,
    pub final_state_root: [u8; 32],
    pub fencing_token: Option<u64>,
    /// `(阵营, Principal, 最终得分 milli)`
    pub scores: Vec<(u8, u64, i64)>,
    pub winner_faction: Option<u8>,
    /// DDL `win_reason` 文本（`Elimination` / `Surrender` / `TieBreakerScore` / …）
    pub win_reason: Option<String>,
    /// 已签名 `.rreplay`
    pub replay: Vec<u8>,
}

/// 宿主向外发布的事件
#[derive(Clone, Debug)]
pub enum HostEvent {
    State {
        match_id: WireUuid,
        fencing_token: Option<u64>,
        state: MatchState,
        tick: u64,
    },
    Audit(NewAudit),
    Completed(Box<CompletedMatch>),
}

/// 当前状态快照
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostStatus {
    pub state: MatchState,
    pub tick: u64,
    pub connections: usize,
    pub pending_inputs: usize,
    pub revision: u64,
    pub state_root: [u8; 32],
}

/// 入座成功
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinAccepted {
    pub conn_id: u64,
    pub faction: u8,
    pub resume_token: [u8; 32],
}

/// Command 通道下行
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandReply {
    /// 触发本回复的命令幂等键（服务端主动推送时为空）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub idempotency_key: Option<String>,
    pub message: ArenaServerMessage,
}

/// Query 通道上行
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueryRequest {
    /// 地图信息（与 `welcome.map` 相同）
    MapInfo { seq: u64 },
    /// 对局状态
    Status { seq: u64 },
    /// 本阵营迷雾裁剪观测（`FactionObservation`）
    Observe { seq: u64 },
    /// 查询单个实体：不可见与不存在返回同一错误（不泄露存在性）
    Entity { seq: u64, id: u32 },
}

/// Query 通道下行
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryReply {
    pub seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<rynvek_agent::error::ErrorBody>,
}

enum HostMsg {
    Patch {
        patch: WorldPatch,
        reply: oneshot::Sender<Result<PatchApplied, ApiError>>,
    },
    Join {
        principal_id: u64,
        faction: u32,
        resume_token: Vec<u8>,
        sender: LinkSender,
        reply: oneshot::Sender<Result<JoinAccepted, ApiError>>,
    },
    Frame {
        conn: u64,
        frame: Frame,
    },
    Violation {
        conn: u64,
        error: NetError,
    },
    Leave {
        conn: u64,
    },
    Advance {
        ticks: u32,
        reply: oneshot::Sender<Result<u64, ApiError>>,
    },
    Abort {
        reason: String,
    },
}

/// 对局句柄（可克隆，供 HTTP 与流连接共享）
#[derive(Clone, Debug)]
pub struct MatchHandle {
    pub match_id: WireUuid,
    pub world_id: WireUuid,
    pub league: League,
    pub mode: rynvek_rts::world::MatchMode,
    /// principal_id → 阵营（入场时客户端据此填写 `ClientHello.faction`）
    pub seats: BTreeMap<u64, u8>,
    tx: mpsc::Sender<HostMsg>,
    status: watch::Receiver<HostStatus>,
}

impl std::fmt::Debug for HostMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Patch { .. } => "Patch",
            Self::Join { .. } => "Join",
            Self::Frame { .. } => "Frame",
            Self::Violation { .. } => "Violation",
            Self::Leave { .. } => "Leave",
            Self::Advance { .. } => "Advance",
            Self::Abort { .. } => "Abort",
        })
    }
}

fn ended() -> ApiError {
    ApiError::forbidden("对局已结束")
}

impl MatchHandle {
    pub async fn apply_patch(&self, patch: WorldPatch) -> Result<PatchApplied, ApiError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostMsg::Patch { patch, reply })
            .await
            .map_err(|_| ended())?;
        rx.await.map_err(|_| ended())?
    }

    pub fn status(&self) -> HostStatus {
        *self.status.borrow()
    }

    /// 等待状态满足条件（测试与 HTTP 长轮询使用）
    pub async fn wait_for(&self, mut pred: impl FnMut(&HostStatus) -> bool) -> HostStatus {
        let mut rx = self.status.clone();
        loop {
            let now = *rx.borrow_and_update();
            if pred(&now) {
                return now;
            }
            if rx.changed().await.is_err() {
                return *rx.borrow();
            }
        }
    }

    pub async fn join(
        &self,
        principal_id: u64,
        faction: u32,
        resume_token: Vec<u8>,
        sender: LinkSender,
    ) -> Result<JoinAccepted, ApiError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostMsg::Join {
                principal_id,
                faction,
                resume_token,
                sender,
                reply,
            })
            .await
            .map_err(|_| ended())?;
        rx.await.map_err(|_| ended())?
    }

    pub async fn frame(&self, conn: u64, frame: Frame) -> bool {
        self.tx.send(HostMsg::Frame { conn, frame }).await.is_ok()
    }

    pub async fn violation(&self, conn: u64, error: NetError) {
        let _ = self.tx.send(HostMsg::Violation { conn, error }).await;
    }

    pub async fn leave(&self, conn: u64) {
        let _ = self.tx.send(HostMsg::Leave { conn }).await;
    }

    /// 推进 `ticks` 个 Tick（仅 [`TickDriver::Manual`]）；返回推进后的 Tick
    pub async fn advance(&self, ticks: u32) -> Result<u64, ApiError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostMsg::Advance { ticks, reply })
            .await
            .map_err(|_| ended())?;
        rx.await.map_err(|_| ended())?
    }

    pub async fn abort(&self, reason: impl Into<String>) {
        let _ = self.tx.send(HostMsg::Abort { reason: reason.into() }).await;
    }
}

/// ResumeToken = BLAKE3-keyed(secret, `rynvek-resume/1` ‖ match_id ‖ principal_id ‖ faction)
pub fn resume_token(secret: &[u8; 32], match_id: &WireUuid, principal_id: u64, faction: u8) -> [u8; 32] {
    let mut h = blake3::Hasher::new_keyed(secret);
    h.update(b"rynvek-resume/1\0");
    h.update(&match_id.0);
    h.update(&principal_id.to_le_bytes());
    h.update(&[faction]);
    *h.finalize().as_bytes()
}

/// 启动一局
pub fn spawn_match(spec: MatchSpec, cfg: HostConfig, events: mpsc::UnboundedSender<HostEvent>) -> MatchHandle {
    let mut runtime = ArenaRuntime::new(spec.world_id.0, ArenaWorld::new(spec.config.clone()));
    runtime.enable_command_log();
    spawn_runtime(spec, cfg, events, runtime, None).expect("新建规范世界可编码")
}

/// 恢复、HTTP 和流连接共用同一个 Actor；不从配置重建传入的权威状态。
pub fn spawn_runtime(
    spec: MatchSpec,
    cfg: HostConfig,
    events: mpsc::UnboundedSender<HostEvent>,
    mut runtime: ArenaRuntime,
    committer: Option<TickCommitter>,
) -> Result<MatchHandle, ApiError> {
    let genesis = runtime.checkpoint().map_err(|e| ApiError::new(e, "宿主初态不可编码"))?;
    if runtime.world_id() != spec.world_id.0 || runtime.config != spec.config {
        return Err(conflict("宿主规范与检查点身份不一致"));
    }
    runtime.enable_command_log();
    let (tx, rx) = mpsc::channel(INBOX_CAPACITY);
    let (status_tx, status) = watch::channel(HostStatus {
        state: MatchState::Created,
        tick: runtime.tick,
        connections: 0,
        pending_inputs: 0,
        revision: runtime.revision(),
        state_root: runtime.state_root(),
    });
    let handle = MatchHandle {
        mode: spec.config.mode,
        match_id: spec.match_id,
        world_id: spec.world_id,
        league: spec.league,
        seats: spec.seats.iter().map(|s| (s.principal_id, s.faction)).collect(),
        tx,
        status,
    };
    let session = ArenaSession::from_runtime(runtime, SystemClock::default());
    let host = Host {
        gate: CommandGate::new(cfg.gate),
        monitor: BehaviorMonitor::new(),
        session,
        conns: BTreeMap::new(),
        next_conn: 1,
        scheduled: BTreeMap::new(),
        patches: BTreeMap::new(),
        committer,
        genesis,
        records: Vec::new(),
        seen_commands: BTreeSet::new(),
        state: MatchState::Created,
        joined: BTreeSet::new(),
        status_tx,
        events,
        spec,
        cfg,
    };
    tokio::spawn(host.run(rx));
    Ok(handle)
}

struct Conn {
    principal_id: u64,
    faction: u8,
    sender: LinkSender,
    enc: FrameEncoder,
    replicator: Replicator,
}

struct Pending {
    conn: u64,
    principal_id: u64,
    key: String,
    message: ArenaClientMessage,
    bytes: usize,
}

enum PendingInput {
    Command(Box<Pending>),
    Patch([u8; 16]),
}
struct PendingPatch {
    patch: WorldPatch,
    replies: Vec<oneshot::Sender<Result<PatchApplied, ApiError>>>,
    bytes: usize,
}

struct Host {
    spec: MatchSpec,
    cfg: HostConfig,
    session: ArenaSession<SystemClock>,
    gate: CommandGate,
    monitor: BehaviorMonitor,
    conns: BTreeMap<u64, Conn>,
    next_conn: u64,
    /// 目标 Tick → 待执行命令（到达顺序）
    scheduled: BTreeMap<u64, Vec<PendingInput>>,
    patches: BTreeMap<[u8; 16], PendingPatch>,
    committer: Option<TickCommitter>,
    genesis: RuntimeCheckpoint,
    records: Vec<TickRecord>,
    seen_commands: BTreeSet<(u64, String)>,
    state: MatchState,
    joined: BTreeSet<u64>,
    status_tx: watch::Sender<HostStatus>,
    events: mpsc::UnboundedSender<HostEvent>,
}

fn agent_kind(kind: PrincipalKindCode, league: League) -> PrincipalKind {
    match (kind, league) {
        (PrincipalKindCode::Human, _) => PrincipalKind::Human,
        (_, League::AgentHybrid) => PrincipalKind::Agent,
        _ => PrincipalKind::Script,
    }
}

fn error_body(code: RynvekErrorCode, message: impl Into<String>) -> rynvek_agent::error::ErrorBody {
    AgentError::new(code, message.into()).body()
}

fn win_reason_text(outcome: &Option<MatchOutcome>) -> (Option<u8>, Option<String>) {
    match outcome {
        Some(MatchOutcome::Victory { winner, reason, .. }) => {
            let text = serde_json::to_value(reason)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            // snake_case → DDL 的 PascalCase（Elimination / Surrender / TieBreakerScore / …）
            let pascal: String = text
                .split('_')
                .map(|w| {
                    let mut c = w.chars();
                    c.next()
                        .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                        .unwrap_or_default()
                })
                .collect();
            (Some(winner.0), Some(pascal))
        }
        _ => (None, None),
    }
}

impl Host {
    async fn run(mut self, mut rx: mpsc::Receiver<HostMsg>) {
        if let Some(committer) = &self.committer {
            match committer.records().await {
                Ok(records) => {
                    for record in records {
                        for input in record.inputs {
                            if let TickInput::Command {
                                principal_id,
                                idempotency_key,
                                ..
                            } = input
                            {
                                self.seen_commands.insert((principal_id, idempotency_key));
                            }
                        }
                    }
                }
                Err(error) => {
                    self.abort(&format!("恢复幂等日志失败：{error}"));
                    return;
                }
            }
        }
        self.transition(MatchState::Lobby);
        let hz = self.cfg.driver.hz();
        let mut ticker = tokio::time::interval(Duration::from_secs_f64(1.0 / f64::from(hz.max(1))));
        // 落后时不突发补帧：服务器过载时整局放慢而不是在单个事件循环里连跑多帧
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let lobby_deadline = self.cfg.lobby_timeout.map(|d| tokio::time::Instant::now() + d);
        loop {
            let realtime_running = hz > 0 && self.state == MatchState::Running;
            let in_lobby = matches!(self.state, MatchState::Lobby | MatchState::Ready);
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(m) => self.on_msg(m).await,
                    None => break,
                },
                _ = ticker.tick(), if realtime_running => self.tick_once().await,
                _ = async { tokio::time::sleep_until(lobby_deadline.expect("有截止时间")).await },
                    if in_lobby && lobby_deadline.is_some() =>
                {
                    self.abort("大厅超时：参赛者未全部入座");
                }
            }
            if self.state.is_terminal() {
                break;
            }
        }
        // 终局后继续应答收件箱中残留的请求（全部以 "对局已结束" 拒绝），然后退出
        for (_, pending) in std::mem::take(&mut self.patches) {
            for reply in pending.replies {
                let _ = reply.send(Err(ended()));
            }
        }
        rx.close();
        while let Some(m) = rx.recv().await {
            match m {
                HostMsg::Patch { reply, .. } => {
                    let _ = reply.send(Err(ended()));
                }
                HostMsg::Join { reply, .. } => {
                    let _ = reply.send(Err(ended()));
                }
                HostMsg::Advance { reply, .. } => {
                    let _ = reply.send(Err(ended()));
                }
                _ => {}
            }
        }
    }

    fn publish(&self) {
        let _ = self.status_tx.send(HostStatus {
            state: self.state,
            tick: self.session.world.tick,
            connections: self.conns.len(),
            pending_inputs: self.scheduled.values().map(Vec::len).sum(),
            revision: self.session.world.revision(),
            state_root: self.session.world.state_root(),
        });
    }

    fn transition(&mut self, to: MatchState) {
        if !self.state.can_transition(to) {
            return;
        }
        self.state = to;
        let _ = self.events.send(HostEvent::State {
            match_id: self.spec.match_id,
            fencing_token: self.committer.as_ref().map(|c| c.fencing_token),
            state: to,
            tick: self.session.world.tick,
        });
        self.publish();
    }

    fn audit(&self, principal_id: Option<u64>, action: &str, details: serde_json::Value) {
        let _ = self.events.send(HostEvent::Audit(NewAudit {
            principal_id,
            action_type: action.to_owned(),
            target_resource: format!("match:{}", self.spec.match_id),
            client_ip: None,
            details_json: details,
        }));
    }

    async fn on_msg(&mut self, msg: HostMsg) {
        match msg {
            HostMsg::Patch { patch, reply } => self.enqueue_patch(patch, reply).await,
            HostMsg::Join {
                principal_id,
                faction,
                resume_token,
                sender,
                reply,
            } => {
                let r = self.join(principal_id, faction, &resume_token, sender);
                let _ = reply.send(r);
            }
            HostMsg::Frame { conn, frame } => self.on_frame(conn, frame),
            HostMsg::Violation { conn, error } => self.violation(conn, error),
            HostMsg::Leave { conn } => self.drop_conn(conn),
            HostMsg::Advance { ticks, reply } => {
                let r = if self.cfg.driver != TickDriver::Manual {
                    Err(ApiError::forbidden("实时驱动的对局不能手动推进"))
                } else if self.state != MatchState::Running {
                    Err(ApiError::forbidden(format!("对局处于 {:?}，不能推进", self.state)))
                } else {
                    for _ in 0..ticks {
                        self.tick_once().await;
                        if self.state.is_terminal() {
                            break;
                        }
                    }
                    if self.state == MatchState::Aborted {
                        Err(conflict("Tick未提交，宿主已终止；不报告推进成功"))
                    } else {
                        Ok(self.session.world.tick)
                    }
                };
                let _ = reply.send(r);
            }
            HostMsg::Abort { reason } => self.abort(&reason),
        }
    }

    fn seat(&self, principal_id: u64) -> Option<SeatSpec> {
        self.spec.seats.iter().copied().find(|s| s.principal_id == principal_id)
    }

    fn join(
        &mut self,
        principal_id: u64,
        faction: u32,
        resume: &[u8],
        sender: LinkSender,
    ) -> Result<JoinAccepted, ApiError> {
        if self.state.is_terminal() {
            return Err(ended());
        }
        let seat = self
            .seat(principal_id)
            .ok_or_else(|| ApiError::forbidden("该 Principal 不是本局参赛者"))?;
        if faction != u32::from(seat.faction) {
            return Err(ApiError::forbidden(format!(
                "该 Principal 的席位是阵营 {}",
                seat.faction
            )));
        }
        let token = resume_token(&self.cfg.resume_secret, &self.spec.match_id, principal_id, seat.faction);
        if !resume.is_empty() && resume != token {
            return Err(ApiError::forbidden("ResumeToken 无效"));
        }
        // 同一 Principal 只保留一条连接：新连接取代旧连接
        let old: Vec<u64> = self
            .conns
            .iter()
            .filter(|(_, c)| c.principal_id == principal_id)
            .map(|(id, _)| *id)
            .collect();
        for id in old {
            self.disconnect(id, NORMAL_CLOSE, "连接已被同一身份的新连接取代");
        }
        let pid = principal_id.to_string();
        let welcome = self.session.handle(
            &pid,
            ArenaClientMessage::Hello {
                protocol: ARENA_PROTOCOL.to_owned(),
                principal_id: pid.clone(),
                kind: agent_kind(seat.kind, self.spec.league),
                faction: u32::from(seat.faction),
            },
        );
        self.gate.bind(principal_id, u32::from(seat.faction));
        self.monitor.register(principal_id, self.spec.league == League::Human);
        let conn_id = self.next_conn;
        self.next_conn += 1;
        let mut conn = Conn {
            principal_id,
            faction: seat.faction,
            sender,
            enc: FrameEncoder::new(true),
            replicator: Replicator::new(FactionId(seat.faction), self.cfg.keyframe_interval_ticks),
        };
        let mut session_id = [0u8; 16];
        session_id.copy_from_slice(
            &blake3::hash(&[self.spec.match_id.0.as_slice(), &conn_id.to_le_bytes()].concat()).as_bytes()[..16],
        );
        let hello = ControlMessage::new(ControlBody::Welcome(ServerHello {
            session_id: session_id.to_vec(),
            principal_id,
            faction: u32::from(seat.faction),
            current_tick: self.session.world.tick,
            resume_token: token.to_vec(),
            tick_rate_hz: self.cfg.driver.hz(),
            keyframe_interval_ticks: self.cfg.keyframe_interval_ticks as u32,
            source_commit: crate::source::SOURCE_COMMIT.to_owned(),
        }));
        let tick = self.session.world.tick;
        let mut ok = send(&mut conn, WireChannelId::Control, tick, &hello.to_bytes(), false);
        for env in welcome {
            let body = serde_json::to_vec(&CommandReply {
                idempotency_key: None,
                message: env.message,
            })
            .expect("可序列化");
            ok &= send(&mut conn, WireChannelId::Command, tick, &body, false);
        }
        let packet = conn.replicator.next_packet(&self.session.world);
        ok &= send(
            &mut conn,
            WireChannelId::StateStream,
            tick,
            &packet.encode(),
            packet.keyframe,
        );
        if !ok {
            return Err(ApiError::new(
                RynvekErrorCode::UnauthorizedCapabilityToken,
                "连接在握手期间关闭",
            ));
        }
        self.conns.insert(conn_id, conn);
        self.joined.insert(principal_id);
        self.audit(
            Some(principal_id),
            "match.join",
            serde_json::json!({ "faction": seat.faction }),
        );
        if self.state == MatchState::Lobby && self.spec.seats.iter().all(|s| self.joined.contains(&s.principal_id)) {
            self.transition(MatchState::Ready);
            self.transition(MatchState::Running);
        }
        self.publish();
        Ok(JoinAccepted {
            conn_id,
            faction: seat.faction,
            resume_token: token,
        })
    }

    fn drop_conn(&mut self, conn: u64) {
        if let Some(c) = self.conns.remove(&conn) {
            if !self.conns.values().any(|o| o.principal_id == c.principal_id) {
                // 离座但保留闸门绑定与租约：断线重连（ResumeToken）后可继续
                self.session.leave(&c.principal_id.to_string());
            }
            self.publish();
        }
    }

    fn disconnect(&mut self, conn: u64, code: u32, reason: &str) {
        if let Some(c) = self.conns.get_mut(&conn) {
            let msg = ControlMessage::new(ControlBody::Disconnect(Disconnect {
                error_code: code,
                reason: reason.to_owned(),
            }));
            let tick = self.session.world.tick;
            let _ = send(c, WireChannelId::Control, tick, &msg.to_bytes(), false);
        }
        self.drop_conn(conn);
    }

    fn violation(&mut self, conn: u64, error: NetError) {
        let pid = self.conns.get(&conn).map(|c| c.principal_id);
        self.audit(
            pid,
            "stream.protocol_violation",
            serde_json::json!({ "error": error.to_string() }),
        );
        self.disconnect(conn, error.code().code().into(), &error.to_string());
    }

    fn on_frame(&mut self, conn: u64, frame: Frame) {
        let Some(c) = self.conns.get(&conn) else { return };
        let principal_id = c.principal_id;
        let payload = match frame.decoded_payload() {
            Ok(p) => p,
            Err(e) => return self.violation(conn, e),
        };
        match frame.channel() {
            WireChannelId::Control => self.on_control(conn, &payload),
            WireChannelId::Command => self.on_command(conn, principal_id, &payload),
            WireChannelId::Query => self.on_query(conn, principal_id, &payload),
            WireChannelId::Telemetry => {}
            WireChannelId::Event | WireChannelId::StateStream => self.violation(
                conn,
                NetError::Payload("Event / StateStream 通道只允许服务端下行".into()),
            ),
        }
    }

    fn on_control(&mut self, conn: u64, payload: &[u8]) {
        let tick = self.session.world.tick;
        match ControlMessage::from_bytes(payload) {
            Ok(ControlBody::Heartbeat(hb)) => {
                let ack = ControlMessage::new(ControlBody::HeartbeatAck(HeartbeatAck {
                    nonce: hb.nonce,
                    server_tick: tick,
                }));
                if let Some(c) = self.conns.get_mut(&conn) {
                    if !send(c, WireChannelId::Control, tick, &ack.to_bytes(), false) {
                        self.drop_conn(conn);
                    }
                }
            }
            Ok(ControlBody::KeyframeRequest(_)) => {
                if let Some(c) = self.conns.get_mut(&conn) {
                    c.replicator.request_keyframe();
                }
            }
            Ok(ControlBody::Disconnect(_)) => self.drop_conn(conn),
            Ok(_) => self.violation(
                conn,
                NetError::Payload("握手后不允许再次发送 Hello / 服务端消息".into()),
            ),
            Err(error) => self.violation(conn, error),
        }
    }

    fn reply(&mut self, conn: u64, key: Option<String>, message: ArenaServerMessage) {
        let tick = self.session.world.tick;
        let body = serde_json::to_vec(&CommandReply {
            idempotency_key: key,
            message,
        })
        .expect("可序列化");
        if let Some(c) = self.conns.get_mut(&conn) {
            if !send(c, WireChannelId::Command, tick, &body, false) {
                self.drop_conn(conn);
            }
        }
    }

    fn reject(&mut self, conn: u64, principal_id: u64, key: Option<String>, code: RynvekErrorCode, message: String) {
        self.monitor.on_rejected(principal_id, self.session.world.tick);
        self.reply(
            conn,
            key,
            ArenaServerMessage::Error {
                error: error_body(code, message),
            },
        );
    }

    fn on_command(&mut self, conn: u64, principal_id: u64, payload: &[u8]) {
        let tick = self.session.world.tick;
        if let Err(e) = self.gate.admit_frame(principal_id, tick) {
            return self.reject(conn, principal_id, None, e.code, e.message);
        }
        let env: CommandEnvelope<ArenaClientMessage> = match serde_json::from_slice(payload) {
            Ok(e) => e,
            Err(e) => {
                return self.reject(
                    conn,
                    principal_id,
                    None,
                    RynvekErrorCode::RuleStaticTypeCheckFailed,
                    format!("命令信封不符合 CommandEnvelope<ArenaClientMessage>：{e}"),
                );
            }
        };
        let key = env.idempotency_key.to_string();
        if self.seen_commands.contains(&(principal_id, key.clone())) {
            return self.reject(
                conn,
                principal_id,
                Some(key),
                RynvekErrorCode::StructuralMutationConflict,
                "命令幂等键已提交".into(),
            );
        }
        if let Err(e) = self.gate.check_command(principal_id, &env, tick) {
            return self.reject(conn, principal_id, Some(key), e.code, e.message);
        }
        match &env.payload {
            ArenaClientMessage::Hello { .. } => {
                return self.reject(
                    conn,
                    principal_id,
                    Some(key),
                    RynvekErrorCode::UnauthorizedCapabilityToken,
                    "已在握手时入座，不能再次 hello".into(),
                );
            }
            ArenaClientMessage::Advance { .. } => {
                return self.reject(
                    conn,
                    principal_id,
                    Some(key),
                    RynvekErrorCode::UnauthorizedCapabilityToken,
                    "排位对局只由服务器计时，客户端不能推进 Tick".into(),
                );
            }
            _ => {}
        }
        if self.state != MatchState::Running {
            return self.reject(
                conn,
                principal_id,
                Some(key),
                RynvekErrorCode::UnauthorizedCapabilityToken,
                format!("对局处于 {:?}，暂不接受命令", self.state),
            );
        }
        self.monitor.on_command(principal_id, tick);
        let pending = Pending {
            conn,
            principal_id,
            key,
            message: env.payload,
            bytes: payload.len(),
        };
        if matches!(
            pending.message,
            ArenaClientMessage::AcquireLease { .. } | ArenaClientMessage::ReleaseLease { .. }
        ) && env.target_tick <= tick
        {
            let (p, envelopes) = self.execute(pending);
            self.deliver(p, envelopes, tick);
        } else if self.scheduled.values().map(Vec::len).sum::<usize>() >= INBOX_CAPACITY
            || self.queued_bytes().saturating_add(pending.bytes) > MAX_QUEUED_BYTES
        {
            self.reject(
                conn,
                principal_id,
                Some(pending.key),
                RynvekErrorCode::StructuralMutationConflict,
                "Tick 输入队列已满".into(),
            );
        } else {
            self.scheduled
                .entry(env.target_tick.max(tick.saturating_add(1)))
                .or_default()
                .push(PendingInput::Command(Box::new(pending)));
            self.publish();
        }
    }

    fn execute(&mut self, p: Pending) -> (Pending, Vec<rynvek_engine::arena::Envelope>) {
        let envelopes = self.session.handle(&p.principal_id.to_string(), p.message.clone());
        (p, envelopes)
    }

    fn deliver(&mut self, p: Pending, envelopes: Vec<rynvek_engine::arena::Envelope>, tick: u64) {
        for mut env in envelopes {
            if let ArenaServerMessage::Ack { tick: applied, .. } = &mut env.message {
                *applied = tick;
            }
            if let ArenaServerMessage::Ack { error: Some(_), .. } = &env.message {
                self.monitor.on_rejected(p.principal_id, self.session.world.tick);
            }
            let Ok(to) = env.to.parse::<u64>() else { continue };
            let targets: Vec<u64> = self
                .conns
                .iter()
                .filter(|(_, c)| c.principal_id == to)
                .map(|(id, _)| *id)
                .collect();
            for id in targets {
                let key = (id == p.conn).then(|| p.key.clone());
                self.reply(id, key, env.message.clone());
            }
        }
    }

    fn on_query(&mut self, conn: u64, principal_id: u64, payload: &[u8]) {
        let tick = self.session.world.tick;
        let faction = self.conns.get(&conn).map(|c| c.faction).unwrap_or_default();
        let reply = match self.gate.admit_frame(principal_id, tick) {
            Err(e) => QueryReply {
                seq: None,
                result: None,
                error: Some(error_body(e.code, e.message)),
            },
            Ok(()) => match serde_json::from_slice::<QueryRequest>(payload) {
                Err(e) => QueryReply {
                    seq: None,
                    result: None,
                    error: Some(error_body(
                        RynvekErrorCode::RuleStaticTypeCheckFailed,
                        format!("查询不符合 QueryRequest：{e}"),
                    )),
                },
                Ok(q) => self.answer(principal_id, faction, q),
            },
        };
        let body = serde_json::to_vec(&reply).expect("可序列化");
        if let Some(c) = self.conns.get_mut(&conn) {
            if !send(c, WireChannelId::Query, tick, &body, false) {
                self.drop_conn(conn);
            }
        }
    }

    fn answer(&mut self, principal_id: u64, faction: u8, q: QueryRequest) -> QueryReply {
        let world = &self.session.world;
        let ok = |seq, v: serde_json::Value| QueryReply {
            seq: Some(seq),
            result: Some(v),
            error: None,
        };
        match q {
            QueryRequest::MapInfo { seq } => ok(seq, serde_json::to_value(map_info(world)).expect("可序列化")),
            QueryRequest::Status { seq } => ok(
                seq,
                serde_json::json!({
                    "match_id": self.spec.match_id.to_string(),
                    "state": self.state,
                    "tick": world.tick,
                    "tick_rate_hz": self.cfg.driver.hz(),
                    "league": self.spec.league,
                }),
            ),
            QueryRequest::Observe { seq } => ok(
                seq,
                serde_json::to_value(world.observe(FactionId(faction))).expect("可序列化"),
            ),
            QueryRequest::Entity { seq, id } => {
                let grid = world
                    .faction(FactionId(faction))
                    .map(|f| VisibilityGrid::from_fog(&f.fog));
                let record = grid.and_then(|g| visible_records(world, FactionId(faction), &g).remove(&id));
                match record {
                    Some(r) => ok(
                        seq,
                        serde_json::json!({
                            "id": r.id, "kind": r.kind, "faction": r.faction, "flags": r.flags,
                            "x": r.x, "y": r.y, "hp_milli": r.hp_milli, "max_hp_milli": r.max_hp_milli,
                            "shield_milli": r.shield_milli, "extra": r.extra,
                        }),
                    ),
                    None => {
                        // 存在但不可见 ⇒ 记一次迷雾试探；回复与 "不存在" 完全相同
                        let exists = world.units.values().any(|u| u.id.0 == id)
                            || world.buildings.values().any(|b| b.id.0 == id);
                        if exists {
                            self.monitor.on_fog_probe(principal_id, world.tick);
                        }
                        QueryReply {
                            seq: Some(seq),
                            result: None,
                            error: Some(error_body(
                                RynvekErrorCode::EntityNotFoundOrTombstoned,
                                "实体不存在或不在视野内",
                            )),
                        }
                    }
                }
            }
        }
    }

    fn queued_bytes(&self) -> usize {
        self.scheduled
            .values()
            .flatten()
            .map(|input| match input {
                PendingInput::Command(p) => p.bytes,
                PendingInput::Patch(id) => self.patches.get(id).map_or(0, |p| p.bytes),
            })
            .fold(0usize, usize::saturating_add)
    }

    async fn enqueue_patch(&mut self, patch: WorldPatch, reply: oneshot::Sender<Result<PatchApplied, ApiError>>) {
        let result = async {
            if self.state != MatchState::Running {
                return Err(conflict("宿主不是 Running"));
            }
            let committer = self
                .committer
                .as_ref()
                .ok_or_else(|| conflict("宿主未配置持久化，不能确认编辑"))?;
            let world = committer
                .store
                .get_world(self.spec.world_id)
                .await?
                .ok_or_else(|| conflict("世界不存在"))?;
            if world.game_mode == "PvP_Ranked" {
                return Err(ApiError::forbidden("排位对局禁止运行中创作编辑"));
            }
            crate::patch::validate(&patch)?;
            if patch.safety_class == LiveEditSafetyClass::RequiresForkRestart {
                return Err(conflict("分叉重启必须走显式分叉"));
            }
            if let Some(receipt) = committer.patch_receipt(&patch).await? {
                return Ok(Some(receipt));
            }
            if let Some(queued) = self.patches.get(&patch.patch_id) {
                if queued.patch != patch {
                    return Err(conflict("同一 patch_id 不能替换载荷"));
                }
                if queued.replies.len() >= MAX_PATCH_WAITERS {
                    return Err(conflict("同一补丁的等待者已满"));
                }
                return Ok(None);
            }
            if patch.base_authoring_revision > self.session.world.revision()
                || patch.base_authoring_revision < self.session.world.authoring.last_edit_revision
            {
                return Err(conflict("编辑基于过期修订"));
            }
            let bytes = serde_json::to_vec(&patch).expect("可序列化").len();
            if self.scheduled.values().map(Vec::len).sum::<usize>() >= INBOX_CAPACITY
                || self.queued_bytes().saturating_add(bytes) > MAX_QUEUED_BYTES
            {
                return Err(conflict("Tick 输入队列已满"));
            }
            let (_, target) = crate::patch::schedule(patch.safety_class, self.session.world.tick);
            let tick = target
                .unwrap_or(self.session.world.tick.saturating_add(1))
                .max(self.session.world.tick.saturating_add(1));
            self.scheduled
                .entry(tick)
                .or_default()
                .push(PendingInput::Patch(patch.patch_id));
            self.patches.insert(
                patch.patch_id,
                PendingPatch {
                    patch: patch.clone(),
                    replies: Vec::new(),
                    bytes,
                },
            );
            Ok(None)
        }
        .await;
        match result {
            Ok(Some(receipt)) => {
                let _ = reply.send(Ok(receipt));
            }
            Ok(None) => {
                self.patches
                    .get_mut(&patch.patch_id)
                    .expect("已排队")
                    .replies
                    .push(reply);
                self.publish();
            }
            Err(error) => {
                let _ = reply.send(Err(error));
            }
        }
    }

    async fn tick_once(&mut self) {
        if self.state != MatchState::Running {
            return;
        }
        let candidate = match self.session.world.candidate_tick() {
            Ok(candidate) => candidate,
            Err(_) => {
                self.abort("无法准备权威 Tick");
                return;
            }
        };
        let previous = std::mem::replace(&mut self.session.world, candidate);
        let target = previous.tick + 1;
        let later = self.scheduled.split_off(&target.saturating_add(1));
        let due = std::mem::replace(&mut self.scheduled, later);
        let mut inputs = Vec::new();
        let mut commands = Vec::new();
        let mut patches = Vec::new();
        let mut patched = false;
        for input in due.into_values().flatten() {
            match input {
                PendingInput::Command(p) => {
                    let before = self.session.world.command_log().len();
                    let principal_id = p.principal_id;
                    let idempotency_key = p.key.clone();
                    commands.push(self.execute(*p));
                    inputs.extend(
                        self.session.world.command_log()[before..]
                            .iter()
                            .map(|c| TickInput::Command {
                                faction: c.faction.0,
                                command: c.command.clone(),
                                principal_id,
                                idempotency_key: idempotency_key.clone(),
                            }),
                    );
                }
                PendingInput::Patch(id) => {
                    let pending = self.patches.remove(&id).expect("排程引用存在");
                    let result = if patched
                        || pending.patch.base_authoring_revision > previous.revision()
                        || pending.patch.base_authoring_revision < previous.authoring.last_edit_revision
                        || self.session.world.outcome.is_some()
                    {
                        Err(conflict("同一基线已改变，补丁冲突"))
                    } else {
                        let tick = self.session.world.tick;
                        self.session
                            .world
                            .transact_authoring_at(tick, |world| pending.patch.apply_at_boundary(world, target))
                            .map_err(ApiError::from)
                    };
                    match result {
                        Ok(report) => {
                            patched = true;
                            inputs.push(TickInput::Patch {
                                patch: pending.patch.clone(),
                                report: report.clone(),
                            });
                            patches.push((pending, report));
                        }
                        Err(error) => {
                            for reply in pending.replies {
                                let _ = reply.send(Err(error.clone()));
                            }
                        }
                    }
                }
            }
        }
        let result = async {
            self.session.world.finish_tick().map_err(ApiError::from)?;
            let record = TickRecord {
                format: "rynvek-tick/2".into(),
                base_revision: previous.revision(),
                base_root: previous.state_root(),
                target_tick: target,
                tick: self.session.world.tick,
                revision: self.session.world.revision(),
                state_root: self.session.world.state_root(),
                inputs,
            };
            if let Some(committer) = &self.committer {
                committer.persist(&self.session.world, &record).await?;
            }
            Ok::<_, ApiError>(record)
        }
        .await;
        let record = match result {
            Ok(record) => record,
            Err(error) => {
                self.session.world = previous;
                for (p, _) in commands {
                    self.reject(
                        p.conn,
                        p.principal_id,
                        Some(p.key),
                        RynvekErrorCode::StructuralMutationConflict,
                        "权威 Tick 未提交".into(),
                    );
                }
                for (pending, _) in patches {
                    for reply in pending.replies {
                        let _ = reply.send(Err(conflict("权威 Tick 未提交")));
                    }
                }
                self.abort(&format!("持久化失败，停止宿主而不发布候选：{error}"));
                return;
            }
        };
        let now = self.session.world.tick;
        for (p, envelopes) in commands {
            self.deliver(p, envelopes, now);
        }
        for (pending, _) in patches {
            let receipt = record
                .patch_receipt(&pending.patch)
                .expect("刚提交记录可解析")
                .expect("包含补丁");
            for reply in pending.replies {
                let _ = reply.send(Ok(receipt.clone()));
            }
        }
        for input in &record.inputs {
            if let TickInput::Command {
                principal_id,
                idempotency_key,
                ..
            } = input
            {
                self.seen_commands.insert((*principal_id, idempotency_key.clone()));
            }
        }
        self.records.push(record);
        let events = self.session.drain_visible_events();
        // 3) 每连接：迷雾裁剪状态包 + 本阵营事件
        let mut dead = Vec::new();
        for (id, c) in &mut self.conns {
            let packet = c.replicator.next_packet(&self.session.world);
            let mut ok = send(c, WireChannelId::StateStream, now, &packet.encode(), packet.keyframe);
            if let Some(ev) = events.get(&u32::from(c.faction)).filter(|e| !e.is_empty()) {
                ok &= send(
                    c,
                    WireChannelId::Event,
                    now,
                    &serde_json::to_vec(ev).expect("可序列化"),
                    false,
                );
            }
            if !ok {
                dead.push(*id);
            }
        }
        for id in dead {
            self.drop_conn(id);
        }
        // 4) 行为审计
        for s in self.monitor.drain() {
            self.audit(
                Some(s.principal_id),
                "anticheat.suspicion",
                serde_json::json!({ "kind": format!("{:?}", s.kind), "tick": s.tick, "detail": s.detail }),
            );
        }
        // 5) 终局
        let ended = events
            .values()
            .flatten()
            .any(|e| matches!(e, RtsEvent::MatchEnded { .. }))
            || self.session.world.outcome.is_some();
        if ended {
            self.complete();
        } else {
            self.publish();
        }
    }

    fn complete(&mut self) {
        let world = &self.session.world;
        let participants: Vec<ReplayParticipant> = self
            .spec
            .seats
            .iter()
            .map(|s| ReplayParticipant {
                faction: s.faction,
                principal_id: s.principal_id,
                league: self.spec.league,
            })
            .collect();
        let recorded = replay::record_runtime(
            world,
            replay::RuntimeReplay {
                world_id: self.spec.world_id.0,
                initial_revision: self.genesis.revision,
                initial_root: self.genesis.state_root,
                initial_snapshot: self.genesis.bytes.clone(),
                ticks: self.records.clone(),
            },
            self.spec.match_id.to_string(),
            self.spec.world_id.to_string(),
            crate::ids::hex(&self.spec.ruleset_hash),
            participants,
        )
        .and_then(|body| replay::encode(&body, self.spec.world_id.0, &self.cfg.signer));
        let replay = match recorded {
            Ok(r) => r,
            Err(e) => {
                // 录制自检失败意味着权威世界出现了不可重放的变更：判定无效局，绝不发布结果
                self.audit(
                    None,
                    "replay.self_check_failed",
                    serde_json::json!({ "error": e.to_string() }),
                );
                return self.abort(&format!("回放自检失败：{e}"));
            }
        };
        let (winner_faction, win_reason) = win_reason_text(&world.outcome);
        let completed = CompletedMatch {
            match_id: self.spec.match_id,
            world_id: self.spec.world_id,
            league: self.spec.league,
            outcome: world.outcome.clone(),
            total_ticks: world.tick,
            final_state_root: world.state_root(),
            fencing_token: self.committer.as_ref().map(|c| c.fencing_token),
            scores: self
                .spec
                .seats
                .iter()
                .map(|s| (s.faction, s.principal_id, world.score_milli(FactionId(s.faction))))
                .collect(),
            winner_faction,
            win_reason,
            replay,
        };
        let _ = self.events.send(HostEvent::Completed(Box::new(completed)));
        // 终局连接关闭前发布真实裁剪观察，浏览器和 SDK 不必在已结束 Actor 上追问。
        for c in self.conns.values_mut() {
            let message = ArenaServerMessage::Observation {
                observation: Box::new(self.session.world.observe(FactionId(c.faction)).expect("有效席位")),
                events: Vec::new(),
            };
            let body = serde_json::to_vec(&CommandReply {
                idempotency_key: None,
                message,
            })
            .expect("JSON");
            let _ = send(c, WireChannelId::Command, self.session.world.tick, &body, false);
        }
        self.transition(MatchState::Completed);
        let ids: Vec<u64> = self.conns.keys().copied().collect();
        for id in ids {
            self.disconnect(id, NORMAL_CLOSE, "对局结束");
        }
        self.publish();
    }

    fn abort(&mut self, reason: &str) {
        if self.state.is_terminal() {
            return;
        }
        self.audit(None, "match.aborted", serde_json::json!({ "reason": reason }));
        self.transition(MatchState::Aborted);
        let ids: Vec<u64> = self.conns.keys().copied().collect();
        for id in ids {
            self.disconnect(id, RynvekErrorCode::SimulationInvariantViolated.code().into(), reason);
        }
        self.publish();
    }
}

/// 编码并非阻塞投递一帧；失败（慢消费者 / 已关闭）返回 `false`
fn send(c: &mut Conn, channel: WireChannelId, tick: u64, payload: &[u8], keyframe: bool) -> bool {
    match c.enc.encode(channel, tick, payload, keyframe) {
        Ok(frame) => c.sender.try_send(frame).is_ok(),
        Err(_) => false,
    }
}
