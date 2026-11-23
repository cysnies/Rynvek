// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 通用 World 声明组件适配：复用 Arena 下沉的同一 Registry/Tier0/Tier1，不执行任意身体写入。
use crate::{engine::SimulationEngine, physics::ONE};
use rynvek_core::RynvekErrorCode;
use rynvek_rules::{
    components::{
        AuthoredRule, ParameterInstance, ParameterState, ParameterTemplate, Registry, RuleBinding, tier1::Tier1Install,
    },
    ir::ValueType,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
const ERROR: RynvekErrorCode = RynvekErrorCode::DimensionMismatch;
pub const MAX_RULES_STATE_BYTES: usize = 2 * 1024 * 1024;
pub const COMPONENT_PROFILE: &str = "rynvek.general-components.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyController {
    pub instance_id: String,
    /// 原持久 serial，不是易变稠密槽位。每个身体/实例至多一个控制绑定。
    pub entity_serial: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralRulesConfig {
    pub profile: String,
    pub world_seed: u64,
    pub gas_budget: u64,
    /// 生物费用只向显式绑定且仍存活的生命身体收取；None 表示无生物能量结算的CPU规则预算。
    pub rule_cost_micro_per_gas: Option<u32>,
    pub templates: BTreeMap<String, ParameterTemplate>,
    pub instances: BTreeMap<String, ParameterInstance>,
    pub bindings: BTreeMap<String, RuleBinding>,
    pub rule_graphs: BTreeMap<String, String>,
    pub tier1_rules: BTreeMap<String, Tier1Install>,
    pub body_controllers: Vec<BodyController>,
}

impl GeneralRulesConfig {
    pub fn compile(&self) -> Result<BTreeMap<String, AuthoredRule>, RynvekErrorCode> {
        if self.profile != COMPONENT_PROFILE
            || self.gas_budget == 0
            || self.gas_budget > rynvek_rules::DEFAULT_TICK_GAS_BUDGET
            || self.rule_graphs.len() > 32
            || self.body_controllers.len() > 128
            || self.rule_cost_micro_per_gas == Some(0)
            || self.rule_graphs.values().map(String::len).sum::<usize>() > 1024 * 1024
            || serde_json::to_vec(self).map_err(|_| ERROR)?.len() > 1024 * 1024
        {
            return Err(ERROR);
        }
        let compiled: BTreeMap<_, _> = self
            .rule_graphs
            .iter()
            .map(|(id, source)| Ok((id.clone(), AuthoredRule::compile_components(source)?)))
            .collect::<Result<_, RynvekErrorCode>>()?;
        self.registry(&compiled).validate_parameter_registry()?;
        // 未绑定或动作profile模块也不能夹带；所有源均经共享严格组件编译器检查。
        for id in compiled.keys() {
            if !rynvek_rules::components::valid_authoring_key(id) {
                return Err(ERROR);
            }
        }
        let mut serials = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for binding in &self.body_controllers {
            if binding.entity_serial == 0 || !serials.insert(binding.entity_serial) || !ids.insert(&binding.instance_id)
            {
                return Err(ERROR);
            }
            let instance = self.instances.get(&binding.instance_id).ok_or(ERROR)?;
            let template = self.templates.get(&instance.template_id).ok_or(ERROR)?;
            for name in ["state.accel_x", "state.accel_y"] {
                if template.components.get(name).is_none_or(|c| c.ty != ValueType::Q8_8) {
                    return Err(ERROR);
                }
            }
        }
        Ok(compiled)
    }
    fn registry<'a>(&'a self, compiled: &'a BTreeMap<String, AuthoredRule>) -> Registry<'a> {
        Registry {
            rules: compiled,
            tier1_rules: &self.tier1_rules,
            parameter_templates: &self.templates,
            parameter_instances: &self.instances,
            rule_bindings: &self.bindings,
        }
    }
    fn validate_control_mode(&self, life: Option<&crate::life::LifeProfile>) -> Result<(), RynvekErrorCode> {
        if let Some(profile) = life.filter(|_| !self.body_controllers.is_empty()) {
            if self.rule_cost_micro_per_gas.is_none() || profile.neural.is_some() {
                return Err(RynvekErrorCode::PluginCapabilityDenied);
            }
        } else if self.rule_cost_micro_per_gas.is_some() {
            return Err(ERROR);
        }
        Ok(())
    }
    /// RYWL 打包期即复核跨剖面引用；创世 serial 按显式身体顺序从1分配。
    pub fn validate_genesis_host(
        &self,
        bodies: &[crate::physics::Body],
        life: Option<&crate::life::LifeProfile>,
    ) -> Result<(), RynvekErrorCode> {
        self.validate_control_mode(life)?;
        for binding in &self.body_controllers {
            let index = binding.entity_serial.checked_sub(1).ok_or(ERROR)? as usize;
            if bodies.get(index).is_none_or(|b| b.fixed) {
                return Err(ERROR);
            }
        }
        Ok(())
    }
    fn validate_host(&self, world: &SimulationEngine, genesis: bool) -> Result<(), RynvekErrorCode> {
        self.validate_control_mode(world.life.as_ref().map(|l| l.profile()))?;
        for binding in &self.body_controllers {
            if let Some(body) = world.physics.body(binding.entity_serial) {
                if body.fixed {
                    return Err(ERROR);
                }
            } else if genesis || u64::from(binding.entity_serial) >= world.physics.registry.next_monotonic_serial {
                return Err(ERROR);
            }
        }
        Ok(())
    }
}

/// 显式撤销隔离态：只允许实付维护Tick走到紧邻Epoch，不执行旧/新作者规则。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleRecovery {
    pub evidence_cid: [u8; 32],
    pub epoch_tick: u64,
    pub maintenance_paid_q: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralRulesState {
    pub config: GeneralRulesConfig,
    pub components: ParameterState,
    pub tier0_gas: u64,
    pub tier1_quota: u64,
    pub paid_q: u64,
    /// 模块到私有签名闭包证据CID；空集合沿用旧快照/配置根字节。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub installed_releases: BTreeMap<String, [u8; 32]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RuleRecovery>,
}
impl GeneralRulesState {
    pub(crate) fn validate(&self, world: &SimulationEngine) -> Result<(), RynvekErrorCode> {
        let compiled = self.config.compile()?;
        if self.installed_releases.len() > 32
            || self
                .installed_releases
                .keys()
                .any(|m| !self.config.rule_graphs.contains_key(m) || self.config.tier1_rules.contains_key(m))
        {
            return Err(ERROR);
        }
        self.config.validate_host(world, false)?;
        if let Some(r) = &self.recovery {
            if r.epoch_tick % 4 != 0
                || world.clock.tick >= r.epoch_tick
                || r.epoch_tick - world.clock.tick > 3
                || self.tier0_gas != 0
                || self.tier1_quota != 0
                || self.paid_q != 0
                || r.maintenance_paid_q == 0
                || r.maintenance_paid_q % price_for_recovery(&self.config)? != 0
            {
                return Err(ERROR);
            }
        }
        self.components.validate_registry(&self.config.registry(&compiled))?;
        if self.components.fault.is_some()
            || self
                .tier0_gas
                .checked_add(self.tier1_quota)
                .is_none_or(|g| g > self.config.gas_budget)
            || self.components.tier1_gas_charged != self.tier1_quota
        {
            return Err(ERROR);
        }
        let price = u64::from(self.config.rule_cost_micro_per_gas.unwrap_or(0)) * 4;
        if self.paid_q > (self.tier0_gas + self.tier1_quota).checked_mul(price).ok_or(ERROR)? {
            return Err(ERROR);
        }
        Ok(())
    }
    pub(crate) fn config_hash(&self) -> [u8; 32] {
        let bytes = if let Some(r) = &self.recovery {
            serde_json::to_vec(&(
                "rynvek-rule-recovery/1",
                &self.config,
                &self.installed_releases,
                r.evidence_cid,
                r.epoch_tick,
            ))
        } else if self.installed_releases.is_empty() {
            serde_json::to_vec(&self.config)
        } else {
            serde_json::to_vec(&("rynvek-installed-rules/1", &self.config, &self.installed_releases))
        }
        .expect("定型组件配置可编码");
        *blake3::hash(&bytes).as_bytes()
    }
    pub(crate) fn state_hash(&self) -> [u8; 32] {
        let bytes = if let Some(r) = &self.recovery {
            serde_json::to_vec(&(
                &self.components,
                self.tier0_gas,
                self.tier1_quota,
                self.paid_q,
                r.maintenance_paid_q,
            ))
        } else {
            serde_json::to_vec(&(&self.components, self.tier0_gas, self.tier1_quota, self.paid_q))
        }
        .expect("定型组件状态");
        *blake3::hash(&bytes).as_bytes()
    }
}

fn price_for_recovery(config: &GeneralRulesConfig) -> Result<u64, RynvekErrorCode> {
    let q = u64::from(config.rule_cost_micro_per_gas.ok_or(ERROR)?)
        .checked_mul(4)
        .ok_or(ERROR)?;
    if q == 0 {
        return Err(ERROR);
    }
    Ok(q)
}
impl SimulationEngine {
    /// 只在创世显式安装，不运行包顶层代码、不隐式启用控制。定义和动态态进入原权威根。
    pub fn with_component_rules(mut self, config: GeneralRulesConfig) -> Result<Self, RynvekErrorCode> {
        if self.clock.tick != 0 || self.rules.is_some() {
            return Err(ERROR);
        }
        let compiled = config.compile()?;
        config.validate_host(&self, true)?;
        let mut components = ParameterState::default();
        components.reconcile_registry(&config.registry(&compiled))?;
        self.rules = Some(GeneralRulesState {
            config,
            components,
            tier0_gas: 0,
            tier1_quota: 0,
            paid_q: 0,
            installed_releases: BTreeMap::new(),
            recovery: None,
        });
        self.publish_commit(self.authoritative_commit([0; 32]));
        Ok(self)
    }
    /// 仅构造下一Coupling Epoch的隔离候选；调用方须紧接原Tick并原子耐久发布，不能单独保存。
    /// 不变更生命、来源账、价格、控制器、组件实例/动态参数，不覆盖Tier1。
    pub fn candidate_with_rule_install(
        &self,
        evidence: [u8; 32],
        graphs: &BTreeMap<String, String>,
    ) -> Result<Self, RynvekErrorCode> {
        if self.clock.tick.checked_add(1).is_none_or(|t| t % 4 != 0) || graphs.is_empty() || graphs.len() > 16 {
            return Err(ERROR);
        }
        let mut candidate = self.clone();
        let mut rules = self.rules.clone().ok_or(ERROR)?;
        if rules.recovery.is_some() && !graphs.keys().eq(rules.config.rule_graphs.keys())
            || graphs
                .keys()
                .any(|m| !rules.config.bindings.values().any(|b| &b.module_id == m))
        {
            return Err(ERROR);
        }
        let old_bundles: std::collections::BTreeSet<_> = graphs
            .keys()
            .filter_map(|m| rules.installed_releases.get(m).copied())
            .collect();
        // 精确依赖包组只允许整体替换，禁止半包组热换制造锁与实际执行不一致。
        if rules
            .installed_releases
            .iter()
            .any(|(m, c)| old_bundles.contains(c) && !graphs.contains_key(m))
        {
            return Err(ERROR);
        }
        for (module, graph) in graphs {
            if !rules.config.rule_graphs.contains_key(module)
                || rules.config.tier1_rules.contains_key(module)
                || !rules.config.bindings.values().any(|b| b.module_id == *module)
            {
                return Err(ERROR);
            }
            rules.config.rule_graphs.insert(module.clone(), graph.clone());
            rules.installed_releases.insert(module.clone(), evidence);
        }
        let compiled = rules.config.compile()?;
        rules.config.validate_host(self, false)?;
        rules.components.reconcile_registry(&rules.config.registry(&compiled))?;
        rules.recovery = None;
        candidate.rules = Some(rules);
        Ok(candidate)
    }
    /// 这是宿主显式隔离维护，不是热换规则：旧图不运行，新图只到Epoch才可激活。
    pub fn candidate_with_rule_recovery(
        &self,
        evidence: [u8; 32],
        graphs: &BTreeMap<String, String>,
    ) -> Result<Self, RynvekErrorCode> {
        let next = self.clock.tick.checked_add(1).ok_or(ERROR)?;
        if next % 4 == 0 {
            return Err(ERROR);
        }
        let mut candidate = self.clone();
        let mut rules = self.rules.clone().ok_or(ERROR)?;
        if self.life.is_none()
            || rules.installed_releases.is_empty()
            || !rules.config.tier1_rules.is_empty()
            || rules.config.body_controllers.is_empty()
            || !graphs.keys().eq(rules.config.rule_graphs.keys())
        {
            return Err(ERROR);
        }
        price_for_recovery(&rules.config)?;
        let mut proposed = rules.config.clone();
        proposed.rule_graphs = graphs.clone();
        proposed.compile()?;
        proposed.validate_host(self, false)?;
        let epoch_tick = next.checked_add(4 - next % 4).ok_or(ERROR)?;
        rules.recovery = Some(RuleRecovery {
            evidence_cid: evidence,
            epoch_tick,
            maintenance_paid_q: 0,
        });
        candidate.rules = Some(rules);
        Ok(candidate)
    }
    pub fn component_rules(&self) -> Option<&GeneralRulesState> {
        self.rules.as_ref()
    }

    pub(crate) fn step_component_rules(&mut self) -> Result<(), RynvekErrorCode> {
        let Some(mut rules) = self.rules.take() else {
            return Ok(());
        };
        if let Some(recovery) = rules.recovery.as_mut() {
            if self.clock.tick >= recovery.epoch_tick {
                return Err(ERROR);
            }
            let q = price_for_recovery(&rules.config)?;
            let life = self.life.as_mut().ok_or(ERROR)?;
            let mut paid = 0u64;
            for binding in &rules.config.body_controllers {
                if let Some((dense, _, _)) = self
                    .physics
                    .registry
                    .iter_alive()
                    .find(|(_, _, id)| id.monotonic_serial() == u64::from(binding.entity_serial))
                {
                    if !life.accounts.get_mut(dense)?.try_dissipate_q(q) {
                        return Err(RynvekErrorCode::ConservationLawMismatch);
                    }
                    paid = paid.checked_add(q).ok_or(ERROR)?;
                    self.physics.bodies.get_mut(dense)?.acceleration_q32 = [0, 0];
                }
            }
            if paid == 0 {
                return Err(ERROR);
            }
            // 单列宿主隔离维护热耗，不伪造已执行指令gas，也不执行撤销规则或默认控制器。
            recovery.maintenance_paid_q = paid;
            rules.tier0_gas = 0;
            rules.tier1_quota = 0;
            rules.paid_q = 0;
            rules.components.tier1_gas_charged = 0;
            self.rules = Some(rules);
            return Ok(());
        }
        // 当前对象本身已是整Tick隔离候选；任一规则/费用/输出失败都不发布候选。
        let compiled = rules.config.compile()?;
        rules.config.validate_host(self, false)?;
        let mut remaining = rules.config.gas_budget;
        let meter = rules.config.registry(&compiled).execute(
            &mut rules.components,
            self.clock.tick,
            rules.config.world_seed,
            &mut remaining,
        );
        if rules.components.fault.is_some() {
            return Err(RynvekErrorCode::PluginCapabilityDenied);
        }
        rules.tier0_gas = meter.tier0_gas;
        rules.tier1_quota = meter.tier1_quota;
        rules.paid_q = 0;
        for binding in &rules.config.body_controllers {
            let Some((dense, _, _)) = self
                .physics
                .registry
                .iter_alive()
                .find(|(_, _, id)| id.monotonic_serial() == u64::from(binding.entity_serial))
            else {
                // 已退役serial不复用：留下的声明实例仅是无身体CPU参数态，不再产生物理动作。
                continue;
            };
            let gas = meter.entity_gas.get(&binding.instance_id).copied().unwrap_or(0);
            if let Some(life) = self.life.as_mut() {
                let q = gas
                    .checked_mul(u64::from(rules.config.rule_cost_micro_per_gas.ok_or(ERROR)?))
                    .and_then(|v| v.checked_mul(4))
                    .ok_or(ERROR)?;
                if !life.accounts.get_mut(dense)?.try_dissipate_q(q) {
                    return Err(RynvekErrorCode::ConservationLawMismatch);
                }
                rules.paid_q = rules.paid_q.checked_add(q).ok_or(ERROR)?;
            }
            let entity = rules.components.entities.get(&binding.instance_id).ok_or(ERROR)?;
            let body = self.physics.bodies.get_mut(dense)?;
            let accel = |key: &str| -> Result<i64, RynvekErrorCode> {
                let raw = i64::from(*entity.values.get(key).ok_or(ERROR)?);
                // 原Q8.8 → Q32.32，无静默夹制；超原物理剖面即整Tick拒绝。
                raw.checked_mul(ONE / 256).ok_or(ERROR)
            };
            body.acceleration_q32 = if gas == 0 && self.life.is_some() {
                [0, 0]
            } else {
                [accel("state.accel_x")?, accel("state.accel_y")?]
            };
            body.validate()?;
        }
        self.rules = Some(rules);
        Ok(())
    }
}
