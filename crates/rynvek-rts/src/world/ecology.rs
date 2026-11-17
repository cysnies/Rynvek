// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 首个能量交换规则版本：复用冻结 Forager 采集率与 1:1 营养转移、实际脉冲成本。
//! 未定义的出生禀赋、基础代谢、能量场恢复基率、分泌/自然分裂规则不在这里猜测。
//! 仅显式 PvE 世界可启用；旧世界/排位保持原语义及规范根。个体池不是新脑模拟器。

use super::ArenaWorld;
use crate::{EntitySerial, FieldId, UnitKind};
use rynvek_neural::metabolism::EnergyAccount;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ECOL_TAG: [u8; 4] = *b"ECOL";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcologyVersion {
    EnergyExchangeV1,
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
    pub(super) fn validate_ecology(&self) -> bool {
        match (&self.config.ecology, &self.ecology) {
            (None, None) => true,
            (Some(_), Some(e)) => {
                self.config.mode == super::MatchMode::PveSurvival
                    && e.bodies.keys().eq(self.units.keys())
                    && e.bodies.iter().all(|(id, b)| {
                        b.account.is_conserved()
                            && b.account.returned_q == 0
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

    /// 先支付本 Epoch 真实发放，再将 Forager 的完整毫单位转入原阵营账本。
    /// 无法入账的能量仍在原个体池，亚毫量子也保留；没有第二份能量或虚构适应度。
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
            b.account.charge_spikes(std::mem::take(&mut b.pending_spikes), cost);
            let exported = if forager { b.account.export_milli(room) } else { 0 };
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
