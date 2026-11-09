// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! LLM Agent 受控工具调用代理 `ToolBroker`（规范 9.1，GAP-26，D167，D486）。
//!
//! 一次调用的处理顺序（任一步失败即拒绝，世界继续推进，对应 `ErrorRecoveryAction::RejectCommandOnly`）：
//!
//! 1. 租约鉴权：`faction + principal_id + fencing_token` 必须匹配当前有效 [`ControlLease`](crate::ControlLease)（`0x8001`）；
//! 2. 工具查找（`0x5001`）与控制等级检查（`0x5006`）；
//! 3. 最小调用间隔 `30 Ticks`（按 Principal，`0x8004`）；
//! 4. JSON Schema 校验与强类型转换（`0x5001`）；
//! 5. 动作类工具检查阵营 `600 APM` 窗口（`0x8004`）；
//! 6. 两阶段执行：宿主 [`ToolHost::plan`] 只读地完成全部校验与查询；若耗时超过 `2,000 ms`，
//!    丢弃计划并返回 `0x8003`，**不会产生任何副作用**；否则 [`ToolHost::commit`] 原子提交；
//! 7. 结果写入该 Principal 的 [`AgentEpisodicMemory`]。
//!
//! 超时以注入的单调时钟 [`MonotonicClock`] 计量，测试使用 [`ManualClock`] 获得确定性。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::apm::ApmLimiter;
use crate::control::{ControlLease, ControlLevel, LeaseManager, Principal};
use crate::error::{AgentError, ErrorBody};
use crate::memory::{AgentEpisodicMemory, EpisodeOutcome};
use crate::schema::ToolRegistry;
use crate::tools::{ToolCall, ToolName};

/// 单次调用超时（毫秒）
pub const TOOL_CALL_TIMEOUT_MS: u64 = 2_000;
/// 同一 Principal 两次调用的最小间隔（Ticks，30 Hz 下 1.0 s）
pub const TOOL_MIN_INTERVAL_TICKS: u64 = 30;

/// 单调毫秒时钟
pub trait MonotonicClock {
    fn now_ms(&self) -> u64;
}

/// 基于 `std::time::Instant` 的真实时钟
#[derive(Clone, Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self { origin: Instant::now() }
    }
}

impl MonotonicClock for SystemClock {
    fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }
}

/// 手动推进的时钟（克隆体共享同一计数，测试中由宿主模拟耗时）
#[derive(Clone, Debug, Default)]
pub struct ManualClock {
    ms: Arc<AtomicU64>,
}

impl ManualClock {
    pub fn advance(&self, ms: u64) {
        self.ms.fetch_add(ms, Ordering::SeqCst);
    }
}

impl MonotonicClock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.ms.load(Ordering::SeqCst)
    }
}

/// 传给宿主的调用上下文
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolContext {
    pub principal: Principal,
    pub faction: u32,
    pub level: ControlLevel,
    pub tick: u64,
}

/// 工具宿主（由 Tier 4 实现，例如 `rynvek-engine::arena::ArenaToolHost`）
pub trait ToolHost {
    /// 已完成校验、等待提交的计划
    type Plan;

    /// 当前权威 Tick
    fn current_tick(&self) -> u64;

    /// 只读阶段：解析实体、校验资源与放置等；不得修改权威状态
    fn plan(&mut self, ctx: &ToolContext, call: &ToolCall) -> Result<Self::Plan, AgentError>;

    /// 提交阶段：把计划写入权威命令流并返回结果 JSON
    fn commit(&mut self, ctx: &ToolContext, plan: Self::Plan) -> Result<Value, AgentError>;
}

/// 一次工具调用请求（LLM 侧 function-call 的线上形态）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolRequest {
    pub principal_id: String,
    pub faction: u32,
    pub fencing_token: u64,
    pub tool: String,
    pub arguments: Value,
}

