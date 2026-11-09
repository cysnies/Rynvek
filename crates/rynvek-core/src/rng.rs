// crates/rynvek-core/src/rng.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 无状态计数器式确定性随机数 `Philox4x32-10`（规范 02 §2.2、ADR-008）。
//!
//! 与 WGSL 端 `crates/rynvek-wgpu/shaders/common/philox_rng.wgsl` 逐位对齐，
//! 并与 D.E. Shaw Random123 官方 Known-Answer 向量严格一致。
//! 由于生成器无状态，任意 `(世界种子, Tick, 实体, 随机流)` 组合都可独立、并行、可重放地求值。

use crate::fixed::Q32_32;

/// Philox 轮函数乘数 M0（作用于计数器第 0 字）
pub const PHILOX_M0: u32 = 0xD251_1F53;
/// Philox 轮函数乘数 M1（作用于计数器第 2 字）
pub const PHILOX_M1: u32 = 0xCD9E_8D57;
/// Weyl 序列密钥增量 W0（黄金分割比）
pub const PHILOX_W0: u32 = 0x9E37_79B9;
/// Weyl 序列密钥增量 W1（sqrt(3) - 1）
pub const PHILOX_W1: u32 = 0xBB67_AE85;
/// 标准轮数
pub const PHILOX_ROUNDS: u32 = 10;

/// 32 位无符号乘法，返回 `(lo, hi)` 两个 32 位半字（与 WGSL `mul_hi_lo_u32` 的 `.x / .y` 对应）
#[inline(always)]
pub const fn mul_hi_lo_u32(a: u32, b: u32) -> (u32, u32) {
    let product = (a as u64) * (b as u64);
    (product as u32, (product >> 32) as u32)
}

/// Philox4x32-10 分组函数：对 128 位计数器与 64 位密钥执行 10 轮置换
#[inline]
pub const fn philox4x32_10(ctr_in: [u32; 4], key_in: [u32; 2]) -> [u32; 4] {
    let mut ctr = ctr_in;
    let mut key = key_in;
    let mut round = 0;
    while round < PHILOX_ROUNDS {
        let (lo0, hi0) = mul_hi_lo_u32(PHILOX_M0, ctr[0]);
        let (lo1, hi1) = mul_hi_lo_u32(PHILOX_M1, ctr[2]);
        ctr = [hi1 ^ ctr[1] ^ key[0], lo1, hi0 ^ ctr[3] ^ key[1], lo0];
        key[0] = key[0].wrapping_add(PHILOX_W0);
        key[1] = key[1].wrapping_add(PHILOX_W1);
        round += 1;
    }
    ctr
}

/// 随机数的完整寻址键：唯一确定一次 4×u32 随机输出。
///
/// 映射规则（Rust / TypeScript / WGSL 三端必须一致）：
/// - 密钥 `key = [lo32(world_seed ^ tick), hi32(world_seed ^ tick)]`；
/// - 计数器 `ctr = [sub_counter, stream_and_phase, lo32(entity_serial), hi32(entity_serial)]`。
///
/// 同一世界内不同 Tick 得到不同密钥，同一 Tick 内由计数器区分实体、随机流、阶段与子序号，
/// 因此世界内任意两个不同的键都不会复用同一 Philox 输出。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RandomKey {
    /// 世界随机种子
    pub world_seed: u64,
    /// 当前权威 Tick
    pub tick: u64,
    /// 实体单调序号（`EntityId` 低 64 位），或调用方自定义的空间编码
    pub entity_serial: u64,
    /// 高 16 位为随机流 ID，低 16 位为 Tick 阶段 ID（见 [`RandomKey::pack_stream_and_phase`]）
    pub stream_and_phase: u32,
    /// 同一键下的子序号，用于连续抽取多组随机数
    pub sub_counter: u32,
}

impl RandomKey {
    /// 将随机流 ID 与阶段 ID 打包为 `stream_and_phase`
    #[inline]
    pub const fn pack_stream_and_phase(stream_id: u16, phase_id: u16) -> u32 {
        ((stream_id as u32) << 16) | (phase_id as u32)
    }

    /// 随机流 ID（`stream_and_phase` 高 16 位）
    #[inline]
    pub const fn stream_id(&self) -> u16 {
        (self.stream_and_phase >> 16) as u16
    }

