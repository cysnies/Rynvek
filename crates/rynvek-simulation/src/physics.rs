// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 单 Chunk 圆形刚体 ECS。Q32.32 辛欧拉来自最终规范 §1.2/§2.1；
//! 128 体、四个 120Hz 子步、四轮无摩擦弹性接触是本实现剖面，不冒充原规范。
use rynvek_core::RynvekErrorCode;
use rynvek_ecs::{EntityRegistry, PagedComponentColumn};
use serde::{Deserialize, Serialize};

pub const ONE: i64 = 1 << 32;
pub const MAX_BODIES: usize = 128;
pub const MAX_COMMANDS: usize = 64;
const SIDE: i64 = 256 * ONE;
const ERROR: RynvekErrorCode = RynvekErrorCode::DimensionMismatch;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    pub position_q32: [i64; 2],
    pub velocity_q32: [i64; 2],
    pub acceleration_q32: [i64; 2],
    pub radius_q32: i64,
    /// 质量不是 CPU 配额，也不表示生物代谢资源。
    pub mass_q32: i64,
    pub fixed: bool,
}
impl Body {
    pub fn validate(&self) -> Result<(), RynvekErrorCode> {
        if !(ONE / 4..=2 * ONE).contains(&self.radius_q32)
            || !(ONE..=1024 * ONE).contains(&self.mass_q32)
            || self
                .position_q32
                .iter()
                .any(|&v| v < self.radius_q32 || v > SIDE - self.radius_q32)
            || self.velocity_q32.iter().any(|&v| !(-8 * ONE..=8 * ONE).contains(&v))
            || self
                .acceleration_q32
                .iter()
                .any(|&v| !(-32 * ONE..=32 * ONE).contains(&v))
            || (self.fixed && (self.velocity_q32 != [0; 2] || self.acceleration_q32 != [0; 2]))
        {
            return Err(ERROR);
        }
        Ok(())
    }
    fn inverse_mass(&self) -> i64 {
        if self.fixed {
            0
        } else {
            ((i128::from(ONE) * i128::from(ONE)) / i128::from(self.mass_q32)) as i64
        }
    }
}

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
                }
                PhysicsCommand::Despawn { serial } => {
                    let (d, handle, _) = self
                        .registry
                        .iter_alive()
                        .find(|(_, _, id)| id.monotonic_serial() == u64::from(serial))
                        .ok_or(ERROR)?;
                    self.registry.despawn(handle, epoch)?;
                    self.bodies.swap_remove(d)?;
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
                if !b.fixed {
                    for axis in 0..2 {
                        b.velocity_q32[axis] += b.acceleration_q32[axis] / 120;
                        b.position_q32[axis] += b.velocity_q32[axis] / 120;
                    }
                }
                walls(b);
            }
            // 有界完整枚举，不使用 CSR 的 32 邻居截断，避免密集接触漏碰。
            for _ in 0..4 {
                for a in 0..order.len() {
                    for b in a + 1..order.len() {
                        let ia = order[a].1;
                        let ib = order[b].1;
                        let mut ba = *self.bodies.get(ia)?;
                        let mut bb = *self.bodies.get(ib)?;
                        contact(&mut ba, &mut bb)?;
                        walls(&mut ba);
                        walls(&mut bb);
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
                    let (_, dist) = geometry(ba, bb);
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
fn mul(a: i64, b: i64) -> i64 {
    (i128::from(a) * i128::from(b) / i128::from(ONE)) as i64
}
fn div(a: i64, b: i64) -> i64 {
    (i128::from(a) * i128::from(ONE) / i128::from(b)) as i64
}
fn geometry(a: &Body, b: &Body) -> ([i64; 2], i64) {
    let d = [
        b.position_q32[0] - a.position_q32[0],
        b.position_q32[1] - a.position_q32[1],
    ];
    let sq = i128::from(d[0]) * i128::from(d[0]) + i128::from(d[1]) * i128::from(d[1]);
    (d, (sq as u128).isqrt() as i64)
}
fn walls(b: &mut Body) {
    if b.fixed {
        return;
    }
    for axis in 0..2 {
        let lo = b.radius_q32;
        let hi = SIDE - lo;
        if b.position_q32[axis] < lo {
            b.position_q32[axis] = lo;
            b.velocity_q32[axis] = b.velocity_q32[axis].abs();
        }
        if b.position_q32[axis] > hi {
            b.position_q32[axis] = hi;
            b.velocity_q32[axis] = -b.velocity_q32[axis].abs();
        }
    }
}
fn contact(a: &mut Body, b: &mut Body) -> Result<(), RynvekErrorCode> {
    let (d, dist) = geometry(a, b);
    let penetration = a.radius_q32 + b.radius_q32 - dist;
    if penetration < 0 {
        return Ok(());
    }
    let ia = a.inverse_mass();
    let ib = b.inverse_mass();
    let sum = ia + ib;
    if sum == 0 {
        return if penetration <= 64 { Ok(()) } else { Err(ERROR) };
    }
    let n = if dist == 0 {
        [ONE, 0]
    } else {
        [div(d[0], dist), div(d[1], dist)]
    };
    let relative = mul(b.velocity_q32[0] - a.velocity_q32[0], n[0]) + mul(b.velocity_q32[1] - a.velocity_q32[1], n[1]);
    if relative < 0 {
        let impulse = div(-2 * relative, sum);
        for (axis, normal) in n.into_iter().enumerate() {
            a.velocity_q32[axis] -= mul(mul(impulse, ia), normal);
            b.velocity_q32[axis] += mul(mul(impulse, ib), normal);
        }
    }
    if penetration > 0 {
        let move_a = mul(penetration + 4, div(ia, sum));
        let move_b = penetration + 4 - move_a;
        for (axis, normal) in n.into_iter().enumerate() {
            a.position_q32[axis] -= mul(move_a, normal);
            b.position_q32[axis] += mul(move_b, normal);
        }
    }
    Ok(())
}
