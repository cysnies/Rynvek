// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 路由 `10`–`13`：匹配队列、对局查询、签名回放下载与 Glicko-2 排行榜。

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rynvek_core::error::RynvekErrorCode;
use rynvek_protocol::{CapabilityScope, WireUuid};
use rynvek_security::{League, authorize};
use serde::Deserialize;

use crate::app::{AppState, arena_ruleset_hash, league_resource};
use crate::error::{ApiError, ApiResult};
use crate::ids::{hex, parse_hex, unix_now_s};
use crate::matchmaking::QueueEntry;
use crate::ranked::{QueueAction, QueueCommand, QueueDecision, QueueTicket};
use crate::store::{MatchRow, MatchState};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueRequest {
    league: String,
    /// 规则集 BLAKE3（十六进制，可带 `blake3:` 前缀）
    ruleset_hash: String,
    /// `true` 表示离队
    #[serde(default)]
    cancel: bool,
    /// 仅刷新已有队列，不得在终局或未知身份离队后重新入队。
    #[serde(default)]
    poll: bool,
}

async fn matched_json(app: &AppState, id: WireUuid, pid: u64) -> ApiResult<serde_json::Value> {
    let (row, parts) = app
        .store()
        .get_match(id)
        .await?
        .ok_or_else(|| ApiError::not_found("对局不存在"))?;
    let part = parts
        .iter()
        .find(|p| p.principal_id == pid)
        .ok_or_else(|| ApiError::forbidden("不是本人席位"))?;
    let world = app
        .store()
        .get_world(row.world_id)
        .await?
        .ok_or_else(|| ApiError::not_found("世界不存在"))?;
    let lease = app.store().ranked_host(row.world_id).await?;
    let live = lease
        .as_ref()
        .filter(|h| h.fence == world.fencing_token && h.expires_at_s > unix_now_s());
    let local = app.registry().get(&id).filter(|h| {
        !h.status().state.is_terminal()
            && live.is_some_and(|l| h.fencing_token == Some(l.fence) && l.node == app.inner.ranked_node)
    });
    let status = local.as_ref().map(|h| h.status());
    Ok(
        serde_json::json!({"status":"matched","principal_id":pid.to_string(),"mode":row.game_mode,
        "tick":status.as_ref().map_or(world.latest_tick,|s|s.tick),"faction":part.faction_id,
        "match_id":id.to_string(),"world_id":row.world_id.to_string(),"league":part.controller_league,
        "match_state":status.as_ref().map_or(row.match_state,|s|s.state),
        "host":{"state":if local.is_some(){"local"}else if live.is_some_and(|h|h.node!=app.inner.ranked_node){"remote"}else{"recoverable"},
            "origin":live.map(|h|h.origin.as_str()),"expires_at_s":live.map(|h|h.expires_at_s)},
        "stream":if local.is_some(){app.stream_urls(&row.world_id)}else{serde_json::Value::Null}}),
    )
}
fn command(
    pid: u64,
    league: League,
    ruleset: [u8; 32],
    action: QueueAction,
    rating: f64,
    deviation: f64,
) -> QueueCommand {
    QueueCommand {
        action,
        ticket: QueueTicket {
            league,
            ruleset,
            entry: QueueEntry {
                principal_id: pid,
                rating,
                deviation,
                enqueued_at_s: unix_now_s(),
            },
        },
    }
}

/// `10 POST /api/v1/matches/queue`（`match.play`）
///
/// 幂等：活动主体返回原对局；显式入队才新建队列成员。后续用 `poll=true` 刷新，不能终局后自动重入。
pub async fn queue(state: State<AppState>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    let mut response = queue_inner(state, headers, body).await?;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().expect("固定头"));
    response
        .headers_mut()
        .insert(header::VARY, "Authorization".parse().expect("固定头"));
    Ok(response)
}

