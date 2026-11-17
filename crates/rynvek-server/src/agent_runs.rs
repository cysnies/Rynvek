// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! Pi参考的控制面持久运行状态；不执行模型、不持凭据，不进入权威Tick。
use crate::{
    agent_package::{AgentDependency, ToolDecision},
    error::{ApiError, ApiResult},
    ids::{new_uuid_v7, unix_now_s},
    lab_jobs::{conflict, uuid_text},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub const LEASE_S: u64 = 30;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub turns: u32,
    pub tool_calls: u32,
    pub wall_seconds: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunEvent {
    pub seq: u32,
    pub kind: String,
    pub data: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunnerLease {
    pub controller_id: String,
    pub fence: u32,
    pub expires_at_unix_s: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingTool {
    pub operation_id: String,
    pub command_id: String,
    pub call: ToolDecision,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentRun {
    pub run_id: String,
    pub owner_id: String,
    pub submission_id: String,
    pub spec_digest: String,
    pub world_id: String,
    pub host_fence: String,
    pub control_fence: String,
    pub faction: u8,
    pub package_id: String,
    pub version: String,
    pub artifact_cid: String,
    pub pins: Vec<AgentDependency>,
    pub tools: Vec<String>,
    pub budget: Budget,
    pub turns_used: u32,
    pub calls_used: u32,
    pub created_at_unix_s: u64,
    pub deadline_unix_s: u64,
    pub revision: u32,
    pub fence: u32,
    pub status: String,
    pub lease: Option<RunnerLease>,
    pub pending: Option<PendingTool>,
    pub memory: Value,
    pub last_result: Value,
    pub events: Vec<RunEvent>,
    pub operations: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub operation_id: String,
    pub expected_revision: u32,
    pub action: String,
    #[serde(default)]
    pub controller_id: Option<String>,
    #[serde(default)]
    pub host_fence: Option<String>,
    #[serde(default)]
    pub control_fence: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub operation_id: String,
    pub controller_id: String,
    pub fence: u32,
    pub provider: String,
    pub message: String,
    pub memory: Value,
    pub call: Option<ToolDecision>,
}
#[derive(Clone, Debug)]
pub enum Change {
    Control(Control),
    Prepare(Proposal),
    Result { operation_id: String, outcome: Value },
}
impl AgentRun {
    pub fn active(&self) -> bool {
        !matches!(self.status.as_str(), "completed" | "cancelled") || self.pending.is_some()
    }
    pub fn new(mut row: Self) -> ApiResult<Self> {
        row.submission_id = uuid_text(&row.submission_id)?;
        row.run_id = new_uuid_v7().to_string();
        row.created_at_unix_s = unix_now_s();
        row.deadline_unix_s = row.created_at_unix_s + u64::from(row.budget.wall_seconds);
        if !(1..=32).contains(&row.budget.turns)
            || !(1..=32).contains(&row.budget.tool_calls)
            || !(10..=900).contains(&row.budget.wall_seconds)
        {
            return Err(ApiError::bad_request(
                "预算须turns/tool_calls 1..32、wall_seconds 10..900",
            ));
        }
        row.spec_digest = blake3::hash(
            &serde_json::to_vec(&(
                &row.world_id,
                &row.host_fence,
                &row.control_fence,
                &row.package_id,
                &row.version,
                &row.artifact_cid,
                &row.pins,
                &row.tools,
                &row.budget,
            ))
            .unwrap(),
        )
        .to_hex()
        .to_string();
        Ok(row)
    }
    fn event(&mut self, kind: &str, data: Value) {
        self.events.push(RunEvent {
            seq: self.events.len() as u32,
            kind: kind.into(),
            data,
        });
    }
    fn revoke(&mut self) -> ApiResult<()> {
        self.fence = self.fence.checked_add(1).ok_or_else(|| conflict("运行fence耗尽"))?;
        self.lease = None;
        Ok(())
    }
    pub fn controller(&self, id: &str, fence: u32, now: u64) -> ApiResult<()> {
        if self.status != "running"
            || self.deadline_unix_s <= now
            || !self
                .lease
                .as_ref()
                .is_some_and(|l| l.controller_id == id && l.fence == fence && l.expires_at_unix_s > now)
        {
            return Err(conflict("Agent运行控制租约已失效"));
        }
        Ok(())
    }
    pub fn apply(&mut self, change: Change, now: u64) -> ApiResult<()> {
        match change {
            Change::Control(mut op) => {
                op.operation_id = uuid_text(&op.operation_id)?;
                let digest = blake3::hash(&serde_json::to_vec(&op).unwrap()).to_hex().to_string();
                if let Some(old) = self.operations.get(&op.operation_id) {
                    return if old == &digest {
                        Ok(())
                    } else {
                        Err(conflict("操作键已绑定不同请求"))
                    };
                }
                if self.operations.len() >= 96 {
                    return Err(conflict("运行操作记录上限96，不驱逐历史"));
                }
                if op.expected_revision != self.revision {
                    return Err(conflict("运行版本已变化，请刷新对账"));
                }
                match op.action.as_str() {
                    "claim" => {
                        if self.status != "ready" || self.deadline_unix_s <= now {
                            return Err(conflict("只可认领ready且预算未过期的运行"));
                        }
                        let controller = uuid_text(op.controller_id.as_deref().unwrap_or(""))?;
                        self.revoke()?;
                        self.lease = Some(RunnerLease {
                            controller_id: controller,
                            fence: self.fence,
                            expires_at_unix_s: (now + LEASE_S).min(self.deadline_unix_s),
                        });
                        self.status = "running".into();
                    }
                    "heartbeat" => {
                        let id = op.controller_id.as_deref().unwrap_or("");
                        self.controller(id, self.fence, now)?;
                        self.lease.as_mut().unwrap().expires_at_unix_s = (now + LEASE_S).min(self.deadline_unix_s);
                    }
                    "cancel" | "pause" => {
                        if !matches!(self.status.as_str(), "completed" | "cancelled") {
                            self.revoke()?;
                            self.status = if self.pending.is_some() {
                                "cancelling"
                            } else if op.action == "cancel" {
                                "cancelled"
                            } else {
                                "paused"
                            }
                            .into();
                        }
                    }
                    "recover" => {
                        if self.lease.as_ref().is_some_and(|l| l.expires_at_unix_s > now) && self.deadline_unix_s > now
                        {
                            return Err(conflict("租约尚有效"));
                        }
                        if self.status == "running" || self.status == "cancelling" {
                            self.revoke()?;
                            self.status = if self.pending.is_some() { "unknown" } else { "paused" }.into();
                        }
                    }
                    "ack_unknown" => {
                        if self.status != "unknown" {
                            return Err(conflict("仅能显式确认unknown，不重发原调用"));
                        }
                        let pending = self.pending.take();
                        self.event(
                            "unknown_acknowledged",
                            json!({"operation_id":pending.map(|p|p.operation_id),"not_retried":true}),
                        );
                        self.status = "paused".into();
                        self.revoke()?;
                    }
                    "resume" => {
                        if !matches!(self.status.as_str(), "paused" | "cancelled")
                            || self.pending.is_some()
                            || self.deadline_unix_s <= now
                            || self.turns_used >= self.budget.turns
                            || self.calls_used >= self.budget.tool_calls
                        {
                            return Err(conflict("运行无法续跑：需先对账未知调用且剩余预算有效"));
                        }
                        if let Some(fence) = op.host_fence.as_ref() {
                            let n = fence.parse::<u64>().map_err(|_| conflict("宿主fence非法"))?;
                            if n.to_string() != *fence {
                                return Err(conflict("fence须规范十进制"));
                            }
                            self.host_fence = fence.clone();
                        }
                        if let Some(fence) = op.control_fence.as_ref() {
                            let n = fence.parse::<u64>().map_err(|_| conflict("控制fence非法"))?;
                            if n.to_string() != *fence {
                                return Err(conflict("fence须规范十进制"));
                            }
                            self.control_fence = fence.clone();
                        }
                        self.revoke()?;
                        self.status = "ready".into();
                    }
                    "finish" => {
                        if self.pending.is_some() {
                            return Err(conflict("尚有派发调用，不能伪称已结束"));
                        }
                        self.revoke()?;
                        self.status = "completed".into();
                    }
                    _ => return Err(ApiError::bad_request("未知运行操作")),
                }
                self.event("lifecycle",json!({"action":op.action,"status":self.status,"host_fence":self.host_fence,"control_fence":self.control_fence}));
                self.operations.insert(op.operation_id, digest);
            }
            Change::Prepare(mut proposal) => {
                proposal.operation_id = uuid_text(&proposal.operation_id)?;
                let digest = blake3::hash(&serde_json::to_vec(&proposal).unwrap())
                    .to_hex()
                    .to_string();
                if let Some(old) = self.operations.get(&proposal.operation_id) {
                    return if old == &digest {
                        Ok(())
                    } else {
                        Err(conflict("候选键已绑定不同请求"))
                    };
                }
                self.controller(&proposal.controller_id, proposal.fence, now)?;
                if self.pending.is_some()
                    || self.turns_used >= self.budget.turns
                    || self.calls_used >= self.budget.tool_calls
                    || self.operations.len() >= 96
                {
                    return Err(conflict("已有派发或运行预算耗尽"));
                }
                if !proposal.memory.is_object()
                    || proposal.memory.to_string().len() > 16384
                    || proposal.message.len() > 2048
                    || proposal.provider.is_empty()
                    || proposal.provider.len() > 80
                    || proposal.call.as_ref().is_some_and(|c| {
                        !self.tools.contains(&c.name)
                            || !c.arguments.is_object()
                            || c.arguments.to_string().len() > 8192
                    })
                {
                    return Err(ApiError::bad_request("候选超出显式工具/上下文边界"));
                }
                crate::agent_package::credential_free(&serde_json::to_string(&proposal).unwrap())?;
                self.turns_used += 1;
                self.memory = proposal.memory.clone();
                self.event(
                    "assistant",
                    json!({"provider":proposal.provider,"message":proposal.message,"call":proposal.call}),
                );
                if let Some(call) = proposal.call {
                    self.calls_used += 1;
                    self.pending = Some(PendingTool {
                        operation_id: proposal.operation_id.clone(),
                        command_id: new_uuid_v7().to_string(),
                        call,
                    });
                    self.event("tool_dispatch_intent",json!({"operation_id":proposal.operation_id,"command_id":self.pending.as_ref().unwrap().command_id}));
                } else {
                    self.status = "completed".into();
                    self.revoke()?;
                    self.event("provider_stopped", json!({"objective_success_asserted":false}));
                }
                self.operations.insert(proposal.operation_id, digest);
            }
            Change::Result { operation_id, outcome } => {
                if self.pending.as_ref().map(|p| &p.operation_id) != Some(&operation_id) {
                    return Err(conflict("派发已确认或已被所有者对账，不重复入账"));
                }
                self.last_result = outcome.clone();
                let unknown = outcome["kind"] == "unknown";
                self.event(
                    "tool_execution_end",
                    json!({"operation_id":operation_id,"outcome":outcome}),
                );
                if unknown {
                    self.status = "unknown".into();
                    self.revoke()?;
                } else {
                    self.pending = None;
                    if self.status == "cancelling" {
                        self.status = "cancelled".into();
                        self.revoke()?;
                    } else if self.turns_used >= self.budget.turns
                        || self.calls_used >= self.budget.tool_calls
                        || now >= self.deadline_unix_s
                    {
                        self.status = "completed".into();
                        self.revoke()?;
                        self.event("budget_end", json!({}));
                    }
                }
            }
        }
        self.revision = self.revision.checked_add(1).ok_or_else(|| conflict("运行版本耗尽"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row() -> AgentRun {
        serde_json::from_value(json!({"run_id":new_uuid_v7().to_string(),"owner_id":"18446744073709551615","submission_id":new_uuid_v7().to_string(),"spec_digest":"ab","world_id":new_uuid_v7().to_string(),"host_fence":"1","control_fence":"2","faction":0,"package_id":"test.agent","version":"1.0.0","artifact_cid":"ab".repeat(32),"pins":[],"tools":["query_visible_clusters"],"budget":{"turns":2,"tool_calls":2,"wall_seconds":300},"turns_used":0,"calls_used":0,"created_at_unix_s":100,"deadline_unix_s":400,"revision":0,"fence":0,"status":"ready","lease":null,"pending":null,"memory":{},"last_result":null,"events":[],"operations":{}})).unwrap()
    }
    fn op(r: &AgentRun, action: &str) -> Control {
        Control {
            operation_id: new_uuid_v7().to_string(),
            expected_revision: r.revision,
            action: action.into(),
            controller_id: Some("00000000-0000-4000-8000-000000000001".into()),
            host_fence: None,
            control_fence: None,
        }
    }
    fn claim(r: &mut AgentRun) {
        r.apply(Change::Control(op(r, "claim")), 100).unwrap();
    }
    fn proposal(r: &AgentRun) -> Proposal {
        Proposal {
            operation_id: new_uuid_v7().to_string(),
            controller_id: r.lease.as_ref().unwrap().controller_id.clone(),
            fence: r.fence,
            provider: "explicit-test/1".into(),
            message: "仅候选".into(),
            memory: json!({"n":1}),
            call: Some(ToolDecision {
                name: "query_visible_clusters".into(),
                arguments: json!({"include_enemy":false}),
            }),
        }
    }
    #[test]
    fn proposal_replay_cannot_repeat_dispatch_or_reset_budget() {
        let mut r = row();
        claim(&mut r);
        let p = proposal(&r);
        r.apply(Change::Prepare(p.clone()), 101).unwrap();
        let revision = r.revision;
        let command = r.pending.as_ref().unwrap().command_id.clone();
        r.apply(Change::Prepare(p.clone()), 102).unwrap();
        assert_eq!(r.revision, revision);
        assert_eq!(r.calls_used, 1);
        assert_eq!(r.pending.as_ref().unwrap().command_id, command);
        r.apply(Change::Result{operation_id:p.operation_id.clone(),outcome:json!({"kind":"broker_result","response":{"tool":"query_visible_clusters","tick":30,"result":[]}})},102).unwrap();
        let revision = r.revision;
        r.apply(Change::Prepare(p), 103).unwrap();
        assert_eq!(r.revision, revision);
        assert!(r.pending.is_none());
        assert_eq!(r.calls_used, 1);
    }
    #[test]
    fn cancel_preserves_real_inflight_receipt_and_explicit_resume_fences_old_controller() {
        let mut r = row();
        claim(&mut r);
        let p = proposal(&r);
        r.apply(Change::Prepare(p.clone()), 101).unwrap();
        let cancel = op(&r, "cancel");
        r.apply(Change::Control(cancel.clone()), 102).unwrap();
        assert_eq!(r.status, "cancelling");
        assert!(r.apply(Change::Control(op(&r, "resume")), 103).is_err());
        r.apply(Change::Result{operation_id:p.operation_id,outcome:json!({"kind":"broker_result","response":{"tool":"query_visible_clusters","tick":30,"result":[]}})},103).unwrap();
        assert_eq!(r.status, "cancelled");
        let mut resume = op(&r, "resume");
        resume.host_fence = Some("3".into());
        resume.control_fence = Some("4".into());
        r.apply(Change::Control(resume), 104).unwrap();
        r.apply(Change::Control(cancel), 105).unwrap();
        assert_eq!(r.status, "ready");
        assert_eq!(r.control_fence, "4");
        assert_eq!(r.calls_used, 1);
        assert!(r.controller("00000000-0000-4000-8000-000000000001", 1, 105).is_err());
    }
    #[test]
    fn expired_pending_becomes_unknown_not_a_fresh_call() {
        let mut r = row();
        claim(&mut r);
        let p = proposal(&r);
        r.apply(Change::Prepare(p.clone()), 101).unwrap();
        r.apply(Change::Control(op(&r, "recover")), 131).unwrap();
        assert_eq!(r.status, "unknown");
        assert!(r.apply(Change::Control(op(&r, "resume")), 132).is_err());
        let n = r.events.len();
        r.apply(Change::Control(op(&r, "ack_unknown")), 133).unwrap();
        assert_eq!(r.events[n].kind, "unknown_acknowledged");
        assert!(r.pending.is_none());
        assert_eq!(r.calls_used, 1);
        r.apply(Change::Prepare(p), 134).unwrap();
        assert!(r.pending.is_none());
        assert_eq!(r.status, "paused");
    }
}
