// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 单 Chunk 五通道场世界：推进、守恒校验与状态根。

use blake3::Hasher;
use bytemuck;
use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;

use crate::cfl::solve_reaction_diffusion_tile_q8_cfl;
use crate::channels::{FieldChannelId, FieldChannelParams, FieldTileQ8, STANDARD_FIELD_CHANNELS};
use crate::solver::{HALO, STRIDE, TILE_DIM, sum_interior_i64};

/// 五通道场世界（一个逻辑 Chunk / 一个 Tile）
#[derive(Clone, Debug)]
pub struct FieldWorld {
    channels: [FieldTileQ8; 5],
}

impl Default for FieldWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl FieldWorld {
    pub fn new() -> Self {
        Self {
            channels: STANDARD_FIELD_CHANNELS.map(FieldTileQ8::new),
        }
    }

    pub fn with_params(params: [FieldChannelParams; 5]) -> Self {
        Self {
            channels: params.map(FieldTileQ8::new),
        }
    }

    pub fn channel(&self, id: FieldChannelId) -> &FieldTileQ8 {
        &self.channels[id.index()]
    }

    pub fn channel_mut(&mut self, id: FieldChannelId) -> &mut FieldTileQ8 {
        &mut self.channels[id.index()]
    }

    /// 推进全部通道一个权威 Tick；返回各通道实际子步数
    pub fn step_all(&mut self) -> Result<[u32; 5], RynvekErrorCode> {
        let mut steps = [0u32; 5];
        for (i, tile) in self.channels.iter_mut().enumerate() {
            let (cur, n) = solve_reaction_diffusion_tile_q8_cfl(
                &mut tile.buffers,
                tile.current,
                &tile.sources,
                tile.params.diffusion_q8,
                tile.params.decay_q8,
                tile.params.min_val_q8,
                tile.params.max_val_q8,
            )?;
            tile.current = cur;
            steps[i] = n;
        }
        Ok(steps)
    }

    /// 五通道内区有符号总和
    pub fn interior_sums(&self) -> [i64; 5] {
        std::array::from_fn(|i| sum_interior_i64(self.channels[i].current_buf()))
    }

    /// 状态根：按通道顺序哈希内区 Q8.8 样本
    ///
    /// 按行整段 `update`（内区在每行中连续），避免逐格 `to_le_bytes` 调用开销。
    pub fn state_root(&self) -> Blake3Hash {
        let mut hasher = Hasher::new();
        for tile in &self.channels {
            let buf = tile.current_buf();
            for y in 0..TILE_DIM {
                let py = y + HALO;
                let start = py * STRIDE + HALO;
                let row = &buf[start..start + TILE_DIM];
                // i16 小端与平台一致（引擎锁定 little-endian 目标）
                let bytes = bytemuck::cast_slice::<i16, u8>(row);
                hasher.update(bytes);
            }
        }
        Blake3Hash(*hasher.finalize().as_bytes())
    }

