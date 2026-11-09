// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! CFL 自动子步（规范 06 / GAP 验收「CFL 自动子步」）。
//!
//! 二维显式五点格式要求单步 `diffusion_q8 <= 64`。若调用方请求更大的扩散率，
//! 则拆成 `n = ceil(D / 64)` 个子步，每子步使用 `D_sub = floor(D / n)`，并在子步间
//! 交换缓冲、刷新 Neumann Halo。衰减与源项按「每权威 Tick 一次」计入——衰减均分到
//! 各子步（整数除法），源项仅加在最后一子步，避免重复注入。

use rynvek_core::error::RynvekErrorCode;

use crate::solver::{CFL_MAX_DIFFUSION_Q8, STRIDE, TILE_DIM, fill_neumann_halo, solve_reaction_diffusion_tile_q8};

/// 计算为满足 CFL 所需的子步数
pub fn cfl_substeps_for_diffusion(diffusion_q8: i32) -> u32 {
    if diffusion_q8 <= 0 {
        return 1;
    }
    let d = diffusion_q8 as u32;
    let max = CFL_MAX_DIFFUSION_Q8 as u32;
    d.div_ceil(max).max(1)
}

/// 带 CFL 自动子步的反应-扩散求解；`buffers` 为双缓冲，返回结束后「当前」缓冲下标（0 或 1）
pub fn solve_reaction_diffusion_tile_q8_cfl(
    buffers: &mut [[i16; STRIDE * STRIDE]; 2],
    current: usize,
    sources_q8: &[i16; TILE_DIM * TILE_DIM],
    diffusion_q8: i32,
    decay_q8: i32,
    min_val_q8: i16,
    max_val_q8: i16,
) -> Result<(usize, u32), RynvekErrorCode> {
    if current > 1 {
        return Err(RynvekErrorCode::DimensionMismatch);
    }
    if diffusion_q8 < 0 || !(0..=256).contains(&decay_q8) {
        return Err(RynvekErrorCode::FieldCflStabilityViolation);
    }

    let steps = cfl_substeps_for_diffusion(diffusion_q8);
    let d_sub = diffusion_q8 / steps as i32;
    // 衰减均分；余数加在最后一子步，保证 Σ decay_sub == decay_q8
    let decay_base = decay_q8 / steps as i32;
    let decay_rem = decay_q8 - decay_base * steps as i32;

    let mut cur = current;
    for step in 0..steps {
        let src_idx = cur;
        let dst_idx = 1 - cur;
        fill_neumann_halo(&mut buffers[src_idx]);
        let decay_sub = if step + 1 == steps {
            decay_base + decay_rem
        } else {
            decay_base
        };
        // 源项只在最后一子步注入，防止多子步重复加源破坏守恒语义
        let zero_sources = [0i16; TILE_DIM * TILE_DIM];
        let sources = if step + 1 == steps { sources_q8 } else { &zero_sources };
        // 用 split_at_mut 同时借出两个缓冲，避免 E0502
        let (first, second) = buffers.split_at_mut(1);
        let (src, dst) = if src_idx == 0 {
            (&first[0], &mut second[0])
        } else {
            (&second[0], &mut first[0])
        };
        // 拆分后 d_sub 必然 <= 64
        solve_reaction_diffusion_tile_q8(src, dst, sources, d_sub, decay_sub, min_val_q8, max_val_q8);
        cur = dst_idx;
    }
    Ok((cur, steps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::{HALO, sum_interior_i64};

    #[test]
    fn substep_count_matches_ceil_div() {
        assert_eq!(cfl_substeps_for_diffusion(64), 1);
        assert_eq!(cfl_substeps_for_diffusion(65), 2);
        assert_eq!(cfl_substeps_for_diffusion(128), 2);
        assert_eq!(cfl_substeps_for_diffusion(129), 3);
    }

    #[test]
    fn oversized_diffusion_runs_via_substeps() {
        let mut buffers = [[0i16; STRIDE * STRIDE]; 2];
        // 中心脉冲
        buffers[0][(HALO + 128) * STRIDE + (HALO + 128)] = 10_000;
        let sources = [0i16; TILE_DIM * TILE_DIM];
        let before = sum_interior_i64(&buffers[0]);
        let (cur, steps) = solve_reaction_diffusion_tile_q8_cfl(
            &mut buffers,
            0,
            &sources,
            128, // 2× CFL 上限 → 2 子步
            0,
            i16::MIN,
            i16::MAX,
        )
        .unwrap();
        assert_eq!(steps, 2);
        // 无衰减、无源、无钳制时总量守恒（Neumann 边界）
        assert_eq!(sum_interior_i64(&buffers[cur]), before);
    }
}
