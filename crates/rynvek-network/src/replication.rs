// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 分层状态 Delta 复制（`WireChannelId::StateStream`，GAP-16：16B 对齐列式二进制 + LZ4）。
//!
//! ## 物理迷雾裁剪（GAP-12）
//!
//! [`visible_records`] 是序列化阶段**唯一**读取权威世界实体表的地方：己方实体全量；敌方单位仅当所在 Tile
//! 在 [`VisibilityGrid`] 中可见；敌方建筑仅当任一占地 Tile 可见。未通过的实体不产生任何字节。
//! 敌方实体离开视野时只下发一次 `EntityVisibilityLost`（[`GhostRecord`]：最后看到的种类与位置的静态快照）；
//! 幽灵所在位置重新可见而实体不在时，以 `remove` 清除幽灵。
//!
//! 整个 Delta 计算只依赖 "当前可见集合 + 此前已下发给该客户端的内容"，因此对同一客户端而言，
//! 迷雾中敌方状态的任何变化都不会改变下发字节（集成测试以 "非干扰性" 断言）。
//!
//! ## 状态包布局（Little-Endian，所有列各自补齐到 16 字节边界）
//!
//! | 偏移 | 长度 | 字段 |
//! | --- | --- | --- |
//! | 0 | 1 | `version` = [`STATE_PACKET_VERSION`] |
//! | 1 | 1 | `flags`：bit0 关键帧、bit1 含可见性格、bit2 含己方私有状态 |
//! | 2 | 1 | `faction`（接收方阵营） |
//! | 3 | 1 | 保留（0） |
//! | 4 | 4 | `upsert_count` |
//! | 8 | 8 | `tick` |
//! | 16 | 8 | `base_tick`（Delta 基于的上一包 Tick；关键帧等于 `tick`） |
//! | 24 | 4 | `remove_count` |
//! | 28 | 4 | `lost_count` |
//! | 32 | 4 | `cells_w` |
//! | 36 | 4 | `cells_h` |
//! | 40 | 8 | 保留（0） |
//!
//! 随后依次为列：upsert `id:u32 / kind:u8 / faction:u8 / flags:u8 / x:i64 / y:i64 / hp:i64 / max_hp:i64 /
//! shield:i64 / extra:u32`；remove `id:u32`；lost `id:u32 / kind:u8 / faction:u8 / x:i64 / y:i64 / last_seen:u64`；
//! 可见性格（2 bit 打包）；私有状态（80 字节）。坐标为 `Q32.32` 原始值；建筑坐标是占地左下角 Tile 的整数坐标。

use std::collections::BTreeMap;

use rynvek_core::Q32_32;
use rynvek_rts::buildings::building_spec;
use rynvek_rts::ids::{BuildingKind, FactionId, UnitKind};
use rynvek_rts::world::ArenaWorld;

use crate::error::NetError;
use crate::visibility::{VisibilityGrid, unpack_cells};

/// 状态包格式版本
pub const STATE_PACKET_VERSION: u8 = 1;
/// 状态包头长度
pub const STATE_HEADER_LEN: usize = 48;
/// 建筑种类码起点（单位 `0..=5`，建筑 `16..=23`）
pub const KIND_BUILDING_BASE: u8 = 16;
/// 缺省关键帧间隔：10 秒
pub const DEFAULT_KEYFRAME_INTERVAL_TICKS: u64 = 300;
/// 私有状态段长度（72 字节有效 + 8 字节补齐）
pub const PRIVATE_STATE_LEN: usize = 80;

const FLAG_KEYFRAME: u8 = 1 << 0;
const FLAG_VISIBILITY: u8 = 1 << 1;
const FLAG_PRIVATE: u8 = 1 << 2;

/// 实体记录标志：是建筑
pub const RECORD_BUILDING: u8 = 1 << 0;
/// 实体记录标志：建筑已完工
pub const RECORD_COMPLETE: u8 = 1 << 1;
/// 实体记录标志：建筑供电正常（仅己方）
pub const RECORD_POWERED: u8 = 1 << 2;

/// 单位种类码
pub const fn unit_kind_code(kind: UnitKind) -> u8 {
    kind as u8
}