/// 调用结果
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolResponse {
    pub tool: String,
    pub tick: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl ToolResponse {
    pub fn is_ok(&self) -> bool {
        self.error.is_none()
    }

    /// 错误码数值（成功时为 `None`）
    pub fn error_code(&self) -> Option<u16> {
        self.error.as_ref().map(|e| e.code)
    }
}

/// 结果摘要写入记忆时的最大长度（字节）
const SUMMARY_MAX_BYTES: usize = 256;

fn summarize(value: &Value) -> String {
    let mut s = value.to_string();
    if s.len() > SUMMARY_MAX_BYTES {
        let mut cut = SUMMARY_MAX_BYTES;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    s
}

/// 受控工具调用代理
#[derive(Debug)]
pub struct ToolBroker<C: MonotonicClock> {
    registry: ToolRegistry,
    leases: LeaseManager,
    apm: BTreeMap<u32, ApmLimiter>,
    last_call_tick: BTreeMap<String, u64>,
    memories: BTreeMap<String, AgentEpisodicMemory>,
    clock: C,
    timeout_ms: u64,
    min_interval_ticks: u64,
}

impl<C: MonotonicClock> ToolBroker<C> {
    pub fn new(clock: C) -> Self {
        Self {
            registry: ToolRegistry::v1(),
            leases: LeaseManager::new(),
            apm: BTreeMap::new(),
            last_call_tick: BTreeMap::new(),
            memories: BTreeMap::new(),
            clock,
            timeout_ms: TOOL_CALL_TIMEOUT_MS,
            min_interval_ticks: TOOL_MIN_INTERVAL_TICKS,
        }
    }

    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    pub fn leases(&self) -> &LeaseManager {
        &self.leases
    }

    pub fn leases_mut(&mut self) -> &mut LeaseManager {
        &mut self.leases
    }

    /// 申请控制租约（便捷封装）
    pub fn acquire_lease(
        &mut self,
        principal: Principal,
        faction: u32,
        level: ControlLevel,
        tick: u64,
        duration_ticks: u64,
    ) -> Result<ControlLease, AgentError> {
        self.leases.acquire(principal, faction, level, tick, duration_ticks)
    }

    /// 阵营共享的 APM 窗口（人类 / 脚本的直接命令也应经此计数）
    pub fn apm_mut(&mut self, faction: u32) -> &mut ApmLimiter {
        self.apm.entry(faction).or_default()
    }

    /// 只读访问某 Principal 的情景记忆（记忆按 Principal 隔离）
    pub fn memory(&self, principal_id: &str) -> Option<&AgentEpisodicMemory> {
        self.memories.get(principal_id)
    }

    /// 处理一次工具调用
    pub fn invoke<H: ToolHost>(&mut self, host: &mut H, request: &ToolRequest) -> ToolResponse {
        let tick = host.current_tick();
        let respond = |result: Result<Value, AgentError>| match result {
            Ok(v) => ToolResponse {
                tool: request.tool.clone(),
                tick,
                result: Some(v),
                error: None,
            },
            Err(e) => ToolResponse {
                tool: request.tool.clone(),
                tick,
                result: None,
                error: Some(e.body()),
            },
        };

        // 1. 鉴权失败不写入任何记忆，避免伪造 principal_id 污染他人记忆
        let lease = match self
            .leases
            .authorize(request.faction, &request.principal_id, request.fencing_token, tick)
        {
            Ok(l) => l.clone(),
            Err(e) => return respond(Err(e)),
        };
        let tool = ToolName::parse(&request.tool);
        let result = self.dispatch(host, &lease, tool, request, tick);
        let outcome = match &result {
            Ok(v) => EpisodeOutcome::Ok { summary: summarize(v) },
            Err(e) => EpisodeOutcome::Rejected {
                code: e.code.code(),
                message: e.message.clone(),
            },
        };
        self.memories
            .entry(request.principal_id.clone())
            .or_default()
            .push(tick, tool, &request.arguments, outcome);
        respond(result)
    }

    fn dispatch<H: ToolHost>(
        &mut self,
        host: &mut H,
        lease: &ControlLease,
        tool: Option<ToolName>,
        request: &ToolRequest,
        tick: u64,
    ) -> Result<Value, AgentError> {
        // 2. 工具与等级
        let tool = tool.ok_or_else(|| AgentError::invalid_arguments(format!("未知工具 `{}`", request.tool)))?;
        if lease.level < tool.required_level() {
            return Err(AgentError::capability_denied(format!(
                "`{}` 需要控制等级 {:?}，当前租约为 {:?}",
                tool.as_str(),
                tool.required_level(),
                lease.level
            )));
        }
        // 3. 最小间隔（只有被受理的调用才刷新时间戳，被拒绝的调用不延长冷却）
        if let Some(&last) = self
            .last_call_tick
            .get(&request.principal_id)
            .filter(|&&last| tick < last + self.min_interval_ticks)
        {
            return Err(AgentError::rate_limited(format!(
                "两次工具调用至少间隔 {} Ticks（还需 {}）",
                self.min_interval_ticks,
                last + self.min_interval_ticks - tick
            )));
        }
        self.last_call_tick.insert(request.principal_id.clone(), tick);
        // 4. Schema 与强类型
        self.registry.validate(tool.as_str(), &request.arguments)?;
        let call = ToolCall::from_arguments(tool, &request.arguments)?;
        // 5. APM（先检查，提交成功后才消耗）
        if tool.is_actuating() {
            self.apm_mut(lease.faction).check(tick)?;
        }
        // 6. 两阶段执行与超时
        let ctx = ToolContext {
            principal: lease.holder.clone(),
            faction: lease.faction,
            level: lease.level,
            tick,
        };
        let started = self.clock.now_ms();
        let plan = host.plan(&ctx, &call)?;
        let elapsed = self.clock.now_ms().saturating_sub(started);
        if elapsed > self.timeout_ms {
            return Err(AgentError::timed_out(format!(
                "工具 `{}` 耗时 {elapsed} ms，超过 {} ms 上限，计划已丢弃",
                tool.as_str(),
                self.timeout_ms
            )));
        }
        let value = host.commit(&ctx, plan)?;
        if tool.is_actuating() {
            self.apm_mut(lease.faction).try_acquire(tick)?;
        }
        Ok(value)
    }
}
