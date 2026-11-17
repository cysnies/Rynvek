// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 受控Agent生命周期/上下文/派发；模型/私有包求值在权威Tick之外，真实工具仍经原Actor。
use crate::{
    agent_package as agent,
    agent_runs::{AgentRun, Budget, Change, Control, Proposal},
    app::AppState,
    error::{ApiError, ApiResult},
    ids::{hex, unix_now_s},
    lab_jobs::{conflict, uuid_text},
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use rynvek_protocol::CapabilityScope;
use rynvek_security::{Subject, Visibility};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn private<T: serde::Serialize>(value: T) -> Response {
    (
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::VARY, "Authorization"),
        ],
        Json(value),
    )
        .into_response()
}
async fn subject(app: &AppState, h: &HeaderMap) -> ApiResult<Subject> {
    let s = app.require(h, CapabilityScope::WorldRead).await?;
    app.require(h, CapabilityScope::WorldConnect).await?;
    app.require(h, CapabilityScope::MatchPlay).await?;
    Ok(s)
}
async fn owned(app: &AppState, pid: u64, id: &str) -> ApiResult<AgentRun> {
    app.store()
        .agent_run_get(pid, uuid_text(id)?)
        .await?
        .ok_or_else(|| ApiError::not_found("运行不存在"))
}
async fn host(app: &AppState, s: &Subject, world_id: &str) -> ApiResult<(crate::host::MatchHandle, u64, u8)> {
    let world = super::worlds::load_world(app, s, world_id, CapabilityScope::WorldRead).await?;
    if world.visibility != Visibility::Private
        || world.owner_principal_id != s.principal_id
        || world.game_mode != "PvE_Survival"
    {
        return Err(agent::reject("仅所有者明确授权的Private单人PvE，不代打Human排位/多人"));
    }
    let host = app
        .registry()
        .by_world(&world.world_id)
        .ok_or_else(|| conflict("先显式连接已有世界；Agent不自动开局/取得控制"))?;
    if host.seats.len() != 1 {
        return Err(agent::reject("受控运行仅开放本人单人PvE，不扩展到多人席位"));
    }
    let faction = *host
        .seats
        .get(&s.principal_id)
        .ok_or_else(|| agent::reject("不是本人席位"))?;
    if host.status().state != crate::store::MatchState::Running || host.fencing_token != Some(world.fencing_token) {
        return Err(conflict("世界宿主未运行或已过期"));
    }
    Ok((host, world.fencing_token, faction))
}
async fn pins(app: &AppState, pid: u64, run: &AgentRun) -> ApiResult<()> {
    for (id, version, cid) in std::iter::once((&run.package_id, &run.version, &run.artifact_cid))
        .chain(run.pins.iter().map(|p| (&p.id, &p.version, &p.blake3_256)))
    {
        let row = app
            .store()
            .private_agent(pid, id.clone(), version.clone())
            .await?
            .ok_or_else(|| agent::reject("私有包/依赖不存在"))?;
        if row.revoked || hex(&row.cid) != *cid {
            return Err(agent::reject("私有包/依赖已撤销或CID变化"));
        }
    }
    Ok(())
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    submission_id: String,
    world_id: String,
    package_id: String,
    version: String,
    artifact_cid: String,
    control_fence: String,
    tools: Vec<String>,
    budget: Budget,
    consent: String,
}
pub async fn create(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    let s = subject(&app, &headers).await?;
    if body.len() > 8192 {
        return Err(ApiError::bad_request("运行创建请求过大"));
    }
    let req: Create = super::parse_json(&body)?;
    if req.consent != "private-pve-tools/1" || req.control_fence.parse::<u64>().is_err() {
        return Err(agent::reject("必须显式同意所选私有包/工具/预算并提供本人控制fence"));
    }
    let (actor, fence, faction) = host(&app, &s, &req.world_id).await?;
    let root = app
        .store()
        .private_agent(s.principal_id, req.package_id.clone(), req.version.clone())
        .await?
        .ok_or_else(|| ApiError::not_found("私有包不存在"))?;
    if root.revoked || hex(&root.cid) != req.artifact_cid {
        return Err(agent::reject("须锁定未撤销的精确私有制品"));
    }
    let package = agent::parse(&agent::load_artifact(&app, &root)?)?;
    if package.profile.role != "agent"
        || req.tools.is_empty()
        || req.tools.len() > 6
        || req.tools.iter().any(|t| !package.profile.tools.contains(t))
        || req.tools.iter().collect::<std::collections::BTreeSet<_>>().len() != req.tools.len()
    {
        return Err(agent::reject("授权工具必须是包声明中的非空唯一子集"));
    }
    let resolved = agent::resolve(&app, s.principal_id, package).await?;
    let row = AgentRun::new(AgentRun {
        run_id: String::new(),
        owner_id: s.principal_id.to_string(),
        submission_id: req.submission_id,
        spec_digest: String::new(),
        world_id: actor.world_id.to_string(),
        host_fence: fence.to_string(),
        control_fence: req.control_fence,
        faction,
        package_id: req.package_id,
        version: req.version,
        artifact_cid: req.artifact_cid,
        pins: resolved
            .pins
            .iter()
            .map(|p| agent::AgentDependency {
                id: p.package_id.clone(),
                version: p.version.clone(),
                blake3_256: hex(&p.cid),
            })
            .collect(),
        tools: req.tools,
        budget: req.budget,
        turns_used: 0,
        calls_used: 0,
        created_at_unix_s: 0,
        deadline_unix_s: 0,
        revision: 0,
        fence: 0,
        status: "ready".into(),
        lease: None,
        pending: None,
        memory: json!({}),
        last_result: Value::Null,
        events: Vec::new(),
        operations: BTreeMap::new(),
    })?;
    Ok(private(app.store().agent_run_create(row).await?))
}
pub async fn list(State(app): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let s = subject(&app, &headers).await?;
    Ok(private(
        json!({"runs":app.store().agent_run_list(s.principal_id).await?}),
    ))
}
pub async fn get(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let s = subject(&app, &headers).await?;
    Ok(private(owned(&app, s.principal_id, &id).await?))
}
pub async fn control(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    let s = subject(&app, &headers).await?;
    if body.len() > 4096 {
        return Err(ApiError::bad_request("运行控制请求超过4KiB"));
    }
    let op: Control = super::parse_json(&body)?;
    if op.action == "claim" || op.action == "resume" {
        let run = owned(&app, s.principal_id, &id).await?;
        pins(&app, s.principal_id, &run).await?;
        let (_, fence, _) = host(&app, &s, &run.world_id).await?;
        if op.action == "resume" {
            if op.host_fence.as_deref() != Some(&fence.to_string())
                || op.control_fence.as_ref().is_none_or(|v| v.parse::<u64>().is_err())
            {
                return Err(conflict("续跑须显式确认当前host_fence与本人新control_fence"));
            }
        } else if run.host_fence != fence.to_string() {
            return Err(conflict("宿主fence已变化，先recover/pause再显式续跑新授权"));
        }
    }
    Ok(private(
        app.store()
            .agent_run_change(s.principal_id, uuid_text(&id)?, Change::Control(op))
            .await?
            .0,
    ))
}
pub async fn context(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let s = subject(&app, &headers).await?;
    let run = owned(&app, s.principal_id, &id).await?;
    pins(&app, s.principal_id, &run).await?;
    let (actor, fence, faction) = host(&app, &s, &run.world_id).await?;
    let execution_authorization_valid = run.host_fence == fence.to_string();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), actor.observe_faction(faction))
        .await
        .map_err(|_| conflict("观测超时"))??;
    let observation = frame["observation"]["observation"].clone();
    if observation.is_null() || observation.to_string().len() > 256 << 10 {
        return Err(conflict("授权fog观测缺失或过大"));
    }
    let schemas = rynvek_agent::schema::ToolRegistry::v1();
    let tools: Vec<_> = run.tools.iter().filter_map(|t| schemas.get(t)).cloned().collect();
    Ok(private(
        json!({"run":run,"observation":observation,"tools":tools,"external_provider_configured":false,"current_host_fence":fence.to_string(),"execution_authorization_valid":execution_authorization_valid}),
    ))
}
pub async fn step(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    let s = subject(&app, &headers).await?;
    if body.len() > 32 << 10 {
        return Err(ApiError::bad_request("候选超过32KiB"));
    }
    let mut proposal: Proposal = super::parse_json(&body)?;
    proposal.operation_id = uuid_text(&proposal.operation_id)?;
    let id = uuid_text(&id)?;
    let before = owned(&app, s.principal_id, &id).await?;
    // Store在单事务内返回是否首次接受；并发重放也只能有一个真正派发者。
    let (run, changed) = app
        .store()
        .agent_run_change(s.principal_id, uuid_text(&id)?, Change::Prepare(proposal.clone()))
        .await?;
    if !changed || proposal.call.is_none() {
        return Ok(private(run));
    }
    let pending = run.pending.as_ref().expect("刚登记的工具意图");
    let operation_id = pending.operation_id.clone();
    let outcome = async {
        app.require(&headers, CapabilityScope::MatchPlay).await?;
        pins(&app, s.principal_id, &before).await?;
        let (actor, fence, _) = host(&app, &s, &before.world_id).await?;
        if before.host_fence != fence.to_string() {
            return Err(conflict("宿主fence变化，未派发"));
        }
        let latest = owned(&app, s.principal_id, &id).await?;
        latest.controller(&proposal.controller_id, proposal.fence, unix_now_s())?;
        let call = &pending.call;
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            actor.agent_tool(
                s.principal_id,
                pending.command_id.clone(),
                before.control_fence.parse().map_err(|_| conflict("控制fence非法"))?,
                call.name.clone(),
                call.arguments.clone(),
            ),
        )
        .await;
        Ok(match response {
            Ok(Ok(response)) => json!({"kind":"broker_result","response":response}),
            Ok(Err(crate::host::AgentToolError::Rejected(error))) => {
                json!({"kind":"actor_rejected","error":error.body()})
            }
            Ok(Err(crate::host::AgentToolError::Unknown)) | Err(_) => {
                json!({"kind":"unknown","reason":"原Actor回执超时，命令可能已执行；禁止自动重试"})
            }
        })
    }
    .await;
    let outcome = outcome.unwrap_or_else(|error: ApiError| json!({"kind":"not_dispatched","error":error.body()}));
    let final_run = app
        .store()
        .agent_run_change(s.principal_id, id, Change::Result { operation_id, outcome })
        .await?
        .0;
    Ok(private(final_run))
}
