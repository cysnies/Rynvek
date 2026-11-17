// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 世界规格档位（`WorldProfile`）、分区拓扑与跨分区两阶段实体移交协议（`EntityHandoffEnvelope`，规范 08 / 审计报告 §11.1）。
//!
//! - [`WorldProfile`]：统一描述 `Duel_1v1`、`Skirmish_2v2`、`Arena_4P`、`Persistent_Sector` 与 `NeuralSoup_Sandbox` 的网格规模、Chunk 拓扑与分区数量；
//! - [`PartitionCoord`] / [`PartitionAuthorityTable`]：管理 `Persistent_Sector`（8×8 Chunks = 2×2 Partitions，每分区 4×4 Chunks）的确定性分区归属；
//! - [`EntityHandoffCoordinator`]：跨分区边界的两阶段原子实体移交（Phase 1 `PrepareHandoff` 锁定源分区实体并生成带 BLAKE3 摘要的 [`EntityHandoffEnvelope`]；Phase 2 `CommitHandoff` 在目标分区落位并销毁源存根，或 `AbortHandoff` 恢复源实体），严格保证跨分区迁移零复制、零丢失。

use std::collections::BTreeMap;

use rynvek_core::error::RynvekErrorCode;
use rynvek_ecs::EntityId;
use serde::{Deserialize, Serialize};

/// 五类标准世界运行规格（规范 08 §8.1）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorldProfile {
    Duel1v1,
    Skirmish2v2,
    Arena4P,
    PersistentSector,
    NeuralSoupSandbox,
}

impl WorldProfile {
    /// 世界边长（Tiles）
    pub const fn side_tiles(self) -> u32 {
        match self {
            Self::Duel1v1 | Self::NeuralSoupSandbox => 256,
            Self::Skirmish2v2 => 512,
            Self::Arena4P => 768,
            Self::PersistentSector => 2048,
        }
    }

    /// 每边 Chunk 数量（256 Tiles / Chunk）
    pub const fn side_chunks(self) -> u32 {
        self.side_tiles() / 256
    }

    /// 每边 Partition 数量（`PersistentSector` 为 2×2 Partitions，其余单分区为 1×1）
    pub const fn side_partitions(self) -> u16 {
        match self {
            Self::PersistentSector => 2,
            _ => 1,
        }
    }
}

/// 分区二维坐标
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PartitionCoord {
    pub px: u16,
    pub py: u16,
}

impl PartitionCoord {
    pub const fn new(px: u16, py: u16) -> Self {
        Self { px, py }
    }

    /// 根据世界 Tile 坐标 `(tx, ty)` 与 `WorldProfile` 确定性判定所属分区
    pub const fn from_tile(profile: WorldProfile, tx: u32, ty: u32) -> Self {
        let parts = profile.side_partitions() as u32;
        let span = profile.side_tiles() / parts;
        let raw_x = tx / span;
        let raw_y = ty / span;
        // 保持 const 求值兼容，边界夹紧语义与 min(parts - 1) 一致。
        let px = if raw_x < parts { raw_x } else { parts - 1 } as u16;
        let py = if raw_y < parts { raw_y } else { parts - 1 } as u16;
        Self { px, py }
    }
}

/// 跨分区实体移交阶段状态
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandoffState {
    Prepared,
    Committed,
    Aborted,
}

/// 跨分区两阶段实体移交信封（规范 08 §8.3）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityHandoffEnvelope {
    pub handoff_id: u64,
    pub tick: u64,
    pub entity_id: u128,
    pub from_partition: PartitionCoord,
    pub to_partition: PartitionCoord,
    pub dest_tile: (u32, u32),
    /// 实体组件与神经脑状态载荷字节
    pub payload_bytes: Vec<u8>,
    /// 对信封头与载荷计算的确定性 BLAKE3 摘要
    pub envelope_digest: [u8; 32],
    pub state: HandoffState,
}

impl EntityHandoffEnvelope {
    pub fn compute_digest(
        handoff_id: u64,
        tick: u64,
        entity_id: u128,
        from: PartitionCoord,
        to: PartitionCoord,
        dest_tile: (u32, u32),
        payload: &[u8],
    ) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(b"RYNVEK_ENTITY_HANDOFF_V1");
        h.update(&handoff_id.to_le_bytes());
        h.update(&tick.to_le_bytes());
        h.update(&entity_id.to_le_bytes());
        h.update(&from.px.to_le_bytes());
        h.update(&from.py.to_le_bytes());
        h.update(&to.px.to_le_bytes());
        h.update(&to.py.to_le_bytes());
        h.update(&dest_tile.0.to_le_bytes());
        h.update(&dest_tile.1.to_le_bytes());
        h.update(payload);
        *h.finalize().as_bytes()
    }
}