/// 建筑种类码
pub const fn building_kind_code(kind: BuildingKind) -> u8 {
    KIND_BUILDING_BASE + kind as u8
}

/// 由种类码取建筑种类
pub fn building_kind_of(code: u8) -> Option<BuildingKind> {
    code.checked_sub(KIND_BUILDING_BASE)
        .and_then(|i| BuildingKind::ALL.get(i as usize).copied())
}

/// 一个可见实体的复制记录
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityRecord {
    pub id: u32,
    pub kind: u8,
    pub faction: u8,
    pub flags: u8,
    pub x: i64,
    pub y: i64,
    pub hp_milli: i64,
    pub max_hp_milli: i64,
    pub shield_milli: i64,
    /// 建筑：施工进度（Tick）；单位：0
    pub extra: u32,
}

impl EntityRecord {
    pub const fn is_building(&self) -> bool {
        self.flags & RECORD_BUILDING != 0
    }
}

/// `EntityVisibilityLost` 静态幽灵快照
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GhostRecord {
    pub id: u32,
    pub kind: u8,
    pub faction: u8,
    pub x: i64,
    pub y: i64,
    pub last_seen_tick: u64,
}

/// 己方私有状态（只发给本阵营）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrivateState {
    pub balance_milli: [i64; 3],
    pub capacity_milli: [i64; 3],
    pub biomass_harvested_total_milli: i64,
    pub supply_used: u32,
    pub supply_cap: u32,
    pub score_milli: i64,
}

/// 可见性格段
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibilityCells {
    pub cells_w: u32,
    pub cells_h: u32,
    /// 每格 `0/1/2`（已解包）
    pub cells: Vec<u8>,
}

/// 一个状态包
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatePacket {
    pub tick: u64,
    pub base_tick: u64,
    pub faction: u8,
    pub keyframe: bool,
    pub upserts: Vec<EntityRecord>,
    pub removes: Vec<u32>,
    pub lost: Vec<GhostRecord>,
    pub visibility: Option<VisibilityCells>,
    pub private: Option<PrivateState>,
}

// ------------------------------------------------------------------ 编解码

fn pad16(out: &mut Vec<u8>) {
    while out.len() % 16 != 0 {
        out.push(0);
    }
}

fn column<T: Copy>(out: &mut Vec<u8>, items: &[T], mut put: impl FnMut(&mut Vec<u8>, T)) {
    for &item in items {
        put(out, item);
    }
    pad16(out);
}