async fn queue_inner(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::MatchPlay).await?;
    let req: QueueRequest = super::parse_json(&body)?;
    let league = League::parse(&req.league)
        .ok_or_else(|| ApiError::bad_request("league 必须是 human | script | agent_hybrid"))?;
    authorize(&subject, CapabilityScope::MatchPlay, &league_resource(league)).map_err(ApiError::from)?;
    let ruleset: [u8; 32] = parse_hex(req.ruleset_hash.trim_start_matches("blake3:"))
        .ok_or_else(|| ApiError::bad_request("ruleset_hash 必须是 32 字节十六进制"))?;
    if ruleset != arena_ruleset_hash() {
        return Err(ApiError::new(
            RynvekErrorCode::DependencyLockConflict,
            format!(
                "本服务器只运行规则集 `{}`（{}）",
                crate::app::ARENA_RULESET_ID,
                hex(&arena_ruleset_hash())
            ),
        ));
    }
    let pid = subject.principal_id;
    if req.cancel && req.poll {
        return Err(ApiError::bad_request("cancel/poll不可同时指定"));
    }
    let rating = app.store().get_rating(pid, league).await?;
    let action = if req.cancel {
        QueueAction::Cancel
    } else if req.poll {
        QueueAction::Poll
    } else {
        QueueAction::Join
    };
    let req = command(pid, league, ruleset, action, rating.rating, rating.deviation);
    let mut decision = app.store().ranked_queue(req.clone(), None).await?;
    if matches!(decision, QueueDecision::Prepare) {
        let genesis = app.prepare_ranked_genesis()?;
        let mut poll = req.clone();
        poll.action = QueueAction::Poll;
        decision = app.store().ranked_queue(poll, Some(genesis)).await?;
    }
    let (status, body) = match decision {
        QueueDecision::Matched(id) => {
            if action != QueueAction::Cancel {
                app.ensure_ranked_host(id).await?;
            }
            (StatusCode::OK, matched_json(&app, id, pid).await?)
        }
        QueueDecision::Queued { position, rating } => (
            StatusCode::ACCEPTED,
            serde_json::json!({"status":"queued","league":league,"position":position,"rating":rating}),
        ),
        QueueDecision::Cancelled => (StatusCode::OK, serde_json::json!({"status":"cancelled"})),
        QueueDecision::Idle => (StatusCode::OK, serde_json::json!({"status":"not_queued"})),
        QueueDecision::Prepare => return Err(ApiError::internal("配对首提交未完成，请刷新队列")),
    };
    Ok((status, Json(body)).into_response())
}

/// 持久席位/队列只读发现：不入队、不认领宿主、不创造对手。
pub async fn queue_context(State(app): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::MatchPlay).await?;
    authorize(&subject, CapabilityScope::MatchPlay, &league_resource(League::Human)).map_err(ApiError::from)?;
    let decision = app
        .store()
        .ranked_queue(
            command(
                subject.principal_id,
                League::Human,
                arena_ruleset_hash(),
                QueueAction::Read,
                0.0,
                0.0,
            ),
            None,
        )
        .await?;
    let active = if let QueueDecision::Matched(id) = decision {
        Some(matched_json(&app, id, subject.principal_id).await?)
    } else {
        None
    };
    Ok(([(header::CACHE_CONTROL,"private, no-store"),(header::VARY,"Authorization")],Json(serde_json::json!({
        "principal_id":subject.principal_id.to_string(),"league":"human","ruleset_hash":hex(&arena_ruleset_hash()),
        "status":if active.is_some(){"matched"}else if matches!(decision,QueueDecision::Queued {..}){"queued"}else{"idle"},"active":active
    }))).into_response())
}
/// 已配对主体显式连接/恢复；远端健康宿主只回报地址，不跨origin转发任何凭据。
pub async fn connect(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::MatchPlay).await?;
    let id = parse_match_id(&id)?;
    let (row, parts) = app
        .store()
        .get_match(id)
        .await?
        .ok_or_else(|| ApiError::not_found("对局不存在"))?;
    let p = parts
        .iter()
        .find(|p| p.principal_id == subject.principal_id)
        .ok_or_else(|| ApiError::forbidden("只能连接本人席位"))?;
    if row.game_mode != "PvP_Ranked" {
        return Err(ApiError::forbidden("仅用于排位，私有自创走原准入"));
    }
    authorize(
        &subject,
        CapabilityScope::MatchPlay,
        &league_resource(p.controller_league),
    )
    .map_err(ApiError::from)?;
    app.ensure_ranked_host(id).await?;
    Ok((
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::VARY, "Authorization"),
        ],
        Json(matched_json(&app, id, subject.principal_id).await?),
    )
        .into_response())
}

