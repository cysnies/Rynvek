// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 声明组件共享核心：Arena 与通用 World 使用同一验证、快照读语义与 Tier0/Tier1 执行器。
use crate::{RuleModule, RuleWorldState, cpu::DEFAULT_TICK_GAS_BUDGET, ir::ValueType};
use rynvek_core::RynvekErrorCode;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub mod tier1;
use tier1::Tier1Install;
type Result<T> = std::result::Result<T, RynvekErrorCode>;
const INVALID: RynvekErrorCode = RynvekErrorCode::DimensionMismatch;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredRule {
    pub rule_graph_json: String,
    pub compiled_rir_blake3: [u8; 32],
    pub rir: Vec<u8>,
}

pub fn valid_authoring_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .split(['-', '_', '.'])
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// 借用原定义，不构造 Arena 或复制一套规则世界。
#[derive(Clone, Copy, Debug)]
pub struct Registry<'a> {
    pub rules: &'a BTreeMap<String, AuthoredRule>,
    pub tier1_rules: &'a BTreeMap<String, Tier1Install>,
    pub parameter_templates: &'a BTreeMap<String, ParameterTemplate>,
    pub parameter_instances: &'a BTreeMap<String, ParameterInstance>,
    pub rule_bindings: &'a BTreeMap<String, RuleBinding>,
}
pub trait RegistrySource {
    fn component_registry(&self) -> Registry<'_>;
}

