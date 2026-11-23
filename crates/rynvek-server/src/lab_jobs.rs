// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! Lab专用持久作业状态机；Store锁/事务内应用，不把进程缓存当事实源。
mod worker;
pub use worker::{start_workers, stop_workers};

use crate::{
    error::{ApiError, ApiResult},
    ids::{new_uuid_v7, unix_now_s},
};
use rynvek_rts::lab::ExperimentSpec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const LEASE_S: u64 = 30;
pub const MAX_ACTIVE: usize = 8;
pub const MAX_JOBS: usize = 512;
pub const MAX_ATTEMPTS: u32 = 5;
pub const KERNEL_PROFILE: &str = "rynvek-lab-kernel/5";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Experiment {
    pub exp_id: String,
    #[serde(with = "principal_text")]
    pub owner_principal_id: u64,
    pub submission_id: String,
    pub spec_digest: String,
    pub kernel_profile: String,
    pub name: String,
    pub seeds: Vec<u64>,
    pub ticks: u64,
    pub sample_every: u64,
    pub variants: Vec<rynvek_rts::lab::VariantSpec>,
    pub status: String,
    pub generation: u32,
    pub attempt: u32,
    pub submitted_at_unix_s: u64,
    pub started_at_unix_s: Option<u64>,
    pub completed_at_unix_s: Option<u64>,
    pub lease_expires_at_unix_s: Option<u64>,
    pub runs_total: usize,
    pub runs_done: usize,
    pub parquet_cid: Option<String>,
    pub tables: BTreeMap<String, String>,
    pub error: Option<String>,
}
mod principal_text {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}
impl Experiment {
    pub fn spec(&self) -> ExperimentSpec {
        ExperimentSpec {
            name: self.name.clone(),
            seeds: self.seeds.clone(),
            ticks: self.ticks,
            sample_every: self.sample_every,
            variants: self.variants.clone(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LabJob {
    pub experiment: Experiment,
    pub fence: u64,
    pub node: Option<String>,
    pub chunks: BTreeMap<u32, String>,
    pub operations: BTreeMap<String, OwnerAction>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerAction {
    pub operation_id: String,
    pub generation: u32,
    pub action: String,
}
#[derive(Clone, Debug)]
pub struct Claim {
    pub id: String,
    pub owner: u64,
    pub node: String,
    pub fence: u64,
}
#[derive(Clone, Debug)]
pub enum WorkerAction {
    Heartbeat,
    Checkpoint { run: u32, cid: String },
    Complete { tables: BTreeMap<String, String> },
    Fail,
    Release,
}
pub fn conflict(message: &str) -> ApiError {
    ApiError::new(rynvek_core::error::RynvekErrorCode::StructuralMutationConflict, message)
}
pub fn uuid_text(id: &str) -> ApiResult<String> {
    id.parse::<rynvek_protocol::WireUuid>()
        .map(|u| u.to_string())
        .map_err(|_| ApiError::bad_request("作业/幂等键必须为UUID"))
}
impl LabJob {
    pub fn new(owner: u64, submission_id: String, mut spec: ExperimentSpec) -> ApiResult<Self> {
        spec.validate().map_err(ApiError::bad_request)?;
        spec.variants = spec.effective_variants();
        let digest = blake3::hash(&serde_json::to_vec(&(KERNEL_PROFILE, &spec)).expect("规格可序列化"))
            .to_hex()
            .to_string();
        Ok(Self {
            experiment: Experiment {
                exp_id: new_uuid_v7().to_string(),
                owner_principal_id: owner,
                submission_id: uuid_text(&submission_id)?,
                spec_digest: digest,
                kernel_profile: KERNEL_PROFILE.into(),
                runs_total: spec.runs().len(),
                name: spec.name,
                seeds: spec.seeds,
                ticks: spec.ticks,
                sample_every: spec.sample_every,
                variants: spec.variants,
                status: "queued".into(),
                generation: 0,
                attempt: 0,
                submitted_at_unix_s: unix_now_s(),
                started_at_unix_s: None,
                completed_at_unix_s: None,
                lease_expires_at_unix_s: None,
                runs_done: 0,
                parquet_cid: None,
                tables: BTreeMap::new(),
                error: None,
            },
            fence: 0,
            node: None,
            chunks: BTreeMap::new(),
            operations: BTreeMap::new(),
        })
    }
    pub fn active(&self) -> bool {
        matches!(self.experiment.status.as_str(), "queued" | "running")
    }
    pub fn eligible(&self, now: u64) -> bool {
        self.experiment.kernel_profile == KERNEL_PROFILE
            && (self.experiment.status == "queued"
                || (self.experiment.status == "running"
                    && self.experiment.lease_expires_at_unix_s.is_none_or(|t| t <= now)))
    }
    fn revoke(&mut self) -> ApiResult<()> {
        self.fence = self.fence.checked_add(1).ok_or_else(|| conflict("作业fence耗尽"))?;
        self.node = None;
        self.experiment.lease_expires_at_unix_s = None;
        Ok(())
    }
    pub fn claim(&mut self, node: String, now: u64) -> ApiResult<bool> {
        if !self.eligible(now) {
            return Ok(false);
        }
        self.revoke()?;
        if self.experiment.attempt >= MAX_ATTEMPTS {
            self.experiment.status = "failed".into();
            self.experiment.error = Some("认领重试上限；请所有者对账后显式重试".into());
            self.experiment.completed_at_unix_s = Some(now);
            return Ok(false);
        }
        self.experiment.attempt += 1;
        self.experiment.status = "running".into();
        self.experiment.started_at_unix_s.get_or_insert(now);
        self.experiment.lease_expires_at_unix_s = Some(now + LEASE_S);
        self.node = Some(node);
        Ok(true)
    }
    pub fn owner_action(&mut self, mut op: OwnerAction, now: u64) -> ApiResult<()> {
        op.operation_id = uuid_text(&op.operation_id)?;
        if let Some(old) = self.operations.get(&op.operation_id) {
            return if old == &op {
                Ok(())
            } else {
                Err(conflict("幂等操作键已用于不同请求"))
            };
        }
        if self.operations.len() >= 64 {
            return Err(conflict("单作业操作记录上限64，历史不驱逐"));
        }
        if op.generation != self.experiment.generation {
            return Err(conflict("作业代次已改变，请刷新对账"));
        }
        match op.action.as_str() {
            "cancel" => {
                if self.active() {
                    self.revoke()?;
                    self.experiment.status = "cancelled".into();
                    self.experiment.completed_at_unix_s = Some(now);
                }
            }
            "retry" => {
                if !matches!(self.experiment.status.as_str(), "failed" | "cancelled") {
                    return Err(conflict("只可显式重试failed/cancelled作业"));
                }
                self.revoke()?;
                self.experiment.generation = self
                    .experiment
                    .generation
                    .checked_add(1)
                    .ok_or_else(|| conflict("作业代次耗尽"))?;
                self.experiment.attempt = 0;
                self.experiment.status = "queued".into();
                self.experiment.started_at_unix_s = None;
                self.experiment.completed_at_unix_s = None;
                self.experiment.error = None;
            }
            _ => return Err(ApiError::bad_request("未知Lab操作")),
        }
        self.operations.insert(op.operation_id.clone(), op);
        Ok(())
    }
    pub fn claim_ref(&self) -> Claim {
        Claim {
            id: self.experiment.exp_id.clone(),
            owner: self.experiment.owner_principal_id,
            node: self.node.clone().expect("已认领"),
            fence: self.fence,
        }
    }
    pub fn worker_action(&mut self, claim: &Claim, action: WorkerAction, now: u64) -> ApiResult<()> {
        if self.experiment.exp_id != claim.id
            || self.experiment.owner_principal_id != claim.owner
            || self.experiment.status != "running"
            || self.node.as_deref() != Some(&claim.node)
            || self.fence != claim.fence
            || self.experiment.lease_expires_at_unix_s.is_none_or(|t| t <= now)
        {
            return Err(conflict("Lab工作者租约已失效，禁止迟到提交"));
        }
        match action {
            WorkerAction::Heartbeat => self.experiment.lease_expires_at_unix_s = Some(now + LEASE_S),
            WorkerAction::Checkpoint { run, cid } => {
                if run as usize >= self.experiment.runs_total || crate::ids::parse_hex::<32>(&cid).is_none() {
                    return Err(conflict("非法run检查点"));
                }
                if let Some(old) = self.chunks.get(&run) {
                    if old != &cid {
                        return Err(conflict("run检查点不可覆盖"));
                    }
                } else {
                    self.chunks.insert(run, cid);
                }
                self.experiment.runs_done = self.chunks.len();
            }
            WorkerAction::Complete { tables } => {
                if self.chunks.len() != self.experiment.runs_total
                    || tables.len() != 4
                    || !["runs", "metrics", "lineage", "energy"]
                        .iter()
                        .all(|k| tables.get(*k).is_some_and(|v| crate::ids::parse_hex::<32>(v).is_some()))
                {
                    return Err(conflict("结果必须包含完整四表及全部run检查点"));
                }
                self.experiment.parquet_cid = tables.get("metrics").cloned();
                self.experiment.tables = tables;
                self.experiment.status = "completed".into();
                self.experiment.completed_at_unix_s = Some(now);
                self.revoke()?;
            }
            WorkerAction::Fail => {
                self.experiment.status = "failed".into();
                self.experiment.error = Some("执行/私有结果编码失败；检查点保留，可显式重试".into());
                self.experiment.completed_at_unix_s = Some(now);
                self.revoke()?;
            }
            WorkerAction::Release => {
                self.experiment.status = "queued".into();
                self.revoke()?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn job() -> LabJob {
        LabJob::new(
            u64::MAX,
            new_uuid_v7().to_string(),
            ExperimentSpec {
                name: "fencing".into(),
                seeds: vec![57],
                ticks: 4,
                sample_every: 1,
                variants: vec![],
            },
        )
        .unwrap()
    }
    fn op(generation: u32, action: &str) -> OwnerAction {
        OwnerAction {
            operation_id: new_uuid_v7().to_string(),
            generation,
            action: action.into(),
        }
    }
    fn tables() -> BTreeMap<String, String> {
        ["runs", "metrics", "lineage", "energy"]
            .into_iter()
            .map(|k| (k.into(), "ab".repeat(32)))
            .collect()
    }
    #[test]
    fn lease_expiry_fences_old_writes_and_four_tables_publish_atomically() {
        let mut job = job();
        assert!(job.claim("first".into(), 100).unwrap());
        let stale = job.claim_ref();
        assert!(
            job.worker_action(&stale, WorkerAction::Complete { tables: tables() }, 101)
                .is_err()
        );
        assert!(job.experiment.tables.is_empty());
        job.worker_action(
            &stale,
            WorkerAction::Checkpoint {
                run: 0,
                cid: "ac".repeat(32),
            },
            101,
        )
        .unwrap();
        assert!(!job.claim("not-yet".into(), 129).unwrap());
        assert!(job.claim("recovered".into(), 130).unwrap());
        let current = job.claim_ref();
        for action in [
            WorkerAction::Heartbeat,
            WorkerAction::Release,
            WorkerAction::Fail,
            WorkerAction::Checkpoint {
                run: 0,
                cid: "ac".repeat(32),
            },
            WorkerAction::Complete { tables: tables() },
        ] {
            assert!(job.worker_action(&stale, action, 131).is_err());
        }
        assert!(
            job.worker_action(
                &current,
                WorkerAction::Complete {
                    tables: BTreeMap::new()
                },
                131
            )
            .is_err()
        );
        assert!(job.experiment.tables.is_empty());
        job.worker_action(&current, WorkerAction::Complete { tables: tables() }, 131)
            .unwrap();
        assert_eq!(job.experiment.status, "completed");
        assert_eq!(job.experiment.tables.len(), 4);
        assert!(job.node.is_none());
        assert_eq!(job.experiment.attempt, 2);
    }
    #[test]
    fn cancel_retry_receipts_do_not_cancel_new_generation() {
        let mut job = job();
        job.claim("old".into(), 100).unwrap();
        let old = job.claim_ref();
        let cancel = op(0, "cancel");
        job.owner_action(cancel.clone(), 101).unwrap();
        assert_eq!(job.experiment.status, "cancelled");
        let retry = op(0, "retry");
        job.owner_action(retry.clone(), 102).unwrap();
        job.owner_action(retry, 103).unwrap();
        job.owner_action(cancel.clone(), 103).unwrap();
        assert_eq!(job.experiment.generation, 1);
        assert_eq!(job.experiment.status, "queued");
        assert!(job.owner_action(op(0, "cancel"), 104).is_err());
        let mut changed = cancel;
        changed.action = "retry".into();
        assert!(job.owner_action(changed, 104).is_err());
        assert!(
            job.worker_action(&old, WorkerAction::Complete { tables: tables() }, 104)
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(&job.experiment).unwrap()["owner_principal_id"],
            u64::MAX.to_string()
        );
    }
    #[test]
    fn failed_claim_budget_and_graceful_release_are_explicit() {
        let mut job = job();
        for attempt in 1..=MAX_ATTEMPTS {
            assert!(job.claim("node".into(), 100 + u64::from(attempt) * LEASE_S).unwrap());
        }
        assert!(
            !job.claim("node".into(), 100 + u64::from(MAX_ATTEMPTS + 1) * LEASE_S)
                .unwrap()
        );
        assert_eq!(job.experiment.status, "failed");
        job.owner_action(op(0, "retry"), 400).unwrap();
        assert!(job.claim("new".into(), 400).unwrap());
        let c = job.claim_ref();
        job.worker_action(&c, WorkerAction::Release, 401).unwrap();
        assert_eq!(job.experiment.status, "queued");
        assert!(job.worker_action(&c, WorkerAction::Heartbeat, 402).is_err());
    }
}