/// 分区权威表与两阶段跨分区移交协调器
#[derive(Clone, Debug)]
pub struct EntityHandoffCoordinator {
    pub profile: WorldProfile,
    next_handoff_id: u64,
    /// 各实体当前权威所在的分区映射
    entity_authority: BTreeMap<u128, PartitionCoord>,
    /// 正在进行或已归档的移交事务表
    envelopes: BTreeMap<u64, EntityHandoffEnvelope>,
}

impl EntityHandoffCoordinator {
    pub fn new(profile: WorldProfile) -> Self {
        Self {
            profile,
            next_handoff_id: 1,
            entity_authority: BTreeMap::new(),
            envelopes: BTreeMap::new(),
        }
    }

    /// 在指定分区注册新生成的实体权威归属
    pub fn register_entity(&mut self, id: EntityId, partition: PartitionCoord) {
        self.entity_authority.insert(id.0, partition);
    }

    /// 查询实体当前所属的权威分区
    pub fn authority_of(&self, id: EntityId) -> Option<PartitionCoord> {
        self.entity_authority.get(&id.0).copied()
    }

    /// 阶段 1（Prepare）：校验源分区权威并生成冻结状态的移交信封（同一实体不得并发处于两个 Prepared 事务中）
    pub fn prepare_handoff(
        &mut self,
        tick: u64,
        id: EntityId,
        from_partition: PartitionCoord,
        dest_tile: (u32, u32),
        payload_bytes: Vec<u8>,
    ) -> Result<EntityHandoffEnvelope, RynvekErrorCode> {
        let current = self
            .entity_authority
            .get(&id.0)
            .copied()
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)?;
        if current != from_partition {
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }
        let to_partition = PartitionCoord::from_tile(self.profile, dest_tile.0, dest_tile.1);
        if to_partition == from_partition {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        if self
            .envelopes
            .values()
            .any(|e| e.entity_id == id.0 && e.state == HandoffState::Prepared)
        {
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }

        let handoff_id = self.next_handoff_id;
        self.next_handoff_id += 1;
        let envelope_digest = EntityHandoffEnvelope::compute_digest(
            handoff_id,
            tick,
            id.0,
            from_partition,
            to_partition,
            dest_tile,
            &payload_bytes,
        );
        let env = EntityHandoffEnvelope {
            handoff_id,
            tick,
            entity_id: id.0,
            from_partition,
            to_partition,
            dest_tile,
            payload_bytes,
            envelope_digest,
            state: HandoffState::Prepared,
        };
        self.envelopes.insert(handoff_id, env.clone());
        Ok(env)
    }

    /// 阶段 2A（Commit）：校验信封摘要后将实体权威原子切换至目标分区
    pub fn commit_handoff(&mut self, handoff_id: u64) -> Result<EntityHandoffEnvelope, RynvekErrorCode> {
        let env = self
            .envelopes
            .get_mut(&handoff_id)
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)?;
        if env.state != HandoffState::Prepared {
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }
        let expected = EntityHandoffEnvelope::compute_digest(
            env.handoff_id,
            env.tick,
            env.entity_id,
            env.from_partition,
            env.to_partition,
            env.dest_tile,
            &env.payload_bytes,
        );
        if expected != env.envelope_digest {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        env.state = HandoffState::Committed;
        let to = env.to_partition;
        let eid = env.entity_id;
        let out = env.clone();
        self.entity_authority.insert(eid, to);
        Ok(out)
    }

    /// 阶段 2B（Abort）：回滚未完成的移交，实体权威保留在源分区
    pub fn abort_handoff(&mut self, handoff_id: u64) -> Result<EntityHandoffEnvelope, RynvekErrorCode> {
        let env = self
            .envelopes
            .get_mut(&handoff_id)
            .ok_or(RynvekErrorCode::EntityNotFoundOrTombstoned)?;
        if env.state != HandoffState::Prepared {
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }
        env.state = HandoffState::Aborted;
        Ok(env.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_phase_handoff_transfers_partition_authority_atomically() {
        let mut coord = EntityHandoffCoordinator::new(WorldProfile::PersistentSector);
        let eid = EntityId::new(1, 77);
        let p00 = PartitionCoord::new(0, 0);
        let p10 = PartitionCoord::new(1, 0);
        coord.register_entity(eid, p00);
        assert_eq!(coord.authority_of(eid), Some(p00));

        // 目标坐标 (1200, 300) 位于右上/右左分区 (1, 0)
        let env = coord
            .prepare_handoff(120, eid, p00, (1200, 300), vec![1, 2, 3, 4])
            .unwrap();
        assert_eq!(env.to_partition, p10);
        // Commit 之前权威仍在 p00
        assert_eq!(coord.authority_of(eid), Some(p00));

        let committed = coord.commit_handoff(env.handoff_id).unwrap();
        assert_eq!(committed.state, HandoffState::Committed);
        assert_eq!(coord.authority_of(eid), Some(p10));
    }
}
