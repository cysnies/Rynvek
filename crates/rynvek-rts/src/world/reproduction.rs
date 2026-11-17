// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 有来源的自然出生：原能量池支付、邻格容纳、单亲复制突变，再通过共享路径注册真实脑。
//! 不复用定向繁育的适应度排序，也不向孵化生产队列塞虚拟任务。
use super::{
    ArenaWorld,
    ecology::{EcologyState, MitosisBirth},
};
use crate::{
    EntitySerial, Vec2,
    brain::UnitBrain,
    evolution::{DEFAULT_MUTATION_RATE_PERMILLE, EvolutionRng, mutate},
};
use rynvek_neural::metabolism::EnergyAccount;
use std::collections::BTreeSet;

impl ArenaWorld {
    /// 出生历史与双向转账必须一一闭合；私有快照不能丢谱系却仍保留亲本转出。
    pub(super) fn validate_mitosis_accounts(&self, ecology: &EcologyState) -> bool {
        let mut allocations = std::collections::BTreeMap::<EntitySerial, u128>::new();
        for (child, birth) in &ecology.births {
            *allocations.entry(birth.parent).or_default() += u128::from(birth.allocated_q);
            let account = ecology
                .bodies
                .get(child)
                .map(|b| &b.account)
                .or_else(|| ecology.remains.get(child).map(|r| &r.account));
            // 不可变出生元数据既供恢复校验，也供Lab；不从slug或目录猜测祖先。
            let parent_matches = if let Some(p) = ecology.births.get(&birth.parent) {
                p.faction == birth.faction
                    && p.unit == birth.unit
                    && p.generation == birth.parent_generation
                    && p.child_genome == birth.parent_genome
                    && p.born_tick < birth.born_tick
            } else if let Some(p) = self.units.get(&birth.parent) {
                p.faction == birth.faction
                    && p.kind == birth.unit
                    && p.genome.generation == birth.parent_generation
                    && p.genome.digest() == birth.parent_genome
                    && p.born_tick <= birth.born_tick
            } else {
                // 已死亡的初始/设施出生根没有伪造的自然亲代元数据；由原快照根认证历史。
                true
            };
            if account.is_none_or(|a| a.received_q < birth.allocated_q)
                || self
                    .role_energy(birth.unit, birth.faction)
                    .is_none_or(|r| !r.natural_reproduction)
                || birth.faction.is_wild()
                || self.faction(birth.faction).is_none()
                || birth.slug.is_empty()
                || !parent_matches
                || self.units.get(child).is_some_and(|u| {
                    u.born_tick != birth.born_tick
                        || u.faction != birth.faction
                        || u.kind != birth.unit
                        || u.genome.slug != birth.slug
                        || u.genome.generation != birth.generation
                        || u.genome.digest() != birth.child_genome
                })
            {
                return false;
            }
        }
        ecology
            .bodies
            .iter()
            .map(|(id, b)| (id, &b.account))
            .chain(ecology.remains.iter().map(|(id, r)| (id, &r.account)))
            .all(|(id, a)| u128::from(a.transferred_q) == allocations.get(id).copied().unwrap_or(0))
    }
    pub(super) fn try_mitosis(&mut self, parent: EntitySerial, retain_q: u64, occupied: &mut BTreeSet<(u32, u32)>) {
        let contract = self
            .role_energy(self.units[&parent].kind, self.units[&parent].faction)
            .expect("调用者已核定角色契约");
        let allocated_q = contract.birth_q();
        let Some(required_q) = allocated_q.checked_add(retain_q) else {
            return;
        };
        let unit = &self.units[&parent];
        let Some(generation) = unit.genome.generation.checked_add(1) else {
            return;
        };
        if self.next_serial == u32::MAX
            || unit.defense.hp_milli < 2
            || self.ecology.as_ref().expect("已检查").bodies[&parent].account.stored_q < required_q
        {
            return;
        }
        let (used, cap) = self.supply(unit.faction);
        if used
            .checked_add(crate::units::unit_spec(unit.kind).supply)
            .is_none_or(|n| n > cap)
        {
            return;
        }
        // 仅邻格，不允许堵塞时远程传送或退回不可通行的默认格；序号/y/x序保证重放一致。
        let (x, y) = unit.tile();
        let spot = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (i64::from(x) + dx, i64::from(y) + dy)))
            .find(|&(nx, ny)| {
                (nx, ny) != (i64::from(x), i64::from(y))
                    && self
                        .nav
                        .step_cost(x, y, (nx - i64::from(x)) as i32, (ny - i64::from(y)) as i32)
                        .is_some()
                    && !occupied.contains(&(nx as u32, ny as u32))
            });
        let Some((x, y)) = spot else { return };
        let parent_genome = unit.genome.digest();
        let parent_generation = unit.genome.generation;
        let faction = unit.faction;
        let kind = unit.kind;
        let hp = unit.defense.hp_milli / 2;
        let shield = unit.defense.shield_milli / 2;
        let mut genome = unit.genome.clone();
        genome.generation = generation;
        genome.slug = format!("mitosis.{}", self.next_serial);
        // 使用既有Philox变异算子与既有默认率；不交叉选优，不继承运行时脑/免费脉冲。
        let mut rng = EvolutionRng::for_mitosis(self.config.world_seed, self.tick, u64::from(self.next_serial));
        mutate(&mut genome, DEFAULT_MUTATION_RATE_PERMILLE, &mut rng);
        let Ok(brain) = UnitBrain::from_genome(&genome) else {
            return;
        };
        let child_genome = genome.digest();
        let slug = genome.slug.clone();
        let mut account = EnergyAccount::default();
        if !self
            .ecology
            .as_mut()
            .expect("已检查")
            .bodies
            .get_mut(&parent)
            .expect("活体账户")
            .account
            .try_transfer_to(&mut account, allocated_q, retain_q)
        {
            return;
        }
        // 自此无可恢复失败点；序号和候选脑均预检。HP/护盾同样拆分，不免费复制满血军队。
        let child = self.spawn_unit_with_brain(faction, kind, genome, Vec2::tile_center(x as u32, y as u32), brain);
        let unit = self.units.get_mut(&parent).expect("仍存活");
        unit.defense.hp_milli -= hp;
        unit.defense.shield_milli -= shield;
        let unit = self.units.get_mut(&child).expect("共享出生注册");
        unit.defense.hp_milli = hp;
        unit.defense.shield_milli = shield;
        let ecology = self.ecology.as_mut().expect("已检查");
        ecology.bodies.get_mut(&child).expect("共享出生建账").account = account;
        ecology.births.insert(
            child,
            MitosisBirth {
                faction,
                unit: kind,
                slug,
                parent,
                born_tick: self.tick,
                parent_generation,
                generation,
                parent_genome,
                child_genome,
                allocated_q,
            },
        );
        occupied.insert((x as u32, y as u32));
    }
}