    /// 阶段 ID（`stream_and_phase` 低 16 位）
    #[inline]
    pub const fn phase_id(&self) -> u16 {
        self.stream_and_phase as u16
    }

    /// 返回仅子序号不同的新键
    #[inline]
    pub const fn with_sub_counter(self, sub_counter: u32) -> Self {
        Self { sub_counter, ..self }
    }

    /// Philox 计数器字
    #[inline]
    pub const fn counter(&self) -> [u32; 4] {
        [
            self.sub_counter,
            self.stream_and_phase,
            self.entity_serial as u32,
            (self.entity_serial >> 32) as u32,
        ]
    }

    /// Philox 密钥字
    #[inline]
    pub const fn key(&self) -> [u32; 2] {
        let mixed = self.world_seed ^ self.tick;
        [mixed as u32, (mixed >> 32) as u32]
    }

    /// 生成 4 个独立均匀分布的 32 位随机数
    #[inline]
    pub const fn generate_u32x4(&self) -> [u32; 4] {
        philox4x32_10(self.counter(), self.key())
    }
}

/// 将 32 位随机数映射为 `[0, 1)` 区间的 Q32.32 定点数（纯整数映射，无浮点）
#[inline]
pub const fn u32_to_unit_q32(value: u32) -> Q32_32 {
    Q32_32(value as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random123_kat_zero() {
        assert_eq!(
            philox4x32_10([0; 4], [0; 2]),
            [0x6627_e8d5, 0xe169_c58d, 0xbc57_ac4c, 0x9b00_dbd8]
        );
    }

    #[test]
    fn random123_kat_all_ones() {
        assert_eq!(
            philox4x32_10([u32::MAX; 4], [u32::MAX; 2]),
            [0x408f_276d, 0x41c8_3b0e, 0xa20b_c7c6, 0x6d54_51fd]
        );
    }

    #[test]
    fn random123_kat_pi_digits() {
        let ctr = [0x243f_6a88, 0x85a3_08d3, 0x1319_8a2e, 0x0370_7344];
        let key = [0xa409_3822, 0x299f_31d0];
        assert_eq!(
            philox4x32_10(ctr, key),
            [0xd16c_fe09, 0x94fd_cceb, 0x5001_e420, 0x2412_6ea1]
        );
    }

    #[test]
    fn mul_hi_lo_splits_product() {
        assert_eq!(mul_hi_lo_u32(u32::MAX, u32::MAX), (1, 0xFFFF_FFFE));
        assert_eq!(mul_hi_lo_u32(PHILOX_M0, 0), (0, 0));
    }

    #[test]
    fn random_key_maps_fields_to_counter_and_key() {
        let rk = RandomKey {
            world_seed: 0x0123_4567_89AB_CDEF,
            tick: 0x10,
            entity_serial: 0xAAAA_BBBB_CCCC_DDDD,
            stream_and_phase: RandomKey::pack_stream_and_phase(1, 7),
            sub_counter: 3,
        };
        assert_eq!(rk.counter(), [3, 0x0001_0007, 0xCCCC_DDDD, 0xAAAA_BBBB]);
        assert_eq!(rk.key(), [0x89AB_CDFF, 0x0123_4567]);
        assert_eq!(rk.stream_id(), 1);
        assert_eq!(rk.phase_id(), 7);
        assert_eq!(rk.generate_u32x4(), philox4x32_10(rk.counter(), rk.key()));
    }

    #[test]
    fn distinct_keys_produce_distinct_outputs() {
        let base = RandomKey {
            world_seed: 42,
            ..RandomKey::default()
        };
        let variants = [
            base,
            RandomKey { tick: 1, ..base },
            RandomKey {
                entity_serial: 1,
                ..base
            },
            RandomKey {
                stream_and_phase: 1,
                ..base
            },
            base.with_sub_counter(1),
        ];
        for (i, a) in variants.iter().enumerate() {
            for b in &variants[i + 1..] {
                assert_ne!(a.generate_u32x4(), b.generate_u32x4());
            }
        }
    }

    #[test]
    fn unit_mapping_stays_below_one() {
        assert_eq!(u32_to_unit_q32(0), Q32_32::ZERO);
        assert!(u32_to_unit_q32(u32::MAX) < Q32_32::ONE);
        assert_eq!(u32_to_unit_q32(1 << 31), Q32_32::HALF);
    }
}
