// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! World/Rules parameter-entity registry. Actual Phase07 state, not RTS bodies or an external runtime.
use super::{
    ArenaWorld,
    authoring::{AuthoringState, valid_authoring_key},
};
use rynvek_core::RynvekErrorCode;
use rynvek_rules::{RuleModule, RuleWorldState, cpu::DEFAULT_TICK_GAS_BUDGET, ir::ValueType};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, RynvekErrorCode>;
const INVALID: RynvekErrorCode = RynvekErrorCode::DimensionMismatch;
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
impl AuthoringState {
    pub fn validate_parameter_registry(&self) -> Result<()> {
        if self.parameter_templates.len() > 16
            || self.parameter_instances.len() > 128
            || self.rule_bindings.len() > 16
            || self.rules.values().map(|r| r.rule_graph_json.len()).sum::<usize>() > 1024 * 1024
        {
            return Err(INVALID);
        }
        for (id, t) in &self.parameter_templates {
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
        for (id, i) in &self.parameter_instances {
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
        for (id, b) in &self.rule_bindings {
            if !valid_authoring_key(id) {
                return Err(INVALID);
            }
            let t = self.parameter_templates.get(&b.template_id).ok_or(INVALID)?;
            let rule = self.rules.get(&b.module_id).ok_or(INVALID)?;
            let m = rynvek_rules::decode_rir(&rule.rir).map_err(|e| e.code)?;
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
        Ok(())
    }
}
impl ParameterState {
    pub fn reconcile(&mut self, defs: &AuthoringState) -> Result<()> {
        self.entities.retain(|id, _| defs.parameter_instances.contains_key(id));
        for (id, i) in &defs.parameter_instances {
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
        if self
            .fault
            .as_ref()
            .is_some_and(|(id, _)| !defs.rule_bindings.contains_key(id))
        {
            self.fault = None;
        }
        self.validate(defs)
    }
    pub fn validate(&self, defs: &AuthoringState) -> Result<()> {
        if self.entities.len() != defs.parameter_instances.len()
            || self
                .fault
                .as_ref()
                .is_some_and(|(id, _)| !defs.rule_bindings.contains_key(id))
        {
            return Err(INVALID);
        }
        let mut serials = BTreeSet::new();
        for (id, i) in &defs.parameter_instances {
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
/// Existing generic SetWorldParam/CRDT register protocol: null deletes; no fallback or arbitrary JSON storage.
pub fn apply_registry_param(
    defs: &mut AuthoringState,
    state: &mut ParameterState,
    key: &str,
    json: &str,
) -> Result<()> {
    if json.len() > 32 * 1024 {
        return Err(INVALID);
    }
    let value: serde_json::Value = serde_json::from_str(json).map_err(|_| INVALID)?;
    macro_rules! apply {
        ($prefix:literal,$map:ident,$ty:ty) => {
            if let Some(id) = key.strip_prefix($prefix) {
                if !valid_authoring_key(id) {
                    return Err(INVALID);
                }
                if value.is_null() {
                    defs.$map.remove(id);
                } else {
                    let next: $ty = serde_json::from_value(value).map_err(|_| INVALID)?;
                    defs.$map.insert(id.into(), next);
                }
                return Ok(());
            }
        };
    }
    if let Some(id) = key.strip_prefix("rules.instances.") {
        if !valid_authoring_key(id) {
            return Err(INVALID);
        }
        if value.is_null() {
            defs.parameter_instances.remove(id);
            state.entities.remove(id);
        } else {
            let next: ParameterInstance = serde_json::from_value(value).map_err(|_| INVALID)?;
            if defs
                .parameter_instances
                .get(id)
                .is_some_and(|old| old.template_id != next.template_id)
            {
                return Err(RynvekErrorCode::StructuralMutationConflict);
            }
            defs.parameter_instances.insert(id.into(), next);
        }
        return Ok(());
    }
    apply!("rules.templates.", parameter_templates, ParameterTemplate);
    apply!("rules.bindings.", rule_bindings, RuleBinding);
    Err(RynvekErrorCode::PluginCapabilityDenied)
}
impl ArenaWorld {
    pub(super) fn step_parameter_rules(&mut self) {
        if self.authoring.rule_bindings.is_empty() {
            self.parameter_state.fault = None;
            return;
        }
        let mut staged = self.parameter_state.clone();
        let mut remaining = DEFAULT_TICK_GAS_BUDGET;
        for (id, b) in &self.authoring.rule_bindings {
            let result = (|| {
                let module = rynvek_rules::decode_rir(&self.authoring.rules[&b.module_id].rir).map_err(|e| e.code)?;
                let ids: Vec<_> = self
                    .authoring
                    .parameter_instances
                    .iter()
                    .filter(|(_, i)| i.template_id == b.template_id)
                    .map(|(id, _)| id)
                    .collect();
                let template = &self.authoring.parameter_templates[&b.template_id];
                let salt = blake3::hash(id.as_bytes());
                let input = RuleWorldState {
                    tick: self.tick,
                    world_seed: self.config.world_seed
                        ^ u64::from_le_bytes(salt.as_bytes()[..8].try_into().unwrap())
                        ^ 0x706172616d657465,
                    entity_serials: ids
                        .iter()
                        .map(|id| u64::from(self.parameter_state.entities[*id].serial))
                        .collect(),
                    components: module
                        .components
                        .iter()
                        .map(|c| {
                            ids.iter()
                                .map(|id| {
                                    i64::from(if c.name.starts_with("state.") {
                                        self.parameter_state.entities[*id].values[&c.name]
                                    } else {
                                        *self.authoring.parameter_instances[*id]
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
                let out =
                    rynvek_rules::execute_rule_tick(&rynvek_rules::emit_cpu_simd_bytecode(&module), &input, remaining)
                        .map_err(|e| e.code)?;
                remaining = remaining.checked_sub(out.gas_used).ok_or(INVALID)?;
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
                self.parameter_state.fault = Some((id.clone(), code as u16));
                return;
            }
        }
        staged.fault = None;
        self.parameter_state = staged;
    }
}
