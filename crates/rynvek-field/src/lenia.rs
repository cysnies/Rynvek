// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 确定性 Q8.8 Lenia 连续元胞自动机核（规范 06 §6.1 / 审计报告 §7.3）。
//!
//! 经典 Lenia 更新方程：
//! $$A^{t+\Delta t}(x) = \text{clamp}\bigl(A^t(x) + \Delta t \cdot G(K * A^t(x)),\, 0,\, 1\bigr)$$
//!
//! 本模块提供纯整数 Q8.8 定点离散化实现：
//! - 环状卷积核 $K(r)$ 由切比雪夫/欧氏整数距离环 `r_min_tiles..=r_max_tiles` 预计算归一化权重（总和为 256 即 `1.0` Q8.8）；
//! - 生长映射 $G(u) \in [-256, +256]$ 以围绕目标均值 $\mu$（Q8.8）、半宽 $\sigma$（Q8.8）的确定性二次抛物钟形曲线
//!   $G(u) = 256 - \lfloor 512 \cdot (u - \mu)^2 / \sigma^2 \rfloor$（钳制到 $[-256, +256]$）计算，全程零浮点。

use crate::solver::{HALO, STRIDE, TILE_DIM};

/// Lenia 核与生长函数参数（Q8.8 定点）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeniaParams {
    /// 环状邻域内半径（Tiles，含）
    pub inner_radius: u8,
    /// 环状邻域外半径（Tiles，含，`1 <= inner_radius <= outer_radius <= 5`）
    pub outer_radius: u8,
    /// 生长最优势能中心 $\mu$（Q8.8，典型值 `38` ≈ 0.15）
    pub mu_q8: i16,
    /// 生长势能容差半宽 $\sigma$（Q8.8，典型值 `13` ≈ 0.05，须 `> 0`）
    pub sigma_q8: i16,
    /// 时间步缩放系数 $\Delta t$（Q8.8，典型值 `26` ≈ 0.10）
    pub dt_q8: i16,
    /// 场值下界（Q8.8，通常为 `0`）
    pub min_val_q8: i16,
    /// 场值上界（Q8.8，通常为 `256` 或 `25_600`）
    pub max_val_q8: i16,
}

impl Default for LeniaParams {
    fn default() -> Self {
        Self {
            inner_radius: 1,
            outer_radius: 2,
            mu_q8: 38,
            sigma_q8: 13,
            dt_q8: 26,
            min_val_q8: 0,
            max_val_q8: 256,
        }
    }
}

/// 预计算的确定性整数环状卷积核
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeniaKernel {
    pub params: LeniaParams,
    /// `(dx, dy, weight_q8)` 列表，所有 `weight_q8` 之和恒等于 `256`
    offsets: Vec<(i8, i8, i16)>,
}

impl LeniaKernel {
    /// 根据 [`LeniaParams`] 构造归一化整数环状核
    pub fn new(params: LeniaParams) -> Self {
        let r_in = params.inner_radius.max(1) as i32;
        let r_out = params.outer_radius.max(params.inner_radius).min(5) as i32;
        let r_in2 = r_in * r_in;
        let r_out2 = r_out * r_out;

        let mut ring = Vec::new();
        for dy in -r_out..=r_out {
            for dx in -r_out..=r_out {
                let d2 = dx * dx + dy * dy;
                if d2 >= r_in2 && d2 <= r_out2 {
                    ring.push((dx as i8, dy as i8));
                }
            }
        }
        let n = ring.len().max(1) as i16;
        let base_w = 256 / n;
        let rem = (256 % n) as usize;
        let offsets = ring
            .into_iter()
            .enumerate()
            .map(|(i, (dx, dy))| (dx, dy, base_w + if i < rem { 1 } else { 0 }))
            .collect();
        Self { params, offsets }
    }

    /// 核内采样点数量
    pub fn tap_count(&self) -> usize {
        self.offsets.len()
    }

    /// 确定性整数生长函数 $G(u) \in [-256, +256]$
    #[inline]
    pub fn growth_q8(&self, potential_q8: i32) -> i32 {
        let diff = (potential_q8 - self.params.mu_q8 as i32) as i64;
        let sigma = (self.params.sigma_q8.max(1) as i64).pow(2);
        let penalty = (512 * diff * diff) / sigma;
        (256 - penalty).clamp(-256, 256) as i32
    }

    /// 对 `260×260` Halo 分块求解一步 Lenia 演化（要求 `outer_radius <= HALO = 2` 时直接零拷贝读取 Halo，更大半径自动钳制到界内）
    pub fn step_tile_q8(&self, src: &[i16], dst: &mut [i16]) {
        assert_eq!(src.len(), STRIDE * STRIDE);
        assert_eq!(dst.len(), STRIDE * STRIDE);
        let p = self.params;
        let max_coord = (TILE_DIM + 2 * HALO - 1) as isize;

        for y in HALO..(TILE_DIM + HALO) {
            let row_off = y * STRIDE;
            for x in HALO..(TILE_DIM + HALO) {
                let idx = row_off + x;
                let mut potential_acc: i32 = 0;
                for &(dx, dy, w) in &self.offsets {
                    let nx = (x as isize + dx as isize).clamp(0, max_coord) as usize;
                    let ny = (y as isize + dy as isize).clamp(0, max_coord) as usize;
                    potential_acc += (src[ny * STRIDE + nx] as i32) * (w as i32);
                }
                let u_q8 = potential_acc >> 8;
                let g_q8 = self.growth_q8(u_q8);
                let delta = (g_q8 * p.dt_q8 as i32) >> 8;
                let next = (src[idx] as i32 + delta).clamp(p.min_val_q8 as i32, p.max_val_q8 as i32);
                dst[idx] = next as i16;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenia_kernel_weights_sum_to_one_q8_and_growth_peaks_at_mu() {
        let kernel = LeniaKernel::new(LeniaParams::default());
        let weight_sum: i16 = kernel.offsets.iter().map(|(_, _, w)| *w).sum();
        assert_eq!(weight_sum, 256);
        assert_eq!(kernel.growth_q8(38), 256);
        assert!(kernel.growth_q8(0) < 0);
        assert_eq!(kernel.growth_q8(200), -256);
    }

    #[test]
    fn lenia_tile_step_is_deterministic_and_bounded() {
        let kernel = LeniaKernel::new(LeniaParams::default());
        let mut src = vec![0i16; STRIDE * STRIDE];
        let mut dst = vec![0i16; STRIDE * STRIDE];
        // 在中心放置一团势能恰好接近 mu 的种子
        for dy in -2..=2isize {
            for dx in -2..=2isize {
                let x = (128 + dx) as usize;
                let y = (128 + dy) as usize;
                src[y * STRIDE + x] = 64;
            }
        }
        kernel.step_tile_q8(&src, &mut dst);
        let mut dst2 = vec![0i16; STRIDE * STRIDE];
        kernel.step_tile_q8(&src, &mut dst2);
        assert_eq!(dst, dst2);
        assert!(dst.iter().all(|&v| (0..=256).contains(&v)));
    }
}
