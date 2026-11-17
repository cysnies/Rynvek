// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! Immutable Studio source capture, executed by the existing ArenaWorld Phase07.
//! This profile has no native bodies, births, RTS faction economy or life-energy contract.
use super::MetricRow;
use crate::world::{
    ArenaWorld, MatchMode,
    authoring::{AuthoredRule, AuthoringState, compile_authored_rule_detailed},
    parameter_rules::{ParameterInstance, ParameterState, ParameterTemplate, RuleBinding, parameter_module},
};
use rynvek_core::Blake3Hash;
use rynvek_rules::ir::ValueType;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MODEL_SCHEMA: &str = "rynvek.lab-parameter-model.v1";
pub const MAX_PARAMETER_ROWS: usize = 200_000;
const MAX_MODEL_BYTES: usize = 256 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterOutput {
    pub instance_id: String,
    pub component: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterModel {
    pub schema: String,
    pub templates: BTreeMap<String, ParameterTemplate>,
    pub instances: BTreeMap<String, ParameterInstance>,
    /// Original bounded RuleGraph JSON, never a script, package URL or executable.
    pub rules: BTreeMap<String, String>,
    pub bindings: BTreeMap<String, RuleBinding>,
    pub outputs: Vec<ParameterOutput>,
}
impl ParameterModel {
    pub fn compile(&self) -> Result<AuthoringState, String> {
        if self.schema != MODEL_SCHEMA
            || self.instances.is_empty()
            || self.bindings.is_empty()
            || self.rules.len() > 32
            || self.templates.len() > 16
            || self.instances.len() > 128
            || self.bindings.len() > 16
            || self.outputs.is_empty()
            || self.outputs.len() > 16
            || serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_MODEL_BYTES
        {
            return Err("模型schema/非空实例与绑定/16采样输出/256KiB等资源界限不满足".into());
        }
        let mut state = AuthoringState {
            parameter_templates: self.templates.clone(),
            parameter_instances: self.instances.clone(),
            rule_bindings: self.bindings.clone(),
            ..Default::default()
        };
        for (id, source) in &self.rules {
            let module = compile_authored_rule_detailed(source).map_err(|e| format!("{id}: {e:?}"))?;
            if !parameter_module(&module) {
                return Err("参数实验不得混入原生身体动作profile".into());
            }
            let rir = rynvek_rules::encode_rir(&module);
            state.rules.insert(
                id.clone(),
                AuthoredRule {
                    rule_graph_json: source.clone(),
                    compiled_rir_blake3: Blake3Hash::of(&rir).0,
                    rir,
                },
            );
        }
        state.validate().map_err(|e| format!("参数模型编译/绑定拒绝: {e:?}"))?;
        let mut selected = BTreeSet::new();
        for out in &self.outputs {
            let instance = self.instances.get(&out.instance_id).ok_or("采样实例不存在")?;
            if !self.templates[&instance.template_id]
                .components
                .contains_key(&out.component)
                || !selected.insert(out)
            {
                return Err("采样组件不存在或重复".into());
            }
        }
        Ok(state)
    }
    /// Source-only authorized export. Defaults restart at tick zero, not a hidden snapshot clone.
    /// Output selection is explicit and editable; capture selects the first 16 sorted state components.
    pub fn capture(source: &AuthoringState) -> Result<Self, String> {
        source.validate().map_err(|e| format!("源定义无效: {e:?}"))?;
        let modules: BTreeSet<_> = source.rule_bindings.values().map(|b| &b.module_id).collect();
        let model = Self {
            schema: MODEL_SCHEMA.into(),
            templates: source.parameter_templates.clone(),
            instances: source.parameter_instances.clone(),
            bindings: source.rule_bindings.clone(),
            rules: modules
                .into_iter()
                .map(|id| {
                    source
                        .rules
                        .get(id)
                        .map(|r| (id.clone(), r.rule_graph_json.clone()))
                        .ok_or("绑定规则不存在")
                })
                .collect::<Result<_, _>>()?,
            outputs: source
                .parameter_instances
                .iter()
                .flat_map(|(id, i)| {
                    source.parameter_templates[&i.template_id]
                        .components
                        .keys()
                        .filter(|n| n.starts_with("state."))
                        .map(move |n| ParameterOutput {
                            instance_id: id.clone(),
                            component: n.clone(),
                        })
                })
                .take(16)
                .collect(),
        };
        model.compile()?;
        Ok(model)
    }
    pub fn digest(&self) -> String {
        Blake3Hash::of(&serde_json::to_vec(self).expect("模型可序列化")).to_hex()
    }
    pub fn install(&self, world: &mut ArenaWorld) -> Result<(), String> {
        if world.config.mode != MatchMode::ParameterEntities
            || world.tick != 0
            || !world.units.is_empty()
            || !world.buildings.is_empty()
            || !world.factions.is_empty()
        {
            return Err("模型只能安装到初始无身体参数profile".into());
        }
        let definitions = self.compile()?;
        let mut dynamic = ParameterState::default();
        dynamic
            .reconcile(&definitions)
            .map_err(|e| format!("参数初始化拒绝: {e:?}"))?;
        world.authoring = definitions;
        world.parameter_state = dynamic;
        Ok(())
    }
}
pub(super) fn sample(world: &ArenaWorld, model: &ParameterModel, run: u32, out: &mut Vec<MetricRow>, root: [u8; 32]) {
    for output in &model.outputs {
        let instance = &world.authoring.parameter_instances[&output.instance_id];
        let component = &world.authoring.parameter_templates[&instance.template_id].components[&output.component];
        let value = if output.component.starts_with("state.") {
            world.parameter_state.entities[&output.instance_id].values[&output.component]
        } else {
            instance
                .parameters
                .get(&output.component)
                .copied()
                .unwrap_or(component.default)
        };
        out.push(MetricRow {
            run,
            tick: world.tick,
            faction: None,
            biomass_milli: None,
            flux_milli: None,
            neurogel_milli: None,
            biomass_harvested_total_milli: None,
            supply_used: None,
            supply_cap: None,
            units: None,
            buildings: None,
            score_milli: None,
            wild_units: None,
            pve_wave: None,
            state_root: root,
            parameter_entity: Some(output.instance_id.clone()),
            parameter_component: Some(output.component.clone()),
            parameter_type: Some(
                match component.ty {
                    ValueType::I32 => "I32",
                    ValueType::Q8_8 => "Q8_8",
                    ValueType::Bool => "Bool",
                    _ => unreachable!("已验证类型"),
                }
                .into(),
            ),
            parameter_value: Some(value),
        });
    }
}
