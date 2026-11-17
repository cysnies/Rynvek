// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 创作世界 → 原 Arena Actor，以及私有签名归档的有界逐记录重演。
//! 依赖 DAG：Studio HTTP → 原世界恢复/MatchHost → TickRecord → 原阵营观测；无第二仿真器。

use crate::{
    app::{AppState, league_resource},
    error::{ApiError, ApiResult},
    host::{MatchSpec, SeatSpec},
    ids::{hex, new_uuid_v7},
    replay::{self, OpenedReplay},
    store::{MatchRow, MatchState, ParticipantRow},
    transactions::conflict,
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use rynvek_protocol::{CapabilityScope, WireUuid};
use rynvek_rts::{ArenaRuntime, FactionId, MatchMode};
use rynvek_security::{League, authorize};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn private_json(value: Value) -> Response {
    (
        [(header::CACHE_CONTROL, "no-store"), (header::VARY, "Authorization")],
        Json(value),
    )
        .into_response()
}
fn run_json(row: &MatchRow, pid: u64, state: MatchState, tick: u64, recovered: bool) -> Response {
    private_json(
        json!({"world_id":row.world_id.to_string(),"match_id":row.match_id.to_string(),
        "principal_id":pid.to_string(),"faction":0,"mode":row.game_mode,"match_state":state,"tick":tick,
        "recovered":recovered,"ephemeral":false}),
    )
}

/// 已有单人 PvE 世界直接开局/重连；真实恢复配置，不分配身份、不造 practice 世界、不覆盖终局。
pub async fn run(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldEdit).await?;
    app.require(&headers, CapabilityScope::MatchPlay).await?;
    app.require(&headers, CapabilityScope::WorldConnect).await?;
    authorize(&subject, CapabilityScope::MatchPlay, &league_resource(League::Human)).map_err(ApiError::from)?;
    let _start = app.inner.authored_arena_start.lock().await;
    let world = super::worlds::load_world(&app, &subject, &id, CapabilityScope::WorldEdit).await?;
    let pid = subject.principal_id;
    if let Some(active) = app.registry().active_for(pid) {
        if active.world_id != world.world_id {
            return Err(conflict("身份已在其他活动对局；先正常结束原局"));
        }
    }
    let active_world = {
        app.registry()
            .by_world(&world.world_id)
            .filter(|h| !h.status().state.is_terminal())
    };
    if let Some(active) = active_world {
        if active.seats.len() != 1 || active.seats.get(&pid) != Some(&0) {
            return Err(conflict("世界活动席位不属于此身份"));
        }
        let row = app
            .store()
            .get_match(active.match_id)
            .await?
            .ok_or_else(|| ApiError::not_found("对局不存在"))?
            .0;
        let status = active.status();
        return Ok(run_json(&row, pid, status.state, status.tick, false));
    }
    let runtime = super::worlds::restore_runtime(&app, &world).await?;
    if runtime.config.mode != MatchMode::PveSurvival || runtime.config.players != 1 {
        return Err(ApiError::bad_request(
            "当前创作开局入口仅绑定单人 PvE；多人/排位必须走正式完整席位流程",
        ));
    }
    let mut previous = app
        .store()
        .matches_of(pid)
        .await?
        .into_iter()
        .filter(|m| m.world_id == world.world_id)
        .collect::<Vec<_>>();
    previous.sort_by_key(|m| m.match_id);
    if runtime.outcome.is_some() {
        let row = previous
            .last()
            .filter(|m| m.match_state == MatchState::Completed)
            .ok_or_else(|| conflict("世界已终局；归档尚未提交或需从已有世界fork，不覆盖历史"))?;
        return Ok(run_json(row, pid, MatchState::Completed, runtime.tick, false));
    }
    // 进程恢复复用未终结 MatchRow，绝不因刷新创建另一条世界/身份/谱系。
    let (row, recovered) = if let Some(row) = previous.into_iter().rev().find(|m| !m.match_state.is_terminal()) {
        let (_, seats) = app
            .store()
            .get_match(row.match_id)
            .await?
            .ok_or_else(|| ApiError::not_found("对局不存在"))?;
        if seats.len() != 1 || seats[0].principal_id != pid || seats[0].faction_id != 0 {
            return Err(conflict("恢复席位不一致"));
        }
        (row, true)
    } else {
        let mut scenario = blake3::Hasher::new();
        scenario.update(b"rynvek-authored-arena/1\0");
        scenario.update(&runtime.state_root());
        let row = MatchRow {
            match_id: new_uuid_v7(),
            world_id: world.world_id,
            game_mode: world.game_mode.clone(),
            ruleset_blake3: *scenario.finalize().as_bytes(),
            map_seed: runtime.config.map_seed,
            match_state: MatchState::Created,
            winner_faction_id: None,
            win_reason: None,
            total_ticks: runtime.tick,
            replay_cid_blake3: None,
            started_at_unix_s: None,
            completed_at_unix_s: None,
        };
        app.store()
            .insert_match(
                row.clone(),
                vec![ParticipantRow {
                    match_id: row.match_id,
                    faction_id: 0,
                    principal_id: pid,
                    controller_league: League::Human,
                    final_score: 0,
                    rating_delta: None,
                }],
            )
            .await?;
        (row, false)
    };
    let host = app
        .attach_world_host(MatchSpec {
                custom: false,
            match_id: row.match_id,
            world_id: world.world_id,
            league: League::Human,
            ruleset_hash: row.ruleset_blake3,
            config: runtime.config.clone(),
            seats: vec![SeatSpec {
                faction: 0,
                principal_id: pid,
                kind: subject.kind,
            }],
        })
        .await?;
    let status = host.status();
    Ok(run_json(&row, pid, status.state, status.tick, recovered))
}

