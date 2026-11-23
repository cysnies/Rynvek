// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 显式角色契约统一真实局部摄食/阵营补给、脉冲预算、运动与生命维护。
//! 孵化槽出生实扣Biomass，自然出生由亲本原池分配；摄食继续等额扣环境场。
use super::ArenaWorld;
use crate::{EntitySerial, FactionId, FieldId, UnitKind};
use rynvek_core::Blake3Hash;
use rynvek_neural::metabolism::{ENERGY_Q_PER_MILLI, EnergyAccount};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const ECOL_TAG: [u8; 4] = *b"ECOL";

/// 显式角色生命成本；数值是实验配置，不冒充原v1已冻结的平衡值。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleEnergyContract {
    pub basal_cost_micro_per_tick: u32,
    pub reserve_ticks: u32,
    pub birth_energy_milli: u32,
    pub movement_cost_micro_per_tile: u32,
    pub synapse_maintenance_micro_per_tick: u32,
    /// 以下均为显式实验价格，不是原规范冻结值；0是明确关闭该费用。
    pub attack_cost_micro_per_shot: u32,
    pub repair_cost_micro_per_milli: u32,
    pub secretion_cost_micro_per_field_raw: u32,
    /// 非脉冲模型每个隐藏神经元每子步的显式实验价格，0明确关闭。
    pub continuous_cost_micro_per_neuron_step: u32,
    pub rule_cost_micro_per_gas: u32,
    pub material_capacity_milli: u32,
    pub material_growth_milli_per_tick: u32,
    pub material_synthesis_micro_per_milli: u32,
    pub birth_synthesis_milli: u32,
    pub activity_reserve_milli: u32,
    pub natural_reproduction: bool,
    pub replication_mutation_rate_permille: u32,
}
impl RoleEnergyContract {
    pub fn validate(self) -> Result<(), &'static str> {
        if self.birth_energy_milli == 0 || self.basal_cost_micro_per_tick == 0 {
            return Err("出生投入和基础代谢必须显式大于0");
        }
        if self.replication_mutation_rate_permille > 1000 {
            return Err("复制突变率必须在0..=1000‰");
        }
        if self.natural_reproduction && self.material_capacity_milli == 1 {
            return Err("自然复制至少需要2 milli材料容量以保留亲子完整材料量子");
        }
        if self.material_capacity_milli == 0
            && (self.material_growth_milli_per_tick != 0 || self.material_synthesis_micro_per_milli != 0)
        {
            return Err("关闭材料容量时须显式关闭材料摄取与合成价");
        }
        self.reserve_q().ok_or("代谢预留超出精确表示范围")?;
        Ok(())
    }
    pub fn birth_q(self) -> u64 {
        u64::from(self.birth_energy_milli) * ENERGY_Q_PER_MILLI
    }
    pub fn synthesis_q(self) -> u64 {
        u64::from(self.birth_synthesis_milli) * ENERGY_Q_PER_MILLI
    }
    pub fn activity_reserve_q(self) -> u64 {
        u64::from(self.activity_reserve_milli) * ENERGY_Q_PER_MILLI
    }
    pub fn reserve_q(self) -> Option<u64> {
        u64::from(self.basal_cost_micro_per_tick)
            .checked_mul(u64::from(self.reserve_ticks))?
            .checked_mul(4)
    }
}
/// 统一当前模型：配置哪些玩家角色，就对哪些角色执行有来源供能与实际成本。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcologyContract {
    pub roles: BTreeMap<UnitKind, RoleEnergyContract>,
}
impl EcologyContract {
    pub fn single(kind: UnitKind, contract: RoleEnergyContract) -> Self {
        Self {
            roles: BTreeMap::from([(kind, contract)]),
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.roles.is_empty() {
            return Err("生态角色配置不能为空");
        }
        for role in self.roles.values() {
            role.validate()?;
        }
        Ok(())
    }
}

/// 已发生动作的热耗子账，不额外增加/扣减能量；死后随遗骸保存。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityHeat {
    pub attack_q: u64,
    pub repair_q: u64,
    pub secretion_q: u64,
    pub synthesis_q: u64,
    pub continuous_q: u64,
    pub rule_q: u64,
}
impl ActivityHeat {
    pub fn total_q(&self) -> u128 {
        u128::from(self.attack_q)
            + u128::from(self.repair_q)
            + u128::from(self.secretion_q)
            + u128::from(self.synthesis_q)
            + u128::from(self.continuous_q)
            + u128::from(self.rule_q)
    }
    fn record(&mut self, kind: ActivityKind, q: u64) {
        let value = match kind {
            ActivityKind::Attack => &mut self.attack_q,
            ActivityKind::Repair => &mut self.repair_q,
            ActivityKind::Secretion => &mut self.secretion_q,
            ActivityKind::Synthesis => &mut self.synthesis_q,
        };
        *value = value.checked_add(q).expect("子账不超过守恒账户的总热耗");
    }
}
#[derive(Clone, Copy)]
pub(super) enum ActivityKind {
    Attack,
    Repair,
    Secretion,
    Synthesis,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyEnergy {
    pub material: super::material::MaterialAccount,
    pub activity: ActivityHeat,
    pub account: EnergyAccount,
    pub pending_spikes: u32,
    /// 移动计费Q32小数余量随原权威根/快照保存，不能靠小步逃费。
    pub movement_remainder: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemainsEnergy {
    pub material: super::material::MaterialAccount,
    pub activity: ActivityHeat,
    pub cell: (u32, u32),
    pub account: EnergyAccount,
}

/// 不可变的出生事实；父母死亡后仍保留谱系与出生时的基因指纹，不以适应度选亲本。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MitosisBirth {
    pub faction: FactionId,
    pub unit: UnitKind,
    pub slug: String,
    pub parent: EntitySerial,
    pub born_tick: u64,
    pub parent_generation: u32,
    pub generation: u32,
    pub parent_genome: Blake3Hash,
    pub child_genome: Blake3Hash,
    pub allocated_q: u64,
    pub allocated_material_u: u64,
    pub birth_contract: RoleEnergyContract,
}

/// 私有权威组件，按实体序号保存；遗骸保留亚格量子/满场截断余量和历史收支。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcologyState {
    pub bodies: BTreeMap<EntitySerial, BodyEnergy>,
    pub remains: BTreeMap<EntitySerial, RemainsEnergy>,
    pub births: BTreeMap<EntitySerial, MitosisBirth>,
}

