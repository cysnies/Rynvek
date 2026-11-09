// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `Principal / Controller` 与五级控制协议（D163–D167，D216–D232）。
//!
//! 规范只冻结了 "五级控制协议" 这一名称，未给出各级定义；本实现采用（ERRATA E-030）：
//!
//! | 等级 | 名称 | 允许的工具 / 命令 |
//! |---|---|---|
//! | 0 | `Spectate` | 仅观战流（不可调用任何工具） |
//! | 1 | `Perceive` | 感知：`query_visible_clusters` / `sample_regional_fields` |
//! | 2 | `Modulate` | L3 调制：`issue_strategic_intent` / `configure_signal_beacon` |
//! | 3 | `Produce` | 经济与演化：`queue_vat_production` / `trigger_crucible_breeding` |
//! | 4 | `Sovereign` | 全部命令（含拆除、投降）；**仅人类 Principal 可持有** |
//!
//! 每个阵营同一时刻只有一个有效 [`ControlLease`]。人类申请租约时无条件抢占非人类持有者（"人类无感接管"），
//! Fencing Token 全局单调递增，被抢占方持有的旧 Token 之后的任何调用都以 `0x8001` 拒绝。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::AgentError;

/// 五级控制等级
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ControlLevel {
    Spectate = 0,
    Perceive = 1,
    Modulate = 2,
    Produce = 3,
    Sovereign = 4,
}

/// 发起控制的主体类别（Human / Script / Agent 三个天梯赛道）
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PrincipalKind {
    Human,
    Script,
    Agent,
}

/// 身份主体
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    pub kind: PrincipalKind,
}

impl Principal {
    pub fn new(id: impl Into<String>, kind: PrincipalKind) -> Self {
        Self { id: id.into(), kind }
    }
}

/// 控制租约
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlLease {
    pub holder: Principal,
    pub faction: u32,
    pub level: ControlLevel,
    pub fencing_token: u64,
    pub issued_tick: u64,
    /// 到期 Tick（不含）
    pub expires_tick: u64,
}

impl ControlLease {
    pub fn is_active(&self, tick: u64) -> bool {
        tick >= self.issued_tick && tick < self.expires_tick
    }
}

/// 租约变更事件（供 UI 提示 "已接管" / "已交还"）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeaseEvent {
    Granted {
        faction: u32,
        holder: String,
        fencing_token: u64,
    },
    Preempted {
        faction: u32,
        previous: String,
        by: String,
    },
    Released {
        faction: u32,
        holder: String,
    },
}

/// 阵营 → 租约
#[derive(Clone, Debug, Default)]
pub struct LeaseManager {
    leases: BTreeMap<u32, ControlLease>,
    next_token: u64,
    events: Vec<LeaseEvent>,
}