impl StatePacket {
    /// 编码为列式二进制
    pub fn encode(&self) -> Vec<u8> {
        let mut flags = 0;
        if self.keyframe {
            flags |= FLAG_KEYFRAME;
        }
        if self.visibility.is_some() {
            flags |= FLAG_VISIBILITY;
        }
        if self.private.is_some() {
            flags |= FLAG_PRIVATE;
        }
        let (cw, ch) = self.visibility.as_ref().map_or((0, 0), |v| (v.cells_w, v.cells_h));
        let mut out = Vec::with_capacity(STATE_HEADER_LEN + self.upserts.len() * 64);
        out.extend_from_slice(&[STATE_PACKET_VERSION, flags, self.faction, 0]);
        out.extend_from_slice(&(self.upserts.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.tick.to_le_bytes());
        out.extend_from_slice(&self.base_tick.to_le_bytes());
        out.extend_from_slice(&(self.removes.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.lost.len() as u32).to_le_bytes());
        out.extend_from_slice(&cw.to_le_bytes());
        out.extend_from_slice(&ch.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        debug_assert_eq!(out.len(), STATE_HEADER_LEN);

        let u = &self.upserts;
        column(&mut out, u, |o, r| o.extend_from_slice(&r.id.to_le_bytes()));
        column(&mut out, u, |o, r| o.push(r.kind));
        column(&mut out, u, |o, r| o.push(r.faction));
        column(&mut out, u, |o, r| o.push(r.flags));
        column(&mut out, u, |o, r| o.extend_from_slice(&r.x.to_le_bytes()));
        column(&mut out, u, |o, r| o.extend_from_slice(&r.y.to_le_bytes()));
        column(&mut out, u, |o, r| o.extend_from_slice(&r.hp_milli.to_le_bytes()));
        column(&mut out, u, |o, r| o.extend_from_slice(&r.max_hp_milli.to_le_bytes()));
        column(&mut out, u, |o, r| o.extend_from_slice(&r.shield_milli.to_le_bytes()));
        column(&mut out, u, |o, r| o.extend_from_slice(&r.extra.to_le_bytes()));

        column(&mut out, &self.removes, |o, id| o.extend_from_slice(&id.to_le_bytes()));

        let l = &self.lost;
        column(&mut out, l, |o, g| o.extend_from_slice(&g.id.to_le_bytes()));
        column(&mut out, l, |o, g| o.push(g.kind));
        column(&mut out, l, |o, g| o.push(g.faction));
        column(&mut out, l, |o, g| o.extend_from_slice(&g.x.to_le_bytes()));
        column(&mut out, l, |o, g| o.extend_from_slice(&g.y.to_le_bytes()));
        column(&mut out, l, |o, g| o.extend_from_slice(&g.last_seen_tick.to_le_bytes()));

        if let Some(v) = &self.visibility {
            out.extend_from_slice(&crate::visibility::pack_cells(&v.cells));
            pad16(&mut out);
        }
        if let Some(p) = &self.private {
            for v in p.balance_milli.iter().chain(&p.capacity_milli) {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.extend_from_slice(&p.biomass_harvested_total_milli.to_le_bytes());
            out.extend_from_slice(&p.supply_used.to_le_bytes());
            out.extend_from_slice(&p.supply_cap.to_le_bytes());
            out.extend_from_slice(&p.score_milli.to_le_bytes());
            pad16(&mut out);
        }
        out
    }

    /// 严格解码：长度、保留字节与补齐字节都必须完全符合布局
    pub fn decode(bytes: &[u8]) -> Result<Self, NetError> {
        let mut r = Reader { bytes, pos: 0 };
        let version = r.u8()?;
        if version != STATE_PACKET_VERSION {
            return Err(bad(format!("不支持的状态包版本 {version}")));
        }
        let flags = r.u8()?;
        if flags & !(FLAG_KEYFRAME | FLAG_VISIBILITY | FLAG_PRIVATE) != 0 {
            return Err(bad("状态包标志含未定义位"));
        }
        let faction = r.u8()?;
        if r.u8()? != 0 {
            return Err(bad("状态包保留字节非 0"));
        }
        let nu = r.u32()? as usize;
        let tick = r.u64()?;
        let base_tick = r.u64()?;
        let nr = r.u32()? as usize;
        let nl = r.u32()? as usize;
        let cells_w = r.u32()?;
        let cells_h = r.u32()?;
        if r.u64()? != 0 {
            return Err(bad("状态包保留字段非 0"));
        }
        // 计数上限防止恶意包导致超大分配：每条记录至少 1 字节，不可能多于剩余字节
        if nu + nr + nl > bytes.len() {
            return Err(bad("状态包记录计数超出负载长度"));
        }
        let ids = r.col(nu, Reader::u32)?;
        let kinds = r.col(nu, Reader::u8)?;
        let factions = r.col(nu, Reader::u8)?;
        let rflags = r.col(nu, Reader::u8)?;
        let xs = r.col(nu, Reader::i64)?;
        let ys = r.col(nu, Reader::i64)?;
        let hps = r.col(nu, Reader::i64)?;
        let maxs = r.col(nu, Reader::i64)?;
        let shields = r.col(nu, Reader::i64)?;
        let extras = r.col(nu, Reader::u32)?;
        let upserts = (0..nu)
            .map(|i| EntityRecord {
                id: ids[i],
                kind: kinds[i],
                faction: factions[i],
                flags: rflags[i],
                x: xs[i],
                y: ys[i],
                hp_milli: hps[i],
                max_hp_milli: maxs[i],
                shield_milli: shields[i],
                extra: extras[i],
            })
            .collect();
        let removes = r.col(nr, Reader::u32)?;
        let gids = r.col(nl, Reader::u32)?;
        let gkinds = r.col(nl, Reader::u8)?;
        let gfactions = r.col(nl, Reader::u8)?;
        let gxs = r.col(nl, Reader::i64)?;
        let gys = r.col(nl, Reader::i64)?;
        let gseen = r.col(nl, Reader::u64)?;
        let lost = (0..nl)
            .map(|i| GhostRecord {
                id: gids[i],
                kind: gkinds[i],
                faction: gfactions[i],
                x: gxs[i],
                y: gys[i],
                last_seen_tick: gseen[i],
            })
            .collect();
        let visibility = if flags & FLAG_VISIBILITY != 0 {
            let n = (cells_w as usize)
                .checked_mul(cells_h as usize)
                .filter(|n| n.div_ceil(4) <= bytes.len())
                .ok_or_else(|| bad("可见性格尺寸非法"))?;
            let packed = r.take(n.div_ceil(4))?;
            let cells = unpack_cells(packed, n).ok_or_else(|| bad("可见性格截断"))?;
            if cells.iter().any(|&c| c > 2) {
                return Err(bad("可见性格取值非法"));
            }
            r.align()?;
            Some(VisibilityCells {
                cells_w,
                cells_h,
                cells,
            })
        } else {
            if cells_w != 0 || cells_h != 0 {
                return Err(bad("无可见性段时尺寸必须为 0"));
            }
            None
        };
        let private = if flags & FLAG_PRIVATE != 0 {
            let mut p = PrivateState::default();
            for v in p.balance_milli.iter_mut().chain(p.capacity_milli.iter_mut()) {
                *v = r.i64()?;
            }
            p.biomass_harvested_total_milli = r.i64()?;
            p.supply_used = r.u32()?;
            p.supply_cap = r.u32()?;
            p.score_milli = r.i64()?;
            r.align()?;
            Some(p)
        } else {
            None
        };
        if r.pos != bytes.len() {
            return Err(bad(format!("状态包末尾多出 {} 字节", bytes.len() - r.pos)));
        }
        let keyframe = flags & FLAG_KEYFRAME != 0;
        if keyframe && base_tick != tick {
            return Err(bad("关键帧的 base_tick 必须等于 tick"));
        }
        Ok(Self {
            tick,
            base_tick,
            faction,
            keyframe,
            upserts,
            removes,
            lost,
            visibility,
            private,
        })
    }
}

fn bad(msg: impl Into<String>) -> NetError {
    NetError::Payload(msg.into())
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], NetError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.bytes.len())
            .ok_or_else(|| bad("状态包截断"))?;
        let s = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, NetError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, NetError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4")))
    }
    fn u64(&mut self) -> Result<u64, NetError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8")))
    }
    fn i64(&mut self) -> Result<i64, NetError> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().expect("8")))
    }
    /// 补齐字节必须为 0
    fn align(&mut self) -> Result<(), NetError> {
        let pad = (16 - self.pos % 16) % 16;
        if self.take(pad)?.iter().any(|&b| b != 0) {
            return Err(bad("补齐字节非 0"));
        }
        Ok(())
    }
    fn col<T>(&mut self, n: usize, mut read: impl FnMut(&mut Self) -> Result<T, NetError>) -> Result<Vec<T>, NetError> {
        let v = (0..n).map(|_| read(self)).collect::<Result<Vec<_>, _>>()?;
        self.align()?;
        Ok(v)
    }
}