    /// 通用 World 的完整权威场分块：显式小端，包含参数、当前缓冲和未来源项。
    /// 非当前工作缓冲不参与未来步，恢复时由求解器重新写入。
    pub fn authority_bytes(&self) -> Vec<u8> {
        let mut out = b"RYFIELD1".to_vec();
        for tile in &self.channels {
            out.extend_from_slice(&tile.params.diffusion_q8.to_le_bytes());
            out.extend_from_slice(&tile.params.decay_q8.to_le_bytes());
            out.extend_from_slice(&tile.params.min_val_q8.to_le_bytes());
            out.extend_from_slice(&tile.params.max_val_q8.to_le_bytes());
            for v in tile.current_buf().iter().chain(tile.sources.iter()) {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    pub fn from_authority_bytes(bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        let expected = 8 + 5 * (12 + 2 * (STRIDE * STRIDE + TILE_DIM * TILE_DIM));
        if bytes.len() != expected || &bytes[..8] != b"RYFIELD1" {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let mut world = Self::new();
        let mut pos = 8;
        for tile in &mut world.channels {
            tile.params.diffusion_q8 = i32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
            pos += 4;
            tile.params.decay_q8 = i32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
            pos += 4;
            tile.params.min_val_q8 = i16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap());
            pos += 2;
            tile.params.max_val_q8 = i16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap());
            pos += 2;
            if !(0..=256).contains(&tile.params.diffusion_q8)
                || !(0..=256).contains(&tile.params.decay_q8)
                || tile.params.min_val_q8 > tile.params.max_val_q8
            {
                return Err(RynvekErrorCode::DimensionMismatch);
            }
            for v in tile.buffers[0].iter_mut().chain(tile.sources.iter_mut()) {
                *v = i16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap());
                pos += 2;
            }
            tile.current = 0;
        }
        Ok(world)
    }

    pub fn interior(&self, id: FieldChannelId, x: usize, y: usize) -> Result<i16, RynvekErrorCode> {
        if x >= TILE_DIM || y >= TILE_DIM {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        let tile = self.channel(id);
        Ok(tile.current_buf()[(y + HALO) * STRIDE + (x + HALO)])
    }

    /// 只扣除当前格实际正值，永不低于 0，也不先扣后钳。
    pub fn withdraw_positive(
        &mut self,
        id: FieldChannelId,
        x: usize,
        y: usize,
        want: u32,
    ) -> Result<u32, RynvekErrorCode> {
        let cur = self.interior(id, x, y)?;
        if cur <= 0 || want == 0 {
            return Ok(0);
        }
        let take = (cur as u32).min(want);
        let tile = self.channel_mut(id);
        tile.current_buf_mut()[(y + HALO) * STRIDE + (x + HALO)] = cur - take as i16;
        Ok(take)
    }

    /// 按通道上限存入，超出部分由调用方另行处理，不抬高 max_val。
    pub fn deposit_room(&mut self, id: FieldChannelId, x: usize, y: usize, add: u32) -> Result<u32, RynvekErrorCode> {
        let cur = self.interior(id, x, y)?;
        let max = self.channel(id).params.max_val_q8;
        let room = i32::from(max).saturating_sub(i32::from(cur)).max(0) as u32;
        let add = add.min(room);
        let tile = self.channel_mut(id);
        tile.current_buf_mut()[(y + HALO) * STRIDE + (x + HALO)] = cur + add as i16;
        Ok(add)
    }

    /// 在指定通道内区写入一个脉冲（测试辅助）
    pub fn set_interior(&mut self, id: FieldChannelId, x: usize, y: usize, value: i16) {
        assert!(x < TILE_DIM && y < TILE_DIM);
        let tile = self.channel_mut(id);
        let px = x + HALO;
        let py = y + HALO;
        tile.current_buf_mut()[py * STRIDE + px] = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::FieldChannelId;

    /// 五通道反应扩散场守恒：decay=0、sources=0、无钳制时，Neumann 边界下
    /// 全局通量对消。为抵消 Q8.8 算术右移截断的非对称误差，本验收用例选用
    /// 使 `laplacian * D` 对典型脉冲可被 256 整除的扩散率（D=64 / D=128）。
    #[test]
    fn five_channel_reaction_diffusion_conserves_mass() {
        // 通道 0..2 用 CFL 边界 D=64（1 子步）；3..4 用 D=128（2 子步）覆盖自动子步路径
        let mut params = STANDARD_FIELD_CHANNELS;
        for (i, p) in params.iter_mut().enumerate() {
            p.decay_q8 = 0;
            p.min_val_q8 = i16::MIN;
            p.max_val_q8 = i16::MAX;
            p.diffusion_q8 = if i < 3 { 64 } else { 128 };
        }
        let mut world = FieldWorld::with_params(params);
        // 使用 256 的倍数作为脉冲幅值，降低 >>8 截断残差
        for (i, id) in FieldChannelId::ALL.into_iter().enumerate() {
            world.set_interior(id, 64 + i, 64 + i, 256 * (20 + i as i16));
        }
        let before = world.interior_sums();
        let steps = world.step_all().unwrap();
        assert_eq!(&steps[..3], &[1, 1, 1]);
        assert_eq!(&steps[3..], &[2, 2]);
        assert_eq!(world.interior_sums(), before, "五通道质量应守恒");
    }

    #[test]
    fn state_root_is_stable_for_identical_fields() {
        let mut a = FieldWorld::new();
        let mut b = FieldWorld::new();
        a.set_interior(FieldChannelId::Nutrient, 1, 1, 42);
        b.set_interior(FieldChannelId::Nutrient, 1, 1, 42);
        assert_eq!(a.state_root(), b.state_root());
        a.step_all().unwrap();
        b.step_all().unwrap();
        assert_eq!(a.state_root(), b.state_root());
    }
}
