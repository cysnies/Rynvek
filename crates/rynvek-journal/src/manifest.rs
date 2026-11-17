// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 提交清单（规范 03 `TickCommitManifest` / D189）。
//!
//! 保留 256B 旧清单；规范世界清单使用显式 `RYC2` 前缀和完整十域根，拒绝静默改变旧格式。

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

/// 规范世界额外的四个领域根；其余五域与前驱根沿用清单字段。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalDomains {
    pub config: [u8; 32],
    pub evolution: [u8; 32],
    pub visibility: [u8; 32],
    pub scheduled_events: [u8; 32],
}

/// 显式编码的 Tick 提交清单；内存布局不是文件 ABI。
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
    pub canonical_domains: Option<CanonicalDomains>,
}

impl CommitManifest {
    /// 编码后的固定字节长度：16+8+8+7*32 = 256
    pub const ENCODED_LEN: usize = 16 + 8 + 8 + 32 * 7;

    /// 带显式版本标识的规范清单长度：4B 前缀 + 256B 基础字段 + 128B 扩展领域。
    pub const CANONICAL_ENCODED_LEN: usize = 4 + Self::ENCODED_LEN + 4 * 32;

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
            canonical_domains: None,
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

    /// 与规范世界共享唯一的十域聚合算法，保持现有 Arena 状态根不变。
    pub fn canonical_state_root(
        tick: u64,
        parent: [u8; 32],
        roots: CommitSubRoots,
        domains: CanonicalDomains,
    ) -> [u8; 32] {
        Blake3Hash::of_concatenated(&[
            b"RYNVEK_CANONICAL_WORLD_STATE_V2",
            &tick.to_le_bytes(),
            &domains.config,
            &roots.entity_root_hash,
            &roots.neural_root_hash,
            &roots.field_root_hash,
            &roots.economy_root_hash,
            &domains.evolution,
            &domains.visibility,
            &roots.rng_root_hash,
            &domains.scheduled_events,
            &parent,
        ])
        .0
    }

    pub fn new_canonical(
        world_id: [u8; 16],
        tick: u64,
        revision: u64,
        parent: [u8; 32],
        roots: CommitSubRoots,
        domains: CanonicalDomains,
    ) -> Self {
        let mut commit = Self::new(world_id, tick, revision, parent, roots);
        commit.state_root_hash = Self::canonical_state_root(tick, parent, roots, domains);
        commit.canonical_domains = Some(domains);
        commit
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::CANONICAL_ENCODED_LEN);
        if self.canonical_domains.is_some() {
            out.extend_from_slice(b"RYC2");
        }
        out.extend_from_slice(&self.world_id);
        out.extend_from_slice(&self.tick.to_le_bytes());
        out.extend_from_slice(&self.world_revision.to_le_bytes());
        for root in [
            self.parent_commit_hash,
            self.entity_root_hash,
            self.field_root_hash,
            self.neural_root_hash,
            self.economy_root_hash,
            self.rng_root_hash,
            self.state_root_hash,
        ] {
            out.extend_from_slice(&root);
        }
        if let Some(d) = self.canonical_domains {
            for root in [d.config, d.evolution, d.visibility, d.scheduled_events] {
                out.extend_from_slice(&root);
            }
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        let (base, domains) = match bytes.len() {
            Self::ENCODED_LEN => (bytes, None),
            Self::CANONICAL_ENCODED_LEN if &bytes[..4] == b"RYC2" => {
                let d = &bytes[4 + Self::ENCODED_LEN..];
                (
                    &bytes[4..4 + Self::ENCODED_LEN],
                    Some(CanonicalDomains {
                        config: d[..32].try_into().unwrap(),
                        evolution: d[32..64].try_into().unwrap(),
                        visibility: d[64..96].try_into().unwrap(),
                        scheduled_events: d[96..128].try_into().unwrap(),
                    }),
                )
            }
            _ => return Err(RynvekErrorCode::InvalidFileMagicOrVersion),
        };
        let roots = CommitSubRoots {
            entity_root_hash: base[64..96].try_into().unwrap(),
            field_root_hash: base[96..128].try_into().unwrap(),
            neural_root_hash: base[128..160].try_into().unwrap(),
            economy_root_hash: base[160..192].try_into().unwrap(),
            rng_root_hash: base[192..224].try_into().unwrap(),
        };
        let world = base[..16].try_into().unwrap();
        let tick = u64::from_le_bytes(base[16..24].try_into().unwrap());
        let revision = u64::from_le_bytes(base[24..32].try_into().unwrap());
        let parent = base[32..64].try_into().unwrap();
        let commit = if let Some(d) = domains {
            Self::new_canonical(world, tick, revision, parent, roots, d)
        } else {
            Self::new(world, tick, revision, parent, roots)
        };
        if commit.state_root_hash != base[224..256] {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        Ok(commit)
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
    #[test]
    fn canonical_v2_binds_every_domain_and_keeps_legacy_codec() {
        let roots = CommitSubRoots {
            entity_root_hash: [1; 32],
            field_root_hash: [2; 32],
            neural_root_hash: [3; 32],
            economy_root_hash: [4; 32],
            rng_root_hash: [5; 32],
        };
        let domains = CanonicalDomains {
            config: [6; 32],
            evolution: [7; 32],
            visibility: [8; 32],
            scheduled_events: [9; 32],
        };
        let legacy = CommitManifest::new([1; 16], 4, 1, [10; 32], roots);
        assert_eq!(legacy.encode().len(), 256);
        assert_eq!(CommitManifest::decode(&legacy.encode()).unwrap(), legacy);
        let canonical = CommitManifest::new_canonical([1; 16], 4, 1, [10; 32], roots, domains);
        let bytes = canonical.encode();
        assert_eq!(bytes.len(), CommitManifest::CANONICAL_ENCODED_LEN);
        assert_eq!(CommitManifest::decode(&bytes).unwrap(), canonical);
        assert_ne!(canonical.state_root_hash, legacy.state_root_hash);
        assert_ne!(
            canonical.state_root_hash,
            CommitManifest::new_canonical([1; 16], 5, 1, [10; 32], roots, domains).state_root_hash
        );
        // 逐个篡改全部领域及前驱，不允许漏掉任何规范域。
        for offset in [4 + 32, 4 + 64, 4 + 96, 4 + 128, 4 + 160, 4 + 192, 260, 292, 324, 356] {
            let mut tampered = bytes.clone();
            tampered[offset] ^= 1;
            assert!(CommitManifest::decode(&tampered).is_err(), "领域偏移 {offset}");
        }
        assert!(CommitManifest::decode(&bytes[4..260]).is_err(), "禁止静默剥离 v2 扩展");
    }
}