// ------------------------------------------------------------------ 物理裁剪

/// 序列化阶段的 AOI 裁剪：返回 `viewer` 本 Tick 可以知道的全部实体记录（按实体序号升序）
pub fn visible_records(world: &ArenaWorld, viewer: FactionId, grid: &VisibilityGrid) -> BTreeMap<u32, EntityRecord> {
    let mut out = BTreeMap::new();
    for u in world.units.values() {
        let own = u.faction == viewer;
        if !own && !u.position.tile().is_some_and(|(x, y)| grid.tile_visible(x, y)) {
            continue; // 零字节下发
        }
        out.insert(
            u.id.0,
            EntityRecord {
                id: u.id.0,
                kind: unit_kind_code(u.kind),
                faction: u.faction.0,
                flags: 0,
                x: u.position.x.0,
                y: u.position.y.0,
                hp_milli: u.defense.hp_milli,
                max_hp_milli: u.max_hp_milli,
                shield_milli: u.defense.shield_milli,
                extra: 0,
            },
        );
    }
    for b in world.buildings.values() {
        let own = b.faction == viewer;
        let f = b.footprint();
        if !own && !grid.rect_visible(b.origin.0, b.origin.1, f, f) {
            continue;
        }
        let mut flags = RECORD_BUILDING;
        if b.complete {
            flags |= RECORD_COMPLETE;
        }
        if own && b.powered {
            flags |= RECORD_POWERED;
        }
        out.insert(
            b.id.0,
            EntityRecord {
                id: b.id.0,
                kind: building_kind_code(b.kind),
                faction: b.faction.0,
                flags,
                x: Q32_32::from_int(b.origin.0 as i32).0,
                y: Q32_32::from_int(b.origin.1 as i32).0,
                hp_milli: b.defense.hp_milli,
                max_hp_milli: b.max_hp_milli,
                shield_milli: b.defense.shield_milli,
                extra: b.build_progress,
            },
        );
    }
    out
}

