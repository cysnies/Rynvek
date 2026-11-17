// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 个体能量的精确转移账本，不定义摄食率、初始禀赋、容量或适应度。
//! 内部单位为四分之一 micro-Energy：Q8.8 营养的 1 个原始量 = 15625，
//! 1 milli = 4000，冻结的 0.80/0.48 milli 脉冲成本分别为 3200/1920，均无取整漂移。

use serde::{Deserialize, Serialize};

pub const ENERGY_Q_PER_FIELD_RAW: u64 = 15_625;
pub const ENERGY_Q_PER_MILLI: u64 = 4_000;
/// 项目文档 §5.4 的基础脉冲成本；科技覆盖由 Arena 提供，不在神经层猜测规则。
pub const BASE_SPIKE_COST_MICRO: u32 = 800;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnergyAccount {
    pub received_q: u64,
    pub stored_q: u64,
    pub dissipated_q: u64,
    pub exported_q: u64,
    pub returned_q: u64,
    /// 已观测但未获供能的计算成本，仅诊断（饱和计数），不是负能量或未来债务。
    pub unfunded_q: u64,
}

impl EnergyAccount {
    pub fn from_milli(milli: i64) -> Self {
        let q = u64::try_from(milli)
            .ok()
            .and_then(|n| n.checked_mul(ENERGY_Q_PER_MILLI))
            .expect("初始能量必须为非负且可表示的毫单位");
        Self {
            received_q: q,
            stored_q: q,
            ..Self::default()
        }
    }

    /// 只接收整格量子；表示范围不足时拒收，调用方不得先扣场再钳制。
    pub fn receive_field_raw(&mut self, raw: u32) -> u32 {
        let accepted = u64::from(raw).min((u64::MAX - self.received_q) / ENERGY_Q_PER_FIELD_RAW) as u32;
        let q = u64::from(accepted) * ENERGY_Q_PER_FIELD_RAW;
        self.received_q += q;
        self.stored_q += q;
        accepted
    }

    pub fn charge_spikes(&mut self, spikes: u32, cost_micro: u32) -> u64 {
        let requested = u128::from(spikes) * u128::from(cost_micro) * 4;
        let paid = u128::from(self.stored_q).min(requested) as u64;
        self.stored_q -= paid;
        self.dissipated_q += paid;
        self.unfunded_q = (u128::from(self.unfunded_q) + requested - u128::from(paid)).min(u128::from(u64::MAX)) as u64;
        paid
    }

    pub fn export_milli(&mut self, room_milli: u64) -> u64 {
        let milli = (self.stored_q / ENERGY_Q_PER_MILLI).min(room_milli);
        let q = milli * ENERGY_Q_PER_MILLI;
        self.stored_q -= q;
        self.exported_q += q;
        milli
    }

    /// 死亡只回收实际剩余池；不足一个内部量子的舍入归热耗，不从躯体造价造营养。
    pub fn prepare_remains(&mut self, recycle_percent: u32) {
        assert!(recycle_percent <= 100);
        let returned = (u128::from(self.stored_q) * u128::from(recycle_percent) / 100) as u64;
        self.dissipated_q += self.stored_q - returned;
        self.stored_q = returned;
    }

    pub fn return_field_raw(&mut self, room_raw: u32) -> u32 {
        let raw = (self.stored_q / ENERGY_Q_PER_FIELD_RAW).min(u64::from(room_raw)) as u32;
        let q = u64::from(raw) * ENERGY_Q_PER_FIELD_RAW;
        self.stored_q -= q;
        self.returned_q += q;
        raw
    }

    pub fn is_conserved(&self) -> bool {
        u128::from(self.received_q)
            == u128::from(self.stored_q)
                + u128::from(self.dissipated_q)
                + u128::from(self.exported_q)
                + u128::from(self.returned_q)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_field_spike_resource_and_return_accounting() {
        for raw in [0, 1, 31, 32, 255, 256, 25_600] {
            for cost in [800, 480] {
                let mut a = EnergyAccount::default();
                assert_eq!(a.receive_field_raw(raw), raw);
                a.charge_spikes(7, cost);
                a.export_milli(3);
                a.prepare_remains(75);
                a.return_field_raw(25_600);
                assert!(a.is_conserved(), "{raw}/{cost}");
            }
        }
        let mut a = EnergyAccount::default();
        a.receive_field_raw(32);
        assert_eq!(a.stored_q, 125 * ENERGY_Q_PER_MILLI);
        assert_eq!(a.charge_spikes(3, 800), 9_600);
        assert_eq!(a.charge_spikes(5, 480), 9_600);
        assert!(a.is_conserved());
    }
    #[test]
    fn empty_and_representation_boundaries_never_mint_or_overflow() {
        let mut a = EnergyAccount::default();
        assert_eq!(a.charge_spikes(u32::MAX, u32::MAX), 0);
        assert_eq!(a.unfunded_q, u64::MAX);
        a.received_q = u64::MAX;
        a.stored_q = u64::MAX;
        assert_eq!(a.receive_field_raw(1), 0);
        assert!(a.is_conserved());
        a.export_milli(u64::MAX);
        a.prepare_remains(90);
        a.return_field_raw(u32::MAX);
        assert!(a.is_conserved());
    }
}
