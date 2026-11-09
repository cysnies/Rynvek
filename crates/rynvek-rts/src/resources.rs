// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 三大战略资源账本（项目文档 §5.2.1 与 §5.1.2 Starter Kit）。
//!
//! 全部数量以 `i64 milli-unit` 记账（1.0 资源 = 1000 milli），加减均为精确整数运算。
//! 存储上限由主脑核心（`2000 / 1000 / 500`）、通量反应堆（通量电容 `+500`）、突触精炼厂
//! （胶质上限 `+250`）与 `tech.flux_capacitance`（基础电容 `+1000`）共同决定，超出上限的产出被丢弃。

use rynvek_core::RynvekErrorCode;
use serde::{Deserialize, Serialize};

use crate::ids::ResourceKind;
use crate::math::MILLI;

/// 资源视觉色值（§5.2.1）
pub const fn resource_color_hex(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Biomass => "#82B96B",
        ResourceKind::Flux => "#A3D65C",
        ResourceKind::Neurogel => "#5CE1E6",
    }
}

/// 三资源数量（毫单位）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceAmounts {
    pub biomass: i64,
    pub flux: i64,
    pub neurogel: i64,
}

impl ResourceAmounts {
    pub const ZERO: Self = Self::new(0, 0, 0);

    /// 由毫单位构造
    pub const fn new(biomass: i64, flux: i64, neurogel: i64) -> Self {
        Self {
            biomass,
            flux,
            neurogel,
        }
    }

    /// 由整数资源单位构造（`× 1000`）
    pub const fn whole(biomass: i64, flux: i64, neurogel: i64) -> Self {
        Self::new(biomass * MILLI, flux * MILLI, neurogel * MILLI)
    }

    pub const fn get(&self, kind: ResourceKind) -> i64 {
        match kind {
            ResourceKind::Biomass => self.biomass,
            ResourceKind::Flux => self.flux,
            ResourceKind::Neurogel => self.neurogel,
        }
    }

    pub fn get_mut(&mut self, kind: ResourceKind) -> &mut i64 {
        match kind {
            ResourceKind::Biomass => &mut self.biomass,
            ResourceKind::Flux => &mut self.flux,
            ResourceKind::Neurogel => &mut self.neurogel,
        }
    }

    pub const fn plus(self, o: Self) -> Self {
        Self::new(self.biomass + o.biomass, self.flux + o.flux, self.neurogel + o.neurogel)
    }

    pub const fn minus(self, o: Self) -> Self {
        Self::new(self.biomass - o.biomass, self.flux - o.flux, self.neurogel - o.neurogel)
    }

    /// 逐项乘以 `numer / denom`（向零截断）
    pub const fn scaled(self, numer: i64, denom: i64) -> Self {
        Self::new(
            self.biomass * numer / denom,
            self.flux * numer / denom,
            self.neurogel * numer / denom,
        )
    }

    /// 是否逐项不小于 `o`
    pub const fn covers(&self, o: &Self) -> bool {
        self.biomass >= o.biomass && self.flux >= o.flux && self.neurogel >= o.neurogel
    }

    /// 三项之和（胜负积分中的 `Cost(·)`）
    pub const fn total(&self) -> i64 {
        self.biomass + self.flux + self.neurogel
    }
}

/// Starter Kit 初始资源账本：`500 Biomass / 300 Flux / 100 Neurogel`
pub const STARTER_RESOURCES: ResourceAmounts = ResourceAmounts::whole(500, 300, 100);

/// 主脑核心提供的基础存储上限 `2000 / 1000 / 500`（Biomass / Flux / Neurogel）
pub const NEXUS_STORAGE_CAP: ResourceAmounts = ResourceAmounts::whole(2000, 1000, 500);

/// 阵营资源账本：余额、上限与累计统计
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLedger {
    pub balance: ResourceAmounts,
    pub capacity: ResourceAmounts,
    /// 累计采集的生物质（胜负裁决 `Biomass_harvested_total`）
    pub biomass_harvested_total: i64,
    /// 因超出上限而丢弃的产出（诊断用）
    pub overflow_discarded: ResourceAmounts,
}

