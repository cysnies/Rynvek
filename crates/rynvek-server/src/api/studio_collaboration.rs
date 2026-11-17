// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 定向私有创作。准入只开放Studio源码/补丁，不授予控制、世界归档或其它世界。
use crate::{
    app::AppState,
    error::{ApiError, ApiResult},
    ids::new_uuid_v7,
    store::{EditorGrant, EditorPermit, WorldRow},
    transactions::{TickInput, TickRecord, conflict},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use rynvek_protocol::CapabilityScope;
use rynvek_security::{Subject, Visibility};
use serde::Deserialize;
use serde_json::{Value, json};
pub fn private(mut r: Response) -> Response {
    r.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("private, no-store"),
    );
    r.headers_mut().insert(
        header::VARY,
        header::HeaderValue::from_static("Authorization, X-Rynvek-Studio-Grant"),
    );
    r
}
pub fn response(v: Value) -> Response {
    (
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::VARY, "Authorization, X-Rynvek-Studio-Grant"),
        ],
        Json(v),
    )
        .into_response()
}
fn missing() -> ApiError {
    ApiError::not_found("世界或创作准入不存在")
}
pub async fn admission(
    app: &AppState,
    headers: &HeaderMap,
    id: &str,
    bound: bool,
) -> ApiResult<(Subject, WorldRow, EditorPermit)> {
    let s = app.require(headers, CapabilityScope::WorldEdit).await?;
    let wid = id.parse().map_err(|_| missing())?;
    let w = app.store().get_world(wid).await?.ok_or_else(missing)?;
    let grant = if s.principal_id == w.owner_principal_id {
        None
    } else {
        if w.visibility != Visibility::Private {
            return Err(missing());
        }
        let g = app
            .store()
            .editor_grants(wid)
            .await?
            .into_iter()
            .find(|g| g.principal_id == s.principal_id && g.active)
            .ok_or_else(missing)?;
        let supplied = headers.get("x-rynvek-studio-grant").and_then(|v| v.to_str().ok());
        if (bound || supplied.is_some()) && supplied != Some(g.grant_id.to_string().as_str()) {
            return Err(missing());
        }
        Some(g.grant_id)
    };
    let permit = EditorPermit {
        principal_id: s.principal_id,
        grant_id: grant,
    };
    app.store()
        .check_editor(wid, permit.clone())
        .await
        .map_err(|_| missing())?;
    Ok((s, w, permit))
}
pub fn access(w: &WorldRow, p: &EditorPermit) -> Value {
    json!({"principal_id":p.principal_id.to_string(),"role":if p.principal_id==w.owner_principal_id{"owner"}else{"studio-editor"},"grant_id":p.grant_id.map(|g|g.to_string())})
}
async fn owner(app: &AppState, h: &HeaderMap, id: &str) -> ApiResult<WorldRow> {
    let (_, w, p) = admission(app, h, id, false).await?;
    if p.principal_id != w.owner_principal_id || w.visibility != Visibility::Private {
        return Err(missing());
    }
    Ok(w)
}
fn grant_view(g: &EditorGrant) -> Value {
    json!({"principal_id":g.principal_id.to_string(),"grant_id":g.grant_id.to_string(),"active":g.active,"role":"studio-editor","consent":"studio-source-edit/1"})
}
pub async fn list(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let w = owner(&app, &headers, &id).await?;
    Ok(response(
        json!({"world_id":id,"grants":app.store().editor_grants(w.world_id).await?.iter().map(grant_view).collect::<Vec<_>>()}),
    ))
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    principal_id: String,
    expected_grant_id: Option<String>,
    active: bool,
    consent: String,
}
pub async fn change(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<Change>,
) -> ApiResult<Response> {
    let w = owner(&app, &headers, &id).await?;
    if b.consent != "studio-source-edit/1" {
        return Err(ApiError::bad_request("须明确授权读取全部Studio创作源码并编辑"));
    }
    let pid = b
        .principal_id
        .parse::<u64>()
        .ok()
        .filter(|p| *p > 0 && p.to_string() == b.principal_id)
        .ok_or_else(|| ApiError::bad_request("主体ID必须是精确十进制字符串"))?;
    let expected = b
        .expected_grant_id
        .map(|v| v.parse().map_err(|_| ApiError::bad_request("准入版本非法")))
        .transpose()?;
    if !b.active && expected.is_none() {
        return Err(conflict("撤销必须指定当前准入版本"));
    }
    let g = EditorGrant {
        world_id: w.world_id,
        principal_id: pid,
        grant_id: new_uuid_v7(),
        active: b.active,
    };
    app.store()
        .change_editor(w.owner_principal_id, g.clone(), expected)
        .await?;
    app.audit(
        Some(w.owner_principal_id),
        "world.studio.admission",
        format!("world:{}", w.world_id),
        grant_view(&g),
    )
    .await;
    Ok(response(grant_view(&g)))
}
#[derive(Debug, Deserialize)]
pub struct Cursor {
    after_revision: u64,
}
pub async fn changes(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<Cursor>,
) -> ApiResult<Response> {
    let (_, w, p) = admission(&app, &headers, &id, true).await?;
    if q.after_revision > w.latest_revision {
        return Err(conflict("游标超前，请重新读取权威基线"));
    }
    let mut cursor = q.after_revision;
    let mut tick = w.latest_tick;
    let mut root = w.latest_state_root;
    let mut out = vec![];
    let mut bytes = 0usize;
    for c in app
        .store()
        .list_commits(w.world_id)
        .await?
        .into_iter()
        .filter(|c| c.world_revision > q.after_revision && c.world_revision <= w.latest_revision)
        .take(64)
    {
        if c.world_revision != cursor + 1 {
            return Err(conflict("创作日志缺口，请重新连接"));
        }
        let raw = app
            .inner
            .world_snapshots
            .get(&rynvek_core::Blake3Hash(c.journal_segment_cid))?;
        if rynvek_core::Blake3Hash::of(&raw).0 != c.journal_segment_cid {
            return Err(conflict("创作日志CID不符"));
        }
        // 单原始日志有原请求上限；一页额外限制2MiB，不丢patch或跳游标。
        if bytes + raw.len() > 2 * 1024 * 1024 && cursor > q.after_revision {
            break;
        }
        bytes += raw.len();
        if let Ok(record) = serde_json::from_slice::<TickRecord>(&raw) {
            if record.format != "rynvek-tick/2"
                || record.revision != c.world_revision
                || record.state_root != c.state_root_blake3
            {
                return Err(conflict("创作日志版本/提交不符"));
            }
            for input in record.inputs {
                if let TickInput::Patch { patch, .. } = input {
                    out.push(json!({"revision":c.world_revision,"patch":patch}));
                }
            }
        } else {
            let patch: rynvek_world::WorldPatch =
                serde_json::from_slice(&raw).map_err(|_| conflict("不是当前可消费创作日志"))?;
            out.push(json!({"revision":c.world_revision,"patch":patch}));
        }
        cursor = c.world_revision;
        tick = c.tick;
        root = c.state_root_blake3;
    }
    app.store().check_editor(w.world_id, p).await.map_err(|_| missing())?;
    Ok(response(
        json!({"world_id":id,"from_revision":q.after_revision,"to_revision":cursor,"tick":tick,"state_root":crate::ids::hex(&root),"patches":out,"has_more":cursor<w.latest_revision}),
    ))
}
