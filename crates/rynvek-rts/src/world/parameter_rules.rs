// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! Arena 创作协议保留原字段；声明组件运行核心下沉 rules，共享而非平行复制。
use super::authoring::{AuthoringState, valid_authoring_key};
use rynvek_core::RynvekErrorCode;
pub use rynvek_rules::components::{
    ParameterComponent, ParameterEntity, ParameterInstance, ParameterState, ParameterTemplate, RuleBinding,
    component_name, parameter_module, value_valid,
};
type Result<T> = std::result::Result<T, RynvekErrorCode>;
const INVALID: RynvekErrorCode = RynvekErrorCode::DimensionMismatch;
impl rynvek_rules::components::RegistrySource for AuthoringState {
    fn component_registry(&self) -> rynvek_rules::components::Registry<'_> {
        rynvek_rules::components::Registry {
            rules: &self.rules,
            tier1_rules: &self.tier1_rules,
            parameter_templates: &self.parameter_templates,
            parameter_instances: &self.parameter_instances,
            rule_bindings: &self.rule_bindings,
        }
    }
}
impl AuthoringState {
    pub fn validate_parameter_registry(&self) -> Result<()> {
        use rynvek_rules::components::RegistrySource;
        self.component_registry().validate_parameter_registry()
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
    apply!("rules.tier1.", tier1_rules, super::tier1::Tier1Install);
    Err(RynvekErrorCode::PluginCapabilityDenied)
}
