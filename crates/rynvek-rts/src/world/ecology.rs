// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 显式Forager契约统一局部摄食、实际脉冲预算与首次真实摄食后的生存成本。
//! 出生池始终为空；摄食从既有环境场扣款，不杜撰出生禀赋或给其他角色断供。
use super::ArenaWorld;
use crate::{EntitySerial, FieldId, UnitKind};
use rynvek_neural::metabolism::EnergyAccount;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ECOL_TAG: [u8; 4] = *b"ECOL";

/// 数值为显式实验参数，并非原v1冻结平衡；首次摄食由原账本received_q推导。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForagerEcologyContract {
    pub basal_cost_micro_per_tick: u32,
    pub reserve_ticks: u32,
}

impl ForagerEcologyContract {
    pub fn validate(self) -> Result<(), &'static str> {
        if self.basal_cost_micro_per_tick == 0 {
            return Err("实验基础代谢成本必须显式大于0");
        }
        self.reserve_q().ok_or("实验代谢预留超出精确能量表示范围")?;
        Ok(())
    }

    pub fn reserve_q(self) -> Option<u64> {
        u64::from(self.basal_cost_micro_per_tick)
            .checked_mul(u64::from(self.reserve_ticks))?
            .checked_mul(4)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyEnergy {
    pub account: EnergyAccount,
    pub pending_spikes: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemainsEnergy {
    pub cell: (u32, u32),
    pub account: EnergyAccount,
}

/// 私有权威组件，按实体序号保存；遗骸保留亚格量子/满场截断余量和历史收支。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcologyState {
    pub bodies: BTreeMap<EntitySerial, BodyEnergy>,
    pub remains: BTreeMap<EntitySerial, RemainsEnergy>,
}

impl ArenaWorld {
    /// 仅已核定真实摄食契约的玩家Forager进入新预算规则；其他角色不擅自断供。
    pub(super) fn has_neural_energy_budget(&self, id: EntitySerial) -> bool {
        self.config.ecology.is_some() && self.units[&id].kind == UnitKind::Forager && !self.units[&id].faction.is_wild()
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
                        b.account.is_conserved()
                            && b.account.returned_q == 0
                            // 稳定权威边界必须已退役耗尽的激活个体，不能存成活体等待新食物复活。
                            && (!self.has_neural_energy_budget(*id)
                                || b.account.received_q == 0
                                || b.account.stored_q > 0)
                            && (!self.has_neural_energy_budget(*id)
                                || (b.pending_spikes == 0 && b.account.unfunded_q == 0))
                            && b.pending_spikes
                                <= self.units[id].genome.n_hid * crate::brain::BRAIN_STEPS_PER_EPOCH as u32
                    })
                    && e.remains.iter().all(|(id, r)| {
                        !self.units.contains_key(id)
                            && id.0 < self.next_serial
                            && r.account.is_conserved()
                            && r.cell.0 < self.map.width
                            && r.cell.1 < self.map.height
                    })
            }
            _ => false,
        }
    }

    /// Forager随候选脑原子支付，此处保留下一Epoch预算；其他角色仅记原诊断成本。
    /// 仅完整毫余量转原账本；未转出的能量仍在原池，没有第二份能量或虚构适应度。
    pub(super) fn step_metabolism(&mut self) {
        if self.ecology.is_none() {
            return;
        }
        let ids: Vec<_> = self.units.keys().copied().collect();
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
            // 只按冻结模型上界保留下一Epoch预算：隐藏神经元×4子步×冻结发放成本。
            // 非脉冲模型没有已定义的基础成本，不能冒充脉冲模型收取同一费用。
            let reserve_q = if budgeted
                && matches!(
                    unit.genome.model,
                    rynvek_neural::NeuronModelKind::LIF | rynvek_neural::NeuronModelKind::Izhikevich
                ) {
                u64::from(unit.genome.n_hid) * crate::brain::BRAIN_STEPS_PER_EPOCH as u64 * u64::from(cost) * 4
            } else {
                0
            };
            let survival = if budgeted { self.config.ecology } else { None };
            let reserve_q = reserve_q.saturating_add(survival.map_or(0, |c| c.reserve_q().expect("创建/恢复已验证")));
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
            let activated = survival.filter(|_| b.account.received_q > 0);
            if let Some(contract) = activated {
                b.account.dissipate_micro(contract.basal_cost_micro_per_tick);
            }
            let exhausted_before_export = activated.is_some() && b.account.stored_q == 0;
            let exported = if forager && !exhausted_before_export {
                b.account.export_milli_retaining(room, reserve_q)
            } else {
                0
            };
            // 显式零预留允许余量恰好全部转出；仍须在同一稳定边界退役，不能留零能活体。
            let starved = activated.is_some() && b.account.stored_q == 0;
            if starved {
                // 在代谢边界通过同一死亡/遗骸路径退役，不能随后再攻击、治疗或重新采集。
                self.units.get_mut(&id).expect("存活个体").defense.hp_milli = 0;
                self.retire_dead_unit(id);
            }
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
