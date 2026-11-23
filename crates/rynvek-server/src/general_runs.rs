// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 原General Actor的显式耐久运行意图；不保存令牌，不启动第二份世界。
mod worker;
use crate::{
    error::{ApiError, ApiResult},
    ids::{hex, new_uuid_v7},
    lab_jobs::{Claim, LEASE_S, MAX_ATTEMPTS, OwnerAction},
    store::{CommitRow, WorldRow},
};
use rynvek_protocol::{IdempotencyKey, WireUuid};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub use worker::{start_workers, stop_workers};
pub const CONSENT: &str = "run_installed_rules_with_bounded_budget";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSpec {
    pub submission_id: String,
    pub expected_revision: u64,
    pub expected_root: String,
    pub ticks: u32,
    /// 墙钟上限包括暂停、失联和故障等待，不能通过恢复延长。
    pub wall_seconds: u32,
    pub interval_ms: u32,
    pub max_energy_q: String,
    pub consent: String,
}
impl RunSpec {
    pub fn validate(&self) -> ApiResult<u64> {
        uuid(&self.submission_id)?;
        let energy = self
            .max_energy_q
            .parse::<u64>()
            .map_err(|_| ApiError::bad_request("预算须是规范正整数EnergyQ字符串"))?;
        if self.consent != CONSENT
            || !(1..=128).contains(&self.ticks)
            || !(10..=900).contains(&self.wall_seconds)
            || !(200..=5000).contains(&self.interval_ms)
            || !(1..=1_000_000_000_000).contains(&energy)
            || energy.to_string() != self.max_energy_q
            || self.expected_root.len() != 64
            || !self
                .expected_root
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ApiError::bad_request(
                "须显式同意，1..128 Tick、10..900秒、200..5000毫秒间隔与有界规范EnergyQ预算",
            ));
        }
        Ok(energy)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeneralRun {
    /// Memory仅进程内治理；不能宣称跨进程耐久。
    #[serde(default)]
    pub storage_backend: String,
    pub id: String,
    pub world_id: WireUuid,
    #[serde(with = "crate::lab_jobs::principal_text")]
    pub owner: u64,
    pub spec: RunSpec,
    pub base_tick: u64,
    pub last_root: String,
    pub done: u32,
    pub energy_used_q: u64,
    pub status: String,
    pub generation: u32,
    pub fence: u64,
    pub attempt: u32,
    pub node: Option<String>,
    pub lease_until_s: u64,
    pub deadline_s: u64,
    pub error: Option<String>,
    /// 复用原作业操作CAS/幂等协议；满额后拒绝新控制，不驱逐旧收据。
    pub operations: BTreeMap<String, OwnerAction>,
}
#[derive(Clone, Debug)]
pub struct RunPermit {
    pub claim: Claim,
    pub step: u32,
    pub energy_q: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStep {
    pub run_id: String,
    pub step: u32,
    pub job_fence: u64,
}
impl GeneralRun {
    pub fn new(row: &WorldRow, spec: RunSpec, now: u64) -> ApiResult<Self> {
        spec.validate()?;
        if spec.expected_revision != row.latest_revision || spec.expected_root != hex(&row.latest_state_root) {
            return Err(crate::transactions::conflict("运行起点已变化，须重新观察并显式启动"));
        }
        Ok(Self {
            storage_backend: "memory".into(),
            id: new_uuid_v7().to_string(),
            world_id: row.world_id,
            owner: row.owner_principal_id,
            deadline_s: now + spec.wall_seconds as u64,
            base_tick: row.latest_tick,
            last_root: spec.expected_root.clone(),
            spec,
            done: 0,
            energy_used_q: 0,
            status: "queued".into(),
            generation: 0,
            fence: 0,
            attempt: 0,
            node: None,
            lease_until_s: 0,
            error: None,
            operations: BTreeMap::new(),
        })
    }
    pub fn reserved(&self) -> bool {
        matches!(self.status.as_str(), "queued" | "running" | "paused" | "blocked")
    }
    pub fn baseline(&self, row: &WorldRow) -> ApiResult<()> {
        if row.world_id != self.world_id
            || row.owner_principal_id != self.owner
            || row.latest_revision != self.spec.expected_revision + self.done as u64
            || row.latest_tick != self.base_tick + self.done as u64
            || hex(&row.latest_state_root) != self.last_root
        {
            return Err(crate::transactions::conflict("运行进度与原World提交链不符"));
        }
        Ok(())
    }
    pub fn claim_of(&self) -> Claim {
        Claim {
            id: self.id.clone(),
            owner: self.owner,
            fence: self.fence,
            node: self.node.clone().unwrap_or_default(),
        }
    }
    pub fn live(&self, c: &Claim, now: u64) -> ApiResult<()> {
        if self.status != "running"
            || c.id != self.id
            || c.owner != self.owner
            || c.fence != self.fence
            || self.node.as_deref() != Some(&c.node)
            || self.lease_until_s <= now
            || self.deadline_s <= now
        {
            return Err(crate::transactions::conflict("运行租约/代次/预算已失效"));
        }
        Ok(())
    }
    fn invalidate(&mut self, status: &str) {
        self.status = status.into();
        self.fence += 1;
        self.node = None;
        self.lease_until_s = 0;
    }
    pub fn claim(&mut self, node: String, now: u64) -> ApiResult<bool> {
        if !(self.status == "queued" || self.status == "running" && self.lease_until_s <= now) {
            return Ok(false);
        }
        if now >= self.deadline_s {
            self.invalidate("expired");
            return Ok(false);
        }
        if self.attempt >= MAX_ATTEMPTS {
            self.invalidate("blocked");
            self.error = Some("故障重领次数已满，须显式恢复".into());
            return Ok(false);
        }
        self.fence += 1;
        self.attempt += 1;
        self.status = "running".into();
        self.node = Some(node);
        self.lease_until_s = now + LEASE_S;
        Ok(true)
    }
    pub fn heartbeat(&mut self, c: &Claim, now: u64) -> ApiResult<()> {
        self.live(c, now)?;
        self.lease_until_s = now + LEASE_S;
        Ok(())
    }
    pub fn release(&mut self, c: &Claim, now: u64) -> ApiResult<()> {
        self.live(c, now)?;
        self.invalidate("queued");
        Ok(())
    }
    pub fn stop(&mut self, c: &Claim, now: u64) -> ApiResult<()> {
        // 只允许撤销能力；即便墙钟已过也能封闭旧claim，不续命。
        if self.claim_of() != *c || self.status != "running" {
            return Err(crate::transactions::conflict("旧作业不可改写运行状态"));
        }
        self.invalidate(if now >= self.deadline_s { "expired" } else { "blocked" });
        self.error = Some("原Actor/授权/规则或预算拒绝；没有跳过规则或自动降级".into());
        Ok(())
    }
    pub fn control(&mut self, op: OwnerAction, now: u64) -> ApiResult<()> {
        uuid(&op.operation_id)?;
        if let Some(old) = self.operations.get(&op.operation_id) {
            return if old == &op {
                Ok(())
            } else {
                Err(crate::transactions::conflict("操作键已绑定不同控制"))
            };
        }
        if op.generation != self.generation || self.operations.len() >= 64 {
            return Err(crate::transactions::conflict("运行控制代次过期或收据配额已满"));
        }
        match op.action.as_str() {
            "pause" if matches!(self.status.as_str(), "queued" | "running") => self.invalidate("paused"),
            "cancel" if self.reserved() => self.invalidate("cancelled"),
            "resume"
                if matches!(self.status.as_str(), "paused" | "blocked")
                    && now < self.deadline_s
                    && self.done < self.spec.ticks =>
            {
                self.invalidate("queued");
                self.attempt = 0;
                self.error = None;
            }
            _ => return Err(crate::transactions::conflict("状态不允许该操作；恢复不延长原预算/墙钟")),
        }
        self.generation += 1;
        self.operations.insert(op.operation_id.clone(), op);
        Ok(())
    }
    pub fn commit(&mut self, p: &RunPermit, row: &WorldRow, commit: &CommitRow, now: u64) -> ApiResult<()> {
        self.live(&p.claim, now)?;
        self.baseline(row)?;
        let energy = self
            .energy_used_q
            .checked_add(p.energy_q)
            .ok_or_else(|| crate::transactions::conflict("EnergyQ预算溢出"))?;
        if p.step != self.done + 1
            || self.done >= self.spec.ticks
            || energy > self.spec.validate()?
            || commit.world_id != self.world_id
            || commit.parent_revision != Some(row.latest_revision)
            || commit.tick != row.latest_tick + 1
            || commit.world_revision != row.latest_revision + 1
            || p.energy_q == 0
        {
            return Err(crate::transactions::conflict("耐久运行步号/前驱/实付能量预算拒绝"));
        }
        self.done += 1;
        self.energy_used_q = energy;
        self.last_root = hex(&commit.state_root_blake3);
        self.lease_until_s = now + LEASE_S;
        if self.done == self.spec.ticks {
            self.invalidate("completed")
        }
        Ok(())
    }
    pub fn step_key(id: &str, step: u32) -> ApiResult<IdempotencyKey> {
        let base = uuid(id)?;
        let mut h = blake3::Hasher::new();
        h.update(b"rynvek-general-run-step/1");
        h.update(&base.0);
        h.update(&step.to_be_bytes());
        let mut bytes: [u8; 16] = h.finalize().as_bytes()[..16].try_into().expect("十六字节");
        bytes[..6].copy_from_slice(&base.0[..6]);
        bytes[6] = (bytes[6] & 15) | 0x70;
        bytes[8] = (bytes[8] & 63) | 0x80;
        IdempotencyKey::new(WireUuid(bytes)).map_err(|_| ApiError::bad_request("步骤UUIDv7非法"))
    }
}
/// 作业调度仍经原ABAC/编辑准入，持久意图不保存或冒用客户端Bearer。
pub async fn admission(
    app: &crate::app::AppState,
    c: &Claim,
) -> ApiResult<(rynvek_security::Subject, WorldRow, crate::store::EditorPermit)> {
    use rynvek_protocol::CapabilityScope;
    use rynvek_security::{PrincipalKindCode, Subject};
    let j = app
        .store()
        .general_run_get(c.owner, c.id.clone())
        .await?
        .ok_or_else(|| crate::transactions::conflict("运行不存在"))?;
    j.live(c, crate::ids::unix_now_s())?;
    let row = app
        .store()
        .get_world(j.world_id)
        .await?
        .ok_or_else(|| crate::transactions::conflict("运行世界不存在"))?;
    if !row.is_controlled_general() || row.owner_principal_id != c.owner {
        return Err(ApiError::forbidden("运行所有权失效"));
    }
    let p = app
        .store()
        .get_principal(c.owner)
        .await?
        .ok_or_else(|| ApiError::forbidden("运行主体失效"))?;
    if p.kind != PrincipalKindCode::Human {
        return Err(ApiError::forbidden("运行只允许明确启动的正式人类所有者"));
    }
    let permit = crate::store::EditorPermit {
        principal_id: c.owner,
        grant_id: None,
        purpose: crate::store::AdmissionPurpose::GeneralRuleDrive,
    };
    app.store().check_editor(row.world_id, permit.clone()).await?;
    let subject = Subject {
        principal_id: p.principal_id,
        kind: p.kind,
        status: p.status,
        scopes: [
            CapabilityScope::WorldConnect,
            CapabilityScope::WorldRead,
            CapabilityScope::WorldEdit,
        ]
        .into_iter()
        .collect(),
        memberships: BTreeMap::new(),
    };
    rynvek_security::authorize(
        &subject,
        CapabilityScope::WorldEdit,
        &rynvek_security::Resource::World {
            owner_principal_id: row.owner_principal_id,
            org_id: row.org_id,
            visibility: row.visibility,
        },
    )
    .map_err(ApiError::from)?;
    Ok((subject, row, permit))
}
/// 私有原运行记录仅所有者可读取/暂停/取消；协作者的手动drive许可不会升级后台授权。
pub async fn get(
    axum::extract::State(app): axum::extract::State<crate::app::AppState>,
    axum::extract::Path((id, run)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> ApiResult<axum::response::Response> {
    let (s, row) = crate::general_host::permit(&app, &headers, &id, false).await?;
    let j = app
        .store()
        .general_run_get(s.principal_id, uuid(&run)?.to_string())
        .await?
        .filter(|j| j.world_id == row.world_id)
        .ok_or_else(|| ApiError::not_found("运行不存在"))?;
    Ok(crate::general_host::private(serde_json::to_value(j).expect("定型运行")))
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lookup {
    submission_id: String,
}
/// 请求回执丢失时按原提交键对账，不认领宿主或重新推进。
pub async fn lookup(
    axum::extract::State(app): axum::extract::State<crate::app::AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<Lookup>,
    headers: axum::http::HeaderMap,
) -> ApiResult<axum::response::Response> {
    let (s, w) = crate::general_host::permit(&app, &headers, &id, false).await?;
    let j = app
        .store()
        .general_run_submission(s.principal_id, uuid(&q.submission_id)?.to_string())
        .await?
        .filter(|j| j.world_id == w.world_id)
        .ok_or_else(|| ApiError::not_found("运行不存在"))?;
    Ok(crate::general_host::private(serde_json::to_value(j).expect("定型运行")))
}
pub async fn control(
    axum::extract::State(app): axum::extract::State<crate::app::AppState>,
    axum::extract::Path((id, run)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
    axum::Json(op): axum::Json<OwnerAction>,
) -> ApiResult<axum::response::Response> {
    let (s, row) = crate::general_host::permit(&app, &headers, &id, true).await?;
    let action = op.action.clone();
    let j = app
        .store()
        .general_run_control(row.world_id, s.principal_id, uuid(&run)?.to_string(), op)
        .await?;
    app.audit(
        Some(s.principal_id),
        "world.general.run.control",
        j.id.clone(),
        serde_json::json!({"action":action,"status":j.status,"generation":j.generation,"done":j.done}),
    )
    .await;
    Ok(crate::general_host::private(serde_json::to_value(j).expect("定型运行")))
}

fn uuid(id: &str) -> ApiResult<WireUuid> {
    let u = id
        .parse::<WireUuid>()
        .map_err(|_| ApiError::bad_request("运行/操作键须为UUIDv7"))?;
    IdempotencyKey::new(u).map_err(|_| ApiError::bad_request("须规范UUIDv7"))?;
    if u.to_string() != id {
        return Err(ApiError::bad_request("须规范小写连字符UUIDv7"));
    }
    Ok(u)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec() -> RunSpec {
        RunSpec {
            submission_id: new_uuid_v7().to_string(),
            expected_revision: 0,
            expected_root: "00".repeat(32),
            ticks: 8,
            wall_seconds: 60,
            interval_ms: 200,
            max_energy_q: "1000000".into(),
            consent: CONSENT.into(),
        }
    }
    fn job() -> GeneralRun {
        GeneralRun {
            storage_backend: "memory".into(),
            id: new_uuid_v7().to_string(),
            world_id: new_uuid_v7(),
            owner: 1,
            spec: spec(),
            base_tick: 0,
            last_root: "00".repeat(32),
            done: 0,
            energy_used_q: 0,
            status: "queued".into(),
            generation: 0,
            fence: 0,
            attempt: 0,
            node: None,
            lease_until_s: 0,
            deadline_s: 160,
            error: None,
            operations: BTreeMap::new(),
        }
    }
    fn op(j: &GeneralRun, action: &str) -> OwnerAction {
        OwnerAction {
            operation_id: new_uuid_v7().to_string(),
            generation: j.generation,
            action: action.into(),
        }
    }
    #[test]
    fn bounded_consent_and_canonical_budget_never_implicitly_enable_compute() {
        let s = spec();
        assert!(s.validate().is_ok());
        for change in ["ticks", "time", "interval", "energy", "leading_zero", "consent", "uuid"] {
            let mut b = s.clone();
            match change {
                "ticks" => b.ticks = 129,
                "time" => b.wall_seconds = 901,
                "interval" => b.interval_ms = 199,
                "energy" => b.max_energy_q = "1000000000001".into(),
                "leading_zero" => b.max_energy_q = "010".into(),
                "consent" => b.consent.clear(),
                _ => b.submission_id = "00000000-0000-4000-8000-000000000000".into(),
            }
            assert!(b.validate().is_err(), "{change}");
        }
        let a = GeneralRun::step_key(&s.submission_id, 1).unwrap();
        assert_eq!(a, GeneralRun::step_key(&s.submission_id, 1).unwrap());
        assert_ne!(a, GeneralRun::step_key(&s.submission_id, 2).unwrap());
    }
    #[test]
    fn pause_resume_and_deadline_revoke_old_job_fences_without_resetting_progress() {
        let mut j = job();
        assert!(j.claim("node-a".into(), 100).unwrap());
        let old = j.claim_of();
        assert!(!j.claim("node-b".into(), 101).unwrap());
        j.done = 2;
        j.energy_used_q = 100;
        let pause = op(&j, "pause");
        j.control(pause.clone(), 102).unwrap();
        assert!(j.live(&old, 102).is_err());
        let generation = j.generation;
        j.control(pause, 103).unwrap();
        assert_eq!(generation, j.generation);
        let resume = op(&j, "resume");
        j.control(resume, 104).unwrap();
        assert!(j.claim("node-b".into(), 105).unwrap());
        assert!(j.fence > old.fence);
        assert_eq!((j.done, j.energy_used_q, j.deadline_s), (2, 100, 160));
        assert!(j.live(&old, 106).is_err());
        let current = j.claim_of();
        assert!(j.live(&current, 160).is_err());
        j.stop(&current, 160).unwrap();
        assert_eq!(j.status, "expired");
        assert!(!j.reserved());
    }
    #[test]
    fn operation_key_conflicts_and_late_fail_cannot_override_cancel() {
        let mut j = job();
        j.claim("node".into(), 100).unwrap();
        let old = j.claim_of();
        let p = op(&j, "pause");
        j.control(p.clone(), 101).unwrap();
        let mut collision = p;
        collision.action = "cancel".into();
        assert!(j.control(collision, 102).is_err());
        let c = op(&j, "cancel");
        j.control(c.clone(), 103).unwrap();
        j.control(c, 104).unwrap();
        assert!(j.stop(&old, 105).is_err());
        assert_eq!(j.status, "cancelled");
    }
}
