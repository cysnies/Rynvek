// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 创作规则复用既有编译器/CPU确定性解释器，只绑定真实动作，不允许直接写能量/生命/场。
use super::ArenaWorld;
use rynvek_core::{Blake3Hash, error::RynvekErrorCode};
use rynvek_rules::{
    CompileOptions, RuleModule, RuleWorldState, compile_rule_graph_json,
    cpu::DEFAULT_TICK_GAS_BUDGET,
    emit_cpu_simd_bytecode, encode_rir, execute_rule_tick,
    ir::{Opcode, ValueType},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ACTION_COMPONENTS: [&str; 6] = [
    "unit.actions.move_x",
    "unit.actions.move_y",
    "unit.actions.harvest",
    "unit.actions.attack",
    "unit.actions.defense",
    "unit.actions.emit",
];
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredRule {
    pub rule_graph_json: String,
    pub compiled_rir_blake3: [u8; 32],
    pub rir: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoringState {
    /// 最近创作提交的权威修订；普通Tick不使安全边界排队的编辑自行过期。
    pub last_edit_revision: u64,
    pub rules: BTreeMap<String, AuthoredRule>,
    pub tier1_rules: BTreeMap<String, super::tier1::Tier1Install>,
    pub parameter_templates: BTreeMap<String, super::parameter_rules::ParameterTemplate>,
    pub parameter_instances: BTreeMap<String, super::parameter_rules::ParameterInstance>,
    pub rule_bindings: BTreeMap<String, super::parameter_rules::RuleBinding>,
    /// 拒绝整个动作写集合，保留原脑动作；错误事实入根和快照，工作台可见。
    pub rule_fault: Option<(String, u16)>,
}
pub fn valid_authoring_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .split(['-', '_', '.'])
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}
pub fn compile_action_rule(source: &str) -> Result<RuleModule, RynvekErrorCode> {
    compile_action_rule_detailed(source).map_err(|e| e.code)
}
pub fn compile_action_rule_detailed(source: &str) -> rynvek_rules::error::RuleResult<RuleModule> {
    let module = compile_authored_rule_detailed(source)?;
    if super::parameter_rules::parameter_module(&module) {
        return Err(rynvek_rules::error::RuleError::new(
            RynvekErrorCode::PluginCapabilityDenied,
            "此调用只接受原动作profile",
        ));
    }
    Ok(module)
}
pub fn compile_authored_rule_detailed(source: &str) -> rynvek_rules::error::RuleResult<RuleModule> {
    if source.len() > 256 * 1024 {
        return Err(rynvek_rules::error::RuleError::new(
            RynvekErrorCode::DimensionMismatch,
            "规则图不能超过256KiB",
        ));
    }
    let module = compile_rule_graph_json(
        source,
        &CompileOptions {
            allowed_write_prefixes: Some(vec!["unit.actions.".into(), "state.".into()]),
        },
    )?;
    let action = module
        .components
        .iter()
        .all(|c| c.ty == ValueType::Q8_8 && ACTION_COMPONENTS.contains(&c.name.as_str()));
    let parameter = super::parameter_rules::parameter_module(&module)
        && module.components.len() <= 16
        && module
            .components
            .iter()
            .all(|c| matches!(c.ty, ValueType::I32 | ValueType::Q8_8 | ValueType::Bool));
    if !(action || parameter)
        || module.field_write_mask != 0
        || module.instrs.iter().any(|i| {
            matches!(
                i.op,
                Opcode::SampleField | Opcode::NeighborCount | Opcode::LoadNeighborComponent
            )
        })
    {
        return Err(rynvek_rules::error::RuleError::new(
            RynvekErrorCode::PluginCapabilityDenied,
            "仅原六动作或模板state/param profile；不混用/不开放HP/能量/场/邻居",
        ));
    }
    Ok(module)
}
pub fn checked_rule(source: &str, expected: [u8; 32]) -> Result<AuthoredRule, RynvekErrorCode> {
    let module = compile_authored_rule_detailed(source).map_err(|e| e.code)?;
    if Blake3Hash::of(&encode_rir(&module)).0 != expected {
        return Err(RynvekErrorCode::Blake3ChecksumMismatch);
    }
    Ok(AuthoredRule {
        rule_graph_json: source.into(),
        compiled_rir_blake3: expected,
        rir: encode_rir(&module),
    })
}
impl AuthoringState {
    pub fn validate(&self) -> Result<(), RynvekErrorCode> {
        self.validate_parameter_registry()?;
        if self.rules.len() > 32 {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        for (id, r) in &self.rules {
            if !valid_authoring_key(id) {
                return Err(RynvekErrorCode::DimensionMismatch);
            }
            if checked_rule(&r.rule_graph_json, r.compiled_rir_blake3)?.rir != r.rir {
                return Err(RynvekErrorCode::Blake3ChecksumMismatch);
            }
        }
        if self
            .rule_fault
            .as_ref()
            .is_some_and(|(id, _)| !self.rules.contains_key(id))
        {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        Ok(())
    }
}
impl ArenaWorld {
    pub(super) fn step_authored_rules(&mut self) {
        self.step_authored_rules_budgeted(DEFAULT_TICK_GAS_BUDGET);
    }
    pub(super) fn step_authored_rules_budgeted(&mut self, gas_budget: u64) {
        let mut remaining = gas_budget;
        self.parameter_state.tier1_gas_charged = 0;
        let original = self.parameter_state.clone();
        self.step_parameter_rules(&mut remaining);
        if self.parameter_state.fault.is_none() { self.step_tier1_rules(&original, &mut remaining); }
        if self.parameter_state.fault.is_some() {
            return; // 失败的参数候选已用过预算，不允许后续动作规则重新使用该份额度。
        }
        if self.authoring.rules.is_empty() {
            return;
        }
        let ids: Vec<_> = self.units.keys().copied().collect();
        let mut prices = vec![0u64; ids.len()];
        let mut actions: Vec<_> = ids.iter().map(|id| self.units[id].actions).collect();
        for (id, rule) in &self.authoring.rules {
            let result = (|| {
                let module = rynvek_rules::decode_rir(&rule.rir).map_err(|e| e.code)?;
                if super::parameter_rules::parameter_module(&module) {
                    return Ok(());
                }
                let state = RuleWorldState {
                    tick: self.tick,
                    world_seed: self.config.world_seed,
                    entity_serials: ids.iter().map(|i| u64::from(i.0)).collect(),
                    components: module
                        .components
                        .iter()
                        .map(|c| {
                            let a = ACTION_COMPONENTS
                                .iter()
                                .position(|n| *n == c.name)
                                .expect("编译绑定已校验");
                            actions.iter().map(|v| i64::from(v[a])).collect()
                        })
                        .collect(),
                    ..Default::default()
                };
                let out = execute_rule_tick(&emit_cpu_simd_bytecode(&module), &state, remaining).map_err(|e| e.code)?;
                remaining = remaining
                    .checked_sub(out.gas_used)
                    .ok_or(RynvekErrorCode::SandboxGasBudgetExhausted)?;
                for (e, serial) in ids.iter().enumerate() {
                    let u = &self.units[serial];
                    let price = self
                        .role_energy(u.kind, u.faction)
                        .map_or(0, |r| u64::from(r.rule_cost_micro_per_gas) * 4);
                    let q = out.entity_gas[e]
                        .checked_mul(price)
                        .ok_or(RynvekErrorCode::SandboxGasBudgetExhausted)?;
                    prices[e] = prices[e]
                        .checked_add(q)
                        .ok_or(RynvekErrorCode::SandboxGasBudgetExhausted)?;
                    if prices[e] > 0
                        && (!self.can_perform_activity(*serial)
                            || self.ecology.as_ref().expect("收费角色有账").bodies[serial]
                                .account
                                .stored_q
                                < prices[e])
                    {
                        return Err(RynvekErrorCode::SandboxGasBudgetExhausted);
                    }
                }
                for (slot, c) in module.components.iter().enumerate().filter(|(_, c)| c.write) {
                    let a = ACTION_COMPONENTS.iter().position(|n| *n == c.name).unwrap();
                    for (e, v) in out.components[slot].iter().enumerate() {
                        actions[e][a] = (*v).clamp(-256, 256) as i16;
                    }
                }
                Ok::<(), RynvekErrorCode>(())
            })();
            if let Err(code) = result {
                self.authoring.rule_fault = Some((id.clone(), code as u16));
                return;
            }
        }
        // 整个动作规则集合与所有身体费用一起提交；任何欠费保留原脑动作/原账户。
        for (id, q) in ids.iter().zip(prices) {
            if q > 0 {
                let b = self
                    .ecology
                    .as_mut()
                    .expect("收费角色有账")
                    .bodies
                    .get_mut(id)
                    .expect("活体");
                assert!(b.account.try_dissipate_q(q));
                b.activity.rule_q = b.activity.rule_q.checked_add(q).expect("子账不超过总热耗");
            }
        }
        for (id, a) in ids.into_iter().zip(actions) {
            self.units.get_mut(&id).unwrap().actions = a;
        }
        self.authoring.rule_fault = None;
    }
}