fn parse_match_id(id: &str) -> ApiResult<WireUuid> {
    id.parse().map_err(|_| ApiError::not_found("match_id 不是合法 UUID"))
}

/// `11 GET /api/v1/matches/{match_id}`：按世界可见性隔离私有对局元数据。
pub async fn get_match(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let match_id = parse_match_id(&id)?;
    let (row, participants) = app
        .store()
        .get_match(match_id)
        .await?
        .ok_or_else(|| ApiError::not_found("对局不存在"))?;
    access(&app, &headers, &row, false).await?;
    let candidate = app.registry().get(&match_id);
    let live = if row.game_mode == "PvP_Ranked" {
        let world = app.store().get_world(row.world_id).await?;
        let lease = app.store().ranked_host(row.world_id).await?;
        candidate
            .filter(|h| {
                lease.as_ref().is_some_and(|l| {
                    l.expires_at_s > unix_now_s()
                        && Some(l.fence) == h.fencing_token
                        && world.as_ref().is_some_and(|w| w.fencing_token == l.fence)
                })
            })
            .map(|h| h.status())
    } else {
        candidate.map(|h| h.status())
    };
    // 持久化可能略滞后于宿主：进行中对局以宿主实时状态为准
    let (state, tick) = match live {
        Some(s) if !row.match_state.is_terminal() => (s.state, s.tick),
        _ => (row.match_state, row.total_ticks),
    };
    let mut ps = Vec::new();
    for p in &participants {
        let handle = app
            .store()
            .get_principal(p.principal_id)
            .await?
            .map(|x| x.handle)
            .unwrap_or_default();
        ps.push(serde_json::json!({
            "faction_id": p.faction_id,
            "principal_id": p.principal_id.to_string(),
            "handle": handle,
            "controller_league": p.controller_league,
            "final_score": p.final_score,
            "rating_delta": p.rating_delta,
        }));
    }
    Ok((
        [(header::CACHE_CONTROL, "no-store"), (header::VARY, "Authorization")],
        Json(serde_json::json!({
            "match_id": row.match_id.to_string(),
            "world_id": row.world_id.to_string(),
            "game_mode": row.game_mode,
            "ruleset_blake3": hex(&row.ruleset_blake3),
            "map_seed": row.map_seed,
            "match_state": state,
            "tick": tick,
            "winner_faction_id": row.winner_faction_id,
            "win_reason": row.win_reason,
            "total_ticks": row.total_ticks,
            "replay_cid_blake3": row.replay_cid_blake3.map(|c| hex(&c)),
            "started_at_unix_s": row.started_at_unix_s,
            "completed_at_unix_s": row.completed_at_unix_s,
            "participants": ps,
            "replay_url": row.replay_cid_blake3.map(|_| format!("/api/v1/matches/{}/replay", row.match_id)),
            "replay_signer": {
                "key_id": app.inner.signer.key_id(),
                "ed25519_public_key": hex(&app.inner.signer.public_key()),
            },
        })),
    )
        .into_response())
}

