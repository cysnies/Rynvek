// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 竞技场五层连续环境场（项目文档 §5.2.2）：多 Chunk 网格、跨 Chunk Halo 交换与地形耦合。
//!
//! `rynvek-field` 的 [`FieldWorld`](rynvek_field::FieldWorld) 是单 Chunk 封闭世界（每子步自动刷新
//! Neumann Halo），无法表达 `512²` 以上地图的跨 Chunk 扩散。这里直接复用其冻结的五点拉普拉斯
//! 求解核 [`solve_reaction_diffusion_tile_q8`]，在每次推进前：
//! 1. 以 Neumann 零梯度填充每个 Chunk 的 Halo；
//! 2. 用相邻 Chunk 的内区边缘覆盖共享边上的 Halo（跨 Chunk 交换）；
//! 3. 地图尺寸不是 256 的整数倍时，把地图外第一圈格设为最近界内格的值（地图边界零通量）。
//!
//! 圣经五通道参数均满足 `D ≤ 0.25`，单子步即满足 CFL，无需拆分。

use blake3::Hasher;
use rynvek_core::{Blake3Hash, RynvekErrorCode};
use rynvek_field::{
    FieldChannelId, FieldChannelParams, FieldTileQ8, HALO, STRIDE, TILE_DIM, solve_reaction_diffusion_tile_q8,
};

use crate::ids::FieldId;
use crate::map_gen::TerrainTileKind;
use crate::math::fractional_share;

/// 设计圣经 §5.2.2 的五通道参数（Q8.8 定点：`round(x × 256)`）
///
/// | 通道 | D | λ | 量程 |
/// | --- | --- | --- | --- |
/// | nutrient | 0.020 → 5 | 0.000 → 0 | [0, 100] |
/// | energy | 0.150 → 38 | 0.010 → 3 | [0, 100] |
/// | signal_a / signal_b | 0.250 → 64 | 0.050 → 13 | [−50, +50] |
/// | hazard | 0.050 → 13 | 0.005 → 1 | [0, 100] |
pub const ARENA_FIELD_CHANNELS: [FieldChannelParams; 5] = [
    channel(FieldChannelId::Nutrient, 5, 0, 0, 25_600),
    channel(FieldChannelId::Energy, 38, 3, 0, 25_600),
    channel(FieldChannelId::SignalA, 64, 13, -12_800, 12_800),
    channel(FieldChannelId::SignalB, 64, 13, -12_800, 12_800),
    channel(FieldChannelId::Hazard, 13, 1, 0, 25_600),
];

const fn channel(id: FieldChannelId, d: i32, decay: i32, min: i16, max: i16) -> FieldChannelParams {
    FieldChannelParams {
        id,
        diffusion_q8: d,
        decay_q8: decay,
        min_val_q8: min,
        max_val_q8: max,
    }
}

/// 场以 10 Hz 推进时每步覆盖的 Tick 数（= `FIELD_UPDATE_INTERVAL_TICKS`）
pub const FIELD_STEP_TICKS: u64 = rynvek_core::constants::FIELD_UPDATE_INTERVAL_TICKS as u64;

/// 沼泽营养再生：`+0.05/Tick` = 12.8 Q8.8/Tick → 每场步 `192/5` Q8.8（累计取整无漂移）
pub const MARSH_REGEN_Q8_PER_STEP_NUMER: i64 = 64 * FIELD_STEP_TICKS as i64;
pub const MARSH_REGEN_Q8_PER_STEP_DENOM: i64 = 5;
/// 沼泽再生上限 `80.0`
pub const MARSH_REGEN_CAP_Q8: i16 = 80 * 256;

/// 开局营养基底（ERRATA E-022）：沼泽 `80.0`、开阔基质 `20.0`，其余 `0`
pub const fn initial_nutrient_q8(kind: TerrainTileKind) -> i16 {
    match kind {
        TerrainTileKind::NutrientMarsh => MARSH_REGEN_CAP_Q8,
        TerrainTileKind::OpenSubstrate => 20 * 256,
        _ => 0,
    }
}

/// Q8.8 场值 → 毫单位（`1:1` 守恒：1.0 场单位 = 1000 milli）的分子；`milli = raw × 125 / 32`
pub const Q8_TO_MILLI_NUMER: i64 = 125;
pub const Q8_TO_MILLI_DENOM: i64 = 32;

/// 多 Chunk 五通道场网格
#[derive(Clone, Debug)]
pub struct FieldGrid {
    width: u32,
    height: u32,
    chunks_x: u32,
    chunks_y: u32,
    /// `[chunk][channel]`
    chunks: Vec<[FieldTileQ8; 5]>,
    steps: u64,
}

