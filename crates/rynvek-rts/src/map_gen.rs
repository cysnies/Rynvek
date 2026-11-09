// crates/rynvek-rts/src/map_gen.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 竞技地图 180° 中心对称确定性生成器（规范 09 §9.2，补全 GAP-19）。
//!
//! 本文件上半部分逐字照搬规范冻结源码（仅为通过 `-D warnings` 增加 clippy 豁免属性），
//! 不得改动算法、常量与随机键布局；地形物理属性、尺寸预设与连通性校验见 [`crate::terrain`] 与 [`crate::arena_map`]。

use rynvek_core::rng::RandomKey;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainTileKind {
    OpenSubstrate = 0,
    ConductiveBasalt = 1,
    NutrientMarsh = 2,
    AcidicFissure = 3,
    ImperviousWall = 4,
}

/// 生成 100% 中心旋转对称的竞技场地形网格与初始环境场（保证 PvP 双方绝对公平）
// 以下 clippy 豁免仅为保持规范冻结源码逐字不变（`r >= 180 && r < 210` 区间写法）。
#[allow(clippy::manual_range_contains)]
pub fn generate_symmetric_arena_map(width: usize, height: usize, map_seed: u64) -> Vec<TerrainTileKind> {
    assert!(width % 2 == 0 && height % 2 == 0, "Arena dimensions must be even");
    let mut tiles = vec![TerrainTileKind::OpenSubstrate; width * height];
    let half_h = height / 2;

    for y in 0..half_h {
        for x in 0..width {
            let rk = RandomKey {
                world_seed: map_seed,
                tick: 0,
                entity_serial: ((y as u64) << 32) | (x as u64),
                stream_and_phase: 0x0001_0000,
                sub_counter: 0,
            };
            let r = rk.generate_u32x4()[0] % 1000;

            // 预留左下角 (32, 32) 半径 20 Tiles 的出生安全平地
            let dx_spawn = (x as i32) - 32;
            let dy_spawn = (y as i32) - 32;
            let dist_sq = dx_spawn * dx_spawn + dy_spawn * dy_spawn;

            let kind = if dist_sq <= 400 {
                TerrainTileKind::ConductiveBasalt
            } else if dist_sq <= 784 && (r < 180) {
                TerrainTileKind::NutrientMarsh
            } else if dist_sq <= 900 && (r >= 180 && r < 210) {
                TerrainTileKind::AcidicFissure
            } else if r < 85 {
                TerrainTileKind::ImperviousWall
            } else if r < 160 {
                TerrainTileKind::NutrientMarsh
            } else if r < 175 {
                TerrainTileKind::AcidicFissure
            } else if r < 300 {
                TerrainTileKind::ConductiveBasalt
            } else {
                TerrainTileKind::OpenSubstrate
            };

            // 同步写入 (x, y) 与 180° 中心对称点 (width - 1 - x, height - 1 - y)
            tiles[y * width + x] = kind;
            let sym_x = width - 1 - x;
            let sym_y = height - 1 - y;
            tiles[sym_y * width + sym_x] = kind;
        }
    }
    tiles
}

// ---------------------------------------------------------------------------
// 以下为规范源码之外的补充：TerrainTileKind 的数值往返与 serde 形态（不影响生成算法）
// ---------------------------------------------------------------------------

impl TerrainTileKind {
    /// 五种地形按 `repr(u8)` 顺序
    pub const ALL: [Self; 5] = [
        Self::OpenSubstrate,
        Self::ConductiveBasalt,
        Self::NutrientMarsh,
        Self::AcidicFissure,
        Self::ImperviousWall,
    ];

    /// 由 `repr(u8)` 值还原
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::OpenSubstrate),
            1 => Some(Self::ConductiveBasalt),
            2 => Some(Self::NutrientMarsh),
            3 => Some(Self::AcidicFissure),
            4 => Some(Self::ImperviousWall),
            _ => None,
        }
    }
}

/// 序列化为 `repr(u8)` 整数（与 Studio `TerrainTileKind 0–4` 及 TypeScript 端一致）
impl serde::Serialize for TerrainTileKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(*self as u8)
    }
}

impl<'de> serde::Deserialize<'de> for TerrainTileKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = u8::deserialize(d)?;
        Self::from_u8(v).ok_or_else(|| serde::de::Error::custom(format!("未知的地形编号：{v}")))
    }
}
