// crates/rynvek-field/src/solver.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `260×260` 带 Halo 二维场分块与五点拉普拉斯定点扩散算子（规范 06 §6.1）。

use rynvek_core::error::RynvekErrorCode;

pub const TILE_DIM: usize = 256;
pub const HALO: usize = 2;
pub const STRIDE: usize = TILE_DIM + 2 * HALO; // 260

/// 二维显式格式的 CFL 上界：`D_c <= 0.25` → Q8.8 原始值 `<= 64`
pub const CFL_MAX_DIFFUSION_Q8: i32 = 64;

/// 定点 Q8.8 五点拉普拉斯反应-扩散算子（跨 CPU/GPU 100% 位级确定性）
pub fn solve_reaction_diffusion_tile_q8(
    src_padded: &[i16; STRIDE * STRIDE],
    dst_padded: &mut [i16; STRIDE * STRIDE],
    sources_q8: &[i16; TILE_DIM * TILE_DIM],
    diffusion_q8: i32, // D_c in Q8.8 (必须 <= 64, 即 D <= 0.25 满足二维显式 CFL 条件)
    decay_q8: i32,     // lambda_c in Q8.8
    min_val_q8: i16,
    max_val_q8: i16,
) {
    assert!(
        diffusion_q8 <= CFL_MAX_DIFFUSION_Q8,
        "CFL Stability Violation: diffusion_q8 must be <= 64 (0.25)"
    );
    let retain_q8 = 256 - decay_q8;

    for y in 0..TILE_DIM {
        let py = y + HALO;
        for x in 0..TILE_DIM {
            let px = x + HALO;
            let idx = py * STRIDE + px;

            let center = src_padded[idx] as i32;
            let north = src_padded[(py - 1) * STRIDE + px] as i32;
            let south = src_padded[(py + 1) * STRIDE + px] as i32;
            let west = src_padded[py * STRIDE + (px - 1)] as i32;
            let east = src_padded[py * STRIDE + (px + 1)] as i32;

            let laplacian = north + south + west + east - 4 * center;
            let decayed = (center * retain_q8) >> 8;
            let diffused = (laplacian * diffusion_q8) >> 8;
            let src_term = sources_q8[y * TILE_DIM + x] as i32;

            let next_val = (decayed + diffused + src_term).clamp(min_val_q8 as i32, max_val_q8 as i32);
            dst_padded[idx] = next_val as i16;
        }
    }
}

/// 可返回错误码的包装（供上层在关闭 panic 的路径使用）
pub fn try_solve_reaction_diffusion_tile_q8(
    src_padded: &[i16; STRIDE * STRIDE],
    dst_padded: &mut [i16; STRIDE * STRIDE],
    sources_q8: &[i16; TILE_DIM * TILE_DIM],
    diffusion_q8: i32,
    decay_q8: i32,
    min_val_q8: i16,
    max_val_q8: i16,
) -> Result<(), RynvekErrorCode> {
    if diffusion_q8 > CFL_MAX_DIFFUSION_Q8 {
        return Err(RynvekErrorCode::FieldCflStabilityViolation);
    }
    solve_reaction_diffusion_tile_q8(
        src_padded,
        dst_padded,
        sources_q8,
        diffusion_q8,
        decay_q8,
        min_val_q8,
        max_val_q8,
    );
    Ok(())
}

/// 统计内区（不含 Halo）的有符号总和，用于守恒检验
pub fn sum_interior_i64(padded: &[i16; STRIDE * STRIDE]) -> i64 {
    let mut sum = 0i64;
    for y in 0..TILE_DIM {
        let py = y + HALO;
        for x in 0..TILE_DIM {
            let px = x + HALO;
            sum += i64::from(padded[py * STRIDE + px]);
        }
    }
    sum
}

/// 把内区拷到 Halo（简单 Neumann 零梯度拷边，便于单 Tile 封闭测试）
pub fn fill_neumann_halo(padded: &mut [i16; STRIDE * STRIDE]) {
    // 左右
    for y in HALO..(HALO + TILE_DIM) {
        for h in 0..HALO {
            padded[y * STRIDE + h] = padded[y * STRIDE + HALO];
            padded[y * STRIDE + (STRIDE - 1 - h)] = padded[y * STRIDE + (STRIDE - 1 - HALO)];
        }
    }
    // 上下（含角）
    for h in 0..HALO {
        let src_top = HALO * STRIDE;
        let dst_top = h * STRIDE;
        let src_bot = (HALO + TILE_DIM - 1) * STRIDE;
        let dst_bot = (STRIDE - 1 - h) * STRIDE;
        for x in 0..STRIDE {
            padded[dst_top + x] = padded[src_top + x];
            padded[dst_bot + x] = padded[src_bot + x];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_field_is_fixed_point_without_decay_or_source() {
        let mut src = [0i16; STRIDE * STRIDE];
        let mut dst = [0i16; STRIDE * STRIDE];
        let sources = [0i16; TILE_DIM * TILE_DIM];
        for y in HALO..(HALO + TILE_DIM) {
            for x in HALO..(HALO + TILE_DIM) {
                src[y * STRIDE + x] = 1000;
            }
        }
        fill_neumann_halo(&mut src);
        solve_reaction_diffusion_tile_q8(&src, &mut dst, &sources, 64, 0, i16::MIN, i16::MAX);
        assert_eq!(sum_interior_i64(&dst), sum_interior_i64(&src));
        assert_eq!(dst[(HALO) * STRIDE + HALO], 1000);
    }
}
