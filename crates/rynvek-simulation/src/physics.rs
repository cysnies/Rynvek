// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 单 Chunk 圆形刚体 ECS。Q32.32 辛欧拉来自最终规范 §1.2/§2.1；
//! 128 体、四个 120Hz 子步、四轮无摩擦弹性接触是本实现剖面，不冒充原规范。
use rynvek_core::RynvekErrorCode;
use rynvek_ecs::{EntityRegistry, PagedComponentColumn};
use rynvek_spatial::circle;
pub use rynvek_spatial::circle::{Body, ONE};
use serde::{Deserialize, Serialize};

pub const MAX_BODIES: usize = 128;
pub const MAX_COMMANDS: usize = 64;
const ERROR: RynvekErrorCode = RynvekErrorCode::DimensionMismatch;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum PhysicsCommand {
    Spawn { body: Body },
    Despawn { serial: u32 },
    SetAcceleration { serial: u32, acceleration_q32: [i64; 2] },
}

#[derive(Clone, Debug)]
pub struct PhysicsWorld {
    pub(crate) registry: EntityRegistry,
    pub(crate) bodies: PagedComponentColumn<Body>,
}
impl PhysicsWorld {
    pub fn new(world_id: [u8; 16]) -> Self {
        let hash = blake3::hash(&world_id);
        Self {
            registry: EntityRegistry::new(u64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap())),
            bodies: PagedComponentColumn::new(),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (u32, &Body)> {
        self.registry.iter_alive().map(|(dense, _, id)| {
            (
                id.to_serial_u32().expect("受限单调序号"),
                self.bodies.get(dense).expect("ECS 列对齐"),
            )
        })
    }
    pub fn positions(&self) -> Vec<(u64, u64)> {
        self.bodies
            .iter()
            .map(|b| (b.position_q32[0] as u64, b.position_q32[1] as u64))
            .collect()
    }
    pub fn body(&self, serial: u32) -> Option<&Body> {
        self.registry
            .iter_alive()
            .find(|(_, _, id)| id.monotonic_serial() == u64::from(serial))
            .and_then(|(d, _, _)| self.bodies.get(d).ok())
    }
    pub(crate) fn apply(&mut self, commands: &[PhysicsCommand], epoch: u64) -> Result<(), RynvekErrorCode> {
        self.apply_with_life(commands, epoch, None)
    }
    pub(crate) fn apply_with_life(
        &mut self,
        commands: &[PhysicsCommand],
        epoch: u64,
        mut coupled: Option<(&mut crate::life::LifeStore, &mut rynvek_field::FieldWorld)>,
    ) -> Result<(), RynvekErrorCode> {
        if commands.len() > MAX_COMMANDS {
            return Err(ERROR);
        }
        self.registry.recycle_tombstones(epoch);
        for cmd in commands {
            match *cmd {
                PhysicsCommand::Spawn { body } => {
                    body.validate()?;
                    if self.bodies.len() >= MAX_BODIES || self.registry.next_monotonic_serial >= u64::from(u32::MAX) {
                        return Err(ERROR);
                    }
                    self.registry.spawn(1)?;
                    self.bodies.push(body);
                    if let Some((store, _)) = coupled.as_mut() {
                        store.on_spawn(&body)?;
                    }
                }
                PhysicsCommand::Despawn { serial } => {
                    let (d, handle, _) = self
                        .registry
                        .iter_alive()
                        .find(|(_, _, id)| id.monotonic_serial() == u64::from(serial))
                        .ok_or(ERROR)?;
                    if let Some((store, field)) = coupled.as_mut() {
                        crate::life::reclaim_dense(self, store, field, d)?;
                    }
                    self.registry.despawn(handle, epoch)?;
                    self.bodies.swap_remove(d)?;
                    if let Some((store, _)) = coupled.as_mut() {
                        store.on_despawn(d)?;
                    }
                }
                PhysicsCommand::SetAcceleration {
                    serial,
                    acceleration_q32,
                } => {
                    let (d, _, _) = self
                        .registry
                        .iter_alive()
                        .find(|(_, _, id)| id.monotonic_serial() == u64::from(serial))
                        .ok_or(ERROR)?;
                    let b = self.bodies.get_mut(d)?;
                    b.acceleration_q32 = acceleration_q32;
                    b.validate()?;
                }
            }
        }
        self.validate()
    }
    pub(crate) fn validate(&self) -> Result<(), RynvekErrorCode> {
        if self.bodies.len() > MAX_BODIES
            || self.registry.iter_alive().count() != self.bodies.len()
            || self.registry.next_monotonic_serial > u64::from(u32::MAX)
        {
            return Err(ERROR);
        }
        for (dense, _, id) in self.registry.iter_alive() {
            if id.to_serial_u32().filter(|s| *s > 0 && *s < u32::MAX).is_none() {
                return Err(ERROR);
            }
            self.bodies.get(dense)?.validate()?;
        }
        Ok(())
    }
    pub(crate) fn step(&mut self) -> Result<(), RynvekErrorCode> {
        self.validate()?;
        // 按持久身份排序；swap_remove 后的稠密槽位不能改变接触求解顺序。
        let mut order: Vec<_> = self.registry.iter_alive().map(|(d, _, id)| (id, d)).collect();
        order.sort_by_key(|(id, _)| *id);
        for _ in 0..4 {
            for b in self.bodies.iter_mut() {
                circle::integrate_substep(b)?;
            }
            // 有界完整枚举，不使用 CSR 的 32 邻居截断，避免密集接触漏碰。
            for _ in 0..4 {
                for a in 0..order.len() {
                    for b in a + 1..order.len() {
                        let ia = order[a].1;
                        let ib = order[b].1;
                        let mut ba = *self.bodies.get(ia)?;
                        let mut bb = *self.bodies.get(ib)?;
                        circle::resolve_contact(&mut ba, &mut bb)?;
                        circle::reflect_walls(&mut ba)?;
                        circle::reflect_walls(&mut bb)?;
                        *self.bodies.get_mut(ia)? = ba;
                        *self.bodies.get_mut(ib)? = bb;
                    }
                }
            }
            // 有限迭代不收敛则拒绝整个候选 Tick，不发布互穿状态。
            for a in 0..order.len() {
                for b in a + 1..order.len() {
                    let ba = self.bodies.get(order[a].1)?;
                    let bb = self.bodies.get(order[b].1)?;
                    let dist = circle::separation(ba, bb)?;
                    if dist + 64 < ba.radius_q32 + bb.radius_q32 {
                        return Err(ERROR);
                    }
                }
            }
            self.validate()?;
        }
        Ok(())
    }
    pub(crate) fn hash(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(b"RYNVEK_PHYSICS_ECS_V1");
        h.update(&self.registry.encode_snapshot_bytes());
        for b in self.bodies.iter() {
            for v in b
                .position_q32
                .into_iter()
                .chain(b.velocity_q32)
                .chain(b.acceleration_q32)
                .chain([b.radius_q32, b.mass_q32])
            {
                h.update(&v.to_le_bytes());
            }
            h.update(&[u8::from(b.fixed)]);
        }
        *h.finalize().as_bytes()
    }
}