/// 仅节点内、可丢弃的重演游标；归档和权威世界从不因浏览/关闭修改。
#[derive(Debug)]
pub struct PlaybackEntry {
    owner: u64,
    match_id: WireUuid,
    touched: Instant,
    cursor: Option<Arc<tokio::sync::Mutex<Playback>>>,
}
/// 与实际ArenaRuntime共用的只读确定性重演状态。
pub struct Playback {
    opened: OpenedReplay,
    initial: ArenaRuntime,
    runtime: ArenaRuntime,
    index: usize,
    faction: u8,
}
// 不派生包含完整OpenedReplay/Genome的Debug，避免诊断日志意外输出私有源码。
impl std::fmt::Debug for Playback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Playback")
            .field("verified_records", &self.index)
            .field("tick", &self.runtime.tick)
            .finish_non_exhaustive()
    }
}

impl Playback {
    fn new(opened: OpenedReplay, faction: u8) -> ApiResult<Self> {
        let log = opened
            .body
            .runtime
            .as_ref()
            .ok_or_else(|| ApiError::bad_request("播放器只消费当前权威TickRecord归档"))?;
        let mut runtime = ArenaRuntime::restore(
            log.world_id,
            log.initial_revision,
            log.initial_root,
            &log.initial_snapshot,
        )
        .map_err(|code| ApiError::new(code, "签名初态恢复失败"))?;
        runtime.enable_command_log();
        if runtime.tick > opened.body.final_tick
            || runtime.observe(FactionId(faction)).is_none()
            || !opened.body.participants.iter().any(|p| p.faction == faction)
        {
            return Err(conflict("回放初态/阵营不一致"));
        }
        if log.ticks.windows(2).any(|w| w[0].tick > w[1].tick)
            || log
                .ticks
                .iter()
                .any(|r| r.tick < runtime.tick || r.tick > opened.body.final_tick)
        {
            return Err(conflict("回放记录Tick顺序/范围不符"));
        }
        let initial = runtime.clone();
        Ok(Self {
            opened,
            initial,
            runtime,
            index: 0,
            faction,
        })
    }
    fn seek(&mut self, tick: u64, max_steps: usize) -> ApiResult<Value> {
        let body = &self.opened.body;
        if tick < self.initial.tick || tick > body.final_tick {
            return Err(ApiError::bad_request("回放Tick越界"));
        }
        let log = body.runtime.as_ref().expect("构造已验证");
        // 定位按真实记录上界，不根据Tick差猜测已消费的命令数或跳过终局根。
        let target = log.ticks.partition_point(|record| record.tick <= tick);
        // 整批在隔离副本重演，坏记录/终态分歧不推进游标，也不触碰权威世界。
        let (mut runtime, mut index) = if target < self.index {
            (self.initial.clone(), 0)
        } else {
            (self.runtime.clone(), self.index)
        };
        let from_record = index;
        let mut events = Vec::new();
        let mut events_truncated = 0usize;
        for record in log.ticks[index..target].iter().take(max_steps) {
            let candidate = record
                .replay(&runtime)
                .map_err(|code| ApiError::new(code, "逐记录重演分歧；没有构造未来帧"))?;
            runtime = candidate;
            index += 1;
            for event in runtime
                .drain_events()
                .into_iter()
                .filter(|e| rynvek_engine::arena::event_visible_to(e, FactionId(self.faction)))
            {
                if events.len() < 256 {
                    events.push(event);
                } else {
                    events_truncated += 1;
                }
            }
        }
        let complete = index == log.ticks.len();
        if complete {
            let checkpoints = log
                .ticks
                .iter()
                .filter(|t| t.tick % replay::CHECKPOINT_INTERVAL_TICKS == 0)
                .map(|t| replay::Checkpoint {
                    tick: t.tick,
                    state_root: hex(&t.state_root),
                })
                .collect::<Vec<_>>();
            if runtime.tick != body.final_tick
                || hex(&runtime.state_root()) != body.final_state_root
                || runtime.config != body.config
                || runtime.outcome != body.outcome
                || runtime.command_log() != body.commands
                || checkpoints != body.checkpoints
            {
                return Err(conflict("完整重演的终局/命令/检查点与签名归档不一致"));
            }
        }
        let mut frame = self.frame(&runtime, index, tick, index == target, complete)?;
        frame["events"] = json!(events);
        frame["events_truncated"] = json!(events_truncated);
        frame["from_record"] = json!(from_record);
        self.runtime = runtime;
        self.index = index;
        Ok(frame)
    }
    fn frame(
        &self,
        runtime: &ArenaRuntime,
        index: usize,
        target: u64,
        ready: bool,
        complete: bool,
    ) -> ApiResult<Value> {
        Ok(
            json!({"match_id":self.opened.body.match_id,"world_id":self.opened.body.world_id,
            "faction":self.faction,"mode":runtime.config.mode,"initial_tick":self.initial.tick,"final_tick":self.opened.body.final_tick,
            "initial_revision":self.initial.revision().to_string(),"initial_state_root":hex(&self.initial.state_root()),
            "tick":runtime.tick,"revision":runtime.revision().to_string(),"state_root":hex(&runtime.state_root()),
            "target_tick":target,"ready":ready,"signature_verified":true,"complete_verified":complete,
            "verified_records":index,"total_records":self.opened.body.runtime.as_ref().expect("当前格式").ticks.len(),
            "map":rynvek_engine::arena::map_info(runtime.world()),
            "observation":runtime.observe(FactionId(self.faction)).ok_or_else(|| conflict("阵营不存在"))?}),
        )
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenRequest {
    #[serde(default)]
    faction: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeekRequest {
    tick: u64,
    #[serde(default = "default_steps")]
    max_steps: usize,
}
fn default_steps() -> usize {
    120
}
fn busy() -> ApiError {
    ApiError::bad_request("重演资源已满，请关闭旧游标后重试").with_status(StatusCode::TOO_MANY_REQUESTS)
}

/// 创建/替换本身份的有界临时回放游标，服务端先验签并恢复真实起点。
pub async fn open_playback(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldRead).await?;
    let id: WireUuid = id.parse().map_err(|_| ApiError::not_found("对局不存在"))?;
    let (row, participants) = app
        .store()
        .get_match(id)
        .await?
        .ok_or_else(|| ApiError::not_found("对局不存在"))?;
    super::matches::access(&app, &headers, &row, true).await?;
    let request: OpenRequest = super::parse_json(&body)?;
    let permit = app
        .inner
        .playback_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| busy())?;
    let bytes = super::matches::archive(&app, &row)?;
    let ring = app.tokens().keyring().clone();
    // 先预留有界槽位；同一身份显式打开新回放会替换自己的临时游标，不碰归档/权威数据。
    let cursor_id = new_uuid_v7();
    {
        let mut entries = app.inner.playbacks.lock().unwrap_or_else(|e| e.into_inner());
        entries.retain(|_, v| v.touched.elapsed() < Duration::from_secs(600) && v.owner != subject.principal_id);
        if entries.len() >= 4 {
            return Err(busy());
        }
        entries.insert(
            cursor_id,
            PlaybackEntry {
                owner: subject.principal_id,
                match_id: id,
                touched: Instant::now(),
                cursor: None,
            },
        );
    }
    let opened = tokio::task::spawn_blocking(move || -> ApiResult<_> {
        let _permit = permit;
        let opened = replay::open(&bytes, Some(&ring)).map_err(|e| ApiError::new(e.code, e.message))?;
        if opened.body.match_id != id.to_string()
            || opened.body.world_id != row.world_id.to_string()
            || opened.body.ruleset_hash != hex(&row.ruleset_blake3)
            || opened.body.participants.len() != participants.len()
            || participants.iter().any(|p| {
                !opened.body.participants.iter().any(|q| {
                    q.principal_id == p.principal_id
                        && i16::from(q.faction) == p.faction_id
                        && q.league == p.controller_league
                })
            })
        {
            return Err(conflict("签名归档与对局身份/规则不一致"));
        }
        let mut cursor = Playback::new(opened, request.faction)?;
        let tick = cursor.initial.tick;
        let value = cursor.seek(tick, 120)?;
        Ok((cursor, value))
    })
    .await
    .map_err(|_| ApiError::internal("回放工作线程失败"))
    .and_then(|v| v);
    let mut entries = app.inner.playbacks.lock().unwrap_or_else(|e| e.into_inner());
    let (cursor, value) = match opened {
        Ok(value) => value,
        Err(error) => {
            entries.remove(&cursor_id);
            return Err(error);
        }
    };
    let entry = entries
        .get_mut(&cursor_id)
        .ok_or_else(|| conflict("游标已关闭或被新会话替换"))?;
    entry.cursor = Some(Arc::new(tokio::sync::Mutex::new(cursor)));
    let mut value = value;
    value["cursor_id"] = json!(cursor_id.to_string());
    Ok(private_json(value))
}

async fn cursor(
    app: &AppState,
    headers: &HeaderMap,
    ids: &(String, String),
    remove: bool,
) -> ApiResult<Option<Arc<tokio::sync::Mutex<Playback>>>> {
    let subject = app.require(headers, CapabilityScope::WorldRead).await?;
    let match_id: WireUuid = ids.0.parse().map_err(|_| ApiError::not_found("游标不存在"))?;
    let cursor_id: WireUuid = ids.1.parse().map_err(|_| ApiError::not_found("游标不存在"))?;
    let (row, _) = app
        .store()
        .get_match(match_id)
        .await?
        .ok_or_else(|| ApiError::not_found("对局不存在"))?;
    super::matches::access(app, headers, &row, true).await?;
    let mut entries = app.inner.playbacks.lock().unwrap_or_else(|e| e.into_inner());
    entries.retain(|_, v| v.touched.elapsed() < Duration::from_secs(600));
    let entry = entries
        .get_mut(&cursor_id)
        .filter(|v| v.owner == subject.principal_id && v.match_id == match_id)
        .ok_or_else(|| ApiError::not_found("游标不存在"))?;
    entry.touched = Instant::now();
    let cursor = entry.cursor.clone();
    if remove {
        entries.remove(&cursor_id);
    }
    Ok(cursor)
}
/// 单请求至多重演120条真实记录；只返回已核对根的前缀。
pub async fn seek(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(ids): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Response> {
    let request: SeekRequest = super::parse_json(&body)?;
    if !(1..=120).contains(&request.max_steps) {
        return Err(ApiError::bad_request("max_steps必须为1–120条记录"));
    }
    let cursor = cursor(&app, &headers, &ids, false)
        .await?
        .ok_or_else(|| conflict("回放初态尚在校验"))?;
    let permit = app
        .inner
        .playback_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| busy())?;
    // 同一游标不排队积累无界 seek；每次最多120条记录，在阻塞池消费原权威日志。
    let mut guard = cursor.try_lock_owned().map_err(|_| conflict("同一游标已有重演请求"))?;
    let value = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        guard.seek(request.tick, request.max_steps)
    })
    .await
    .map_err(|_| ApiError::internal("回放工作线程失败"))??;
    Ok(private_json(value))
}
/// 关闭本身份/本对局游标，不修改运行世界或归档。
pub async fn close_playback(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(ids): Path<(String, String)>,
) -> ApiResult<Response> {
    cursor(&app, &headers, &ids, true).await?;
    Ok(private_json(json!({"closed":true})))
}

