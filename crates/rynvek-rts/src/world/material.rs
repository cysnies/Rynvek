// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 有来源的Biomass等价结构体量：1 milli=256u，1 Nutrient raw=1000u。
//! 这是显式实验的单材料密度模型，不冒充真实化学元素或三维物理定律。
use super::{ArenaWorld, ecology::ActivityKind};
use crate::{EntitySerial, FieldId, ResourceKind};
use rynvek_core::Q32_32;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialAccount {
    pub received_u: u64,
    pub external_u: u64,
    /// 从同一个身体能量/化学预算实转的材料，不是第二份外源营养。
    pub converted_u: u64,
    pub stored_u: u64,
    pub transferred_u: u64,
    pub returned_u: u64,
}
impl MaterialAccount {
    pub fn is_conserved(&self) -> bool {
        u128::from(self.external_u) + u128::from(self.converted_u) <= u128::from(self.received_u)
            && self.converted_u % 256 == 0
            && u128::from(self.received_u)
                == u128::from(self.stored_u) + u128::from(self.transferred_u) + u128::from(self.returned_u)
    }
}
impl ArenaWorld {
    /// 结构体量影响实际二维接触截面；零材料零截面，满容量恢复原兵种半径。
    /// 面积随材料线性增加，整数平方根；不把抽象体量称为三维几何体积。
    pub fn unit_radius(&self, id: EntitySerial) -> Q32_32 {
        Q32_32::from_milli(self.unit_radius_milli(id))
    }
    pub fn unit_radius_milli(&self, id: EntitySerial) -> i64 {
        let u = &self.units[&id];
        let Some(r) = self
            .role_energy(u.kind, u.faction)
            .filter(|r| r.material_capacity_milli > 0)
        else {
            return crate::units::unit_spec(u.kind).radius_milli;
        };
        let cap = u64::from(r.material_capacity_milli) * 256;
        let mass = self.ecology.as_ref().expect("角色已建账").bodies[&id]
            .material
            .stored_u
            .min(cap);
        let base = crate::units::unit_spec(u.kind).radius_milli as u128;
        let radius = (base * base * u128::from(mass) / u128::from(cap)).isqrt();
        radius as i64
    }
    /// 只摄取真实阵营Biomass；原采集者的营养摄取/盈余输出是其上游，不重复扣同一格场。
    pub(super) fn step_material_growth(&mut self) {
        let ids: Vec<_> = self.units.keys().copied().collect();
        for id in ids {
            let u = &self.units[&id];
            let Some(r) = self.role_energy(u.kind, u.faction) else {
                continue;
            };
            if r.material_capacity_milli == 0 || u.defense.hp_milli <= 0 {
                continue;
            }
            let faction = u.faction;
            if faction.is_wild() {
                self.grow_wild_material(id, r);
                continue;
            }
            let b = &self.ecology.as_ref().expect("有契约").bodies[&id];
            let cap = u64::from(r.material_capacity_milli) * 256;
            let price = u64::from(r.material_synthesis_micro_per_milli) * 4;
            let requested = u64::from(r.material_growth_milli_per_tick)
                .min(cap.saturating_sub(b.material.stored_u) / 256)
                .min((u64::MAX - b.material.received_u) / 256)
                .min((u64::MAX - b.material.external_u) / 256)
                .min(self.faction(faction).expect("玩家").ledger.balance.biomass.max(0) as u64);
            let accepted = if price == 0 {
                requested
            } else {
                requested.min(b.account.stored_q / price)
            };
            if accepted == 0 {
                continue;
            }
            // 所有失败条件预检后同一权威调用提交源池、合成耗能和材料；不凭空注资。
            assert!(
                self.faction_mut(faction)
                    .expect("玩家")
                    .ledger
                    .try_consume(ResourceKind::Biomass, accepted as i64)
            );
            assert!(self.charge_activity(id, ActivityKind::Synthesis, accepted * price));
            let m = &mut self
                .ecology
                .as_mut()
                .expect("有契约")
                .bodies
                .get_mut(&id)
                .expect("活体")
                .material;
            m.received_u += accepted * 256;
            m.external_u += accepted * 256;
            m.stored_u += accepted * 256;
        }
    }
    /// 死亡只移动同一个材料账；仅归还目标格容得下的完整raw，亚raw余量留遗骸。
    pub(super) fn step_material_returns(&mut self) {
        let Some(e) = &mut self.ecology else { return };
        let limit = i32::from(crate::fields::ARENA_FIELD_CHANNELS[FieldId::Nutrient.channel().index()].max_val_q8);
        for r in e.remains.values_mut() {
            let (x, y) = r.cell;
            let room = (limit - i32::from(self.fields.get(FieldId::Nutrient, x as i64, y as i64))).max(0) as u64;
            let raw = (r.material.stored_u / 1000).min(room);
            assert_eq!(self.fields.add(FieldId::Nutrient, x, y, raw as i32), raw as i32);
            r.material.stored_u -= raw * 1000;
            r.material.returned_u += raw * 1000;
        }
    }
}
