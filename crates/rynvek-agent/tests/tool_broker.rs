// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! ToolBroker 受控工具调用：鉴权、等级、间隔、Schema、APM、超时零副作用、人类接管与记忆隔离。

use rynvek_agent::{
    AgentError, ControlLevel, ManualClock, Principal, PrincipalKind, ToolBroker, ToolCall, ToolContext, ToolHost,
    ToolRequest,
};
use serde_json::{Value, json};

/// 记录提交的测试宿主；`plan_cost_ms` 模拟只读阶段耗时
struct MockHost {
    tick: u64,
    clock: ManualClock,
    plan_cost_ms: u64,
    committed: Vec<ToolCall>,
}

impl MockHost {
    fn new(clock: ManualClock) -> Self {
        Self {
            tick: 0,
            clock,
            plan_cost_ms: 5,
            committed: Vec::new(),
        }
    }
}

impl ToolHost for MockHost {
    type Plan = ToolCall;

    fn current_tick(&self) -> u64 {
        self.tick
    }

    fn plan(&mut self, _ctx: &ToolContext, call: &ToolCall) -> Result<ToolCall, AgentError> {
        self.clock.advance(self.plan_cost_ms);
        if let ToolCall::QueueVatProduction { vat_entity_id, .. } = call {
            if vat_entity_id == "999" {
                return Err(AgentError::new(
                    rynvek_core::RynvekErrorCode::EntityNotFoundOrTombstoned,
                    "无此孵化槽",
                ));
            }
        }
        Ok(call.clone())
    }

    fn commit(&mut self, ctx: &ToolContext, plan: ToolCall) -> Result<Value, AgentError> {
        self.committed.push(plan.clone());
        Ok(json!({"faction": ctx.faction, "tool": plan.name().as_str()}))
    }
}

fn setup(level: ControlLevel) -> (ToolBroker<ManualClock>, MockHost, u64) {
    let clock = ManualClock::default();
    let mut broker = ToolBroker::new(clock.clone());
    let lease = broker
        .acquire_lease(Principal::new("agent:a", PrincipalKind::Agent), 0, level, 0, 1_000_000)
        .unwrap();
    (broker, MockHost::new(clock), lease.fencing_token)
}

fn req(token: u64, tool: &str, arguments: Value) -> ToolRequest {
    ToolRequest {
        principal_id: "agent:a".into(),
        faction: 0,
        fencing_token: token,
        tool: tool.into(),
        arguments,
    }
}

fn intent() -> Value {
    json!({"unit_archetype": "unit.striker", "order_kind": "AttackMove", "target_world_x": 100.25, "target_world_y": 40})
}

#[test]
fn successful_calls_commit_typed_commands() {
    let (mut b, mut h, t) = setup(ControlLevel::Produce);
    let r = b.invoke(&mut h, &req(t, "issue_strategic_intent", intent()));
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(r.result.unwrap()["tool"], "issue_strategic_intent");
    assert_eq!(
        h.committed,
        [ToolCall::IssueStrategicIntent {
            unit_archetype: "unit.striker".into(),
            order_kind: "AttackMove".into(),
            target_world_milli: (100_250, 40_000),
        }]
    );
}

#[test]
fn rejection_codes_follow_spec_registry() {
    let (mut b, mut h, t) = setup(ControlLevel::Modulate);
    // 0x8001：错误的 Fencing Token
    assert_eq!(
        b.invoke(&mut h, &req(t + 7, "issue_strategic_intent", intent()))
            .error_code(),
        Some(0x8001)
    );
    // 0x5001：未知工具
    assert_eq!(b.invoke(&mut h, &req(t, "rm_rf", json!({}))).error_code(), Some(0x5001));
    // 0x5006：Modulate 不足以生产
    h.tick = 30;
    let q = json!({"vat_entity_id": "4", "unit_archetype": "unit.forager", "genome_slug": "seed.forager", "count": 2});
    assert_eq!(
        b.invoke(&mut h, &req(t, "queue_vat_production", q)).error_code(),
        Some(0x5006)
    );
    // 0x5001：Schema 违例（枚举外的指令）
    h.tick = 60;
    let bad = json!({"unit_archetype": "all", "order_kind": "Teleport", "target_world_x": 0, "target_world_y": 0});
    assert_eq!(
        b.invoke(&mut h, &req(t, "issue_strategic_intent", bad)).error_code(),
        Some(0x5001)
    );
    // 0x8004：未满 30 Ticks
    h.tick = 89;
    assert_eq!(
        b.invoke(&mut h, &req(t, "issue_strategic_intent", intent()))
            .error_code(),
        Some(0x8004)
    );
    h.tick = 90;
    assert!(b.invoke(&mut h, &req(t, "issue_strategic_intent", intent())).is_ok());
    assert_eq!(h.committed.len(), 1, "只有合法调用被提交");
}

