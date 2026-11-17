// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 自创私有世界的定向阵营观战：准入不等于WorldRead/参赛席位，更不授予源码、归档或租约。
use crate::{
    app::AppState,
    error::{ApiError, ApiResult},
    ids::new_uuid_v7,
    store::{ObserverGrant, WorldRow},
    transactions::conflict,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use rynvek_protocol::{CapabilityScope, WireUuid};
use rynvek_security::{PrincipalStatus, Visibility};
use serde::Deserialize;
use serde_json::{Value, json};

fn response(value: Value) -> Response {
    (
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::VARY, "Authorization"),
        ],
        Json(value),
    )
        .into_response()
}
fn missing() -> ApiError {
    ApiError::not_found("世界或观战准入不存在")
}
fn parse(id: &str) -> ApiResult<WireUuid> {
    id.parse().map_err(|_| ApiError::bad_request("UUID非法"))
}
fn view(g: &ObserverGrant) -> Value {
    json!({"principal_id":g.principal_id.to_string(),"faction":g.faction,"grant_id":g.grant_id.to_string(),"role":"observer"})
}
async fn owner(app: &AppState, headers: &HeaderMap, id: &str) -> ApiResult<WorldRow> {
    let s = app.require(headers, CapabilityScope::WorldEdit).await?;
    let w = super::worlds::load_world(app, &s, id, CapabilityScope::WorldEdit).await?;
    if w.owner_principal_id != s.principal_id || w.visibility != Visibility::Private {
        return Err(missing());
    }
    Ok(w)
}
async fn admission(app: &AppState, headers: &HeaderMap, id: &str) -> ApiResult<ObserverGrant> {
    let s = app.require(headers, CapabilityScope::WorldConnect).await?;
    if s.status != PrincipalStatus::Active {
        return Err(ApiError::forbidden("身份不可用"));
    }
    let world = parse(id)?;
    let w = app.store().get_world(world).await?.ok_or_else(missing)?;
    if w.visibility != Visibility::Private {
        return Err(missing());
    }
    app.store()
        .observer_grants(world)
        .await?
        .into_iter()
        .find(|g| g.principal_id == s.principal_id)
        .ok_or_else(missing)
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantInput {
    principal_id: String,
    faction: u8,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantVersion {
    grant_id: String,
}

pub async fn list(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let w = owner(&app, &headers, &id).await?;
    Ok(response(
        json!({"world_id":id,"grants":app.store().observer_grants(w.world_id).await?.iter().map(view).collect::<Vec<_>>()}),
    ))
}
pub async fn grant(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<GrantInput>,
) -> ApiResult<Response> {
    let _guard = app.inner.authored_arena_start.lock().await;
    let w = owner(&app, &headers, &id).await?;
    let target = input
        .principal_id
        .parse::<u64>()
        .ok()
        .filter(|p| *p > 0 && p.to_string() == input.principal_id)
        .ok_or_else(|| ApiError::bad_request("主体ID必须是十进制字符串"))?;
    let p = app
        .store()
        .get_principal(target)
        .await?
        .filter(|p| p.status == PrincipalStatus::Active)
        .ok_or_else(|| ApiError::bad_request("目标正式身份不存在或不可用"))?;
    let host = app
        .registry()
        .by_world(&w.world_id)
        .filter(|h| !h.status().state.is_terminal())
        .ok_or_else(|| conflict("所有者须先运行/重连原世界"))?;
    // 仅可共享所有者自己控制的阵营；绝不允许猜测对手阵营或把排位变成私有旁路。
    if target == w.owner_principal_id
        || host.seats.contains_key(&p.principal_id)
        || host.seats.get(&w.owner_principal_id) != Some(&input.faction)
    {
        return Err(ApiError::forbidden("只能向非参赛者授予所有者所控阵营观战"));
    }
    let grants = app.store().observer_grants(w.world_id).await?;
    if grants.len() >= 32 && !grants.iter().any(|g| g.principal_id == target) {
        return Err(conflict("单世界最多32个有效观战准入"));
    }
    let grant = ObserverGrant {
        world_id: w.world_id,
        principal_id: target,
        faction: input.faction,
        grant_id: new_uuid_v7(),
        active: true,
    };
    app.store().set_observer_grant(grant.clone()).await?;
    app.audit(
        Some(w.owner_principal_id),
        "world.observer.grant",
        format!("world:{}", w.world_id),
        view(&grant),
    )
    .await;
    Ok(response(view(&grant)))
}
pub async fn revoke(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, pid)): Path<(String, String)>,
    Json(version): Json<GrantVersion>,
) -> ApiResult<Response> {
    let _guard = app.inner.authored_arena_start.lock().await;
    let w = owner(&app, &headers, &id).await?;
    let pid = pid.parse::<u64>().map_err(|_| ApiError::bad_request("主体ID非法"))?;
    let changed = app
        .store()
        .revoke_observer(w.world_id, pid, parse(&version.grant_id)?)
        .await?;
    if !changed {
        return Err(conflict("准入已撤销或版本已改变，请刷新，不覆盖新授权"));
    }
    app.audit(
        Some(w.owner_principal_id),
        "world.observer.revoke",
        format!("world:{}", w.world_id),
        json!({"principal_id":pid.to_string(),"grant_id":version.grant_id}),
    )
    .await;
    Ok(response(json!({"status":"revoked"})))
}
pub async fn leave(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(version): Json<GrantVersion>,
) -> ApiResult<Response> {
    let _guard = app.inner.authored_arena_start.lock().await;
    let g = admission(&app, &headers, &id).await?;
    if g.grant_id != parse(&version.grant_id)? {
        return Err(conflict("准入版本改变，请重新确认"));
    }
    if !app
        .store()
        .revoke_observer(g.world_id, g.principal_id, g.grant_id)
        .await?
    {
        return Err(conflict("准入已改变"));
    }
    app.audit(
        Some(g.principal_id),
        "world.observer.leave",
        format!("world:{}", g.world_id),
        view(&g),
    )
    .await;
    Ok(response(json!({"status":"left"})))
}
pub async fn observe(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let g = admission(&app, &headers, &id).await?;
    let host = app
        .registry()
        .by_world(&g.world_id)
        .filter(|h| !h.status().state.is_terminal())
        .ok_or_else(|| conflict("原Actor未运行或已结束；观战不自动开局，须由所有者恢复"))?;
    let world = app.store().get_world(g.world_id).await?.ok_or_else(missing)?;
    if host.seats.get(&world.owner_principal_id) != Some(&g.faction) || host.seats.contains_key(&g.principal_id) {
        return Err(missing());
    }
    let messages = tokio::time::timeout(std::time::Duration::from_secs(2), host.observe_faction(g.faction))
        .await
        .map_err(|_| conflict("Actor只读查询超时"))??;
    // 与Actor排队期间撤销/重授并发时丢弃结果；不缓存长寿命观战凭据。
    if admission(&app, &headers, &id).await? != g {
        return Err(missing());
    }
    let principal = app.store().get_principal(g.principal_id).await?.ok_or_else(missing)?;
    Ok(response(
        json!({"world_id":g.world_id.to_string(),"match_id":host.match_id.to_string(),"principal_id":g.principal_id.to_string(),"principal_kind":principal.kind,"grant_id":g.grant_id.to_string(),"faction":g.faction,"role":"observer","messages":messages}),
    ))
}