impl ResourceLedger {
    /// 开局账本：Starter Kit 余额 + 主脑基础上限
    pub fn starter() -> Self {
        Self {
            balance: STARTER_RESOURCES,
            capacity: NEXUS_STORAGE_CAP,
            biomass_harvested_total: 0,
            overflow_discarded: ResourceAmounts::ZERO,
        }
    }

    /// 无上限账本（`Sandbox_Creative` 与 PvE 野生阵营）
    pub fn unlimited() -> Self {
        Self {
            balance: ResourceAmounts::ZERO,
            capacity: ResourceAmounts::new(i64::MAX / 4, i64::MAX / 4, i64::MAX / 4),
            biomass_harvested_total: 0,
            overflow_discarded: ResourceAmounts::ZERO,
        }
    }

    pub const fn can_afford(&self, cost: &ResourceAmounts) -> bool {
        self.balance.covers(cost)
    }

    /// 原子扣费：任一资源不足则不扣任何资源，返回 `0x8005 InsufficientFactionResources`
    pub fn spend(&mut self, cost: &ResourceAmounts) -> Result<(), RynvekErrorCode> {
        if !self.can_afford(cost) {
            return Err(RynvekErrorCode::InsufficientFactionResources);
        }
        self.balance = self.balance.minus(*cost);
        Ok(())
    }

    /// 入账（受上限钳制，超出部分计入 `overflow_discarded`）；返回实际入账量
    pub fn deposit(&mut self, kind: ResourceKind, amount: i64) -> i64 {
        debug_assert!(amount >= 0);
        let cap = self.capacity.get(kind);
        let bal = self.balance.get_mut(kind);
        let room = (cap - *bal).max(0);
        let accepted = amount.min(room);
        *bal += accepted;
        *self.overflow_discarded.get_mut(kind) += amount - accepted;
        accepted
    }

    /// 退款（与入账相同受上限约束）
    pub fn refund(&mut self, amounts: &ResourceAmounts) {
        for kind in ResourceKind::ALL {
            let v = amounts.get(*kind);
            if v > 0 {
                self.deposit(*kind, v);
            }
        }
    }

    /// 尝试扣除单一资源（运转能耗等），不足时不扣并返回 `false`
    pub fn try_consume(&mut self, kind: ResourceKind, amount: i64) -> bool {
        let bal = self.balance.get_mut(kind);
        if *bal >= amount {
            *bal -= amount;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_kit_matches_design_bible() {
        let l = ResourceLedger::starter();
        assert_eq!(l.balance, ResourceAmounts::new(500_000, 300_000, 100_000));
        assert_eq!(l.capacity, ResourceAmounts::new(2_000_000, 1_000_000, 500_000));
    }

    #[test]
    fn spend_is_atomic_and_reports_0x8005() {
        let mut l = ResourceLedger::starter();
        let too_much = ResourceAmounts::whole(100, 0, 200);
        assert_eq!(l.spend(&too_much), Err(RynvekErrorCode::InsufficientFactionResources));
        assert_eq!(RynvekErrorCode::InsufficientFactionResources.code(), 0x8005);
        assert_eq!(l.balance, STARTER_RESOURCES);
        l.spend(&ResourceAmounts::whole(120, 0, 0)).unwrap();
        assert_eq!(l.balance.biomass, 380_000);
    }

    #[test]
    fn deposit_is_capped() {
        let mut l = ResourceLedger::starter();
        assert_eq!(l.deposit(ResourceKind::Neurogel, 450_000), 400_000);
        assert_eq!(l.balance.neurogel, 500_000);
        assert_eq!(l.overflow_discarded.neurogel, 50_000);
    }

    #[test]
    fn resource_colors_match_bible() {
        assert_eq!(resource_color_hex(ResourceKind::Biomass), "#82B96B");
        assert_eq!(resource_color_hex(ResourceKind::Flux), "#A3D65C");
        assert_eq!(resource_color_hex(ResourceKind::Neurogel), "#5CE1E6");
    }
}