#[test]
fn host_errors_pass_through() {
    let (mut b, mut h, t) = setup(ControlLevel::Produce);
    let q =
        json!({"vat_entity_id": "999", "unit_archetype": "unit.forager", "genome_slug": "seed.forager", "count": 1});
    assert_eq!(
        b.invoke(&mut h, &req(t, "queue_vat_production", q)).error_code(),
        Some(0x2002)
    );
    assert!(h.committed.is_empty());
}

#[test]
fn timeout_discards_plan_without_side_effects() {
    let (mut b, mut h, t) = setup(ControlLevel::Produce);
    h.plan_cost_ms = 2_001;
    let r = b.invoke(&mut h, &req(t, "issue_strategic_intent", intent()));
    assert_eq!(r.error_code(), Some(0x8003));
    assert!(h.committed.is_empty());
    assert_eq!(b.apm_mut(0).used(0), 0, "超时调用不消耗 APM");
    // 恰好 2000 ms 不算超时
    h.plan_cost_ms = 2_000;
    h.tick = 30;
    assert!(b.invoke(&mut h, &req(t, "issue_strategic_intent", intent())).is_ok());
}

#[test]
fn shared_600_apm_window_blocks_only_actuating_tools() {
    let (mut b, mut h, t) = setup(ControlLevel::Produce);
    for i in 0..600 {
        b.apm_mut(0).try_acquire(i / 10).unwrap(); // 人类直接命令共用窗口
    }
    h.tick = 100;
    assert_eq!(
        b.invoke(&mut h, &req(t, "issue_strategic_intent", intent()))
            .error_code(),
        Some(0x8004)
    );
    h.tick = 130;
    assert!(
        b.invoke(
            &mut h,
            &req(t, "sample_regional_fields", json!({"chunk_x": 0, "chunk_y": 0}))
        )
        .is_ok()
    );
    // 窗口滑过后恢复
    h.tick = 1_900;
    assert!(b.invoke(&mut h, &req(t, "issue_strategic_intent", intent())).is_ok());
}

#[test]
fn human_takeover_fences_agent_mid_session() {
    let (mut b, mut h, t) = setup(ControlLevel::Produce);
    assert!(b.invoke(&mut h, &req(t, "issue_strategic_intent", intent())).is_ok());
    let human = b
        .acquire_lease(
            Principal::new("human:h", PrincipalKind::Human),
            0,
            ControlLevel::Sovereign,
            10,
            600,
        )
        .unwrap();
    h.tick = 40;
    assert_eq!(
        b.invoke(&mut h, &req(t, "issue_strategic_intent", intent()))
            .error_code(),
        Some(0x8001)
    );
    let mut hr = req(
        human.fencing_token,
        "configure_signal_beacon",
        json!({"beacon_entity_id": "5", "signal_channel": "field.signal_a", "signal_strength": 20, "neuromodulators": [1, 0, 0, 0]}),
    );
    hr.principal_id = "human:h".into();
    assert!(b.invoke(&mut h, &hr).is_ok());
    // 记忆隔离：两个 Principal 各自只看到自己的调用；鉴权失败不写记忆
    assert_eq!(b.memory("agent:a").unwrap().len(), 1);
    assert_eq!(b.memory("human:h").unwrap().len(), 1);
}

#[test]
fn memory_records_rejections_and_wire_format_is_stable() {
    let (mut b, mut h, t) = setup(ControlLevel::Perceive);
    let r = b.invoke(&mut h, &req(t, "issue_strategic_intent", intent()));
    let wire = serde_json::to_value(&r).unwrap();
    assert_eq!(wire["error"]["code"], 0x5006);
    assert_eq!(wire["error"]["name"], "PluginCapabilityDenied");
    assert!(wire.get("result").is_none());
    let rec = b.memory("agent:a").unwrap().recent(1);
    assert!(matches!(
        rec[0].outcome,
        rynvek_agent::EpisodeOutcome::Rejected { code: 0x5006, .. }
    ));
}