// 直接对Q32原始位移平方开整数根，避免先做定点平方把小步截成0而逃费。
pub(super) fn movement_distance_raw(delta: crate::Vec2) -> u128 {
    let x = u128::from(delta.x.0.unsigned_abs());
    let y = u128::from(delta.y.0.unsigned_abs());
    (x * x + y * y).isqrt()
}

impl ArenaWorld {
    pub(super) fn role_energy(&self, kind: UnitKind, faction: FactionId) -> Option<RoleEnergyContract> {
        if faction.is_wild() {
            return None;
        }
        self.config.ecology.as_ref()?.roles.get(&kind).copied()
    }
    pub(super) fn has_neural_energy_budget(&self, id: EntitySerial) -> bool {
        let u = &self.units[&id];
        self.role_energy(u.kind, u.faction).is_some()
    }
    pub(super) fn can_perform_activity(&self, id: EntitySerial) -> bool {
        self.units.get(&id).is_some_and(|u| {
            u.defense.hp_milli > 0
                && self.role_energy(u.kind, u.faction).is_none_or(|r| {
                    r.material_capacity_milli == 0
                        || self.ecology.as_ref().expect("有角色账").bodies[&id].material.stored_u > 0
                })
        }) && !self
            .ecology
            .as_ref()
            .and_then(|e| e.bodies.get(&id))
            .is_some_and(|b| self.has_neural_energy_budget(id) && b.account.received_q > 0 && b.account.stored_q == 0)
    }
    pub(super) fn activity_price_q(&self, id: EntitySerial, kind: ActivityKind) -> u64 {
        let u = &self.units[&id];
        self.role_energy(u.kind, u.faction).map_or(0, |r| match kind {
            ActivityKind::Attack => u64::from(r.attack_cost_micro_per_shot) * 4,
            ActivityKind::Repair => u64::from(r.repair_cost_micro_per_milli) * 4,
            ActivityKind::Secretion => u64::from(r.secretion_cost_micro_per_field_raw) * 4,
            ActivityKind::Synthesis => r.synthesis_q(),
        })
    }
    /// 整个原子动作/最小输出量子必须可支付。不舍弃余额，不按请求量而按真实输出计价。
    pub(super) fn affordable_activity(&self, id: EntitySerial, requested: u64, price_q: u64) -> u64 {
        if !self.can_perform_activity(id) {
            return 0;
        }
        if price_q == 0 {
            return requested;
        }
        requested.min(
            self.ecology.as_ref().expect("收费角色已建账").bodies[&id]
                .account
                .stored_q
                / price_q,
        )
    }
    pub(super) fn charge_activity(&mut self, id: EntitySerial, kind: ActivityKind, q: u64) -> bool {
        if q == 0 {
            return true;
        }
        let b = self
            .ecology
            .as_mut()
            .expect("收费角色已建账")
            .bodies
            .get_mut(&id)
            .expect("真实身体");
        if !b.account.try_dissipate_q(q) {
            return false;
        }
        b.activity.record(kind, q);
        true
    }
    /// 自主信号发射：只对显式生态角色启用，不把免费RTS或野生身体伪装为已计价。
    /// a_emit的[-256,256]直接映射每Tick的有符号SignalA raw，最大1.0场单位。
    /// 这是有界实验映射，不是原规范已冻结的化学产率或物质守恒模型。
    pub(super) fn step_signal_emission(&mut self) {
        let ids: Vec<_> = self.units.keys().copied().collect();
        for id in ids {
            let u = &self.units[&id];
            if self.role_energy(u.kind, u.faction).is_none() || !self.can_perform_activity(id) {
                continue;
            }
            let delta = i32::from(u.actions[crate::genome::action::EMIT].clamp(-256, 256));
            if delta == 0 {
                continue;
            }
            let Some((x, y)) = u
                .position
                .tile()
                .filter(|(x, y)| *x < self.map.width && *y < self.map.height)
            else {
                continue;
            };
            let channel = crate::fields::ARENA_FIELD_CHANNELS[FieldId::SignalA.channel().index()];
            let current = i32::from(self.fields.get(FieldId::SignalA, i64::from(x), i64::from(y)));
            let room = if delta > 0 {
                i32::from(channel.max_val_q8) - current
            } else {
                current - i32::from(channel.min_val_q8)
            }
            .max(0) as u64;
            let price = self.activity_price_q(id, ActivityKind::Secretion);
            let accepted = self.affordable_activity(id, u64::from(delta.unsigned_abs()).min(room), price);
            assert!(self.charge_activity(id, ActivityKind::Secretion, accepted * price));
            let actual = self
                .fields
                .add(FieldId::SignalA, x, y, delta.signum() * accepted as i32);
            assert_eq!(actual, delta.signum() * accepted as i32);
        }
    }
    /// 后半Tick动作恰好耗尽，也必须走唯一死亡路径，不能留下可恢复的零能量幽灵。
    pub(super) fn retire_activity_exhausted(&mut self) {
        let ids: Vec<_> = self
            .units
            .keys()
            .copied()
            .filter(|id| {
                self.has_neural_energy_budget(*id)
                    && self.ecology.as_ref().is_some_and(|e| {
                        let a = &e.bodies[id].account;
                        a.received_q > 0 && a.stored_q == 0
                    })
            })
            .collect();
        for id in ids {
            self.units.get_mut(&id).expect("存在").defense.hp_milli = 0;
            self.retire_dead_unit(id);
        }
    }
    fn maintenance_q(&self, id: EntitySerial, role: RoleEnergyContract) -> u64 {
        let g = &self.units[&id].genome;
        let edges = g
            .w_in
            .iter()
            .chain(&g.w_rec)
            .chain(&g.w_out)
            .filter(|&&w| w != 0)
            .count() as u64;
        u64::from(role.basal_cost_micro_per_tick) * 4 + edges * u64::from(role.synapse_maintenance_micro_per_tick) * 4
    }
    /// 计模型隐藏状态更新数，不以连续读出幅值伪造脉冲；不冒称CPU指令或突触MAC计量。
    pub(super) fn continuous_neural_q(&self, id: EntitySerial) -> u64 {
        let u = &self.units[&id];
        if !matches!(
            u.genome.model,
            rynvek_neural::NeuronModelKind::CTRNN | rynvek_neural::NeuronModelKind::GatedMLP
        ) {
            return 0;
        }
        self.role_energy(u.kind, u.faction).map_or(0, |r| {
            u64::from(u.genome.n_hid)
                * crate::brain::BRAIN_STEPS_PER_EPOCH as u64
                * u64::from(r.continuous_cost_micro_per_neuron_step)
                * 4
        })
    }
    fn neural_reserve_q(&self, id: EntitySerial) -> u64 {
        let u = &self.units[&id];
        if !matches!(
            u.genome.model,
            rynvek_neural::NeuronModelKind::LIF | rynvek_neural::NeuronModelKind::Izhikevich
        ) {
            return self.continuous_neural_q(id);
        }
        let cost = self
            .faction(u.faction)
            .map_or(800, |f| f.modifiers().spike_cost_micro_energy) as u64;
        u64::from(u.genome.n_hid) * crate::brain::BRAIN_STEPS_PER_EPOCH as u64 * cost * 4
    }
    /// 非采集角色从既有Biomass池实际转移补给。按实体序稳定分配，满账/余额不足不扣款。
    pub(super) fn step_role_supply(&mut self) {
        let ids: Vec<_> = self.units.keys().copied().collect();
        for id in ids {
            let u = &self.units[&id];
            if u.defense.hp_milli <= 0 {
                continue;
            }
            let Some(role) = self.role_energy(u.kind, u.faction) else {
                continue;
            };
            // 启用结构模型的初始采集者需要一次真实源池施工供能，不能零结构零能量永远卡死。
            // 一旦已收过能量就不再自动补给；自然子代已有亲本转账，继续靠真实局部摄食。
            if u.kind == UnitKind::Forager
                && (role.material_capacity_milli == 0
                    || self.ecology.as_ref().expect("有角色账").bodies[&id].account.received_q > 0)
            {
                continue;
            }
            let faction = u.faction;
            let reserve = self
                .maintenance_q(id, role)
                .saturating_mul(u64::from(role.reserve_ticks).max(1));
            let movement_upper = u64::from(role.movement_cost_micro_per_tile)
                * 4
                * (crate::units::unit_spec(u.kind).speed_milli_per_s as u64).div_ceil(30_000);
            let reserve = reserve
                .saturating_add(self.neural_reserve_q(id))
                .saturating_add(role.activity_reserve_q())
                .saturating_add(movement_upper.saturating_mul(u64::from(role.reserve_ticks).max(1)));
            let target = if role.natural_reproduction {
                reserve
                    .max(role.birth_q())
                    .saturating_add(role.birth_q())
                    .saturating_add(role.synthesis_q())
            } else {
                reserve.max(role.birth_q())
            };
            let a = &self.ecology.as_ref().expect("已配置").bodies[&id].account;
            let needed = target.saturating_sub(a.stored_q).div_ceil(ENERGY_Q_PER_MILLI);
            let room = (u64::MAX - a.received_q) / ENERGY_Q_PER_MILLI;
            let available = self.faction(faction).expect("玩家").ledger.balance.biomass.max(0) as u64;
            let milli = needed.min(room).min(available);
            if milli == 0 {
                continue;
            }
            assert!(
                self.faction_mut(faction)
                    .expect("玩家")
                    .ledger
                    .try_consume(crate::ResourceKind::Biomass, milli as i64)
            );
            assert!(
                self.ecology
                    .as_mut()
                    .expect("已配置")
                    .bodies
                    .get_mut(&id)
                    .expect("活体")
                    .account
                    .try_receive_milli(milli)
            );
        }
    }