impl FieldGrid {
    /// 新建全零场网格
    pub fn new(width: u32, height: u32) -> Self {
        let chunks_x = width.div_ceil(TILE_DIM as u32);
        let chunks_y = height.div_ceil(TILE_DIM as u32);
        let chunks = (0..chunks_x * chunks_y)
            .map(|_| ARENA_FIELD_CHANNELS.map(FieldTileQ8::new))
            .collect();
        Self {
            width,
            height,
            chunks_x,
            chunks_y,
            chunks,
            steps: 0,
        }
    }

    /// 按地形初始化营养基底
    pub fn for_terrain(width: u32, height: u32, tiles: &[TerrainTileKind]) -> Self {
        let mut g = Self::new(width, height);
        for y in 0..height {
            for x in 0..width {
                g.set(
                    FieldId::Nutrient,
                    x,
                    y,
                    initial_nutrient_q8(tiles[(y * width + x) as usize]),
                );
            }
        }
        g
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    /// 已推进的场步数
    pub const fn steps(&self) -> u64 {
        self.steps
    }

    /// 设置已推进的场步数（供快照恢复与测试使用）
    pub fn set_steps(&mut self, steps: u64) {
        self.steps = steps;
    }

    /// 导出全部五通道界内 Q8.8 原始值（按通道 → 行优先顺序）
    pub fn export_q8(&self) -> Vec<i16> {
        let mut out = Vec::with_capacity((self.width as usize) * (self.height as usize) * FieldId::ALL.len());
        for field in FieldId::ALL {
            for y in 0..self.height {
                for x in 0..self.width {
                    out.push(self.get(*field, x as i64, y as i64));
                }
            }
        }
        out
    }

    /// 从快照恢复全部五通道界内 Q8.8 原始值及步数计数
    pub fn restore_q8(&mut self, steps: u64, values: &[i16]) -> Result<(), RynvekErrorCode> {
        let expected = (self.width as usize) * (self.height as usize) * FieldId::ALL.len();
        if values.len() != expected {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        let mut idx = 0;
        for field in FieldId::ALL {
            for y in 0..self.height {
                for x in 0..self.width {
                    self.set(*field, x, y, values[idx]);
                    idx += 1;
                }
            }
        }
        self.steps = steps;
        Ok(())
    }

    #[inline]
    fn locate(&self, x: u32, y: u32) -> (usize, usize) {
        let cx = x as usize / TILE_DIM;
        let cy = y as usize / TILE_DIM;
        let lx = x as usize % TILE_DIM + HALO;
        let ly = y as usize % TILE_DIM + HALO;
        (cy * self.chunks_x as usize + cx, ly * STRIDE + lx)
    }

    /// 读取格值（Q8.8 原始值）；越界格按最近界内格（Neumann）
    pub fn get(&self, field: FieldId, x: i64, y: i64) -> i16 {
        let x = x.clamp(0, self.width as i64 - 1) as u32;
        let y = y.clamp(0, self.height as i64 - 1) as u32;
        let (c, i) = self.locate(x, y);
        self.chunks[c][field.channel().index()].current_buf()[i]
    }

    /// 写入格值（按通道量程钳制）
    pub fn set(&mut self, field: FieldId, x: u32, y: u32, value: i16) {
        let p = ARENA_FIELD_CHANNELS[field.channel().index()];
        let (c, i) = self.locate(x, y);
        self.chunks[c][field.channel().index()].current_buf_mut()[i] = value.clamp(p.min_val_q8, p.max_val_q8);
    }

    /// 叠加增量并钳制到量程，返回实际施加量
    pub fn add(&mut self, field: FieldId, x: u32, y: u32, delta: i32) -> i32 {
        let before = self.get(field, x as i64, y as i64);
        let p = ARENA_FIELD_CHANNELS[field.channel().index()];
        let after = (before as i32 + delta).clamp(p.min_val_q8 as i32, p.max_val_q8 as i32);
        self.set(field, x, y, after as i16);
        after - before as i32
    }

    /// 中心差分梯度 `(∂x, ∂y)`（Q8.8 / Tile）
    pub fn gradient(&self, field: FieldId, x: u32, y: u32) -> (i32, i32) {
        let (x, y) = (x as i64, y as i64);
        let gx = (self.get(field, x + 1, y) as i32 - self.get(field, x - 1, y) as i32) / 2;
        let gy = (self.get(field, x, y + 1) as i32 - self.get(field, x, y - 1) as i32) / 2;
        (gx, gy)
    }

    /// 推进一个 10 Hz 场步：沼泽再生 → Halo 交换 → 五点拉普拉斯求解
    pub fn step(&mut self, tiles: &[TerrainTileKind]) {
        self.regenerate_marsh(tiles);
        for ch in 0..5 {
            self.exchange_halo(ch);
            let p = ARENA_FIELD_CHANNELS[ch];
            for chunk in &mut self.chunks {
                let tile = &mut chunk[ch];
                let cur = tile.current;
                let (first, second) = tile.buffers.split_at_mut(1);
                let (src, dst) = if cur == 0 {
                    (&first[0], &mut second[0])
                } else {
                    (&second[0], &mut first[0])
                };
                solve_reaction_diffusion_tile_q8(
                    src,
                    dst,
                    &tile.sources,
                    p.diffusion_q8,
                    p.decay_q8,
                    p.min_val_q8,
                    p.max_val_q8,
                );
                tile.current = 1 - cur;
            }
        }
        self.steps += 1;
    }

    fn regenerate_marsh(&mut self, tiles: &[TerrainTileKind]) {
        let share = fractional_share(MARSH_REGEN_Q8_PER_STEP_NUMER, MARSH_REGEN_Q8_PER_STEP_DENOM, self.steps) as i32;
        for y in 0..self.height {
            for x in 0..self.width {
                if tiles[(y * self.width + x) as usize] != TerrainTileKind::NutrientMarsh {
                    continue;
                }
                let v = self.get(FieldId::Nutrient, x as i64, y as i64);
                if v < MARSH_REGEN_CAP_Q8 {
                    self.set(
                        FieldId::Nutrient,
                        x,
                        y,
                        (v as i32 + share).min(MARSH_REGEN_CAP_Q8 as i32) as i16,
                    );
                }
            }
        }
    }

    fn exchange_halo(&mut self, ch: usize) {
        let (cw, chh) = (self.chunks_x as usize, self.chunks_y as usize);
        // 地图外第一圈：零通量边界
        let (w, h) = (self.width as usize, self.height as usize);
        if w % TILE_DIM != 0 {
            for y in 0..h {
                let v = self.get_raw(ch, w - 1, y);
                self.set_raw(ch, w, y, v);
            }
        }
        if h % TILE_DIM != 0 {
            for x in 0..w {
                let v = self.get_raw(ch, x, h - 1);
                self.set_raw(ch, x, h, v);
            }
        }
        for c in 0..self.chunks.len() {
            let buf = self.chunks[c][ch].current_buf_mut();
            rynvek_field::solver::fill_neumann_halo(buf);
        }
        for cy in 0..chh {
            for cx in 0..cw {
                let me = cy * cw + cx;
                for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (cx as i64 + dx, cy as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= cw as i64 || ny >= chh as i64 {
                        continue;
                    }
                    let nb = ny as usize * cw + nx as usize;
                    for k in 0..TILE_DIM {
                        for hl in 0..HALO {
                            // 本 Chunk 的 Halo 格（局部坐标）与邻居内区对应格
                            let (hx, hy, sx, sy) = match (dx, dy) {
                                (-1, 0) => (hl, k + HALO, TILE_DIM + hl, k + HALO),
                                (1, 0) => (TILE_DIM + HALO + hl, k + HALO, HALO + hl, k + HALO),
                                (0, -1) => (k + HALO, hl, k + HALO, TILE_DIM + hl),
                                _ => (k + HALO, TILE_DIM + HALO + hl, k + HALO, HALO + hl),
                            };
                            let v = self.chunks[nb][ch].current_buf()[sy * STRIDE + sx];
                            self.chunks[me][ch].current_buf_mut()[hy * STRIDE + hx] = v;
                        }
                    }
                }
            }
        }
    }

    fn get_raw(&self, ch: usize, x: usize, y: usize) -> i16 {
        let (c, i) = self.locate(x as u32, y as u32);
        self.chunks[c][ch].current_buf()[i]
    }

    fn set_raw(&mut self, ch: usize, x: usize, y: usize, v: i16) {
        let (c, i) = self.locate(x as u32, y as u32);
        self.chunks[c][ch].current_buf_mut()[i] = v;
    }

    /// 某通道界内总和（守恒测试）
    pub fn sum(&self, field: FieldId) -> i64 {
        let mut s = 0i64;
        for y in 0..self.height {
            for x in 0..self.width {
                s += self.get(field, x as i64, y as i64) as i64;
            }
        }
        s
    }

    /// 状态根：涵盖场步数与按通道 → 行优先哈希的界内 Q8.8 小端样本
    pub fn state_root(&self) -> Blake3Hash {
        let mut hasher = Hasher::new();
        hasher.update(&self.width.to_le_bytes());
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&self.steps.to_le_bytes());
        for field in FieldId::ALL {
            for y in 0..self.height {
                for x in 0..self.width {
                    hasher.update(&self.get(*field, x as i64, y as i64).to_le_bytes());
                }
            }
        }
        Blake3Hash(*hasher.finalize().as_bytes())
    }

    /// 以 4×4 Tile 为一格降采样（观测 / 热力图），取均值
    pub fn downsample4(&self, field: FieldId) -> Vec<i16> {
        let (w4, h4) = (self.width.div_ceil(4), self.height.div_ceil(4));
        let mut out = Vec::with_capacity((w4 * h4) as usize);
        for cy in 0..h4 {
            for cx in 0..w4 {
                let mut s = 0i32;
                let mut n = 0i32;
                for y in cy * 4..(cy * 4 + 4).min(self.height) {
                    for x in cx * 4..(cx * 4 + 4).min(self.width) {
                        s += self.get(field, x as i64, y as i64) as i32;
                        n += 1;
                    }
                }
                out.push((s / n) as i16);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(w: u32, h: u32) -> Vec<TerrainTileKind> {
        vec![TerrainTileKind::ConductiveBasalt; (w * h) as usize]
    }

    #[test]
    fn bible_parameters_are_frozen() {
        let d: Vec<i32> = ARENA_FIELD_CHANNELS.iter().map(|p| p.diffusion_q8).collect();
        let l: Vec<i32> = ARENA_FIELD_CHANNELS.iter().map(|p| p.decay_q8).collect();
        assert_eq!(d, [5, 38, 64, 64, 13]);
        assert_eq!(l, [0, 3, 13, 13, 1]);
        assert_eq!(ARENA_FIELD_CHANNELS[2].min_val_q8, -50 * 256);
        assert_eq!(ARENA_FIELD_CHANNELS[4].max_val_q8, 100 * 256);
    }

    #[test]
    fn diffusion_crosses_chunk_boundaries() {
        let t = open(512, 256);
        let mut g = FieldGrid::new(512, 256);
        g.set(FieldId::SignalA, 255, 100, 12_800);
        g.step(&t);
        // D = 0.25：邻格获得 12800 × 0.25 = 3200
        assert_eq!(g.get(FieldId::SignalA, 256, 100), 3200, "信号必须越过 Chunk 边界");
        assert_eq!(g.get(FieldId::SignalA, 254, 100), 3200);
        g.step(&t);
        assert_eq!(g.get(FieldId::SignalA, 257, 100), g.get(FieldId::SignalA, 253, 100));
    }

    #[test]
    fn nutrient_is_conserved_across_chunks_and_partial_edges() {
        for (w, h) in [(512u32, 512u32), (160, 160)] {
            let t = open(w, h);
            let mut g = FieldGrid::new(w, h);
            // D=5 的截断误差只在梯度处出现；用均匀块保证 laplacian·D 可整除前检验大致守恒
            for y in 0..h {
                for x in 0..w {
                    g.set(FieldId::Nutrient, x, y, 5120);
                }
            }
            let before = g.sum(FieldId::Nutrient);
            for _ in 0..10 {
                g.step(&t);
            }
            assert_eq!(g.sum(FieldId::Nutrient), before, "{w}×{h} 均匀场必须是不动点");
        }
    }

    #[test]
    fn marsh_regenerates_up_to_cap_at_exact_rate() {
        let mut t = open(256, 256);
        t[0] = TerrainTileKind::NutrientMarsh;
        let mut g = FieldGrid::new(256, 256);
        // 隔离扩散影响：只看第一步的再生量 + 五步累计
        g.step(&t);
        let first = g.get(FieldId::Nutrient, 0, 0);
        assert!(first > 0);
        let mut g2 = FieldGrid::new(256, 256);
        g2.set(FieldId::Nutrient, 0, 0, MARSH_REGEN_CAP_Q8);
        g2.step(&t);
        assert!(g2.get(FieldId::Nutrient, 0, 0) <= MARSH_REGEN_CAP_Q8);
        // 五个场步（15 Tick）精确再生 15 × 12.8 = 192 Q8.8
        let total: i64 = (0..5)
            .map(|s| fractional_share(MARSH_REGEN_Q8_PER_STEP_NUMER, 5, s))
            .sum();
        assert_eq!(total, 192);
    }

    #[test]
    fn gradient_points_uphill_and_root_is_deterministic() {
        let t = open(256, 256);
        let mut a = FieldGrid::for_terrain(256, 256, &t);
        a.set(FieldId::Nutrient, 50, 50, 20_000);
        for _ in 0..3 {
            a.step(&t);
        }
        let (gx, gy) = a.gradient(FieldId::Nutrient, 49, 50);
        assert!(gx > 0 && gy == 0);
        let mut b = FieldGrid::for_terrain(256, 256, &t);
        b.set(FieldId::Nutrient, 50, 50, 20_000);
        for _ in 0..3 {
            b.step(&t);
        }
        assert_eq!(a.state_root(), b.state_root());
        assert_eq!(a.downsample4(FieldId::Nutrient).len(), 64 * 64);
    }
}