/// 己方私有状态
pub fn private_state(world: &ArenaWorld, viewer: FactionId) -> Option<PrivateState> {
    let f = world.faction(viewer)?;
    let l = &f.ledger;
    let (used, cap) = world.supply(viewer);
    Some(PrivateState {
        balance_milli: [l.balance.biomass, l.balance.flux, l.balance.neurogel],
        capacity_milli: [l.capacity.biomass, l.capacity.flux, l.capacity.neurogel],
        biomass_harvested_total_milli: l.biomass_harvested_total,
        supply_used: used,
        supply_cap: cap,
        score_milli: world.score_milli(viewer),
    })
}

/// 幽灵所在位置当前是否可见（可见则幽灵失效）
fn ghost_spot_visible(g: &GhostRecord, grid: &VisibilityGrid) -> bool {
    let (x, y) = ((g.x >> 32) as u32, (g.y >> 32) as u32);
    match building_kind_of(g.kind) {
        Some(kind) => {
            let f = building_spec(kind).footprint;
            grid.rect_visible(x, y, f, f)
        }
        None => grid.tile_visible(x, y),
    }
}

// ------------------------------------------------------------------ 服务端：每客户端复制器

/// 每个连接一个的服务端复制器
#[derive(Clone, Debug)]
pub struct Replicator {
    viewer: FactionId,
    keyframe_interval: u64,
    last_keyframe_tick: Option<u64>,
    last_tick: Option<u64>,
    force_keyframe: bool,
    known: BTreeMap<u32, EntityRecord>,
    ghosts: BTreeMap<u32, GhostRecord>,
    last_cells: Vec<u8>,
}

impl Replicator {
    pub fn new(viewer: FactionId, keyframe_interval: u64) -> Self {
        Self {
            viewer,
            keyframe_interval: keyframe_interval.max(1),
            last_keyframe_tick: None,
            last_tick: None,
            force_keyframe: true,
            known: BTreeMap::new(),
            ghosts: BTreeMap::new(),
            last_cells: Vec::new(),
        }
    }

    pub const fn viewer(&self) -> FactionId {
        self.viewer
    }

    /// 下一包强制为关键帧（新连接、断线重连、客户端请求）
    pub fn request_keyframe(&mut self) {
        self.force_keyframe = true;
    }