    pub(super) fn validate_ecology(&self) -> bool {
        if self.config.validate_energy_contract().is_err() {
            return false;
        }
        match (&self.config.ecology, &self.ecology) {
            (None, None) => true,
            (Some(_), Some(e)) => {
                self.config.mode == super::MatchMode::PveSurvival
                    && e.bodies.keys().eq(self.units.keys())
                    && e.bodies.iter().all(|(id, b)| {
                        b.material.is_conserved() && b.account.is_conserved() && b.activity.total_q() <= u128::from(b.account.dissipated_q)
                            && b.account.returned_q == 0
                            // 稳定权威边界必须已退役耗尽的激活个体，不能存成活体等待新食物复活。
                            && (!self.has_neural_energy_budget(*id)
                                || (self.units[id].kind==UnitKind::Forager && b.account.received_q == 0)
                                || b.account.stored_q > 0)
                            && (!self.has_neural_energy_budget(*id)
                                || (b.pending_spikes == 0 && b.account.unfunded_q == 0))
                            && b.pending_spikes
                                <= self.units[id].genome.n_hid * crate::brain::BRAIN_STEPS_PER_EPOCH as u32
                    })
                    && self.validate_mitosis_accounts(e)
                    && e.births.iter().all(|(child, birth)| {
                        birth.parent < *child
                            && child.0 < self.next_serial
                            && birth.born_tick <= self.tick
                            && birth.parent_generation.checked_add(1) == Some(birth.generation)
                            // 出生时规则不可被后来的创作调参覆盖；实付金额另由不可变转账事实核验。
                            && birth.birth_contract.validate().is_ok()
                            && birth.birth_contract.natural_reproduction
                            && birth.allocated_q == birth.birth_contract.birth_q()
                            && (e.bodies.contains_key(&birth.parent) || e.remains.contains_key(&birth.parent))
                            && (e.bodies.contains_key(child) || e.remains.contains_key(child))
                    })
                    && e.remains.iter().all(|(id, r)| {
                        !self.units.contains_key(id)
                            && id.0 < self.next_serial
                            && r.material.is_conserved()
                            && r.account.is_conserved()
                            && r.activity.total_q() <= u128::from(r.account.dissipated_q)
                            && r.cell.0 < self.map.width
                            && r.cell.1 < self.map.height
                    })
            }
            _ => false,
        }
    }

