// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Arena 权威事务：引擎和服务端复用相同状态、Tick、规范根及完整检查点。
//! 当前是检查点优先的正确性路径，不代替实时对局宿主或承诺热路径性能。

use crate::commands::RtsCommand;
use crate::{ArenaWorld, FactionId};
use rynvek_core::{Blake3Hash, RynvekErrorCode};
use rynvek_journal::CommitManifest;
use rynvek_snapshot::{CMIT_TAG, SnapshotReader};

#[derive(Debug)]
pub struct ArenaRuntime {
    world: ArenaWorld,
    world_id: [u8; 16],
    revision: u64,
    parent_root: [u8; 32],
}

#[derive(Debug)]
pub struct RuntimeCheckpoint {
    pub bytes: Vec<u8>,
    pub state_root: [u8; 32],
    pub tick: u64,
    pub revision: u64,
}

impl ArenaRuntime {
    pub fn new(world_id: [u8; 16], world: ArenaWorld) -> Self {
        Self {
            world,
            world_id,
            revision: 0,
            parent_root: [0; 32],
        }
    }

    pub fn world(&self) -> &ArenaWorld {
        &self.world
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn state_root(&self) -> [u8; 32] {
        self.world
            .canonical_state_with_parent(Blake3Hash(self.parent_root))
            .state_root()
            .0
    }

    pub fn checkpoint(&self) -> Result<RuntimeCheckpoint, RynvekErrorCode> {
        let (bytes, output) = self
            .world
            .encode_snapshot(self.world_id, self.revision, self.parent_root)?;
        Ok(RuntimeCheckpoint {
            bytes,
            state_root: output.state_root,
            tick: self.world.tick,
            revision: self.revision,
        })
    }

    /// 不从模板猜测缺失状态；旧格式、跨世界和元数据不一致都显式拒绝。
    pub fn restore(world_id: [u8; 16], revision: u64, root: [u8; 32], bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        let read = SnapshotReader::read_bytes(bytes)?;
        if read.header.world_id != world_id || read.header.world_revision != revision || read.state_root != root {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        let commit = CommitManifest::decode(
            read.find_chunk(CMIT_TAG)
                .ok_or(RynvekErrorCode::InvalidFileMagicOrVersion)?,
        )?;
        let world = ArenaWorld::decode_snapshot(bytes)?;
        Ok(Self {
            world,
            world_id,
            revision,
            parent_root: commit.parent_commit_hash,
        })
    }

    /// 以当前根为前驱，在指定 Tick 的 Phase 01 前边界应用整包操作，并只产生一次修订。
    /// 克隆候选检查点保证闭包失败时，Tick、状态和修订全部不变；外部持久化另做 CAS。
    pub fn transact_at<T>(
        &mut self,
        tick: u64,
        apply: impl FnOnce(&mut ArenaWorld) -> Result<T, RynvekErrorCode>,
    ) -> Result<T, RynvekErrorCode> {
        if tick < self.world.tick || tick.saturating_sub(self.world.tick) > 4 {
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }
        let previous_root = self.state_root();
        let checkpoint = self.checkpoint()?;
        let mut candidate = Self::restore(self.world_id, self.revision, previous_root, &checkpoint.bytes)?;
        while candidate.world.tick < tick.saturating_sub(1) {
            Self::advance_once(&mut candidate.world)?;
        }
        let boundary_tick = candidate.world.tick;
        let result = apply(&mut candidate.world)?;
        if candidate.world.tick != boundary_tick {
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }
        if candidate.world.tick < tick {
            Self::advance_once(&mut candidate.world)?;
        }
        candidate.revision = self
            .revision
            .checked_add(1)
            .ok_or(RynvekErrorCode::SimulationInvariantViolated)?;
        candidate.parent_root = previous_root;
        *self = candidate;
        Ok(result)
    }

    fn advance_once(world: &mut ArenaWorld) -> Result<(), RynvekErrorCode> {
        let before = world.tick;
        world.step();
        // 终局等状态不再推进 Tick，必须拒绝事务而不是陷入无进展循环。
        if world.tick != before.saturating_add(1) {
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }
        Ok(())
    }

    pub fn step(&mut self) -> Result<(), RynvekErrorCode> {
        let tick = self
            .world
            .tick
            .checked_add(1)
            .ok_or(RynvekErrorCode::SimulationInvariantViolated)?;
        self.transact_at(tick, |_| Ok(()))
    }

    pub fn command(&mut self, faction: FactionId, command: &RtsCommand) -> Result<(), RynvekErrorCode> {
        self.transact_at(self.world.tick, |world| {
            world
                .apply_command(faction, command)
                .map(|_| ())
                .map_err(|error| error.code())
        })
    }

    /// 分叉显式重新绑定身份与修订，不能直接重用旧世界文件头。
    pub fn fork(mut self, world_id: [u8; 16]) -> Self {
        self.parent_root = self.state_root();
        self.world_id = world_id;
        self.revision = 0;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MatchConfig;

    #[test]
    fn failed_transaction_restores_tick_root_revision_and_terrain() {
        let mut runtime = ArenaRuntime::new([1; 16], ArenaWorld::new(MatchConfig::duel(42)));
        let before = runtime.checkpoint().unwrap();
        let result: Result<(), _> = runtime.transact_at(4, |world| {
            world.map.tiles[0] = crate::TerrainTileKind::ImperviousWall;
            Err(RynvekErrorCode::PluginCapabilityDenied)
        });
        assert!(result.is_err());
        let after = runtime.checkpoint().unwrap();
        assert_eq!(before.bytes, after.bytes);
        assert_eq!(runtime.world().tick, 0);
        assert_eq!(runtime.revision(), 0);
        assert!(
            runtime
                .transact_at(1, |world| {
                    world.tick = 99;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(runtime.checkpoint().unwrap().bytes, before.bytes);
        assert!(runtime.transact_at(5, |_| Ok(())).is_err());
    }

    #[test]
    fn checkpoint_identity_and_continuation_are_bound_to_one_runtime() {
        let mut first = ArenaRuntime::new([1; 16], ArenaWorld::new(MatchConfig::duel(42)));
        first.step().unwrap();
        let checkpoint = first.checkpoint().unwrap();
        assert!(ArenaRuntime::restore([2; 16], 1, checkpoint.state_root, &checkpoint.bytes).is_err());
        assert!(ArenaRuntime::restore([1; 16], 0, checkpoint.state_root, &checkpoint.bytes).is_err());
        assert!(ArenaRuntime::restore([1; 16], 1, [0; 32], &checkpoint.bytes).is_err());
        let mut restored = ArenaRuntime::restore([1; 16], 1, checkpoint.state_root, &checkpoint.bytes).unwrap();
        let command = RtsCommand::IssueOrder {
            units: crate::commands::UnitSelector::Archetype(None),
            order: crate::OrderKind::HoldPosition,
            target: crate::Vec2::ZERO,
        };
        first.command(FactionId(0), &command).unwrap();
        restored.command(FactionId(0), &command).unwrap();
        for _ in 0..4 {
            first.step().unwrap();
            restored.step().unwrap();
            assert_eq!(first.state_root(), restored.state_root());
        }
        let fork = restored.fork([3; 16]);
        let snapshot = fork.checkpoint().unwrap();
        assert!(ArenaRuntime::restore([3; 16], 0, snapshot.state_root, &snapshot.bytes).is_ok());
        assert!(ArenaRuntime::restore([1; 16], 0, snapshot.state_root, &snapshot.bytes).is_err());
    }
    #[test]
    fn operation_runs_before_target_tick_and_terminal_step_fails_closed() {
        let mut runtime = ArenaRuntime::new([1; 16], ArenaWorld::new(MatchConfig::duel(42)));
        runtime
            .transact_at(4, |world| {
                assert_eq!(world.tick, 3);
                Ok(())
            })
            .unwrap();
        assert_eq!(runtime.world().tick, 4);
        runtime.command(FactionId(0), &RtsCommand::Surrender).unwrap();
        runtime.step().unwrap();
        let before = runtime.checkpoint().unwrap();
        assert!(runtime.step().is_err());
        assert_eq!(runtime.checkpoint().unwrap().bytes, before.bytes);
    }
}
