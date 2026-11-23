// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 通用World领域Actor：复用正式身份/ABAC、CommandGate、LeaseManager与原TickCommitter/Store。
//! Private所有者与显式目的隔离的规则驱动准入共享原Actor；协作者仅见最小帧，无全知观察。
use crate::{
    app::{AppInner, AppState},
    error::{ApiError, ApiResult},
    ids::{hex, new_uuid_v7},
    ranked::HostClaim,
    store::{AdmissionPurpose, CommitRow, EditorPermit, WorldRow},
    transactions::{CHECKPOINT_INTERVAL, TickCommitter, conflict},
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use rynvek_agent::{ApmLimiter, ControlLevel, LeaseManager, Principal, PrincipalKind};
use rynvek_core::{Blake3Hash, RynvekErrorCode};
use rynvek_journal::CommitManifest;
use rynvek_protocol::{CapabilityScope, CommandEnvelope, IdempotencyKey, WireUuid};
use rynvek_security::{CommandGate, CommandGateConfig, PrincipalKindCode, Resource, Subject, Visibility, authorize};
use rynvek_simulation::{engine::SimulationEngine, physics::PhysicsCommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Weak, time::Duration};
use tokio::sync::{mpsc, oneshot};

const DOMAIN: u32 = 0; // 原租约表的控制域键，不创建仿真阵营。
const RETENTION: u64 = 1800;
fn has_controlled_rules(world: &SimulationEngine) -> bool {
    world
        .component_rules()
        .is_some_and(|r| !r.config.rule_graphs.is_empty() || !r.config.tier1_rules.is_empty())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralInput {
    pub host_fence: u64,
    pub control_fence: u64,
    pub envelope: CommandEnvelope<Vec<PhysicsCommand>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<crate::rule_package::RuleInstallRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<crate::rule_package::RuleInstallRequest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralTickRecord {
    pub format: String,
    pub base_revision: u64,
    pub base_root: [u8; 32],
    pub input: GeneralInput,
    /// 原RYC2 CommitManifest编码，不另起领域根算法。
    pub commit_bytes: Vec<u8>,
    /// 历史准入版本仅作出处与幂等绑定，不作为恢复后的当前授权。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission_grant_id: Option<WireUuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_step: Option<crate::general_runs::RunStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation: Option<crate::rule_package::RuleInstallReplay>,
}
impl GeneralTickRecord {
    fn valid_format(&self) -> bool {
        if let Some(r) = &self.run_step {
            return self.format == "rynvek-general-tick/5"
                && self.admission_grant_id.is_none()
                && r.step > 0
                && r.job_fence > 0
                && crate::general_runs::GeneralRun::step_key(&r.run_id, r.step)
                    .is_ok_and(|k| k == self.input.envelope.idempotency_key)
                && self.input.install.is_none()
                && self.input.recovery.is_none()
                && self.installation.is_none()
                && self.input.envelope.payload.is_empty();
        }
        // 新日志显式区分历史委托出处；不是用当前准入重新授权过去已提交的Tick。
        if self.admission_grant_id.is_some() {
            return self.format == "rynvek-general-tick/4"
                && self.input.install.is_none()
                && self.input.recovery.is_none()
                && self.installation.is_none()
                && self.input.envelope.payload.is_empty();
        }
        match (&self.input.install, &self.input.recovery, &self.installation) {
            (None, None, None) => self.format == "rynvek-general-tick/1",
            (Some(_), None, Some(_)) => {
                self.format == "rynvek-general-tick/2"
                    && self.input.envelope.target_tick % 4 == 0
                    && self.input.envelope.payload.is_empty()
            }
            (None, Some(_), Some(_)) => {
                self.format == "rynvek-general-tick/3"
                    && self.input.envelope.target_tick % 4 != 0
                    && self.input.envelope.payload.is_empty()
            }
            _ => false,
        }
    }
    pub fn replay(&self, world: &SimulationEngine) -> ApiResult<SimulationEngine> {
        let next = world.clock.tick.checked_add(1).ok_or_else(|| conflict("Tick溢出"))?;
        if !self.valid_format()
            || ((self.admission_grant_id.is_some() || self.run_step.is_some()) && !has_controlled_rules(world))
            || self.base_revision != world.clock.world_revision
            || self.base_root != world.parent_commit_hash
            || self.input.envelope.target_tick != next
            || self.input.envelope.issued_at_tick != world.clock.tick
            || self.input.envelope.payload.len() > 32
        {
            return Err(conflict("通用Tick日志基线/格式/时序不符"));
        }
        if world.life().is_some()
            && self
                .input
                .envelope
                .payload
                .iter()
                .any(|c| !matches!(c, PhysicsCommand::Despawn { .. }))
        {
            return Err(conflict("生命日志含无来源外部命令"));
        }
        let manifest = CommitManifest::decode(&self.commit_bytes).map_err(ApiError::from)?;
        let mut candidate = if let Some(plan) = &self.installation {
            if self.input.recovery.is_some() {
                world.candidate_with_rule_recovery(plan.evidence_cid, &plan.graphs)
            } else {
                world.candidate_with_rule_install(plan.evidence_cid, &plan.graphs)
            }
            .map_err(ApiError::from)?
        } else {
            world.clone()
        };
        candidate
            .tick_with_commands(&self.input.envelope.payload)
            .map_err(|e| ApiError::bad_request(format!("通用Tick拒绝：{e:?}")))?;
        if candidate.authoritative_commit(self.base_root) != manifest {
            return Err(conflict("通用Tick逐领域根不符"));
        }
        Ok(candidate)
    }
}
fn meta(w: &SimulationEngine) -> Value {
    json!({"tick":w.clock.tick,"revision":w.clock.world_revision,"state_root":hex(&w.parent_commit_hash)})
}
fn observation(w: &SimulationEngine) -> Value {
    let bodies: Vec<_> = w
        .physics()
        .iter()
        .map(|(serial, body)| json!({"serial":serial,"body":body}))
        .collect();
    json!({"world_id":WireUuid(w.world_id).to_string(),"commit":meta(w),"bodies":bodies,
        "components":w.component_rules().map(|r| &r.components),
        "installed_releases":w.component_rules().map(|r| &r.installed_releases),
        "rule_recovery":w.component_rules().and_then(|r|r.recovery.as_ref()),
        "recovery_maintenance_q_per_body":w.component_rules().and_then(|r|r.config.rule_cost_micro_per_gas).map(|p|u64::from(p)*4),
        "life":w.life().map(|l|json!({"genesis_energy_q":l.genesis_energy_q().to_string(),"universe_energy_q":rynvek_simulation::life::universe_energy_q(l,&w.field).ok().map(|v|v.to_string()),"accounts":l.accounts().collect::<Vec<_>>()})),
        "rule_meter":w.component_rules().map(|r|json!({"tier0_gas":r.tier0_gas,"tier1_quota":r.tier1_quota,"paid_q":r.paid_q})),
        "visibility":"private_owner_only","execution":"server-authoritative-single-chunk"})
}
pub(crate) fn private(value: Value) -> Response {
    (
        [
            (axum::http::header::CACHE_CONTROL, "private, no-store"),
            (axum::http::header::VARY, "Authorization"),
        ],
        Json(value),
    )
        .into_response()
}
pub(crate) async fn permit(
    app: &AppState,
    headers: &HeaderMap,
    id: &str,
    write: bool,
) -> ApiResult<(Subject, WorldRow)> {
    let subject = app.require(headers, CapabilityScope::WorldConnect).await?;
    let row = crate::api::worlds::load_world(app, &subject, id, CapabilityScope::WorldConnect).await?;
    if !row.is_controlled_general() || row.owner_principal_id != subject.principal_id {
        return Err(ApiError::not_found("通用World不存在"));
    }
    if write {
        let s = app.require(headers, CapabilityScope::WorldEdit).await?;
        authorize(
            &s,
            CapabilityScope::WorldEdit,
            &Resource::World {
                owner_principal_id: row.owner_principal_id,
                org_id: row.org_id,
                visibility: row.visibility,
            },
        )
        .map_err(ApiError::from)?;
    }
    app.store()
        .check_editor(
            row.world_id,
            EditorPermit {
                purpose: AdmissionPurpose::GeneralRuleDrive,
                principal_id: subject.principal_id,
                grant_id: None,
            },
        )
        .await?;
    Ok((subject, row))
}
/// 新受限服务器入口，仍写原WorldRow/首Commit及私有CAS，复用个人组织配额/正式WorldCreate。
pub async fn create(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldCreate).await?;
    authorize(
        &subject,
        CapabilityScope::WorldCreate,
        &Resource::Org {
            org_id: subject.principal_id,
        },
    )
    .map_err(ApiError::from)?;
    let config = rynvek_world::general::GeneralWorldConfig::from_package_bytes(&body)
        .map_err(|e| ApiError::new(e.code, e.detail))?;
    let id = new_uuid_v7();
    let world = config.instantiate(id.0).map_err(ApiError::bad_request)?;
    let package = app.inner.world_snapshots.put(&body)?;
    let genesis = app.inner.world_snapshots.put(
        &serde_json::to_vec(
            &json!({"format":"rynvek-general-genesis/1","package_cid":package.to_hex(),"config":config}),
        )
        .expect("定型配置"),
    )?;
    let snapshot = app
        .inner
        .world_snapshots
        .put(&world.encode_snapshot().map_err(ApiError::from)?)?;
    let row = WorldRow {
        world_id: id,
        org_id: subject.principal_id,
        owner_principal_id: subject.principal_id,
        title: "通用受控World".into(),
        world_profile: "General_Circle_v1".into(),
        game_mode: "General_Controlled".into(),
        execution_profile: "D0_Strict".into(),
        visibility: Visibility::Private,
        parent_world_id: None,
        forked_at_revision: None,
        latest_tick: 0,
        latest_revision: 0,
        latest_state_root: world.parent_commit_hash,
        lease_holder_node: None,
        fencing_token: 1,
    };
    app.store()
        .insert_world(
            row,
            CommitRow {
                world_id: id,
                world_revision: 0,
                tick: 0,
                parent_revision: None,
                state_root_blake3: world.parent_commit_hash,
                journal_segment_cid: genesis.0,
                snapshot_cid: Some(snapshot.0),
            },
        )
        .await?;
    Ok((StatusCode::CREATED,private(json!({"world_id":id.to_string(),"commit":meta(&world),"visibility":"Private","profile":"General_Circle_v1"}))).into_response())
}
async fn checked_blob(app: &AppState, cid: [u8; 32], max: usize) -> ApiResult<Vec<u8>> {
    let bytes = app.inner.world_snapshots.get(&Blake3Hash(cid))?;
    if bytes.len() > max || Blake3Hash::of(&bytes).0 != cid {
        return Err(conflict("私有CAS大小/摘要不符"));
    }
    Ok(bytes)
}
/// 原私有Commit链+最近完整锚点；至多300步重仿真，幂等回执只回看原1800Tick窗口。
pub(crate) async fn restore(
    app: &AppState,
    row: &WorldRow,
) -> ApiResult<(SimulationEngine, BTreeMap<IdempotencyKey, GeneralTickRecord>)> {
    let _restore_slot = app
        .inner
        .agent_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| conflict("私有恢复复验名额已满"))?;
    let commits = app.store().list_commits(row.world_id).await?;
    let first = commits.first().ok_or_else(|| conflict("缺提交"))?;
    if first.world_id != row.world_id || first.world_revision != 0 || first.tick != 0 || first.parent_revision.is_some()
    {
        return Err(conflict("通用创世提交身份/时序不符"));
    }
    for pair in commits.windows(2) {
        if pair[1].world_id != row.world_id
            || Some(pair[1].world_revision) != pair[0].world_revision.checked_add(1)
            || Some(pair[1].tick) != pair[0].tick.checked_add(1)
            || pair[1].parent_revision != Some(pair[0].world_revision)
        {
            return Err(conflict("通用提交链不连续"));
        }
    }
    let last = commits.last().ok_or_else(|| conflict("缺提交"))?;
    if last.world_revision != row.latest_revision
        || last.tick != row.latest_tick
        || last.state_root_blake3 != row.latest_state_root
    {
        return Err(conflict("世界元数据与提交不符"));
    }
    let (index, anchor) = commits
        .iter()
        .enumerate()
        .rev()
        .find(|(_, c)| c.snapshot_cid.is_some())
        .ok_or_else(|| conflict("缺锚点"))?;
    if commits.len() - index - 1 > CHECKPOINT_INTERVAL as usize {
        return Err(conflict("缺周期锚点"));
    }
    let mut world =
        SimulationEngine::decode_snapshot(&checked_blob(app, anchor.snapshot_cid.unwrap(), 8 * 1024 * 1024).await?)
            .map_err(ApiError::from)?;
    if world.world_id != row.world_id.0
        || world.clock.world_revision != anchor.world_revision
        || world.clock.tick != anchor.tick
        || world.parent_commit_hash != anchor.state_root_blake3
    {
        return Err(conflict("锚点身份/根不符"));
    }
    crate::rule_package::installed_permits(app, row.owner_principal_id, &world).await?;
    let mut receipts = BTreeMap::new();
    for (n, c) in commits
        .iter()
        .enumerate()
        .filter(|(_, c)| c.world_revision > 0 && c.tick.saturating_add(RETENTION) >= row.latest_tick)
    {
        let r: GeneralTickRecord =
            serde_json::from_slice(&checked_blob(app, c.journal_segment_cid, 2 * 1024 * 1024).await?)
                .map_err(|_| conflict("不支持通用Tick日志"))?;
        let m = CommitManifest::decode(&r.commit_bytes).map_err(ApiError::from)?;
        // 旧格式始终只认owner；/4仅允许带版本的非owner空驱动。历史已由原提交锁授权，撤销不抹历史。
        if r.input.envelope.principal_id == 0
            || (r.admission_grant_id.is_none() != (r.input.envelope.principal_id == row.owner_principal_id))
            || r.input.host_fence == 0
            || r.input.control_fence == 0
            || r.input.envelope.payload.len() > 32
            || r.input.envelope.target_tick != c.tick
            || r.input.envelope.issued_at_tick.checked_add(1) != Some(c.tick)
            || r.base_root != commits[n - 1].state_root_blake3
            || m.parent_commit_hash != r.base_root
            || !r.valid_format()
            || m.world_id != row.world_id.0
            || m.tick != c.tick
            || m.world_revision != c.world_revision
            || m.state_root_hash != c.state_root_blake3
            || Some(r.base_revision) != c.parent_revision
        {
            return Err(conflict("日志/提交元数据不符"));
        }
        if let (Some(request), Some(plan)) = (r.input.install.as_ref().or(r.input.recovery.as_ref()), &r.installation) {
            crate::rule_package::validate_replay(app, row.owner_principal_id, request, plan).await?;
        }
        if n > index {
            world = r.replay(&world)?;
        }
        let key = r.input.envelope.idempotency_key;
        if receipts.insert(key, r).is_some() {
            return Err(conflict("日志含重复幂等键"));
        }
    }
    if world.parent_commit_hash != row.latest_state_root
        || world.clock.tick != row.latest_tick
        || world.clock.world_revision != row.latest_revision
    {
        return Err(conflict("通用恢复终根不符"));
    }
    crate::rule_package::installed_permits(app, row.owner_principal_id, &world).await?;
    Ok((world, receipts))
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Connect,
    StartRun {
        host_fence: u64,
        control_fence: u64,
        spec: crate::general_runs::RunSpec,
    },
    #[serde(skip)]
    RunStep,
    Acquire {
        host_fence: u64,
        level: ControlLevel,
    },
    Release {
        host_fence: u64,
        control_fence: u64,
    },
    Tick {
        input: GeneralInput,
    },
    Install {
        input: GeneralInput,
    },
    Recover {
        input: GeneralInput,
    },
    Close {
        host_fence: u64,
        control_fence: u64,
    },
}
struct Message {
    headers: HeaderMap,
    action: Action,
    grant: Option<WireUuid>,
    run: Option<crate::lab_jobs::Claim>,
    reply: oneshot::Sender<ApiResult<Value>>,
}
#[derive(Clone, Debug)]
pub struct GeneralHandle {
    tx: mpsc::Sender<Message>,
}
impl GeneralHandle {
    fn closed(&self) -> bool {
        self.tx.is_closed()
    }
    async fn request(
        &self,
        headers: HeaderMap,
        action: Action,
        grant: Option<WireUuid>,
        run: Option<crate::lab_jobs::Claim>,
    ) -> ApiResult<Value> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .try_send(Message {
                headers,
                action,
                grant,
                run,
                reply,
            })
            .map_err(|_| conflict("通用Actor忙或已退役"))?;
        rx.await
            .map_err(|_| conflict("Actor已退役；提交结果未知，先读取耐久状态对账"))?
    }
}
struct Actor {
    world: SimulationEngine,
    committer: TickCommitter,
    leases: LeaseManager,
    gate: CommandGate,
    receipts: BTreeMap<IdempotencyKey, GeneralTickRecord>,
    apm: ApmLimiter,
    dead: bool,
    /// 控制租约还必须绑定取得时的准入版本；重新授权不能复用旧control fence。
    driver_lease: Option<(u64, WireUuid, u64)>,
    run_lease: Option<(crate::lab_jobs::Claim, u64)>,
}
impl Actor {
    fn check(&self, pid: u64, host: u64, control: u64, sovereign: bool) -> ApiResult<()> {
        if host != self.committer.fencing_token {
            return Err(ApiError::new(
                RynvekErrorCode::StalePartitionFencingToken,
                "旧宿主fence",
            ));
        }
        let l = self
            .leases
            .authorize(DOMAIN, &pid.to_string(), control, self.world.clock.tick)
            .map_err(|e| ApiError::new(e.code, e.message))?;
        if sovereign && l.level < ControlLevel::Sovereign {
            return Err(ApiError::forbidden("此动作需要Sovereign控制等级"));
        }
        Ok(())
    }
    async fn process(&mut self, app: &AppState, msg: &Message) -> ApiResult<Value> {
        let (subject, row, admission) = message_permit(
            app,
            &msg.headers,
            &self.committer.world_id.to_string(),
            msg.grant,
            msg.run.as_ref(),
        )
        .await?;
        if msg.grant.is_some() && !crate::general_collaboration::allowed(&msg.action) {
            return Err(ApiError::forbidden("准入仅可驱动已安装规则"));
        }
        if let Some((claim, fence)) = self.run_lease.clone() {
            let current = app.store().general_run_get(claim.owner, claim.id.clone()).await?;
            if current
                .as_ref()
                .is_none_or(|j| j.live(&claim, crate::ids::unix_now_s()).is_err())
            {
                self.leases
                    .release(DOMAIN, fence)
                    .map_err(|e| ApiError::new(e.code, e.message))?;
                self.leases.drain_events();
                self.gate.unbind(claim.owner);
                self.run_lease = None;
            }
        }
        if msg.run.is_none()
            && !matches!(msg.action, Action::Connect | Action::StartRun { .. })
            && app.store().general_run_reserved(row.world_id).await?
        {
            return Err(conflict("持久运行保留控制，须先暂停/取消；暂停仍保留精确起点"));
        }
        // 只清理已失效目的版本绑定的旧控制租约；原release保留递增计数，重授权不得复用fence。
        if let Some((principal_id, grant_id, fence)) = self.driver_lease {
            let p = EditorPermit {
                principal_id,
                grant_id: Some(grant_id),
                purpose: AdmissionPurpose::GeneralRuleDrive,
            };
            if let Err(e) = app.store().check_editor(row.world_id, p).await {
                if e.status.is_server_error() {
                    return Err(e);
                }
                self.leases
                    .release(DOMAIN, fence)
                    .map_err(|e| ApiError::new(e.code, e.message))?;
                self.leases.drain_events();
                self.gate.unbind(principal_id);
                self.driver_lease = None;
            }
        }
        if row.fencing_token != self.committer.fencing_token
            || !app
                .store()
                .renew_ranked_host(row.world_id, self.committer.fencing_token)
                .await?
        {
            self.dead = true;
            return Err(conflict("宿主租约已失效"));
        }
        let pid = subject.principal_id;
        let mut action = msg.action.clone();
        if let Some(c) = &msg.run {
            if !matches!(action, Action::RunStep) {
                return Err(ApiError::forbidden("后台上下文只可执行原规则Tick"));
            }
            let j = app
                .store()
                .general_run_get(c.owner, c.id.clone())
                .await?
                .ok_or_else(|| conflict("运行不存在"))?;
            j.live(c, crate::ids::unix_now_s())?;
            j.baseline(&row)?;
            if !has_controlled_rules(&self.world) {
                return Err(conflict("运行不允许无规则免费推进"));
            }
            let control = if let Some((old, token)) = &self.run_lease {
                if old != c {
                    return Err(conflict("后台控制绑定已过期"));
                }
                *token
            } else {
                let l = self
                    .leases
                    .acquire(
                        Principal::new(pid.to_string(), PrincipalKind::Human),
                        DOMAIN,
                        ControlLevel::Sovereign,
                        self.world.clock.tick,
                        900,
                    )
                    .map_err(|e| ApiError::new(e.code, e.message))?;
                self.leases.drain_events();
                self.gate.bind(pid, DOMAIN);
                self.run_lease = Some((c.clone(), l.fencing_token));
                l.fencing_token
            };
            action = Action::Tick {
                input: GeneralInput {
                    host_fence: self.committer.fencing_token,
                    control_fence: control,
                    envelope: CommandEnvelope {
                        idempotency_key: crate::general_runs::GeneralRun::step_key(&j.id, j.done + 1)?,
                        principal_id: pid,
                        issued_at_tick: self.world.clock.tick,
                        target_tick: self.world.clock.tick + 1,
                        payload: vec![],
                    },
                    install: None,
                    recovery: None,
                },
            };
        }
        match &action {
            Action::RunStep => Err(ApiError::forbidden("内部作业动作不可由HTTP提交")),
            Action::StartRun {
                host_fence,
                control_fence,
                spec,
            } => {
                spec.validate()?;
                if let Some(old) = app
                    .store()
                    .general_run_submission(pid, spec.submission_id.clone())
                    .await?
                {
                    if old.world_id != row.world_id || old.spec != *spec {
                        return Err(conflict("提交键绑定不同运行"));
                    }
                    return Ok(serde_json::to_value(old).expect("定型运行"));
                }
                self.check(pid, *host_fence, *control_fence, true)?;
                if app.store().backend_name() == "postgres" && app.config().blob_dir.is_none() {
                    return Err(conflict("PG耐久运行还需显式私有文件CAS目录，不能只持久化计数"));
                }
                if !has_controlled_rules(&self.world)
                    || self.world.life().is_none()
                    || self
                        .world
                        .component_rules()
                        .is_none_or(|r| r.config.rule_cost_micro_per_gas.is_none() || r.recovery.is_some())
                {
                    return Err(conflict(
                        "耐久运行只驱动已有有源定价规则，拒绝空推进/维护/CPU配额冒充能量",
                    ));
                }
                let j = crate::general_runs::GeneralRun::new(&row, spec.clone(), crate::ids::unix_now_s())?;
                let j = app.store().general_run_submit(j, self.committer.fencing_token).await?;
                self.leases
                    .release(DOMAIN, *control_fence)
                    .map_err(|e| ApiError::new(e.code, e.message))?;
                self.leases.drain_events();
                self.gate.unbind(pid);
                self.driver_lease = None;
                app.audit(Some(pid),"world.general.run.start",j.id.clone(),json!({"world_id":row.world_id,"ticks":j.spec.ticks,"max_energy_q":j.spec.max_energy_q,"deadline_s":j.deadline_s})).await;
                Ok(serde_json::to_value(j).expect("定型运行"))
            }
            Action::Connect => Ok(if msg.grant.is_some() {
                json!({"host_fence":self.committer.fencing_token,"frame":rynvek_engine::general::GeneralRenderFrame::capture(&self.world)})
            } else {
                json!({"host_fence":self.committer.fencing_token,"observation":observation(&self.world)})
            }),
            Action::Acquire { host_fence, level } => {
                if *host_fence != self.committer.fencing_token {
                    return Err(conflict("宿主fence不符"));
                }
                let kind = match subject.kind {
                    PrincipalKindCode::Human => PrincipalKind::Human,
                    PrincipalKindCode::Agent => PrincipalKind::Agent,
                    _ => PrincipalKind::Script,
                };
                let lease = self
                    .leases
                    .acquire(
                        Principal::new(pid.to_string(), kind),
                        DOMAIN,
                        *level,
                        self.world.clock.tick,
                        900,
                    )
                    .map_err(|e| ApiError::new(e.code, e.message))?;
                self.leases.drain_events();
                self.driver_lease = msg.grant.map(|g| (pid, g, lease.fencing_token));
                self.gate.bind(pid, DOMAIN);
                Ok(json!({"host_fence":self.committer.fencing_token,"lease":lease}))
            }
            Action::Release {
                host_fence,
                control_fence,
            }
            | Action::Close {
                host_fence,
                control_fence,
            } => {
                self.check(pid, *host_fence, *control_fence, matches!(action, Action::Close { .. }))?;
                self.leases
                    .release(DOMAIN, *control_fence)
                    .map_err(|e| ApiError::new(e.code, e.message))?;
                self.leases.drain_events();
                self.gate.unbind(pid);
                self.driver_lease = None;
                if matches!(action, Action::Close { .. }) {
                    self.dead = true;
                }
                Ok(json!({"released":true,"closed":self.dead,"commit":meta(&self.world)}))
            }
            Action::Tick { input } | Action::Install { input } | Action::Recover { input } => {
                if matches!(action, Action::Install { .. }) != input.install.is_some()
                    || matches!(action, Action::Recover { .. }) != input.recovery.is_some()
                {
                    return Err(ApiError::bad_request(
                        "安装/撤销隔离维护必须分别显式请求，不可普通Tick偷带",
                    ));
                }
                self.check(pid, input.host_fence, input.control_fence, true)?;
                if input.envelope.principal_id != pid {
                    return Err(ApiError::forbidden("信封主体与正式身份不符"));
                }
                if let Some(r) = self.receipts.get(&input.envelope.idempotency_key) {
                    if r.admission_grant_id != msg.grant
                        || r.input.envelope != input.envelope
                        || r.input.install != input.install
                        || r.input.recovery != input.recovery
                    {
                        return Err(conflict("同幂等键不能替换载荷"));
                    }
                    let m = CommitManifest::decode(&r.commit_bytes).map_err(ApiError::from)?;
                    return Ok(if msg.grant.is_some() {
                        json!({"duplicate":true,"commit":{"tick":m.tick,"revision":m.world_revision,"state_root":hex(&m.state_root_hash)}})
                    } else {
                        json!({"duplicate":true,"tick":m.tick,"revision":m.world_revision,"state_root":hex(&m.state_root_hash)})
                    });
                }
                self.gate
                    .admit_frame(pid, self.world.clock.tick)
                    .map_err(ApiError::from)?;
                // 失败请求仍消耗原帧速率令牌；只有耐久提交成功才发布幂等键，避免欠费污染回执。
                let mut gate = self.gate.clone();
                gate.check_command(pid, &input.envelope, self.world.clock.tick)
                    .map_err(ApiError::from)?;
                if Some(input.envelope.target_tick) != self.world.clock.tick.checked_add(1)
                    || input.envelope.issued_at_tick != self.world.clock.tick
                    || input.envelope.payload.len() > 32
                {
                    return Err(ApiError::bad_request(
                        "仅可提交当前观察的下一Tick，最多32条结构/物理命令",
                    ));
                }
                // 此目的只驱动已有规则；不能借空世界将纯物理推进偷换为“规则实付”。
                if (msg.grant.is_some() || msg.run.is_some()) && !has_controlled_rules(&self.world) {
                    return Err(ApiError::forbidden("此目的需要世界已有受控规则，不授权空物理驱动"));
                }
                if self.world.life().is_some()
                    && input
                        .envelope
                        .payload
                        .iter()
                        .any(|c| !matches!(c, PhysicsCommand::Despawn { .. }))
                {
                    return Err(ApiError::forbidden(
                        "有来源生命只允许回收；运动须经已定价规则/神经，不得外部免费驱动或增料",
                    ));
                }
                let mut apm = self.apm.clone();
                for _ in &input.envelope.payload {
                    apm.try_acquire(self.world.clock.tick)
                        .map_err(|e| ApiError::new(e.code, e.message))?;
                }
                let _verification_slot = if input.install.is_some()
                    || self
                        .world
                        .component_rules()
                        .is_some_and(|r| !r.installed_releases.is_empty())
                {
                    Some(
                        app.inner
                            .agent_slots
                            .clone()
                            .try_acquire_owned()
                            .map_err(|_| conflict("规则复验名额已满"))?,
                    )
                } else {
                    None
                };
                if input.install.is_none()
                    && input.recovery.is_none()
                    && self.world.component_rules().is_some_and(|r| r.recovery.is_some())
                {
                    return Err(conflict("已隔离，须显式维护至Epoch并安装；不自动续跑"));
                }
                if input.recovery.is_some() && self.world.component_rules().is_none_or(|r| r.recovery.is_none()) {
                    let old = crate::rule_package::installed_permits(app, row.owner_principal_id, &self.world).await?;
                    let mut revoked = false;
                    for permit in old {
                        for pin in permit.pins {
                            if app
                                .store()
                                .private_rule(row.owner_principal_id, pin.id, pin.version)
                                .await?
                                .is_none_or(|p| p.revoked)
                            {
                                revoked = true;
                            }
                        }
                    }
                    if !revoked {
                        return Err(conflict("仅已撤销的规则世界可显式隔离维护，不是廉价跳过正常规则"));
                    }
                }
                let installation = if let Some(request) = input.install.as_ref().or(input.recovery.as_ref()) {
                    if !input.envelope.payload.is_empty()
                        || (input.install.is_some() && input.envelope.target_tick % 4 != 0)
                        || (input.recovery.is_some() && input.envelope.target_tick % 4 == 0)
                    {
                        return Err(ApiError::bad_request(
                            "安装只在Epoch，隔离维护只在前置真实Tick，二者均不得混入物理命令",
                        ));
                    }
                    apm.try_acquire(self.world.clock.tick)
                        .map_err(|e| ApiError::new(e.code, e.message))?;
                    Some(crate::rule_package::prepare_install(app, pid, request).await?)
                } else {
                    None
                };
                let base = self.world.clock.world_revision;
                let root = self.world.parent_commit_hash;
                let mut candidate = if let Some(plan) = &installation {
                    if input.recovery.is_some() {
                        self.world.candidate_with_rule_recovery(plan.evidence_cid, &plan.graphs)
                    } else {
                        self.world.candidate_with_rule_install(plan.evidence_cid, &plan.graphs)
                    }
                    .map_err(ApiError::from)?
                } else {
                    self.world.clone()
                };
                let rule_permits =
                    crate::rule_package::installed_permits(app, row.owner_principal_id, &candidate).await?;
                // 先拒绝已撤销包；最终线性化点仍是同一Store提交事务内的精确pins守卫。
                for permit in &rule_permits {
                    for pin in &permit.pins {
                        let current = app
                            .store()
                            .private_rule(row.owner_principal_id, pin.id.clone(), pin.version.clone())
                            .await?
                            .ok_or_else(|| conflict("已安装版本缺失"))?;
                        if current.revoked || hex(&current.cid) != pin.blake3_256 {
                            return Err(conflict("已安装规则/依赖撤销，须显式整体替换包组"));
                        }
                    }
                }
                candidate
                    .tick_with_commands(&input.envelope.payload)
                    .map_err(|e| ApiError::bad_request(format!("通用候选Tick拒绝：{e:?}")))?;
                let run_permit = if let Some(c) = &msg.run {
                    let j = app
                        .store()
                        .general_run_get(c.owner, c.id.clone())
                        .await?
                        .ok_or_else(|| conflict("运行不存在"))?;
                    Some(crate::general_runs::RunPermit {
                        claim: c.clone(),
                        step: j.done + 1,
                        energy_q: candidate.component_rules().map_or(0, |r| r.paid_q),
                    })
                } else {
                    None
                };
                let record = GeneralTickRecord {
                    admission_grant_id: msg.grant,
                    run_step: msg.run.as_ref().map(|c| crate::general_runs::RunStep {
                        run_id: c.id.clone(),
                        step: run_permit.as_ref().expect("运行守卫").step,
                        job_fence: c.fence,
                    }),
                    format: if msg.run.is_some() {
                        "rynvek-general-tick/5".into()
                    } else if msg.grant.is_some() {
                        "rynvek-general-tick/4".into()
                    } else if input.recovery.is_some() {
                        "rynvek-general-tick/3"
                    } else if installation.is_some() {
                        "rynvek-general-tick/2"
                    } else {
                        "rynvek-general-tick/1"
                    }
                    .into(),
                    installation,
                    base_revision: base,
                    base_root: root,
                    input: input.clone(),
                    commit_bytes: candidate.authoritative_commit(root).encode(),
                };
                let journal = app
                    .inner
                    .world_snapshots
                    .put(&serde_json::to_vec(&record).expect("定型日志"))?;
                let snapshot = if candidate.clock.tick % CHECKPOINT_INTERVAL == 0 {
                    Some(
                        app.inner
                            .world_snapshots
                            .put(&candidate.encode_snapshot().map_err(ApiError::from)?)?
                            .0,
                    )
                } else {
                    None
                };
                let commit = CommitRow {
                    world_id: row.world_id,
                    world_revision: candidate.clock.world_revision,
                    tick: candidate.clock.tick,
                    parent_revision: Some(base),
                    state_root_blake3: candidate.parent_commit_hash,
                    journal_segment_cid: journal.0,
                    snapshot_cid: snapshot,
                };
                if let Err(e) = self
                    .committer
                    .publish_running_row(commit, vec![admission], rule_permits, run_permit)
                    .await
                {
                    self.dead = true;
                    return Err(e);
                }
                self.world = candidate;
                self.gate = gate;
                self.apm = apm;
                self.receipts
                    .retain(|_, r| r.input.envelope.target_tick.saturating_add(RETENTION) >= self.world.clock.tick);
                self.receipts.insert(input.envelope.idempotency_key, record);
                Ok(if msg.run.is_some() {
                    json!({"commit":meta(&self.world)})
                } else if msg.grant.is_some() {
                    json!({"duplicate":false,"commit":meta(&self.world),"frame":rynvek_engine::general::GeneralRenderFrame::capture(&self.world)})
                } else {
                    json!({"duplicate":false,"commit":meta(&self.world),"observation":observation(&self.world)})
                })
            }
        }
    }
}
fn spawn(app: Weak<AppInner>, mut actor: Actor) -> GeneralHandle {
    let (tx, mut rx) = mpsc::channel::<Message>(64);
    tokio::spawn(async move {
        while let Ok(Some(msg)) = tokio::time::timeout(Duration::from_secs(25), rx.recv()).await {
            let Some(inner) = app.upgrade() else {
                break;
            };
            let app = AppState { inner };
            let result = actor.process(&app, &msg).await;
            // 返回前再次校验会话/所有者准入；已持久Tick不能因回执丢失自动重复。
            let result = match result {
                Ok(v) => {
                    match message_permit(
                        &app,
                        &msg.headers,
                        &actor.committer.world_id.to_string(),
                        msg.grant,
                        msg.run.as_ref(),
                    )
                    .await
                    {
                        Ok((_, row, _)) => {
                            let live = app.store().ranked_host(row.world_id).await;
                            match live {
                                Ok(Some(h))
                                    if row.fencing_token == actor.committer.fencing_token
                                        && h.fence == actor.committer.fencing_token
                                        && h.node == app.inner.ranked_node
                                        && h.expires_at_s > crate::ids::unix_now_s() =>
                                {
                                    Ok(v)
                                }
                                Ok(_) => {
                                    actor.dead = true;
                                    Err(conflict("回执前宿主已失效；先查询耐久状态，勿自动重发动作"))
                                }
                                Err(e) => {
                                    actor.dead = true;
                                    Err(e)
                                }
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            };
            let _ = msg.reply.send(result);
            if actor.dead {
                break;
            }
        }
        // 不回收世界/历史；宿主无请求25秒退役，30秒耐久租约自然过期。
    });
    GeneralHandle { tx }
}
pub async fn control(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    if body.len() > 64 * 1024 {
        return Err(ApiError::bad_request("通用命令超64KiB"));
    }
    let action: Action = crate::api::parse_json(&body)?;
    control_request(app, headers, id, action, None).await
}
pub(crate) async fn request_permit(
    app: &AppState,
    headers: &HeaderMap,
    id: &str,
    grant: Option<WireUuid>,
) -> ApiResult<(Subject, WorldRow, EditorPermit)> {
    if let Some(g) = grant {
        crate::general_collaboration::admission(app, headers, id, g, true).await
    } else {
        let (s, w) = permit(app, headers, id, true).await?;
        let p = EditorPermit {
            principal_id: s.principal_id,
            grant_id: None,
            purpose: AdmissionPurpose::GeneralRuleDrive,
        };
        Ok((s, w, p))
    }
}
pub(crate) async fn control_request(
    app: AppState,
    headers: HeaderMap,
    id: String,
    action: Action,
    grant: Option<WireUuid>,
) -> ApiResult<Response> {
    control_request_inner(app, headers, id, action, grant, None).await
}
async fn control_request_inner(
    app: AppState,
    headers: HeaderMap,
    id: String,
    action: Action,
    grant: Option<WireUuid>,
    run: Option<crate::lab_jobs::Claim>,
) -> ApiResult<Response> {
    let (_, row, _) = message_permit(&app, &headers, &id, grant, run.as_ref()).await?;
    if grant.is_some() && !crate::general_collaboration::allowed(&action) {
        return Err(ApiError::forbidden("准入仅可驱动已安装规则"));
    }
    let mut hosts = app.inner.general_hosts.lock().await;
    hosts.retain(|_, h| !h.closed());
    let handle = if let Some(h) = hosts.get(&row.world_id) {
        h.clone()
    } else {
        if !matches!(action, Action::Connect) && !(run.is_some() && matches!(action, Action::RunStep)) {
            return Err(conflict("请先显式connect恢复宿主并取得新控制租约"));
        }
        if hosts.len() >= 64 {
            return Err(conflict("本节点通用宿主达到64上限"));
        }
        // 即使旧Actor属于本节点，也不可在仍有效的宿主fence下重置控制租约计数。
        if app
            .store()
            .ranked_host(row.world_id)
            .await?
            .is_some_and(|h| h.fence == row.fencing_token && h.expires_at_s > crate::ids::unix_now_s())
        {
            return Err(conflict("健康或正在退役的宿主不可重置；等待原30秒租约失效后重连"));
        }
        let (world, receipts) = restore(&app, &row).await?;
        let mut apm = ApmLimiter::default();
        let mut ordered: Vec<_> = receipts.values().collect();
        ordered.sort_by_key(|r| r.input.envelope.target_tick);
        for r in ordered {
            if r.input.install.is_some() || r.input.recovery.is_some() {
                apm.try_acquire(r.input.envelope.issued_at_tick)
                    .map_err(|e| ApiError::new(e.code, e.message))?;
            }
            for _ in &r.input.envelope.payload {
                apm.try_acquire(r.input.envelope.issued_at_tick)
                    .map_err(|e| ApiError::new(e.code, e.message))?;
            }
        }
        let lease = app
            .store()
            .claim_ranked_host(HostClaim {
                world: row.world_id,
                revision: row.latest_revision,
                fence: row.fencing_token,
                node: app.inner.ranked_node.clone(),
                origin: app.config().public_base_url.clone(),
            })
            .await?;
        if lease.node != app.inner.ranked_node {
            return Err(conflict("健康远端宿主不可抢占；此入口不转发凭据"));
        }
        let h = spawn(
            std::sync::Arc::downgrade(&app.inner),
            Actor {
                world,
                committer: TickCommitter {
                    store: app.store().clone(),
                    snapshots: app.inner.world_snapshots.clone(),
                    world_id: row.world_id,
                    fencing_token: lease.fence,
                },
                leases: LeaseManager::new(),
                gate: CommandGate::new(CommandGateConfig::default()),
                receipts,
                apm,
                dead: false,
                driver_lease: None,
                run_lease: None,
            },
        );
        hosts.insert(row.world_id, h.clone());
        h
    };
    drop(hosts);
    let value = handle.request(headers, action, grant, run).await?;
    Ok(private(value))
}
pub async fn observe(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let (_, row) = permit(&app, &headers, &id, false).await?;
    let (world, _) = restore(&app, &row).await?;
    permit(&app, &headers, &id, false).await?;
    Ok(private(observation(&world)))
}

/// 原Private所有者准入的最小表现投影；不传快照/规则/账户给Worker。
pub async fn render_frame(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let (_, row) = permit(&app, &headers, &id, false).await?;
    let (world, _) = restore(&app, &row).await?;
    permit(&app, &headers, &id, false).await?;
    Ok(private(
        serde_json::to_value(rynvek_engine::general::GeneralRenderFrame::capture(&world)).expect("定型渲染投影"),
    ))
}

async fn message_permit(
    app: &AppState,
    headers: &HeaderMap,
    id: &str,
    grant: Option<WireUuid>,
    run: Option<&crate::lab_jobs::Claim>,
) -> ApiResult<(Subject, WorldRow, EditorPermit)> {
    if let Some(c) = run {
        if grant.is_some() {
            return Err(ApiError::forbidden("后台运行不能借用协作准入"));
        }
        let (s, w, p) = crate::general_runs::admission(app, c).await?;
        if w.world_id.to_string() != id {
            return Err(conflict("运行世界不符"));
        }
        Ok((s, w, p))
    } else {
        request_permit(app, headers, id, grant).await
    }
}
/// 内部调度不转发Bearer；健康原宿主仍不可抢占或重置控制fence。
pub(crate) async fn run_step(app: &AppState, c: crate::lab_jobs::Claim) -> ApiResult<bool> {
    let (_, w, _) = crate::general_runs::admission(app, &c).await?;
    let local = app
        .inner
        .general_hosts
        .lock()
        .await
        .get(&w.world_id)
        .is_some_and(|h| !h.closed());
    if !local
        && app
            .store()
            .ranked_host(w.world_id)
            .await?
            .is_some_and(|h| h.expires_at_s > crate::ids::unix_now_s())
    {
        return Ok(false);
    }
    control_request_inner(
        app.clone(),
        HeaderMap::new(),
        w.world_id.to_string(),
        Action::RunStep,
        None,
        Some(c),
    )
    .await?;
    Ok(true)
}