    /// 已配置角色随候选脑和候选位移原子支付，再支付实际基础/突触维护；未配置角色只诊断。
    /// 仅完整毫余量转原账本；未转出的能量仍在原池，没有第二份能量或虚构适应度。
    pub(super) fn step_metabolism(&mut self) {
        if self.ecology.is_none() {
            return;
        }
        let ids: Vec<_> = self.units.keys().copied().collect();
        let mut occupied: BTreeSet<_> = self.units.values().map(|u| u.tile()).collect();
        for id in ids {
            let unit = &self.units[&id];
            if unit.defense.hp_milli <= 0 {
                continue;
            }
            let faction = unit.faction;
            let cost = self
                .faction(faction)
                .map_or(800, |f| f.modifiers().spike_cost_micro_energy) as u32;
            let forager = unit.kind == UnitKind::Forager && !faction.is_wild();
            let budgeted = self.has_neural_energy_budget(id);
            let survival = self.role_energy(unit.kind, faction);
            let maintenance_q = survival.map_or(0, |r| self.maintenance_q(id, r));
            let reserve_q = self
                .neural_reserve_q(id)
                .saturating_add(survival.map_or(0, |r| maintenance_q.saturating_mul(u64::from(r.reserve_ticks))))
                .saturating_add(survival.map_or(0, |r| r.activity_reserve_q()));
            let room = self
                .faction(faction)
                .map_or(0, |f| (f.ledger.capacity.biomass - f.ledger.balance.biomass).max(0))
                as u64;
            let b = self
                .ecology
                .as_mut()
                .expect("已检查")
                .bodies
                .get_mut(&id)
                .expect("存活个体必有能量账本");
            if budgeted {
                assert_eq!(b.pending_spikes, 0, "新预算规则的发放已与脑状态原子支付，不得再次扣费");
            } else {
                b.account.charge_spikes(std::mem::take(&mut b.pending_spikes), cost);
            }
            let activated = survival.filter(|_| !forager || b.account.received_q > 0);
            if activated.is_some() {
                b.account.dissipate_q(maintenance_q);
            }
            let starved = activated.is_some() && b.account.stored_q == 0;
            if starved {
                self.units.get_mut(&id).expect("存活个体").defense.hp_milli = 0;
                self.retire_dead_unit(id);
                continue;
            }
            // 出生优先于资源盈余转出；只用既有出生投入和下一Epoch/代谢预留决定门槛。
            let birth_q = survival.filter(|r| r.natural_reproduction).map_or(0, |r| r.birth_q());
            let parent_reserve_q = reserve_q.max(birth_q);
            if survival.is_some_and(|r| r.natural_reproduction) {
                self.try_mitosis(id, parent_reserve_q, &mut occupied);
            }
            // 亲子都保留既有出生投入；额外积累一份出生预算，不逐Tick把繁殖所需余量转走。
            let retain_q = parent_reserve_q.saturating_add(birth_q).saturating_add(
                survival
                    .filter(|r| r.natural_reproduction)
                    .map_or(0, |r| r.synthesis_q()),
            );
            let b = self
                .ecology
                .as_mut()
                .expect("已检查")
                .bodies
                .get_mut(&id)
                .expect("仍存活");
            let exported = if forager {
                b.account.export_milli_retaining(room, retain_q)
            } else {
                0
            };
            if exported > 0 {
                let ledger = &mut self.faction_mut(faction).expect("玩家阵营存在").ledger;
                let accepted = ledger.deposit(crate::ResourceKind::Biomass, exported as i64);
                assert_eq!(accepted, exported as i64, "转移前已检查同一权威账本容量");
                ledger.biomass_harvested_total += accepted;
            }
        }
    }