/// 离页/网络未确认时也可关闭本身份在此对局的待建游标；迟到工作线程不得重新插入。
pub async fn close_owned_playback(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldRead).await?;
    let id: WireUuid = id.parse().map_err(|_| ApiError::not_found("对局不存在"))?;
    let (row, _) = app
        .store()
        .get_match(id)
        .await?
        .ok_or_else(|| ApiError::not_found("对局不存在"))?;
    super::matches::access(&app, &headers, &row, true).await?;
    app.inner
        .playbacks
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|_, v| v.owner != subject.principal_id || v.match_id != id);
    Ok(private_json(json!({"closed":true})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactions::{TickInput, TickRecord};
    use rynvek_rts::{ArenaWorld, MatchConfig};
    #[test]
    fn playback_consumes_terminal_record_before_claiming_final_root() {
        let world_id = [39; 16];
        let mut initial = ArenaRuntime::new(world_id, ArenaWorld::new(MatchConfig::duel(42)));
        initial.enable_command_log();
        let origin = initial.checkpoint().unwrap();
        let mut candidate = initial.candidate_tick().unwrap();
        let command = rynvek_rts::commands::RtsCommand::Surrender;
        candidate.command(FactionId(0), &command).unwrap();
        candidate.finish_tick().unwrap();
        let final_tick = candidate.tick;
        assert!(candidate.outcome.is_some());
        let record = TickRecord {
            format: "rynvek-tick/2".into(),
            base_revision: initial.revision(),
            base_root: initial.state_root(),
            target_tick: 1,
            tick: candidate.tick,
            revision: candidate.revision(),
            state_root: candidate.state_root(),
            inputs: vec![TickInput::Command {
                faction: 0,
                command,
                principal_id: 11,
                idempotency_key: "test".into(),
            }],
        };
        let history = replay::RuntimeReplay {
            world_id,
            initial_revision: 0,
            initial_root: origin.state_root,
            initial_snapshot: origin.bytes,
            ticks: vec![record],
        };
        let body = replay::record_runtime(
            &candidate,
            history,
            WireUuid([40; 16]).to_string(),
            WireUuid(world_id).to_string(),
            hex(&[0; 32]),
            vec![replay::ReplayParticipant {
                faction: 0,
                principal_id: 11,
                league: League::Human,
            }],
        )
        .unwrap();
        let signer = rynvek_security::ServerSigningKey::from_seed(1, [41; 32]);
        let bytes = replay::encode(&body, world_id, &signer).unwrap();
        let mut keys = rynvek_security::Keyring::new();
        keys.insert(1, signer.public_key()).unwrap();
        let opened = replay::open(&bytes, Some(&keys)).unwrap();
        let mut cursor = Playback::new(opened, 0).unwrap();
        assert_ne!(cursor.runtime.state_root(), candidate.state_root());
        let final_frame = cursor.seek(final_tick, 1).unwrap();
        assert_eq!(final_frame["tick"], final_tick);
        assert_eq!(final_frame["verified_records"], 1);
        assert_eq!(final_frame["complete_verified"], true);
        assert_eq!(final_frame["state_root"], hex(&candidate.state_root()));
        assert!(!final_frame["events"].as_array().unwrap().is_empty());
    }
}
