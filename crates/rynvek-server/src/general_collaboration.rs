// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! General已安装规则驱动准入；复用原持久目的许可/提交锁/Actor，不授予owner观察或私有包。
use crate::{
    app::AppState,
    error::{ApiError, ApiResult},
    general_host::{self, Action},
    ids::new_uuid_v7,
    store::{AdmissionPurpose, EditorGrant, EditorPermit, WorldRow},
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use rynvek_agent::ControlLevel;
use rynvek_protocol::{CapabilityScope, WireUuid};
use rynvek_security::{PrincipalKindCode, Subject};
use serde::Deserialize;
use serde_json::{Value, json};
const PURPOSE: AdmissionPurpose = AdmissionPurpose::GeneralRuleDrive;
fn missing() -> ApiError {
    ApiError::not_found("General或此目的准入不存在")
}
pub(crate) async fn admission(
    app: &AppState,
    headers: &HeaderMap,
    id: &str,
    grant: WireUuid,
    write: bool,
) -> ApiResult<(Subject, WorldRow, EditorPermit)> {
    let s = app.require(headers, CapabilityScope::WorldConnect).await?;
    app.require(headers, CapabilityScope::WorldRead).await?;
    if write {
        app.require(headers, CapabilityScope::WorldEdit).await?;
    }
    let wid = id.parse().map_err(|_| missing())?;
    let w = app
        .store()
        .get_world(wid)
        .await?
        .filter(WorldRow::is_controlled_general)
        .ok_or_else(missing)?;
    if s.kind != PrincipalKindCode::Human || s.principal_id == w.owner_principal_id {
        return Err(missing());
    }
    let p = EditorPermit {
        principal_id: s.principal_id,
        grant_id: Some(grant),
        purpose: PURPOSE,
    };
    app.store()
        .check_editor(wid, p.clone())
        .await
        .map_err(|e| if e.status.is_server_error() { e } else { missing() })?;
    Ok((s, w, p))
}
pub(crate) fn allowed(action: &Action) -> bool {
    match action {
        Action::Connect | Action::Release { .. } => true,
        Action::Acquire { level, .. } => *level == ControlLevel::Sovereign,
        Action::Tick { input } => {
            input.envelope.payload.is_empty() && input.install.is_none() && input.recovery.is_none()
        }
        _ => false,
    }
}
fn view(g: &EditorGrant) -> Value {
    json!({"principal_id":g.principal_id.to_string(),"grant_id":g.grant_id.to_string(),"active":g.active,"purpose":"general_rule_drive","consent":"general-installed-rule-drive/1"})
}
pub async fn list(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let (_, w) = general_host::permit(&app, &headers, &id, true).await?;
    Ok(general_host::private(
        json!({"world_id":id,"grants":app.store().editor_grants(w.world_id).await?.iter().filter(|g|g.purpose==PURPOSE).map(view).collect::<Vec<_>>()}),
    ))
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    principal_id: String,
    expected_grant_id: Option<WireUuid>,
    active: bool,
    consent: String,
}
pub async fn change(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<Change>,
) -> ApiResult<Response> {
    let (_, w) = general_host::permit(&app, &headers, &id, true).await?;
    if b.consent != "general-installed-rule-drive/1" {
        return Err(ApiError::bad_request(
            "须明确授权最小身体观察与已有规则逐Tick驱动，沿用原世界费用；不授予源码/安装/归档",
        ));
    }
    let pid = b
        .principal_id
        .parse::<u64>()
        .ok()
        .filter(|p| *p > 0 && p.to_string() == b.principal_id)
        .ok_or_else(|| ApiError::bad_request("主体须精确u64十进制字符串"))?;
    if !b.active && b.expected_grant_id.is_none() {
        return Err(crate::transactions::conflict("撤销须指定当前目的准入版本"));
    }
    let g = EditorGrant {
        world_id: w.world_id,
        principal_id: pid,
        grant_id: new_uuid_v7(),
        active: b.active,
        purpose: PURPOSE,
    };
    app.store()
        .change_editor(w.owner_principal_id, g.clone(), b.expected_grant_id)
        .await?;
    app.audit(
        Some(w.owner_principal_id),
        "world.general.admission",
        format!("world:{}", w.world_id),
        view(&g),
    )
    .await;
    Ok(general_host::private(view(&g)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DriverRequest {
    grant_id: WireUuid,
    #[serde(default)]
    frame: bool,
    command: Option<Action>,
}
async fn drive_inner(app: AppState, headers: HeaderMap, id: String, body: Bytes) -> ApiResult<Response> {
    if body.len() > 64 * 1024 {
        return Err(ApiError::bad_request("驱动请求超64KiB"));
    }
    let b: DriverRequest = crate::api::parse_json(&body)?;
    match (b.frame, b.command) {
        (true, None) => {
            let (_, w, _) = admission(&app, &headers, &id, b.grant_id, false).await?;
            let (world, _) = general_host::restore(&app, &w).await?;
            admission(&app, &headers, &id, b.grant_id, false).await?;
            Ok(general_host::private(
                json!({"world_id":id,"grant_id":b.grant_id.to_string(),"frame":rynvek_engine::general::GeneralRenderFrame::capture(&world)}),
            ))
        }
        (false, Some(command)) => general_host::control_request(app, headers, id, command, Some(b.grant_id)).await,
        _ => Err(ApiError::bad_request("仅可读取最小帧或提交一个明确动作")),
    }
}
/// 成败均不缓存；错误保留稳定码但不向协作者暴露私有包/恢复细节。
pub async fn drive(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>, body: Bytes) -> Response {
    match drive_inner(app, headers, id, body).await {
        Ok(r) => r,
        Err(mut e) => {
            e.message = "规则驱动未确认；请刷新并由所有者检查目的准入/租约/费用或恢复，不自动重试".into();
            let mut r = e.into_response();
            r.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("private, no-store"),
            );
            r.headers_mut().insert(
                axum::http::header::VARY,
                axum::http::HeaderValue::from_static("Authorization"),
            );
            r
        }
    }
}
