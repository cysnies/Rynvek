// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 提交清单（规范 03 `TickCommitManifest` / D189）。
//!
//! `state_root_hash` = BLAKE3(entity ‖ field ‖ neural ‖ economy ‖ rng ‖ parent 六个 32B 子根按此顺序拼接)。

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;

/// 五个领域子根（parent 单独传入）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommitSubRoots {
    pub entity_root_hash: [u8; 32],
    pub field_root_hash: [u8; 32],
    pub neural_root_hash: [u8; 32],
    pub economy_root_hash: [u8; 32],
    pub rng_root_hash: [u8; 32],
}

/// 固定布局的 Tick 提交清单（可直接写入 journal 帧载荷）
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitManifest {
    pub world_id: [u8; 16],
    pub tick: u64,
    pub world_revision: u64,
    pub parent_commit_hash: [u8; 32],
    pub entity_root_hash: [u8; 32],
    pub field_root_hash: [u8; 32],
    pub neural_root_hash: [u8; 32],
    pub economy_root_hash: [u8; 32],
    pub rng_root_hash: [u8; 32],
    /// 由以上 6 个 32B 子根拼接后计算的最终 32B BLAKE3 状态根
    pub state_root_hash: [u8; 32],
}

impl CommitManifest {
    /// 编码后的固定字节长度：16+8+8+7*32 = 256
    pub const ENCODED_LEN: usize = 16 + 8 + 8 + 32 * 7;

    /// 由各子根组装，并计算 `state_root_hash`
    pub fn new(
        world_id: [u8; 16],
        tick: u64,
        world_revision: u64,
        parent_commit_hash: [u8; 32],
        roots: CommitSubRoots,
    ) -> Self {
        let state_root_hash = Self::compute_state_root(
            roots.entity_root_hash,
            roots.field_root_hash,
            roots.neural_root_hash,
            roots.economy_root_hash,
            roots.rng_root_hash,
            parent_commit_hash,
        );
        Self {
            world_id,
            tick,
            world_revision,
            parent_commit_hash,
            entity_root_hash: roots.entity_root_hash,
            field_root_hash: roots.field_root_hash,
            neural_root_hash: roots.neural_root_hash,
            economy_root_hash: roots.economy_root_hash,
            rng_root_hash: roots.rng_root_hash,
            state_root_hash,
        }
    }

    /// 规范：6 个 32B 子根按 entity/field/neural/economy/rng/parent 顺序拼接后 BLAKE3
    pub fn compute_state_root(
        entity: [u8; 32],
        field: [u8; 32],
        neural: [u8; 32],
        economy: [u8; 32],
        rng: [u8; 32],
        parent: [u8; 32],
    ) -> [u8; 32] {
        Blake3Hash::of_concatenated(&[&entity, &field, &neural, &economy, &rng, &parent]).0
    }

    pub fn encode(&self) -> [u8; Self::ENCODED_LEN] {
        let mut out = [0u8; Self::ENCODED_LEN];
        out[0..16].copy_from_slice(&self.world_id);
        out[16..24].copy_from_slice(&self.tick.to_le_bytes());
        out[24..32].copy_from_slice(&self.world_revision.to_le_bytes());
        out[32..64].copy_from_slice(&self.parent_commit_hash);
        out[64..96].copy_from_slice(&self.entity_root_hash);
        out[96..128].copy_from_slice(&self.field_root_hash);
        out[128..160].copy_from_slice(&self.neural_root_hash);
        out[160..192].copy_from_slice(&self.economy_root_hash);
        out[192..224].copy_from_slice(&self.rng_root_hash);
        out[224..256].copy_from_slice(&self.state_root_hash);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        if bytes.len() != Self::ENCODED_LEN {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let mut world_id = [0u8; 16];
        world_id.copy_from_slice(&bytes[0..16]);
        let parent_commit_hash: [u8; 32] = bytes[32..64].try_into().unwrap();
        let entity_root_hash: [u8; 32] = bytes[64..96].try_into().unwrap();
        let field_root_hash: [u8; 32] = bytes[96..128].try_into().unwrap();
        let neural_root_hash: [u8; 32] = bytes[128..160].try_into().unwrap();
        let economy_root_hash: [u8; 32] = bytes[160..192].try_into().unwrap();
        let rng_root_hash: [u8; 32] = bytes[192..224].try_into().unwrap();
        let state_root_hash: [u8; 32] = bytes[224..256].try_into().unwrap();
        let expected = Self::compute_state_root(
            entity_root_hash,
            field_root_hash,
            neural_root_hash,
            economy_root_hash,
            rng_root_hash,
            parent_commit_hash,
        );
        if expected != state_root_hash {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        Ok(Self {
            world_id,
            tick: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            world_revision: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
            parent_commit_hash,
            entity_root_hash,
            field_root_hash,
            neural_root_hash,
            economy_root_hash,
            rng_root_hash,
            state_root_hash: expected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_root_is_deterministic() {
        let m = CommitManifest::new(
            [1; 16],
            9,
            3,
            [2; 32],
            CommitSubRoots {
                entity_root_hash: [3; 32],
                field_root_hash: [4; 32],
                neural_root_hash: [5; 32],
                economy_root_hash: [6; 32],
                rng_root_hash: [7; 32],
            },
        );
        let again = CommitManifest::decode(&m.encode()).unwrap();
        assert_eq!(m, again);
    }
}
