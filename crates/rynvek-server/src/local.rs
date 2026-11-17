// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 显式回环开发模式：每个会话创建真实私有 PvE 世界，仍使用正式权限和 Actor。
use crate::{
    ApiError, ApiResult, AppState,
    app::{arena_ruleset_hash, encode_token_text},
    host::{MatchSpec, SeatSpec},
    store::{MatchRow, MatchState, ParticipantRow},
};
use axum::{
    Json,
    body::{Bytes, to_bytes},
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use rynvek_protocol::WireUuid;
use rynvek_rts::MatchConfig;
use rynvek_security::{League, PrincipalKindCode, ServerSigningKey};
use serde_json::{Value, json};

pub async fn practice(
    State(app): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<(StatusCode, HeaderMap, Json<Value>)> {
    let origin = app
        .config()
        .local_practice_origin
        .as_deref()
        .ok_or_else(|| ApiError::not_found("本地练习未启用"))?;
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(origin)
        || headers.get("x-rynvek-local").and_then(|v| v.to_str().ok()) != Some("1")
    {
        return Err(ApiError::forbidden("本地开发入口仅接受配置的同源页面与显式请求头"));
    }
    let permit = app.inner.practice_slots.clone().try_acquire_owned().map_err(|_| {
        ApiError::bad_request("最多四个本地活动练习；请先结束已有对局").with_status(StatusCode::TOO_MANY_REQUESTS)
    })?;
    let pid = crate::ids::new_principal_id();
    let key = ServerSigningKey::generate(0).map_err(|e| ApiError::internal(e.to_string()))?;
    app.register_principal(pid, &format!("local-{pid}"), PrincipalKindCode::Human, key.public_key())
        .await?;
    let token = encode_token_text(&app.issue_token(pid, PrincipalKindCode::Human));
    let mut auth = HeaderMap::new();
    auth.insert("authorization", format!("Bearer {token}").parse().expect("token"));
    auth.insert("content-type", "application/json".parse().expect("常量"));
    // 复用正式创作入口，不能绕开 Capability/ABAC、私有 genesis 或配额语义。
    let created = crate::api::worlds::create(
        State(app.clone()),
        auth,
        Query(Default::default()),
        Bytes::from(
            json!({"title":"本地 PvE 生存练习","template":"arena_pve","map_seed":42,"visibility":"Private"})
                .to_string(),
        ),
    )
    .await?;
    let value: Value = serde_json::from_slice(
        &to_bytes(created.into_body(), 65536)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let world_id: WireUuid = value["world_id"]
        .as_str()
        .ok_or_else(|| ApiError::internal("缺少世界标识"))?
        .parse()
        .map_err(|_| ApiError::internal("世界标识错误"))?;
    let match_id = crate::ids::new_uuid_v7();
    app.store()
        .insert_match(
            MatchRow {
                match_id,
                world_id,
                game_mode: "PvE_Survival".into(),
                ruleset_blake3: arena_ruleset_hash(),
                map_seed: 42,
                match_state: MatchState::Created,
                winner_faction_id: None,
                win_reason: None,
                total_ticks: 0,
                replay_cid_blake3: None,
                started_at_unix_s: None,
                completed_at_unix_s: None,
            },
            vec![ParticipantRow {
                match_id,
                faction_id: 0,
                principal_id: pid,
                controller_league: League::Human,
                final_score: 0,
                rating_delta: None,
            }],
        )
        .await?;
    let host = app
        .attach_world_host(MatchSpec {
            match_id,
            world_id,
            league: League::Human,
            ruleset_hash: arena_ruleset_hash(),
            config: MatchConfig::pve(42, 1),
            seats: vec![SeatSpec {
                faction: 0,
                principal_id: pid,
                kind: PrincipalKindCode::Human,
            }],
        })
        .await?;
    tokio::spawn(async move {
        host.wait_for(|s| s.state.is_terminal()).await;
        drop(permit);
    });
    let mut headers = HeaderMap::new();
    headers.insert("cache-control", "no-store".parse().expect("常量"));
    Ok((
        StatusCode::CREATED,
        headers,
        Json(
            json!({"world_id":world_id.to_string(),"match_id":match_id.to_string(),"principal_id":pid.to_string(),"faction":0,"token":token,"mode":"PvE_Survival","ephemeral":true}),
        ),
    ))
}
