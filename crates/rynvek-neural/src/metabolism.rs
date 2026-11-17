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

    /// 先验支付许可：不足时整个账户不变，不生成“计算债务”掩盖免费提交。
    pub fn try_charge_spikes(&mut self, spikes: u32, cost_micro: u32) -> bool {
        let requested = u128::from(spikes) * u128::from(cost_micro) * 4;
        if requested > u128::from(self.stored_q) {
            return false;
        }
        self.charge_spikes(spikes, cost_micro);
        true
    }

    /// 实际基础代谢仅将现有能量转为热，不产生负池或虚构计算欠费。
    /// 请求不足由宿主执行其显式生存规则；此层不决定角色或死亡。
    pub fn dissipate_micro(&mut self, requested_micro: u32) -> u64 {
        let paid = self.stored_q.min(u64::from(requested_micro) * 4);
        self.stored_q -= paid;
        self.dissipated_q += paid;
        paid
    }

    pub fn export_milli(&mut self, room_milli: u64) -> u64 {
        self.export_milli_retaining(room_milli, 0)
    }

    /// 预算只是原池内保留，不是额外资源、禀赋或新耗能常数。
    pub fn export_milli_retaining(&mut self, room_milli: u64, reserve_q: u64) -> u64 {
        let milli = (self.stored_q.saturating_sub(reserve_q) / ENERGY_Q_PER_MILLI).min(room_milli);
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

#[cfg(test)]
mod budget_tests {
    use super::*;
    #[test]
    fn payment_is_atomic_at_exact_boundaries_and_never_overflows() {
        for cost in [0, 480, 800, u32::MAX] {
            for spikes in [0, 1, 96, u32::MAX] {
                let needed = u128::from(cost) * u128::from(spikes) * 4;
                for available in [
                    0,
                    needed.saturating_sub(1).min(u64::MAX.into()) as u64,
                    needed.min(u64::MAX.into()) as u64,
                    u64::MAX,
                ] {
                    let mut a = EnergyAccount {
                        received_q: available,
                        stored_q: available,
                        ..Default::default()
                    };
                    let before = a.clone();
                    let accepted = a.try_charge_spikes(spikes, cost);
                    assert_eq!(accepted, needed <= u128::from(available));
                    if !accepted {
                        assert_eq!(a, before);
                    } else {
                        assert_eq!(u128::from(a.dissipated_q), needed);
                    }
                    assert!(a.is_conserved());
                    assert_eq!(a.unfunded_q, 0);
                }
            }
        }
    }
    #[test]
    fn reserve_never_creates_energy_or_exports_fractional_budget() {
        let mut a = EnergyAccount::default();
        a.receive_field_raw(32);
        let initial = a.stored_q;
        assert_eq!(a.export_milli_retaining(u64::MAX, u64::MAX), 0);
        assert_eq!(a.stored_q, initial);
        let reserve = 96 * 800 * 4;
        let exported = a.export_milli_retaining(u64::MAX, reserve);
        assert!(exported > 0);
        assert!(a.stored_q >= reserve);
        assert!(a.stored_q < reserve + ENERGY_Q_PER_MILLI);
        assert!(a.is_conserved());
    }
}

#[cfg(test)]
mod basal_tests {
    use super::*;
    #[test]
    fn basal_spending_is_exact_partial_and_never_creates_debt() {
        for available in [0, 1, 3, 4, 4000, u64::MAX] {
            for cost in [0, 1, 1000, u32::MAX] {
                let mut a = EnergyAccount {
                    received_q: available,
                    stored_q: available,
                    ..Default::default()
                };
                let paid = a.dissipate_micro(cost);
                assert_eq!(paid, available.min(u64::from(cost) * 4));
                assert_eq!(a.stored_q, available - paid);
                assert_eq!(a.dissipated_q, paid);
                assert_eq!(a.unfunded_q, 0);
                assert!(a.is_conserved());
            }
        }
    }
}