/// 指令Gas与QuickJS轮询配额分列；仅宿主显式定价后才成为身体费用。
#[derive(Clone, Debug, Default)]
pub struct ExecutionMeter {
    pub entity_gas: BTreeMap<String, u64>,
    pub tier0_gas: u64,
    pub tier1_quota: u64,
}
impl ExecutionMeter {
    fn add(&mut self, id: &str, gas: u64) {
        *self.entity_gas.entry(id.into()).or_default() += gas;
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterComponent {
    pub ty: ValueType,
    pub default: i32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterTemplate {
    pub schema: String,
    pub components: BTreeMap<String, ParameterComponent>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterInstance {
    pub template_id: String,
    pub parameters: BTreeMap<String, i32>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleBinding {
    pub module_id: String,
    pub template_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterEntity {
    pub serial: u32,
    pub values: BTreeMap<String, i32>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterState {
    pub next_serial: u32,
    pub tier1_gas_charged: u64,
    pub entities: BTreeMap<String, ParameterEntity>,
    pub fault: Option<(String, u16)>,
}
pub fn value_valid(ty: ValueType, v: i64) -> bool {
    match ty {
        ValueType::I32 => i32::try_from(v).is_ok(),
        ValueType::Q8_8 => i16::try_from(v).is_ok(),
        ValueType::Bool => v == 0 || v == 1,
        _ => false,
    }
}
pub fn component_name(name: &str) -> bool {
    (name.starts_with("state.") || name.starts_with("param.")) && valid_authoring_key(name)
}
pub fn parameter_module(m: &RuleModule) -> bool {
    !m.components.is_empty() && m.components.iter().all(|c| component_name(&c.name))
}
impl Registry<'_> {
    pub fn validate_parameter_registry(&self) -> Result<()> {
        if self.parameter_templates.len() > 16
            || self.parameter_instances.len() > 128
            || self.rule_bindings.len() > 16
            || self.rules.values().map(|r| r.rule_graph_json.len()).sum::<usize>() > 1024 * 1024
        {
            return Err(INVALID);
        }
        for (id, t) in self.parameter_templates {
            if !valid_authoring_key(id)
                || t.schema != "rynvek.parameter-template.v1"
                || t.components.is_empty()
                || t.components.len() > 16
                || t.components
                    .iter()
                    .any(|(n, c)| !component_name(n) || !value_valid(c.ty, i64::from(c.default)))
            {
                return Err(INVALID);
            }
        }
        for (id, i) in self.parameter_instances {
            if !valid_authoring_key(id) {
                return Err(INVALID);
            }
            let t = self.parameter_templates.get(&i.template_id).ok_or(INVALID)?;
            for (n, v) in &i.parameters {
                let c = t.components.get(n).ok_or(INVALID)?;
                if !n.starts_with("param.") || !value_valid(c.ty, i64::from(*v)) {
                    return Err(INVALID);
                }
            }
        }
        let mut writes = BTreeSet::new();
        let mut total_gas = 0u64;
        for (id, b) in self.rule_bindings {
            if !valid_authoring_key(id) {
                return Err(INVALID);
            }
            let t = self.parameter_templates.get(&b.template_id).ok_or(INVALID)?;
            let rule = self.rules.get(&b.module_id).ok_or(INVALID)?;
            let m = crate::decode_rir(&rule.rir).map_err(|e| e.code)?;
            if !parameter_module(&m) {
                return Err(RynvekErrorCode::PluginCapabilityDenied);
            }
            for c in &m.components {
                if t.components.get(&c.name).is_none_or(|d| d.ty != c.ty)
                    || c.write && !writes.insert((b.template_id.clone(), c.name.clone()))
                {
                    return Err(INVALID);
                }
            }
            let count = self
                .parameter_instances
                .values()
                .filter(|i| i.template_id == b.template_id)
                .count() as u64;
            total_gas = total_gas
                .checked_add(m.gas_estimate.checked_mul(count).ok_or(INVALID)?)
                .ok_or(INVALID)?;
        }
        if total_gas > DEFAULT_TICK_GAS_BUDGET {
            return Err(RynvekErrorCode::PluginCapabilityDenied);
        }
        self.validate_tier1(&mut writes)?;
        Ok(())
    }
}
impl ParameterState {
    pub fn reconcile(&mut self, defs: &impl RegistrySource) -> Result<()> {
        self.reconcile_registry(&defs.component_registry())
    }
    pub fn reconcile_registry(&mut self, defs: &Registry<'_>) -> Result<()> {
        self.entities.retain(|id, _| defs.parameter_instances.contains_key(id));
        for (id, i) in defs.parameter_instances {
            let t = &defs.parameter_templates[&i.template_id];
            if !self.entities.contains_key(id) {
                self.next_serial = self.next_serial.checked_add(1).ok_or(INVALID)?;
                self.entities.insert(
                    id.clone(),
                    ParameterEntity {
                        serial: self.next_serial,
                        values: t
                            .components
                            .iter()
                            .filter(|(n, _)| n.starts_with("state."))
                            .map(|(n, c)| (n.clone(), c.default))
                            .collect(),
                    },
                );
            }
        }
        if self.fault.as_ref().is_some_and(|(id, _)| {
            !defs.rule_bindings.contains_key(id)
                && !id
                    .strip_prefix("tier1:")
                    .is_some_and(|i| defs.tier1_rules.contains_key(i))
        }) {
            self.fault = None;
        }
        self.validate_registry(&defs)
    }
    pub fn validate(&self, defs: &impl RegistrySource) -> Result<()> {
        self.validate_registry(&defs.component_registry())
    }
    pub fn validate_registry(&self, defs: &Registry<'_>) -> Result<()> {
        if self.tier1_gas_charged > DEFAULT_TICK_GAS_BUDGET
            || self.entities.len() != defs.parameter_instances.len()
            || self.fault.as_ref().is_some_and(|(id, _)| {
                !defs.rule_bindings.contains_key(id)
                    && !id
                        .strip_prefix("tier1:")
                        .is_some_and(|i| defs.tier1_rules.contains_key(i))
            })
        {
            return Err(INVALID);
        }
        let mut serials = BTreeSet::new();
        for (id, i) in defs.parameter_instances {
            let t = defs.parameter_templates.get(&i.template_id).ok_or(INVALID)?;
            let e = self.entities.get(id).ok_or(INVALID)?;
            if e.serial == 0
                || e.serial > self.next_serial
                || !serials.insert(e.serial)
                || e.values.len() != t.components.keys().filter(|n| n.starts_with("state.")).count()
            {
                return Err(INVALID);
            }
            for (n, v) in &e.values {
                let c = t.components.get(n).ok_or(INVALID)?;
                if !n.starts_with("state.") || !value_valid(c.ty, i64::from(*v)) {
                    return Err(INVALID);
                }
            }
        }
        Ok(())
    }
}
impl Registry<'_> {
    fn step_parameter_rules(
        &self,
        state: &mut ParameterState,
        tick: u64,
        world_seed: u64,
        remaining: &mut u64,
        meter: &mut ExecutionMeter,
    ) {
        if self.rule_bindings.is_empty() {
            (*state).fault = None;
            return;
        }
        let mut staged = (*state).clone();
        for (id, b) in self.rule_bindings {
            let result = (|| {
                let module = crate::decode_rir(&self.rules[&b.module_id].rir).map_err(|e| e.code)?;
                let ids: Vec<_> = self
                    .parameter_instances
                    .iter()
                    .filter(|(_, i)| i.template_id == b.template_id)
                    .map(|(id, _)| id)
                    .collect();
                let template = &self.parameter_templates[&b.template_id];
                let salt = blake3::hash(id.as_bytes());
                let input = RuleWorldState {
                    tick,
                    world_seed: world_seed
                        ^ u64::from_le_bytes(salt.as_bytes()[..8].try_into().unwrap())
                        ^ 0x706172616d657465,
                    entity_serials: ids.iter().map(|id| u64::from((*state).entities[*id].serial)).collect(),
                    components: module
                        .components
                        .iter()
                        .map(|c| {
                            ids.iter()
                                .map(|id| {
                                    i64::from(if c.name.starts_with("state.") {
                                        (*state).entities[*id].values[&c.name]
                                    } else {
                                        *self.parameter_instances[*id]
                                            .parameters
                                            .get(&c.name)
                                            .unwrap_or(&template.components[&c.name].default)
                                    })
                                })
                                .collect()
                        })
                        .collect(),
                    ..Default::default()
                };
                let out = crate::execute_rule_tick(&crate::emit_cpu_simd_bytecode(&module), &input, *remaining)
                    .map_err(|e| e.code)?;
                *remaining = remaining.checked_sub(out.gas_used).ok_or(INVALID)?;
                meter.tier0_gas += out.gas_used;
                for (id, gas) in ids.iter().zip(&out.entity_gas) {
                    meter.add(id, *gas);
                }
                for (slot, c) in module.components.iter().enumerate().filter(|(_, c)| c.write) {
                    for (e, id) in ids.iter().enumerate() {
                        let v = out.components[slot][e];
                        if !value_valid(c.ty, v) {
                            return Err(INVALID);
                        }
                        staged
                            .entities
                            .get_mut(*id)
                            .ok_or(INVALID)?
                            .values
                            .insert(c.name.clone(), v as i32);
                    }
                }
                Ok::<(), RynvekErrorCode>(())
            })();
            if let Err(code) = result {
                (*state).fault = Some((id.clone(), code as u16));
                return;
            }
        }
        staged.fault = None;
        (*state) = staged;
    }
}

impl Registry<'_> {
    /// 两层规则始终读同一旧状态。任一失败不发布组件写集；宿主决定故障还是整Tick拒绝。
    pub fn execute(
        &self,
        state: &mut ParameterState,
        tick: u64,
        world_seed: u64,
        remaining: &mut u64,
    ) -> ExecutionMeter {
        state.tier1_gas_charged = 0;
        let original = state.clone();
        let mut meter = ExecutionMeter::default();
        self.step_parameter_rules(state, tick, world_seed, remaining, &mut meter);
        if state.fault.is_none() {
            self.step_tier1_rules(state, &original, tick, world_seed, remaining, &mut meter);
        }
        meter
    }
}

impl AuthoredRule {
    /// 原RuleGraph编译器与同一profile限制；不信任包内自述RIR或摘要。
    pub fn compile_components(source: &str) -> Result<Self> {
        if source.len() > 256 * 1024 {
            return Err(INVALID);
        }
        let module = crate::compile_rule_graph_json(
            source,
            &crate::CompileOptions {
                allowed_write_prefixes: Some(vec!["state.".into()]),
            },
        )
        .map_err(|e| e.code)?;
        if !parameter_module(&module)
            || module.components.len() > 16
            || module
                .components
                .iter()
                .any(|c| !matches!(c.ty, ValueType::I32 | ValueType::Q8_8 | ValueType::Bool))
            || module.field_write_mask != 0
            || module.instrs.iter().any(|i| {
                matches!(
                    i.op,
                    crate::Opcode::SampleField | crate::Opcode::NeighborCount | crate::Opcode::LoadNeighborComponent
                )
            })
        {
            return Err(RynvekErrorCode::PluginCapabilityDenied);
        }
        let rir = crate::encode_rir(&module);
        Ok(Self {
            rule_graph_json: source.into(),
            compiled_rir_blake3: *blake3::hash(&rir).as_bytes(),
            rir,
        })
    }
}