    /// 由权威世界生成下一状态包
    pub fn next_packet(&mut self, world: &ArenaWorld) -> StatePacket {
        let grid = match world.faction(self.viewer) {
            Some(f) => VisibilityGrid::from_fog(&f.fog),
            None => VisibilityGrid::from_fog(&rynvek_rts::fog::FactionFog::new(world.map.width, world.map.height)),
        };
        let current = visible_records(world, self.viewer, &grid);
        let tick = world.tick;
        let keyframe = self.force_keyframe
            || self
                .last_keyframe_tick
                .is_none_or(|k| tick >= k + self.keyframe_interval);

        // 1) 离开 "可知集合" 的实体：己方 → remove；敌方 → EntityVisibilityLost 幽灵
        let mut removes = Vec::new();
        let mut lost = Vec::new();
        for (id, rec) in &self.known {
            if current.contains_key(id) {
                continue;
            }
            let ghost = GhostRecord {
                id: *id,
                kind: rec.kind,
                faction: rec.faction,
                x: rec.x,
                y: rec.y,
                last_seen_tick: self.last_tick.unwrap_or(tick),
            };
            // 己方实体消失、或敌方实体在仍可见的位置消失（视野内死亡）⇒ remove；
            // 只有真正 "离开视野" 的敌方实体才留下静态幽灵
            if rec.faction == self.viewer.0 || ghost_spot_visible(&ghost, &grid) {
                removes.push(*id);
            } else {
                self.ghosts.insert(*id, ghost);
                lost.push(ghost);
            }
        }
        // 2) 幽灵失效：实体重新可见（被 upsert 覆盖），或幽灵位置已可见但实体不在
        let mut expired = Vec::new();
        self.ghosts.retain(|id, g| {
            if current.contains_key(id) {
                return false;
            }
            if ghost_spot_visible(g, &grid) {
                expired.push(*id);
                return false;
            }
            true
        });
        removes.extend(expired);
        removes.sort_unstable();
        // 3) 新增或变化的实体
        let upserts: Vec<EntityRecord> = if keyframe {
            current.values().copied().collect()
        } else {
            current
                .values()
                .filter(|r| self.known.get(&r.id) != Some(r))
                .copied()
                .collect()
        };
        let cells_changed = grid.cells() != self.last_cells.as_slice();
        let visibility = (keyframe || cells_changed).then(|| VisibilityCells {
            cells_w: grid.cells_w,
            cells_h: grid.cells_h,
            cells: grid.cells().to_vec(),
        });
        let packet = StatePacket {
            tick,
            base_tick: if keyframe { tick } else { self.last_tick.unwrap_or(tick) },
            faction: self.viewer.0,
            keyframe,
            upserts,
            removes: if keyframe { Vec::new() } else { removes },
            lost: if keyframe {
                self.ghosts.values().copied().collect()
            } else {
                lost
            },
            visibility,
            private: private_state(world, self.viewer),
        };
        self.known = current;
        self.last_cells = grid.cells().to_vec();
        self.last_tick = Some(tick);
        if keyframe {
            self.last_keyframe_tick = Some(tick);
            self.force_keyframe = false;
        }
        packet
    }
}

// ------------------------------------------------------------------ 客户端：副本

/// 客户端状态副本
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplicaState {
    pub tick: Option<u64>,
    pub entities: BTreeMap<u32, EntityRecord>,
    pub ghosts: BTreeMap<u32, GhostRecord>,
    pub visibility: Option<VisibilityCells>,
    pub private: Option<PrivateState>,
}

