// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 通用 World 的有来源生命场：复用原 `EnergyAccount` 与可选付费 LIF。
//! 营养只从实际场格扣除，死亡整 raw 返还同一格，亚 raw 归热耗。
//! 速率、回收比与脑规模是实现剖面，不冒充原规范冻结值；不写入 Arena 阵营账。

use rynvek_core::RynvekErrorCode;
use rynvek_core::fixed::Q8_8;
use rynvek_ecs::PagedComponentColumn;
use rynvek_field::{FieldChannelId, FieldWorld, TILE_DIM};
use rynvek_neural::LIF_INPUTS;
use rynvek_neural::brain::LifBrain;
use rynvek_neural::metabolism::{BASE_SPIKE_COST_MICRO, ENERGY_Q_PER_FIELD_RAW, EnergyAccount};
use rynvek_neural::runtime::NeuronModelKind;
use rynvek_snapshot::NeurChunk;
use serde::{Deserialize, Serialize};

use crate::physics::{Body, ONE, PhysicsWorld};

const ERROR: RynvekErrorCode = RynvekErrorCode::DimensionMismatch;
pub const LIFE_PROFILE: &str = "rynvek.general-sourced-life.v1";
const MAX_HID: usize = 8;
const MAX_DEPOSITS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NutrientDeposit {
    pub x: u16,
    pub y: u16,
    pub value: i16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralLifGenome {
    pub hidden: u16,
    pub weights_base_q8: Vec<i16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifeProfile {
    pub profile: String,
    /// 每 Tick 最多从所在格摄取的 Nutrient raw，实现选择而非规范原值。
    pub max_ingest_raw_per_tick: u32,
    /// 每 Tick 强制维护热耗（内部 q）。不足则耗尽后死亡，不生成欠账。
    pub maintenance_q_per_tick: u64,
    pub recycle_percent: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neural: Option<GeneralLifGenome>,
}

impl LifeProfile {
    pub fn validate(&self) -> Result<(), RynvekErrorCode> {
        if self.profile != LIFE_PROFILE
            || self.max_ingest_raw_per_tick > 256
            || self.maintenance_q_per_tick > 256 * ENERGY_Q_PER_FIELD_RAW
            || self.recycle_percent > 100
        {
            return Err(ERROR);
        }
        if let Some(g) = &self.neural {
            let n = g.hidden as usize;
            if !(1..=MAX_HID).contains(&n) || g.weights_base_q8.len() != n * (LIF_INPUTS + 6) {
                return Err(ERROR);
            }
        }
        Ok(())
    }
}

pub fn validate_deposits(deposits: &[NutrientDeposit]) -> Result<(), RynvekErrorCode> {
    if deposits.len() > MAX_DEPOSITS {
        return Err(ERROR);
    }
    let mut seen = std::collections::BTreeSet::new();
    for d in deposits {
        if d.x as usize >= TILE_DIM || d.y as usize >= TILE_DIM || d.value <= 0 || !seen.insert((d.x, d.y)) {
            return Err(ERROR);
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LifeSnapshotDto {
    pub profile: LifeProfile,
    pub accounts: Vec<EnergyAccount>,
    pub genesis_energy_q: u128,
    pub env_dissipated_q: u128,
    pub retired_closed_q: u128,
}

#[derive(Clone, Debug)]
pub struct LifeStore {
    pub(crate) profile: LifeProfile,
    pub(crate) accounts: PagedComponentColumn<EnergyAccount>,
    pub(crate) brains: PagedComponentColumn<Option<LifBrain>>,
    pub(crate) genesis_energy_q: u128,
    pub(crate) env_dissipated_q: u128,
    pub(crate) retired_closed_q: u128,
}

impl LifeStore {
    pub fn new(profile: LifeProfile) -> Result<Self, RynvekErrorCode> {
        profile.validate()?;
        Ok(Self {
            profile,
            accounts: PagedComponentColumn::new(),
            brains: PagedComponentColumn::new(),
            genesis_energy_q: 0,
            env_dissipated_q: 0,
            retired_closed_q: 0,
        })
    }

    pub fn profile(&self) -> &LifeProfile {
        &self.profile
    }

    pub fn genesis_energy_q(&self) -> u128 {
        self.genesis_energy_q
    }

    pub fn accounts(&self) -> impl Iterator<Item = &EnergyAccount> {
        self.accounts.iter()
    }

    pub(crate) fn retire(&mut self, account: &EnergyAccount) -> Result<(), RynvekErrorCode> {
        if account.stored_q != 0 || !account.is_conserved() {
            return Err(RynvekErrorCode::ConservationLawMismatch);
        }
        let closed = u128::from(account.dissipated_q)
            .checked_add(u128::from(account.exported_q))
            .and_then(|v| v.checked_add(u128::from(account.transferred_q)))
            .ok_or(RynvekErrorCode::ConservationLawMismatch)?;
        self.retired_closed_q = self
            .retired_closed_q
            .checked_add(closed)
            .ok_or(RynvekErrorCode::ConservationLawMismatch)?;
        Ok(())
    }

    pub fn account(&self, physics: &PhysicsWorld, serial: u32) -> Option<&EnergyAccount> {
        physics
            .registry
            .iter_alive()
            .find(|(_, _, id)| id.monotonic_serial() == u64::from(serial))
            .and_then(|(d, _, _)| self.accounts.get(d).ok())
    }

    pub(crate) fn on_spawn(&mut self, body: &Body) -> Result<(), RynvekErrorCode> {
        // 创世与后继出生均空账；能量只能来自随后实际场扣除。
        self.accounts.push(EnergyAccount::default());
        let brain = if body.fixed {
            None
        } else if let Some(g) = &self.profile.neural {
            Some(LifBrain::new(g.hidden as usize, g.weights_base_q8.clone())?)
        } else {
            None
        };
        self.brains.push(brain);
        Ok(())
    }

    pub(crate) fn on_despawn(&mut self, dense: u32) -> Result<(), RynvekErrorCode> {
        self.accounts.swap_remove(dense)?;
        self.brains.swap_remove(dense)?;
        Ok(())
    }

    pub(crate) fn align(&self, physics: &PhysicsWorld) -> Result<(), RynvekErrorCode> {
        if self.accounts.len() != physics.bodies.len() || self.brains.len() != physics.bodies.len() {
            return Err(ERROR);
        }
        for (dense, _, _) in physics.registry.iter_alive() {
            let a = self.accounts.get(dense)?;
            if !a.is_conserved() {
                return Err(RynvekErrorCode::ConservationLawMismatch);
            }
        }
        Ok(())
    }

    pub(crate) fn hash_accounts(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(b"RYNVEK_GENERAL_LIFE_V1");
        h.update(self.profile.profile.as_bytes());
        h.update(&self.profile.max_ingest_raw_per_tick.to_le_bytes());
        h.update(&self.profile.maintenance_q_per_tick.to_le_bytes());
        h.update(&self.profile.recycle_percent.to_le_bytes());
        if let Some(g) = &self.profile.neural {
            h.update(&g.hidden.to_le_bytes());
            for w in &g.weights_base_q8 {
                h.update(&w.to_le_bytes());
            }
        } else {
            h.update(&[0]);
        }
        h.update(&self.genesis_energy_q.to_le_bytes());
        h.update(&self.env_dissipated_q.to_le_bytes());
        h.update(&self.retired_closed_q.to_le_bytes());
        for a in self.accounts.iter() {
            for v in [
                a.received_q,
                a.stored_q,
                a.dissipated_q,
                a.exported_q,
                a.transferred_q,
                a.returned_q,
                a.unfunded_q,
            ] {
                h.update(&v.to_le_bytes());
            }
        }
        *h.finalize().as_bytes()
    }

    pub(crate) fn neur_chunk(&self) -> Option<NeurChunk> {
        if self.profile.neural.is_none() {
            return None;
        }
        let mut rows = Vec::new();
        for (dense, brain) in self.brains.iter().enumerate() {
            if let Some(b) = brain {
                rows.extend(b.to_neur_rows(dense as u32));
            }
        }
        Some(NeurChunk { rows })
    }

    pub(crate) fn neural_root(&self) -> [u8; 32] {
        if let Some(chunk) = self.neur_chunk() {
            *blake3::hash(&chunk.encode()).as_bytes()
        } else {
            *blake3::hash(b"GENERAL_NO_NEURAL_COMPONENTS_V1").as_bytes()
        }
    }

    pub(crate) fn snapshot_dto(&self) -> LifeSnapshotDto {
        LifeSnapshotDto {
            profile: self.profile.clone(),
            accounts: self.accounts.iter().cloned().collect(),
            genesis_energy_q: self.genesis_energy_q,
            env_dissipated_q: self.env_dissipated_q,
            retired_closed_q: self.retired_closed_q,
        }
    }
}

pub(crate) fn tile_of(body: &Body) -> Result<(usize, usize), RynvekErrorCode> {
    let x = body.position_q32[0].div_euclid(ONE);
    let y = body.position_q32[1].div_euclid(ONE);
    if x < 0 || y < 0 || x as usize >= TILE_DIM || y as usize >= TILE_DIM {
        return Err(ERROR);
    }
    Ok((x as usize, y as usize))
}

pub(crate) fn apply_deposits(field: &mut FieldWorld, deposits: &[NutrientDeposit]) -> Result<(), RynvekErrorCode> {
    validate_deposits(deposits)?;
    for d in deposits {
        let got = field.deposit_room(FieldChannelId::Nutrient, d.x as usize, d.y as usize, d.value as u32)?;
        if got != d.value as u32 {
            return Err(ERROR);
        }
    }
    Ok(())
}

/// 场求解之后、神经之前：先收款再等额扣场，绝不先扣后钳。
pub(crate) fn ingest(
    physics: &PhysicsWorld,
    life: &mut LifeStore,
    field: &mut FieldWorld,
) -> Result<i64, RynvekErrorCode> {
    life.align(physics)?;
    let budget = life.profile.max_ingest_raw_per_tick;
    let mut consumed = 0i64;
    let mut order: Vec<_> = physics.registry.iter_alive().map(|(d, _, id)| (id, d)).collect();
    order.sort_by_key(|(id, _)| *id);
    for (_, dense) in order {
        let body = physics.bodies.get(dense)?;
        if body.fixed {
            continue;
        }
        let (x, y) = tile_of(body)?;
        let available = field.interior(FieldChannelId::Nutrient, x, y)?.max(0) as u32;
        let want = available.min(budget);
        let accepted = life.accounts.get_mut(dense)?.receive_field_raw(want);
        let taken = field.withdraw_positive(FieldChannelId::Nutrient, x, y, accepted)?;
        if taken != accepted {
            return Err(RynvekErrorCode::ConservationLawMismatch);
        }
        consumed = consumed.saturating_add(i64::from(taken));
    }
    Ok(consumed)
}

/// 付费后才把 LIF 读出写入加速度；欠费不提交新的运动意图。
pub(crate) fn paid_neural(
    physics: &mut PhysicsWorld,
    life: &mut LifeStore,
    field: &FieldWorld,
) -> Result<(), RynvekErrorCode> {
    life.align(physics)?;
    if life.profile.neural.is_none() {
        return Ok(());
    }
    let mut order: Vec<_> = physics.registry.iter_alive().map(|(d, _, id)| (id, d)).collect();
    order.sort_by_key(|(id, _)| *id);
    for (_, dense) in order {
        if physics.bodies.get(dense)?.fixed {
            continue;
        }
        let sensory = sensory(physics.bodies.get(dense)?, field, life.accounts.get(dense)?)?;
        let Some(brain) = life.brains.get_mut(dense)? else {
            continue;
        };
        let outputs = brain.step(&sensory, [0; 4], false);
        let spikes = brain.spike_ring.iter().map(|r| (r & 1) as u32).sum::<u32>();
        let paid = spikes > 0
            && life
                .accounts
                .get_mut(dense)?
                .try_charge_spikes(spikes, BASE_SPIKE_COST_MICRO);
        let body = physics.bodies.get_mut(dense)?;
        // 欠费或无脉冲不沿用上帧加速度，避免无成本持续推进。
        body.acceleration_q32 = if paid {
            [scale_accel(outputs[0].0), scale_accel(outputs[1].0)]
        } else {
            [0, 0]
        };
        body.validate()?;
    }
    Ok(())
}

fn scale_accel(q8: i16) -> i64 {
    // Q8.8 读出按 1.0→1 世界单位/秒² 再钳到剖面加速度界，属实现选择。
    let raw = i64::from(q8).saturating_mul(ONE / 256);
    raw.clamp(-32 * ONE, 32 * ONE)
}

fn sample(field: &FieldWorld, x: usize, y: usize, channel: FieldChannelId) -> Result<i16, RynvekErrorCode> {
    let x = x.min(TILE_DIM - 1);
    let y = y.min(TILE_DIM - 1);
    field.interior(channel, x, y)
}

fn sensory(body: &Body, field: &FieldWorld, account: &EnergyAccount) -> Result<[Q8_8; LIF_INPUTS], RynvekErrorCode> {
    let (x, y) = tile_of(body)?;
    let n = sample(field, x, y, FieldChannelId::Nutrient)?;
    let west = sample(field, x.saturating_sub(1), y, FieldChannelId::Nutrient)?;
    let east = sample(field, (x + 1).min(TILE_DIM - 1), y, FieldChannelId::Nutrient)?;
    let south = sample(field, x, y.saturating_sub(1), FieldChannelId::Nutrient)?;
    let north = sample(field, x, (y + 1).min(TILE_DIM - 1), FieldChannelId::Nutrient)?;
    let hazard = sample(field, x, y, FieldChannelId::Hazard)?;
    let stored = (account.stored_q / ENERGY_Q_PER_FIELD_RAW).min(i16::MAX as u64) as i16;
    let vx = (body.velocity_q32[0] / (ONE / 256)).clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16;
    let vy = (body.velocity_q32[1] / (ONE / 256)).clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16;
    let mut s = [Q8_8::ZERO; LIF_INPUTS];
    for (i, v) in [n, east - west, north - south, hazard, stored, vx, vy]
        .into_iter()
        .enumerate()
    {
        s[i] = Q8_8(v);
    }
    Ok(s)
}

/// 死亡或命令回收：先按回收比准备残骸，整 raw 返还当前格，余量归热后再记入退役闭包。
pub(crate) fn reclaim_dense(
    physics: &PhysicsWorld,
    life: &mut LifeStore,
    field: &mut FieldWorld,
    dense: u32,
) -> Result<(), RynvekErrorCode> {
    let (x, y) = tile_of(physics.bodies.get(dense)?)?;
    let account = life.accounts.get_mut(dense)?;
    account.prepare_remains(life.profile.recycle_percent);
    let room = {
        let cur = field.interior(FieldChannelId::Nutrient, x, y)?;
        let max = field.channel(FieldChannelId::Nutrient).params.max_val_q8;
        i32::from(max).saturating_sub(i32::from(cur)).max(0) as u32
    };
    let returned = account.return_field_raw(room);
    let deposited = field.deposit_room(FieldChannelId::Nutrient, x, y, returned)?;
    if deposited != returned {
        return Err(RynvekErrorCode::ConservationLawMismatch);
    }
    let leftover = account.stored_q;
    if leftover > 0 && !account.try_dissipate_q(leftover) {
        return Err(RynvekErrorCode::ConservationLawMismatch);
    }
    if !account.is_conserved() {
        return Err(RynvekErrorCode::ConservationLawMismatch);
    }
    let closed = account.clone();
    life.retire(&closed)
}

/// 维护后空仓即死亡；残骸整 raw 返还当前格，装不下或亚 raw 归热耗，不从造价造营养。
pub(crate) fn maintain_and_reclaim(
    physics: &mut PhysicsWorld,
    life: &mut LifeStore,
    field: &mut FieldWorld,
    epoch: u64,
) -> Result<(), RynvekErrorCode> {
    life.align(physics)?;
    let maintenance = life.profile.maintenance_q_per_tick;
    let mut dying = Vec::new();
    let mut order: Vec<_> = physics.registry.iter_alive().map(|(d, h, id)| (id, d, h)).collect();
    order.sort_by_key(|(id, _, _)| *id);
    for (id, dense, handle) in order {
        if physics.bodies.get(dense)?.fixed {
            continue;
        }
        let account = life.accounts.get_mut(dense)?;
        account.dissipate_q(maintenance);
        // 无维护需求的空仓不是死亡；只有实际基础代谢把库存耗尽才回收。
        if maintenance > 0 && account.stored_q == 0 {
            dying.push((id, dense, handle));
        }
    }
    dying.sort_by_key(|(_, dense, _)| std::cmp::Reverse(*dense));
    for (_, dense, handle) in dying {
        reclaim_dense(physics, life, field, dense)?;
        physics.registry.despawn(handle, epoch)?;
        physics.bodies.swap_remove(dense)?;
        life.on_despawn(dense)?;
    }
    physics.validate()?;
    life.align(physics)
}

pub(crate) fn restore(
    physics: &PhysicsWorld,
    dto: LifeSnapshotDto,
    neur: Option<&NeurChunk>,
) -> Result<LifeStore, RynvekErrorCode> {
    dto.profile.validate()?;
    if dto.accounts.len() != physics.bodies.len() {
        return Err(ERROR);
    }
    let mut life = LifeStore::new(dto.profile)?;
    life.genesis_energy_q = dto.genesis_energy_q;
    life.env_dissipated_q = dto.env_dissipated_q;
    life.retired_closed_q = dto.retired_closed_q;
    for (dense, account) in dto.accounts.into_iter().enumerate() {
        if !account.is_conserved() || account.exported_q != 0 || account.transferred_q != 0 {
            return Err(RynvekErrorCode::ConservationLawMismatch);
        }
        life.accounts.push(account);
        let body = physics.bodies.get(dense as u32)?;
        let brain = if body.fixed || life.profile.neural.is_none() {
            None
        } else {
            let g = life.profile.neural.as_ref().expect("已判定");
            let mut b = LifBrain::new(g.hidden as usize, g.weights_base_q8.clone())?;
            let chunk = neur.ok_or(ERROR)?;
            b.restore_from_neur(chunk, dense as u32)?;
            Some(b)
        };
        life.brains.push(brain);
    }
    if life.profile.neural.is_some() {
        let expected = life.neur_chunk().ok_or(ERROR)?;
        let got = neur.ok_or(ERROR)?;
        if got.encode() != expected.encode() {
            return Err(ERROR);
        }
        if got.rows.iter().any(|r| r.model_kind != NeuronModelKind::LIF as u8) {
            return Err(ERROR);
        }
    } else if neur.is_some() {
        return Err(ERROR);
    }
    life.align(physics)?;
    Ok(life)
}

pub fn nutrient_energy_q(field: &FieldWorld) -> Result<u128, RynvekErrorCode> {
    let raw = field.interior_sums()[FieldChannelId::Nutrient.index()];
    if raw < 0 {
        return Err(RynvekErrorCode::ConservationLawMismatch);
    }
    (raw as u64)
        .checked_mul(ENERGY_Q_PER_FIELD_RAW)
        .map(u128::from)
        .ok_or(RynvekErrorCode::ConservationLawMismatch)
}

pub fn universe_energy_q(life: &LifeStore, field: &FieldWorld) -> Result<u128, RynvekErrorCode> {
    let mut total = nutrient_energy_q(field)?;
    total = total
        .checked_add(life.env_dissipated_q)
        .and_then(|v| v.checked_add(life.retired_closed_q))
        .ok_or(RynvekErrorCode::ConservationLawMismatch)?;
    for account in life.accounts.iter() {
        if !account.is_conserved() {
            return Err(RynvekErrorCode::ConservationLawMismatch);
        }
        let live = u128::from(account.stored_q)
            .checked_add(u128::from(account.dissipated_q))
            .and_then(|v| v.checked_add(u128::from(account.exported_q)))
            .and_then(|v| v.checked_add(u128::from(account.transferred_q)))
            .ok_or(RynvekErrorCode::ConservationLawMismatch)?;
        total = total
            .checked_add(live)
            .ok_or(RynvekErrorCode::ConservationLawMismatch)?;
    }
    Ok(total)
}

pub(crate) fn book_environment_dissipation(
    life: &mut LifeStore,
    before_q: u128,
    after_q: u128,
) -> Result<(), RynvekErrorCode> {
    if after_q > before_q {
        // 通用生命不把反应扩散当造能源；营养只降不增。
        return Err(RynvekErrorCode::ConservationLawMismatch);
    }
    life.env_dissipated_q = life
        .env_dissipated_q
        .checked_add(before_q - after_q)
        .ok_or(RynvekErrorCode::ConservationLawMismatch)?;
    Ok(())
}

pub(crate) fn assert_closed(life: &LifeStore, field: &FieldWorld) -> Result<(), RynvekErrorCode> {
    if universe_energy_q(life, field)? != life.genesis_energy_q {
        return Err(RynvekErrorCode::ConservationLawMismatch);
    }
    Ok(())
}
