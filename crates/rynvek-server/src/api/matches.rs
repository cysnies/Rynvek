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
use crate::host::MatchHandle;
use crate::ids::{hex, parse_hex, unix_now_s};
use crate::matchmaking::QueueEntry;
use crate::store::MatchState;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueRequest {
    league: String,
    /// 规则集 BLAKE3（十六进制，可带 `blake3:` 前缀）
    ruleset_hash: String,
    /// `true` 表示离队
    #[serde(default)]
    cancel: bool,
}

fn matched_json(app: &AppState, h: &MatchHandle) -> serde_json::Value {
    let status = h.status();
    serde_json::json!({
        "status": "matched",
        "match_id": h.match_id.to_string(),
        "world_id": h.world_id.to_string(),
        "league": h.league,
        "match_state": status.state,
        "stream": app.stream_urls(&h.world_id),
    })
}

/// `10 POST /api/v1/matches/queue`（`match.play`）
///
/// 幂等：已在进行中对局里的主体直接得到该对局；否则入队并立即尝试配对。客户端以相同请求轮询。
pub async fn queue(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
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
    if req.cancel {
        let removed = app.matchmaker().cancel(pid);
        return Ok(Json(serde_json::json!({ "status": if removed { "cancelled" } else { "not_queued" } })).into_response());
    }
    if let Some(h) = app.registry().active_for(pid) {
        return Ok(Json(matched_json(&app, &h)).into_response());
    }
    let rating = app.store().get_rating(pid, league).await?;
    let now = unix_now_s();
    let (position, pairings) = {
        let mut mm = app.matchmaker();
        let pos = mm.enqueue(
            (league, ruleset),
            QueueEntry {
                principal_id: pid,
                rating: rating.rating,
                deviation: rating.deviation,
                enqueued_at_s: now,
            },
        );
        (pos, mm.pair(now))
    };
    for p in &pairings {
        if let Err(e) = app.start_match(p).await {
            // 建局失败：把双方放回队列，避免丢人
            let mut mm = app.matchmaker();
            for q in p.players {
                mm.enqueue((p.league, p.ruleset_hash), q);
            }
            return Err(e);
        }
    }
    if let Some(h) = app.registry().active_for(pid) {
        return Ok(Json(matched_json(&app, &h)).into_response());
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "queued",
            "league": league,
            "position": position,
            "rating": rating.rating,
        })),
    )
        .into_response())
}

fn parse_match_id(id: &str) -> ApiResult<WireUuid> {
    id.parse().map_err(|_| ApiError::not_found("match_id 不是合法 UUID"))
}

/// `11 GET /api/v1/matches/{match_id}`（Public）
pub async fn get_match(State(app): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let match_id = parse_match_id(&id)?;
    let (row, participants) = app
        .store()
        .get_match(match_id)
        .await?
        .ok_or_else(|| ApiError::not_found("对局不存在"))?;
    let live = app.registry().get(&match_id).map(|h| h.status());
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
    Ok(Json(serde_json::json!({
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
    }))
    .into_response())
}

/// `12 GET /api/v1/matches/{match_id}/replay`（Public）：签名 `.rreplay`
pub async fn replay(State(app): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let match_id = parse_match_id(&id)?;
    let (row, _) = app
        .store()
        .get_match(match_id)
        .await?
        .ok_or_else(|| ApiError::not_found("对局不存在"))?;
    let Some(cid) = row.replay_cid_blake3.filter(|_| row.match_state == MatchState::Completed) else {
        return Err(ApiError::not_found("对局尚未结束，回放不可用"));
    };
    let bytes = app.inner.blobs.get(&rynvek_core::hash::Blake3Hash(cid))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.rynvek.rreplay".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{match_id}.rreplay\""),
            ),
            (header::ETAG, format!("\"{}\"", hex(&cid))),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_owned()),
        ],
        bytes,
    )
        .into_response())
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