/// `12 GET /api/v1/matches/{match_id}/replay`：公共排位公开，自创完整源码归档要求WorldEdit。
pub async fn replay(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let match_id = parse_match_id(&id)?;
    let (row, _) = app
        .store()
        .get_match(match_id)
        .await?
        .ok_or_else(|| ApiError::not_found("对局不存在"))?;
    let public = access(&app, &headers, &row, true).await?;
    let Some(cid) = row
        .replay_cid_blake3
        .filter(|_| row.match_state == MatchState::Completed)
    else {
        return Err(ApiError::not_found("对局尚未结束，回放不可用"));
    };
    let bytes = archive(&app, &row)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.rynvek.rreplay".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{match_id}.rreplay\""),
            ),
            (header::ETAG, format!("\"{}\"", hex(&cid))),
            (
                header::CACHE_CONTROL,
                if public {
                    "public, max-age=31536000, immutable"
                } else {
                    "no-store"
                }
                .to_owned(),
            ),
            (header::VARY, "Authorization".to_owned()),
        ],
        bytes,
    )
        .into_response())
}

/// 公共排位可公开；自创世界归档含全量私有基因/规则，必须 WorldEdit，不能把 WorldRead 当源码授权。
/// 不存在、无凭据、越权统一404；完整归档绝不经 Hub 公共制品路由。
pub(crate) async fn access(app: &AppState, headers: &HeaderMap, row: &MatchRow, full_archive: bool) -> ApiResult<bool> {
    let world = app
        .store()
        .get_world(row.world_id)
        .await?
        .ok_or_else(|| ApiError::not_found("对局不存在"))?;
    if world.visibility == rynvek_security::Visibility::Public && (!full_archive || row.game_mode == "PvP_Ranked") {
        return Ok(true);
    }
    let scope = if full_archive {
        CapabilityScope::WorldEdit
    } else {
        CapabilityScope::WorldRead
    };
    let subject = app
        .require(headers, scope)
        .await
        .map_err(|_| ApiError::not_found("对局不存在"))?;
    super::worlds::load_world(app, &subject, &row.world_id.to_string(), scope)
        .await
        .map_err(|_| ApiError::not_found("对局不存在"))?;
    Ok(false)
}

pub(crate) fn archive(app: &AppState, row: &MatchRow) -> ApiResult<Vec<u8>> {
    let cid = row
        .replay_cid_blake3
        .filter(|_| row.match_state == MatchState::Completed)
        .ok_or_else(|| ApiError::not_found("对局尚未结束，回放不可用"))?;
    let bytes = app.inner.world_snapshots.get(&rynvek_core::hash::Blake3Hash(cid))?;
    if blake3::hash(&bytes).as_bytes() != &cid {
        return Err(ApiError::new(RynvekErrorCode::Blake3ChecksumMismatch, "回放CID不符"));
    }
    Ok(bytes)
}

#[derive(Debug, Deserialize)]
pub struct RankingQuery {
    #[serde(default)]
    limit: Option<u32>,
}

/// `13 GET /api/v1/rankings/{league}`（Public）
pub async fn rankings(
    State(app): State<AppState>,
    Path(league): Path<String>,
    Query(q): Query<RankingQuery>,
) -> ApiResult<Response> {
    let league = League::parse(&league).ok_or_else(|| ApiError::not_found("未知赛道"))?;
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let rows = app.store().leaderboard(league, limit).await?;
    Ok(Json(serde_json::json!({
        "league": league,
        "system": { "algorithm": "glicko2", "tau": crate::glicko2::DEFAULT_TAU },
        "entries": rows.iter().enumerate().map(|(i, (r, handle))| serde_json::json!({
            "rank": i + 1,
            "principal_id": r.principal_id.to_string(),
            "handle": handle,
            "rating": r.rating,
            "deviation": r.deviation,
            "volatility": r.volatility,
            "matches_played": r.matches_played,
        })).collect::<Vec<_>>(),
    }))
    .into_response())
}