impl ReplicaState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 应用一个状态包；Delta 的 `base_tick` 必须等于副本当前 Tick，否则需要关键帧
    pub fn apply(&mut self, p: &StatePacket) -> Result<(), NetError> {
        if p.keyframe {
            self.entities.clear();
            self.ghosts.clear();
        } else if self.tick != Some(p.base_tick) {
            return Err(bad(format!(
                "Delta 基于 Tick {}，但副本位于 {:?}：需要关键帧",
                p.base_tick, self.tick
            )));
        }
        for id in &p.removes {
            self.entities.remove(id);
            self.ghosts.remove(id);
        }
        for g in &p.lost {
            self.entities.remove(&g.id);
            self.ghosts.insert(g.id, *g);
        }
        for r in &p.upserts {
            self.ghosts.remove(&r.id);
            self.entities.insert(r.id, *r);
        }
        if let Some(v) = &p.visibility {
            self.visibility = Some(v.clone());
        }
        self.private = p.private;
        self.tick = Some(p.tick);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StatePacket {
        StatePacket {
            tick: 120,
            base_tick: 118,
            faction: 1,
            keyframe: false,
            upserts: vec![
                EntityRecord {
                    id: 7,
                    kind: 1,
                    faction: 1,
                    flags: 0,
                    x: (40i64 << 32) + (1 << 31),
                    y: -5,
                    hp_milli: 120_000,
                    max_hp_milli: 150_000,
                    shield_milli: 0,
                    extra: 0,
                },
                EntityRecord {
                    id: 9,
                    kind: building_kind_code(BuildingKind::SpawningVat),
                    faction: 1,
                    flags: RECORD_BUILDING | RECORD_COMPLETE,
                    x: 30 << 32,
                    y: 31 << 32,
                    hp_milli: 1,
                    max_hp_milli: 2,
                    shield_milli: 3,
                    extra: 150,
                },
            ],
            removes: vec![3],
            lost: vec![GhostRecord {
                id: 11,
                kind: 0,
                faction: 0,
                x: 1,
                y: 2,
                last_seen_tick: 117,
            }],
            visibility: Some(VisibilityCells {
                cells_w: 3,
                cells_h: 2,
                cells: vec![0, 1, 2, 2, 1, 0],
            }),
            private: Some(PrivateState {
                balance_milli: [1, 2, 3],
                capacity_milli: [4, 5, 6],
                biomass_harvested_total_milli: 7,
                supply_used: 8,
                supply_cap: 9,
                score_milli: -10,
            }),
        }
    }

    #[test]
    fn packet_roundtrip_and_16_byte_alignment() {
        let p = sample();
        let bytes = p.encode();
        assert_eq!(bytes.len() % 16, 0);
        assert_eq!(StatePacket::decode(&bytes).unwrap(), p);
        // 头 48 + upsert 列 10×16 + remove 16 + lost 6×16 + 可见性 16 + 私有 80
        assert_eq!(bytes.len(), 48 + 10 * 16 + 16 + 6 * 16 + 16 + 80);
        assert_eq!(&bytes[..4], &[1, 0b110, 1, 0]);
    }

    #[test]
    fn empty_keyframe_is_just_the_header() {
        let p = StatePacket {
            tick: 5,
            base_tick: 5,
            faction: 0,
            keyframe: true,
            upserts: vec![],
            removes: vec![],
            lost: vec![],
            visibility: None,
            private: None,
        };
        let bytes = p.encode();
        assert_eq!(bytes.len(), STATE_HEADER_LEN);
        assert_eq!(StatePacket::decode(&bytes).unwrap(), p);
    }

    #[test]
    fn strict_decoder_rejects_every_truncation_and_trailing_byte() {
        let bytes = sample().encode();
        for n in 0..bytes.len() {
            assert!(StatePacket::decode(&bytes[..n]).is_err(), "截断到 {n} 字节");
        }
        let mut longer = bytes.clone();
        longer.extend_from_slice(&[0; 16]);
        assert!(StatePacket::decode(&longer).is_err());
        let mut bad_pad = bytes.clone();
        bad_pad[48 + 8] = 1; // id 列补齐区
        assert!(StatePacket::decode(&bad_pad).is_err());
        let mut huge = bytes;
        huge[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(StatePacket::decode(&huge).is_err());
    }

    #[test]
    fn replica_rejects_delta_on_wrong_base() {
        let mut r = ReplicaState::new();
        assert!(r.apply(&sample()).is_err(), "没有关键帧的副本不能接受 Delta");
        let mut kf = sample();
        kf.keyframe = true;
        kf.base_tick = kf.tick;
        r.apply(&kf).unwrap();
        assert_eq!(r.entities.len(), 2);
        assert_eq!(r.ghosts.len(), 1);
    }

    #[test]
    fn kind_codes() {
        assert_eq!(unit_kind_code(UnitKind::Forager), 0);
        assert_eq!(building_kind_code(BuildingKind::NexusCore), 16);
        assert_eq!(building_kind_of(16 + 7), Some(BuildingKind::ALL[7]));
        assert_eq!(building_kind_of(3), None);
        assert_eq!(building_kind_of(24), None);
    }
}
