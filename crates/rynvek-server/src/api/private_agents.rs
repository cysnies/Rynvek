// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 私有Agent发布/发现/下载/撤销及只生成候选工具的受限求值；永不自动取得控制权或执行底层命令。
use crate::{
    agent_package as agent,
    app::AppState,
    error::{ApiError, ApiResult},
    ids::{hex, parse_hex},
    store::PrivateArtifactRow,
    transactions::conflict,
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use rynvek_protocol::CapabilityScope;
use rynvek_security::Visibility;
use serde::Deserialize;
use serde_json::{Value, json};
fn private(value: Value) -> Response {
    (
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::VARY, "Authorization"),
        ],
        Json(value),
    )
        .into_response()
}
fn metadata(row: &PrivateArtifactRow) -> Value {
    json!({"owner_id":row.owner_id.to_string(),"package_id":row.package_id,"version":row.version,"artifact_cid":hex(&row.cid),"publisher_key":hex(&row.publisher_key),"signature":hex(&row.signature),"profile":row.profile,"dependencies":row.dependencies,"verification":row.verification,"revoked":row.revoked,"visibility":"Private"})
}
async fn row(app: &AppState, pid: u64, id: String, version: String) -> ApiResult<PrivateArtifactRow> {
    app.store()
        .private_agent(pid, id, version)
        .await?
        .ok_or_else(|| ApiError::not_found("私有Agent版本不存在"))
}
async fn unchanged(app: &AppState, root: &PrivateArtifactRow, pins: &[PrivateArtifactRow]) -> ApiResult<()> {
    for old in std::iter::once(root).chain(pins) {
        let current = row(app, old.owner_id, old.package_id.clone(), old.version.clone()).await?;
        if current.revoked || current.cid != old.cid {
            return Err(agent::reject("Agent或精确依赖已撤销/改变"));
        }
    }
    Ok(())
}
pub async fn list(State(app): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldRead).await?;
    let rows = app.store().private_agents(subject.principal_id).await?;
    Ok(private(
        json!({"packages":rows.iter().map(metadata).collect::<Vec<_>>(),"visibility":"Private"}),
    ))
}
pub async fn get(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldRead).await?;
    Ok(private(metadata(&row(&app, subject.principal_id, id, version).await?)))
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publish {
    artifact_b64: String,
    signature: String,
}
pub async fn publish(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::HubPublish).await?;
    if body.len() > 512 << 10 {
        return Err(agent::reject("Agent上传请求超过512KiB"));
    }
    let req: Publish = super::parse_json(&body)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&req.artifact_b64)
        .map_err(|_| agent::reject("制品Base64非法"))?;
    let package = agent::parse(&bytes)?;
    if package.id != id || package.version != version {
        return Err(agent::reject("签名容器与私有发布路径不一致"));
    }
    let signature: [u8; 64] = parse_hex(&req.signature).ok_or_else(|| agent::reject("发布签名必须64字节hex"))?;
    let publisher = app
        .store()
        .get_principal(subject.principal_id)
        .await?
        .ok_or_else(|| ApiError::unauthenticated("发布身份已失效"))?;
    rynvek_security::verify_ed25519(
        &publisher.ed25519_pubkey,
        &agent::signing_message(subject.principal_id, &id, &version, &bytes),
        &signature,
    )
    .map_err(ApiError::from)?;
    if app
        .store()
        .private_agent(subject.principal_id, id.clone(), version.clone())
        .await?
        .is_some()
    {
        return Err(conflict("私有版本不可覆盖，包括撤销墓碑"));
    }
    if app.store().private_agents(subject.principal_id).await?.len() >= 256 {
        return Err(conflict("私有版本配额已满"));
    }
    let _permit = app
        .inner
        .agent_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| conflict("Agent扫描/执行名额已满，请稍后显式重试"))?;
    let resolved = agent::resolve(&app, subject.principal_id, package).await?;
    let pins = resolved.pins.clone();
    let profile = serde_json::to_value(&resolved.package.profile).unwrap();
    let report = tokio::task::spawn_blocking(move || {
        let _permit = _permit;
        agent::scan(&resolved)
    })
    .await
    .map_err(|_| ApiError::internal("Agent扫描任务失败"))??;
    app.require(&headers, CapabilityScope::HubPublish).await?;
    let cid = app.inner.world_snapshots.put(&bytes)?;
    let row = PrivateArtifactRow {
        owner_id: subject.principal_id,
        package_id: id.clone(),
        version: version.clone(),
        cid: cid.0,
        signature: signature.to_vec(),
        publisher_key: publisher.ed25519_pubkey,
        profile,
        dependencies: pins
            .iter()
            .map(|p| agent::AgentDependency {
                id: p.package_id.clone(),
                version: p.version.clone(),
                blake3_256: hex(&p.cid),
            })
            .collect(),
        verification: report,
        revoked: false,
    };
    app.store().put_private_agent(row.clone()).await?;
    app.audit(
        Some(subject.principal_id),
        "private_agent.publish",
        format!("agent:{id}@{version}"),
        json!({"visibility":"Private"}),
    )
    .await;
    let mut response = private(metadata(&row));
    *response.status_mut() = StatusCode::CREATED;
    Ok(response)
}
pub async fn artifact(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldRead).await?;
    let row = row(&app, subject.principal_id, id, version).await?;
    let bytes = agent::load_artifact(&app, &row)?;
    Ok((
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::VARY, "Authorization"),
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
}
pub async fn revoke(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::HubPublish).await?;
    if !app
        .store()
        .revoke_private_agent(subject.principal_id, id.clone(), version.clone())
        .await?
    {
        return Err(ApiError::not_found("私有版本不存在"));
    }
    app.audit(
        Some(subject.principal_id),
        "private_agent.revoke",
        format!("agent:{id}@{version}"),
        json!({}),
    )
    .await;
    Ok(private(json!({"status":"revoked"})))
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evaluate {
    world_id: String,
    artifact_cid: String,
    memory: Value,
    last_result: Value,
}
pub async fn evaluate(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldConnect).await?;
    app.require(&headers, CapabilityScope::MatchPlay).await?;
    app.require(&headers, CapabilityScope::WorldRead).await?;
    if body.len() > 32 << 10 {
        return Err(agent::reject("Agent步请求超过32KiB"));
    }
    let req: Evaluate = super::parse_json(&body)?;
    if !req.memory.is_object()
        || req.memory.to_string().len() > agent::MAX_MEMORY
        || req.last_result.to_string().len() > 8192
    {
        return Err(agent::reject("memory/last_result结构或大小超限"));
    }
    agent::credential_free(&req.memory.to_string())?;
    agent::credential_free(&req.last_result.to_string())?;
    let world = super::worlds::load_world(&app, &subject, &req.world_id, CapabilityScope::WorldRead).await?;
    if world.visibility != Visibility::Private
        || world.owner_principal_id != subject.principal_id
        || world.game_mode != "PvE_Survival"
    {
        return Err(agent::reject(
            "当前Agent包只允许所有者显式用于私有单人PvE，不代打Human排位/多人",
        ));
    }
    let host = app
        .registry()
        .by_world(&world.world_id)
        .filter(|h| !h.status().state.is_terminal())
        .ok_or_else(|| conflict("原世界Actor未运行；Agent求值不自动开局或认领"))?;
    let faction = *host
        .seats
        .get(&subject.principal_id)
        .ok_or_else(|| agent::reject("不是本人控制席位"))?;
    if host.status().state != crate::store::MatchState::Running || host.fencing_token != Some(world.fencing_token) {
        return Err(conflict("宿主未运行或fence过期"));
    }
    let root = row(&app, subject.principal_id, id, version).await?;
    if hex(&root.cid) != req.artifact_cid {
        return Err(agent::reject("必须使用显式锁定的Agent制品CID"));
    }
    let package = agent::parse(&agent::load_artifact(&app, &root)?)?;
    if package.profile.role != "agent" {
        return Err(agent::reject("library不能作为Agent直接运行"));
    }
    let _permit = app
        .inner
        .agent_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| conflict("Agent执行名额已满"))?;
    let resolved = agent::resolve(&app, subject.principal_id, package).await?;
    let pins = resolved.pins.clone();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), host.observe_faction(faction))
        .await
        .map_err(|_| conflict("原Actor观测超时"))??;
    let observation = frame["observation"]["observation"].clone();
    if observation.is_null() || observation.to_string().len() > 256 << 10 {
        return Err(agent::reject("真实观测缺失或超过当前Agent输入上限"));
    }
    let tick = observation["tick"].clone();
    let input = json!({"observation":observation,"memory":req.memory,"last_result":req.last_result});
    let (output, gas) = tokio::task::spawn_blocking(move || {
        let _permit = _permit;
        let sandbox = agent::sandbox(&resolved.source)?;
        agent::output(&sandbox, &resolved.package.profile, &input)
    })
    .await
    .map_err(|_| ApiError::internal("Agent运行任务失败"))??;
    app.require(&headers, CapabilityScope::WorldConnect).await?;
    unchanged(&app, &root, &pins).await?;
    let latest = super::worlds::load_world(&app, &subject, &req.world_id, CapabilityScope::WorldRead).await?;
    if latest.game_mode != world.game_mode
        || latest.fencing_token != world.fencing_token
        || latest.visibility != Visibility::Private
        || latest.owner_principal_id != subject.principal_id
        || host.status().state.is_terminal()
    {
        return Err(conflict("执行期间世界/授权已改变，丢弃候选"));
    }
    Ok(private(
        json!({"package_id":root.package_id,"version":root.version,"artifact_cid":hex(&root.cid),"world_id":world.world_id.to_string(),"principal_id":subject.principal_id.to_string(),"faction":faction,"tick":tick,"output":output,"gas_used":gas,"executed":false}),
    ))
}
