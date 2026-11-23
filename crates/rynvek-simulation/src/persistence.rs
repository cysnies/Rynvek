// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 复用 RYC2 / 原 `.rsnap` 和 `.rjournal`，不是另一套简化存档。
use crate::{
    engine::{SimulationEngine, SimulationError},
    physics::{Body, MAX_COMMANDS, PhysicsCommand},
};
use rynvek_core::RynvekErrorCode;
use rynvek_ecs::PagedComponentColumn;
use rynvek_journal::{
    CanonicalDomains, CommitManifest, CommitSubRoots, CrashRecoveryStateMachine, JournalCommandRecord, JournalRecord,
    JournalWriter, RecoveryInput, RecoveryOutput,
};
use rynvek_snapshot::{CMIT_TAG, SnapshotCustomChunk, SnapshotReader, SnapshotWriter};
use serde::{Deserialize, Serialize};
use std::path::Path;
const ERROR: RynvekErrorCode = RynvekErrorCode::Blake3ChecksumMismatch;
pub const PHYSICS_PROFILE: &str = "rynvek.general-circle-ecs.v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    profile: String,
    economy: [i64; 6],
    closed_world: bool,
    diagnostics: u64,
    phase_counts: [u64; 16],
    field_steps: u64,
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}
fn sim_error(_: SimulationError) -> RynvekErrorCode {
    ERROR
}
impl SimulationEngine {
    pub fn authoritative_commit(&self, parent: [u8; 32]) -> CommitManifest {
        let mut rng = Vec::new();
        for v in [
            self.clock.tick,
            self.clock.world_revision,
            self.clock.coupling_epoch,
            self.field_steps_completed,
            self.diagnostics_nonzero_count,
        ] {
            rng.extend_from_slice(&v.to_le_bytes());
        }
        rng.push(u8::from(self.closed_world));
        let mut config = PHYSICS_PROFILE.as_bytes().to_vec();
        config.extend_from_slice(&self.world_id);
        if let Some(life) = &self.life {
            config.extend_from_slice(&life.hash_accounts());
        }
        let roots = CommitSubRoots {
            entity_root_hash: self.hash_entities(),
            field_root_hash: hash(&self.field.authority_bytes()),
            neural_root_hash: self
                .life
                .as_ref()
                .map(|l| l.neural_root())
                .unwrap_or_else(|| hash(b"GENERAL_NO_NEURAL_COMPONENTS_V1")),
            economy_root_hash: self.hash_economy(),
            rng_root_hash: hash(&rng),
        };
        CommitManifest::new_canonical(
            self.world_id,
            self.clock.tick,
            self.clock.world_revision,
            parent,
            roots,
            CanonicalDomains {
                config: hash(&config),
                evolution: hash(b"GENERAL_NO_EVOLUTION_V1"),
                visibility: hash(b"GENERAL_OFFLINE_OWNER_ONLY_V1"),
                scheduled_events: hash(b"GENERAL_NO_SCHEDULED_EVENTS_V1"),
            },
        )
    }
    pub fn encode_snapshot(&self) -> Result<Vec<u8>, RynvekErrorCode> {
        self.physics.validate()?;
        let parent = self.last_manifest.as_ref().ok_or(ERROR)?.parent_commit_hash;
        let commit = self.authoritative_commit(parent);
        if commit.state_root_hash != self.parent_commit_hash {
            return Err(ERROR);
        }
        let meta = Metadata {
            profile: PHYSICS_PROFILE.into(),
            economy: [
                self.biomass,
                self.flux,
                self.neurogel,
                self.biomass_capacity,
                self.flux_capacity,
                self.neurogel_capacity,
            ],
            closed_world: self.closed_world,
            diagnostics: self.diagnostics_nonzero_count,
            phase_counts: self.phase_exec_counts,
            field_steps: self.field_steps_completed,
        };
        let mut chunks = vec![
            SnapshotCustomChunk {
                tag: CMIT_TAG,
                payload: commit.encode(),
            },
            SnapshotCustomChunk {
                tag: *b"COMP",
                payload: serde_json::to_vec(&self.physics.bodies.iter().collect::<Vec<_>>()).map_err(|_| ERROR)?,
            },
            SnapshotCustomChunk {
                tag: *b"FILD",
                payload: self.field.authority_bytes(),
            },
            SnapshotCustomChunk {
                tag: *b"RNGS",
                payload: serde_json::to_vec(&meta).map_err(|_| ERROR)?,
            },
        ];
        if let Some(life) = &self.life {
            chunks.push(SnapshotCustomChunk {
                tag: *b"LIFE",
                payload: serde_json::to_vec(&life.snapshot_dto()).map_err(|_| ERROR)?,
            });
        }
        let neur = self.life.as_ref().and_then(|l| l.neur_chunk());
        Ok(SnapshotWriter::write_bytes_with_chunks(
            self.world_id,
            self.clock.tick,
            self.clock.world_revision,
            parent,
            &self.physics.registry,
            neur.as_ref(),
            &chunks,
        )?
        .0)
    }
    pub fn decode_snapshot(bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(ERROR);
        }
        let read = SnapshotReader::read_bytes_bounded(bytes, 16 * 1024 * 1024)?;
        if read
            .header
            .payload_bytes
            .checked_add(rynvek_snapshot::SNAPSHOT_HEADER_LEN as u64 + 32)
            != Some(bytes.len() as u64)
        {
            return Err(ERROR);
        }
        if read.header.format_version != rynvek_snapshot::header::CANONICAL_SNAPSHOT_VERSION
            || read.header.flags != rynvek_snapshot::header::SNAPSHOT_FLAG_ZSTD
            || !(read.custom_chunks.len() == 4 || read.custom_chunks.len() == 5)
        {
            return Err(ERROR);
        }
        if read.find_chunk(*b"LIFE").is_none() && read.custom_chunks.len() != 4 {
            return Err(ERROR);
        }
        if read.find_chunk(*b"LIFE").is_some() && read.custom_chunks.len() != 5 {
            return Err(ERROR);
        }
        let commit = CommitManifest::decode(read.find_chunk(CMIT_TAG).ok_or(ERROR)?)?;
        if commit.world_id != read.header.world_id
            || commit.tick != read.header.tick
            || commit.world_revision != commit.tick
            || commit.world_revision != read.header.world_revision
            || commit.canonical_domains.is_none()
        {
            return Err(ERROR);
        }
        let raw_comp = read.find_chunk(*b"COMP").ok_or(ERROR)?;
        if raw_comp.len() > 128 * 512 {
            return Err(ERROR);
        }
        let bodies: Vec<Body> = serde_json::from_slice(raw_comp).map_err(|_| ERROR)?;
        let meta: Metadata = serde_json::from_slice(read.find_chunk(*b"RNGS").ok_or(ERROR)?).map_err(|_| ERROR)?;
        if meta.profile != PHYSICS_PROFILE
            || meta.phase_counts != [commit.tick; 16]
            || meta.field_steps != commit.tick.div_ceil(3)
        {
            return Err(ERROR);
        }
        let mut world = Self::new(read.header.world_id);
        world.field = rynvek_field::FieldWorld::from_authority_bytes(read.find_chunk(*b"FILD").ok_or(ERROR)?)?;
        let life_bytes = read.find_chunk(*b"LIFE").map(|b| b.to_vec());
        let neur = read.neur;
        world.physics.registry = read.registry;
        world.physics.bodies = PagedComponentColumn::new();
        for body in bodies {
            world.physics.bodies.push(body);
        }
        world.physics.validate()?;
        if let Some(raw) = life_bytes {
            if raw.len() > 128 * 1024 {
                return Err(ERROR);
            }
            let dto: crate::life::LifeSnapshotDto = serde_json::from_slice(&raw).map_err(|_| ERROR)?;
            world.life = Some(crate::life::restore(&world.physics, dto, neur.as_ref())?);
            crate::life::assert_closed(world.life.as_ref().unwrap(), &world.field)?;
        } else if neur.is_some() {
            return Err(ERROR);
        }
        world.clock.tick = commit.tick;
        world.clock.world_revision = commit.world_revision;
        world.clock.coupling_epoch = commit.tick / 4;
        [
            world.biomass,
            world.flux,
            world.neurogel,
            world.biomass_capacity,
            world.flux_capacity,
            world.neurogel_capacity,
        ] = meta.economy;
        world.closed_world = meta.closed_world;
        world.diagnostics_nonzero_count = meta.diagnostics;
        world.phase_exec_counts = meta.phase_counts;
        world.field_steps_completed = meta.field_steps;
        world
            .run_phase(crate::phase::CanonicalPhaseId::InvariantAndNumericCheck)
            .map_err(sim_error)?;
        let actual = world.authoritative_commit(commit.parent_commit_hash);
        if actual != commit || actual.state_root_hash != read.state_root {
            return Err(ERROR);
        }
        world.publish_commit(actual);
        world.spatial = rynvek_spatial::SpatialCsrIndex::build_from_local_q32(&world.physics.positions())?;
        Ok(world)
    }
    /// WAL 记录全部命令及提交并经过原 sync_data 屏障后才发布内存候选。
    /// I/O 失败后必须丢弃该 writer 并从持久状态恢复，不能在未知落盘状态继续提交。
    pub fn tick_recorded(
        &mut self,
        commands: &[PhysicsCommand],
        writer: &mut JournalWriter,
    ) -> Result<CommitManifest, RynvekErrorCode> {
        if writer.header().world_id != self.world_id {
            return Err(ERROR);
        }
        let mut candidate = self.clone();
        candidate.tick_with_commands(commands).map_err(sim_error)?;
        let commit = candidate.authoritative_commit(self.parent_commit_hash);
        if !commands.is_empty() {
            writer.append_record(&JournalRecord::CommandAccepted(JournalCommandRecord {
                tick: commit.tick,
                world_revision: commit.world_revision,
                faction_id: 0,
                command_bytes: serde_json::to_vec(commands).map_err(|_| ERROR)?,
            }))?;
        }
        writer.append_commit(&commit)?;
        *self = candidate;
        Ok(commit)
    }
    pub fn recover(snapshot: &Path, journal: &Path) -> Result<(Self, RecoveryOutput), RynvekErrorCode> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(snapshot)
            .map_err(|_| ERROR)?
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ERROR)?;
        let mut world = Self::decode_snapshot(&bytes)?;
        let mut header = [0u8; rynvek_journal::JOURNAL_HEADER_LEN];
        std::fs::File::open(journal)
            .map_err(|_| ERROR)?
            .read_exact(&mut header)
            .map_err(|_| ERROR)?;
        let h = rynvek_journal::JournalFileHeader::decode(&header)?;
        if h.world_id != world.world_id
            || h.base_tick != world.clock.tick
            || h.base_revision != world.clock.world_revision
            || h.snapshot_tick != world.clock.tick
            || h.flags != 0
            || header[10..12] != [0; 2]
            || header[56..64] != [0; 8]
        {
            return Err(ERROR);
        }
        let input = RecoveryInput {
            snapshot_path: Some(snapshot.to_owned()),
            journal_path: journal.to_owned(),
            snapshot_state_root: world.parent_commit_hash,
            snapshot_tick: world.clock.tick,
            snapshot_revision: world.clock.world_revision,
        };
        let mut pending: Vec<PhysicsCommand> = Vec::new();
        let mut pending_tick = None;
        let out = CrashRecoveryStateMachine::new().run_with_bounded_replayer(&input, 32 * 1024 * 1024, |record| {
            let next_tick = world.clock.tick.checked_add(1).ok_or(ERROR)?;
            let next_revision = world.clock.world_revision.checked_add(1).ok_or(ERROR)?;
            match record {
                JournalRecord::CommandAccepted(c) => {
                    if c.faction_id != 0
                        || c.tick != next_tick
                        || c.world_revision != next_revision
                        || c.command_bytes.len() > MAX_COMMANDS * 512
                        || pending_tick.is_some()
                    {
                        return Err(ERROR);
                    }
                    pending = serde_json::from_slice(&c.command_bytes).map_err(|_| ERROR)?;
                    if pending.len() > MAX_COMMANDS {
                        return Err(ERROR);
                    }
                    pending_tick = Some(c.tick);
                    Ok(None)
                }
                JournalRecord::TickCommit(c) => {
                    if c.world_id != world.world_id || c.tick != next_tick || c.world_revision != next_revision {
                        return Err(ERROR);
                    }
                    let parent = world.parent_commit_hash;
                    world.tick_with_commands(&pending).map_err(sim_error)?;
                    if world.authoritative_commit(parent) != *c {
                        return Err(ERROR);
                    }
                    pending.clear();
                    pending_tick = None;
                    Ok(Some(world.parent_commit_hash))
                }
                _ => Err(ERROR),
            }
        })?;
        // 尾部仅接受而未提交的命令不应用，保留原恢复器的撕裂写标志。
        Ok((world, out))
    }
}