    /// 不因网格量程截断而消灭残骸能量。无自定义衰减率；只归还可容纳的整格量子。
    pub(super) fn step_ecology_returns(&mut self) {
        let Some(ecology) = &mut self.ecology else {
            return;
        };
        for r in ecology.remains.values_mut() {
            let (x, y) = r.cell;
            let room = 25_600 - self.fields.get(FieldId::Nutrient, x as i64, y as i64) as u32;
            let returned = r.account.return_field_raw(room);
            let actual = self.fields.add(FieldId::Nutrient, x, y, returned as i32);
            assert_eq!(actual, returned as i32);
        }
    }
}

#[cfg(test)]
mod role_energy_tests {
    use super::*;
    use crate::OrderKind;
    use crate::commands::{CommandAck, RtsCommand, UnitSelector};
    use crate::genome::Genome;
    use crate::{ArenaMap, ArenaRuntime, BuildingKind, MatchConfig, TerrainTileKind, Vec2};
    const PLAYER: FactionId = FactionId(0);
    fn role() -> RoleEnergyContract {
        RoleEnergyContract {
            basal_cost_micro_per_tick: 10,
            reserve_ticks: 4,
            birth_energy_milli: 1000,
            movement_cost_micro_per_tile: 11,
            synapse_maintenance_micro_per_tick: 1,
            natural_reproduction: false,
            attack_cost_micro_per_shot: 0,
            repair_cost_micro_per_milli: 0,
            rule_cost_micro_per_gas: 0,
            material_capacity_milli: 0,
            material_growth_milli_per_tick: 0,
            material_synthesis_micro_per_milli: 0,
            continuous_cost_micro_per_neuron_step: 0,
            secretion_cost_micro_per_field_raw: 0,
            birth_synthesis_milli: 0,
            activity_reserve_milli: 0,
            replication_mutation_rate_permille: 50,
        }
    }
    fn world(roles: BTreeMap<UnitKind, RoleEnergyContract>) -> ArenaWorld {
        let mut c = MatchConfig::pve(57, 1);
        c.ecology = Some(EcologyContract { roles });
        ArenaWorld::with_map(
            c,
            ArenaMap::from_tiles(96, 96, vec![TerrainTileKind::ConductiveBasalt; 96 * 96]),
        )
    }
    #[test]
    fn every_supplied_role_uses_actual_faction_pool_and_replays_the_same_root() {
        let kinds = [
            UnitKind::Striker,
            UnitKind::Carapace,
            UnitKind::Spitter,
            UnitKind::Weaver,
            UnitKind::Leviathan,
        ];
        let mut w = world(kinds.into_iter().map(|k| (k, role())).collect());
        for (i, kind) in kinds.into_iter().enumerate().skip(1) {
            w.spawn_unit(
                PLAYER,
                kind,
                Genome::seed(kind),
                Vec2::tile_center(20 + i as u32 * 4, 20),
            );
        }
        let initial = w.faction(PLAYER).unwrap().ledger.balance.biomass;
        w.step();
        let accounts: Vec<_> = w
            .units
            .iter()
            .filter(|(_, u)| kinds.contains(&u.kind))
            .map(|(id, _)| &w.ecology.as_ref().unwrap().bodies[id].account)
            .collect();
        assert_eq!(accounts.len(), 5);
        assert_eq!(
            (initial - w.faction(PLAYER).unwrap().ledger.balance.biomass) as u128 * 4000,
            accounts.iter().map(|a| u128::from(a.received_q)).sum::<u128>()
        );
        assert!(
            accounts
                .iter()
                .all(|a| a.received_q > 0 && a.dissipated_q > 40 && a.unfunded_q == 0 && a.is_conserved())
        );
        let mut a = ArenaRuntime::new([121; 16], w);
        let cp = a.checkpoint().unwrap();
        let mut b = ArenaRuntime::restore([121; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
        assert!(
            a.transact_at(a.tick + 2, |_| Err::<(), _>(
                rynvek_core::RynvekErrorCode::SimulationInvariantViolated
            ))
            .is_err()
        );
        assert_eq!(a.checkpoint().unwrap().bytes, cp.bytes);
        for _ in 0..6 {
            a.step().unwrap();
            b.step().unwrap();
            assert_eq!(a.state_root(), b.state_root());
        }
    }
    #[test]
    fn dry_faction_cannot_give_a_combat_role_free_life_or_neural_debt() {
        let mut w = world(BTreeMap::from([(UnitKind::Striker, role())]));
        let id = w.units.values().find(|u| u.kind == UnitKind::Striker).unwrap().id;
        w.faction_mut(PLAYER).unwrap().ledger.balance.biomass = 0;
        w.step();
        assert!(!w.units.contains_key(&id));
        let a = &w.ecology.as_ref().unwrap().remains[&id].account;
        assert_eq!((a.received_q, a.stored_q, a.unfunded_q), (0, 0, 0));
        assert!(a.is_conserved());
    }
    #[test]
    fn paid_real_movement_carries_subquantum_cost_and_failed_move_is_atomic() {
        assert_eq!(
            movement_distance_raw(Vec2::new(rynvek_core::Q32_32(1), rynvek_core::Q32_32::ZERO)),
            1
        );
        for cost in [11, u32::MAX] {
            let mut r = role();
            r.movement_cost_micro_per_tile = cost;
            let mut w = world(BTreeMap::from([(UnitKind::Striker, r)]));
            w.faction_mut(PLAYER).unwrap().ledger.balance.biomass = 1;
            let id = w.units.values().find(|u| u.kind == UnitKind::Striker).unwrap().id;
            w.step_role_supply();
            assert_eq!(w.faction(PLAYER).unwrap().ledger.balance.biomass, 0);
            let p = w.units[&id].position;
            w.apply_command(
                PLAYER,
                &RtsCommand::IssueOrder {
                    units: UnitSelector::Ids(vec![id]),
                    order: OrderKind::Move,
                    target: p + Vec2::tile_center(5, 0),
                },
            )
            .unwrap();
            let before = w.ecology.as_ref().unwrap().bodies[&id].clone();
            let mut total = 0u128;
            for _ in 0..12 {
                let old = w.units[&id].position;
                w.step_movement();
                total += movement_distance_raw(w.units[&id].position - old) * u128::from(cost) * 4;
            }
            let body = &w.ecology.as_ref().unwrap().bodies[&id];
            if cost == u32::MAX {
                assert_eq!(w.units[&id].position, p);
                assert_eq!(body, &before);
            } else {
                assert_ne!(w.units[&id].position, p);
                assert_eq!(
                    body.account.dissipated_q - before.account.dissipated_q,
                    (total >> 32) as u64
                );
                assert_eq!(body.movement_remainder, total as u32);
                assert!(body.account.is_conserved());
                let (bytes, root) = w.encode_snapshot([122; 16], 1, [0; 32]).unwrap();
                let loaded = ArenaWorld::decode_snapshot(&bytes).unwrap();
                assert_eq!(loaded.ecology, w.ecology);
                assert_eq!(loaded.state_hash().0, root.state_root);
            }
        }
    }
    #[test]
    fn combat_production_requires_its_real_birth_payment_before_shared_spawn() {
        let mut w = world(BTreeMap::from([(UnitKind::Striker, role())]));
        let origin = (0..96)
            .flat_map(|y| (0..96).map(move |x| (x, y)))
            .find(|&p| w.validate_placement(PLAYER, BuildingKind::SpawningVat, p).is_ok())
            .unwrap();
        let CommandAck::BuildingPlaced { id: vat } = w
            .apply_command(
                PLAYER,
                &RtsCommand::PlaceBuilding {
                    kind: BuildingKind::SpawningVat,
                    origin,
                },
            )
            .unwrap()
        else {
            panic!("真实建设")
        };
        for _ in 0..1000 {
            if w.buildings[&vat].complete {
                break;
            }
            w.step();
        }
        assert!(w.buildings[&vat].complete);
        w.apply_command(
            PLAYER,
            &RtsCommand::QueueUnit {
                vat,
                unit: UnitKind::Striker,
                genome_slug: "seed.striker.v1".into(),
                count: 1,
            },
        )
        .unwrap();
        for _ in 0..1000 {
            if w.buildings[&vat].queue.front().unwrap().remaining_ticks <= 1 {
                break;
            }
            w.step();
        }
        assert_eq!(w.buildings[&vat].queue.front().unwrap().remaining_ticks, 1);
        let old = w.next_serial();
        let count = w.units.len();
        w.faction_mut(PLAYER).unwrap().ledger.balance.biomass = 999;
        w.step_economy();
        assert_eq!(w.next_serial(), old);
        assert_eq!(w.units.len(), count);
        assert_eq!(w.faction(PLAYER).unwrap().ledger.balance.biomass, 999);
        w.faction_mut(PLAYER).unwrap().ledger.balance.biomass += 1;
        w.step_economy();
        assert!(w.buildings[&vat].queue.is_empty());
        assert_eq!(w.faction(PLAYER).unwrap().ledger.balance.biomass, 0);
        let a = &w.ecology.as_ref().unwrap().bodies[&EntitySerial(old)].account;
        assert_eq!(a.received_q, 4_000_000);
        assert_eq!(a.stored_q, 4_000_000);
        assert!(a.is_conserved());
    }
}