impl LeaseManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 申请（或续期）租约
    pub fn acquire(
        &mut self,
        principal: Principal,
        faction: u32,
        level: ControlLevel,
        tick: u64,
        duration_ticks: u64,
    ) -> Result<ControlLease, AgentError> {
        if level == ControlLevel::Sovereign && principal.kind != PrincipalKind::Human {
            return Err(AgentError::capability_denied("Sovereign 等级仅限人类 Principal"));
        }
        if duration_ticks == 0 {
            return Err(AgentError::invalid_arguments("租约时长必须大于 0"));
        }
        if let Some(current) = self
            .leases
            .get(&faction)
            .filter(|l| l.is_active(tick) && l.holder.id != principal.id)
        {
            let human_preempts = principal.kind == PrincipalKind::Human && current.holder.kind != PrincipalKind::Human;
            if !human_preempts {
                return Err(AgentError::unauthorized(format!(
                    "阵营 {faction} 的控制权由 `{}` 持有",
                    current.holder.id
                )));
            }
            self.events.push(LeaseEvent::Preempted {
                faction,
                previous: current.holder.id.clone(),
                by: principal.id.clone(),
            });
        }
        self.next_token += 1;
        let lease = ControlLease {
            holder: principal,
            faction,
            level,
            fencing_token: self.next_token,
            issued_tick: tick,
            expires_tick: tick.saturating_add(duration_ticks),
        };
        self.events.push(LeaseEvent::Granted {
            faction,
            holder: lease.holder.id.clone(),
            fencing_token: lease.fencing_token,
        });
        self.leases.insert(faction, lease.clone());
        Ok(lease)
    }

    /// 主动交还租约（Token 必须匹配）
    pub fn release(&mut self, faction: u32, fencing_token: u64) -> Result<(), AgentError> {
        match self.leases.get(&faction) {
            Some(l) if l.fencing_token == fencing_token => {
                let holder = l.holder.id.clone();
                self.leases.remove(&faction);
                self.events.push(LeaseEvent::Released { faction, holder });
                Ok(())
            }
            _ => Err(AgentError::unauthorized("Fencing Token 已失效")),
        }
    }

    /// 校验调用方持有当前有效租约
    pub fn authorize(
        &self,
        faction: u32,
        principal_id: &str,
        fencing_token: u64,
        tick: u64,
    ) -> Result<&ControlLease, AgentError> {
        let lease = self
            .leases
            .get(&faction)
            .ok_or_else(|| AgentError::unauthorized(format!("阵营 {faction} 无有效控制租约")))?;
        if lease.fencing_token != fencing_token || lease.holder.id != principal_id {
            return Err(AgentError::unauthorized("Fencing Token 已失效（控制权已被接管或续期）"));
        }
        if !lease.is_active(tick) {
            return Err(AgentError::unauthorized("控制租约已过期"));
        }
        Ok(lease)
    }

    pub fn current(&self, faction: u32) -> Option<&ControlLease> {
        self.leases.get(&faction)
    }

    /// 取出并清空事件
    pub fn drain_events(&mut self) -> Vec<LeaseEvent> {
        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_preempts_agent_and_fences_old_token() {
        let mut m = LeaseManager::new();
        let agent = Principal::new("agent:gpt", PrincipalKind::Agent);
        let human = Principal::new("human:alice", PrincipalKind::Human);
        let a = m.acquire(agent.clone(), 0, ControlLevel::Produce, 0, 1000).unwrap();
        assert!(m.authorize(0, "agent:gpt", a.fencing_token, 10).is_ok());
        let h = m.acquire(human.clone(), 0, ControlLevel::Sovereign, 20, 1000).unwrap();
        assert!(h.fencing_token > a.fencing_token);
        let e = m.authorize(0, "agent:gpt", a.fencing_token, 30).unwrap_err();
        assert_eq!(e.code.code(), 0x8001);
        // 人类持有期间 Agent 不能夺回
        assert_eq!(
            m.acquire(agent.clone(), 0, ControlLevel::Produce, 40, 100)
                .unwrap_err()
                .code
                .code(),
            0x8001
        );
        m.release(0, h.fencing_token).unwrap();
        let a2 = m.acquire(agent, 0, ControlLevel::Produce, 50, 100).unwrap();
        assert!(m.authorize(0, "agent:gpt", a2.fencing_token, 149).is_ok());
        assert!(m.authorize(0, "agent:gpt", a2.fencing_token, 150).is_err(), "到期");
        let ev = m.drain_events();
        assert!(ev.iter().any(|e| matches!(e, LeaseEvent::Preempted { .. })));
    }

    #[test]
    fn sovereign_is_human_only_and_agents_do_not_preempt_each_other() {
        let mut m = LeaseManager::new();
        let a = Principal::new("a", PrincipalKind::Agent);
        let b = Principal::new("b", PrincipalKind::Script);
        assert_eq!(
            m.acquire(a.clone(), 1, ControlLevel::Sovereign, 0, 10)
                .unwrap_err()
                .code
                .code(),
            0x5006
        );
        m.acquire(a, 1, ControlLevel::Modulate, 0, 10).unwrap();
        assert_eq!(
            m.acquire(b.clone(), 1, ControlLevel::Modulate, 5, 10)
                .unwrap_err()
                .code
                .code(),
            0x8001
        );
        assert!(
            m.acquire(b, 1, ControlLevel::Modulate, 10, 10).is_ok(),
            "旧租约过期后可申请"
        );
    }
}
