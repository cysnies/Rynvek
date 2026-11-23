// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `ArenaWorld` 的 `.rsnap` 全世界分块快照读写与 `.rjournal` WAL 崩溃恢复闭环（审计报告 §4）。
//!
//! 分块映射：
//! - `ENTI`：旧工具兼容用计数注册表，不作为 Arena 权威实体根；
//! - `CMIT`：显式 RYC2 十域清单；`MAPT`：实际地形格、尺寸与种子；
//! - `NEUR`：全部存活单位的四种脑模型（`GatedMLP` / `LIF` / `Izhikevich` / `CTRNN`）动态内部状态；
//! - `CONF`：`MatchConfig` 与自定义地形尺寸；
//! - `COMP`：全部建筑与单位的宏观组件快照（不含脑内部动态态，后者从 `NEUR` 精确恢复）；
//! - `FILD`：五通道连续反应扩散环境场 Q8.8 网格与步数计数；
//! - `ECON`：各阵营资源账本、科技树、主脑锚点与淘汰/投降状态；
//! - `EVOL`：各阵营基因池、繁育计数、PvE 虫潮导演与适应度归档记录；
//! - `FOGW`：各阵营对称阴影投射战争迷雾位图与敌方建筑静态幽灵快照；
//! - `RNGS`：世界随机种子、当前 Tick、下一实体序号、突然死亡倒计时与胜负结果；
//! - `ROOT`：`CanonicalWorldState` 十个分层子根与最终 256-bit `state_hash()`。

use std::collections::BTreeMap;
use std::path::Path;

use rynvek_core::{Blake3Hash, RynvekErrorCode};
use rynvek_ecs::EntityRegistry;
use rynvek_journal::{
    CommitManifest, CrashRecoveryStateMachine, JournalCommandRecord, JournalRecord, JournalWriter, RecoveryInput,
    RecoveryOutput,
};
use rynvek_snapshot::{
    CMIT_TAG, COMP_TAG, CONF_TAG, ECON_TAG, EVOL_TAG, FILD_TAG, FOGW_TAG, MAPT_TAG, RNGS_TAG, ROOT_TAG,
    SnapshotCustomChunk, SnapshotReader, SnapshotWriteOutput, SnapshotWriter,
};
use serde::{Deserialize, Serialize};

use crate::arena_map::ArenaMap;
use crate::brain::UnitBrain;
use crate::combat::Defense;
use crate::commands::RtsCommand;
use crate::evolution::FitnessRecord;
use crate::fields::FieldGrid;
use crate::fog::{BuildingGhost, FactionFog};
use crate::genome::Genome;
use crate::ids::{EntitySerial, FactionId, UnitKind};
use crate::math::Vec2;
use crate::modulators::Modulators;
use crate::nav::{HpaGraph, NavGrid};
use crate::pve::PveDirector;
use crate::resources::ResourceLedger;
use crate::tech::TechSet;
use crate::terrain::TerrainTileKind;
use crate::world::{ArenaWorld, Building, CanonicalWorldState, Faction, MatchConfig, MatchOutcome, Order, Unit};

/// 单位宏观组件快照（不含 `brain` 内部动态态，`brain` 由 `Genome` 重建后再由 `NEUR` 块无损恢复）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct UnitSnapshotDto {
    pub id: EntitySerial,
    pub faction: FactionId,
    pub kind: UnitKind,
    pub genome: Genome,
    pub position: Vec2,
    pub velocity: Vec2,
    pub defense: Defense,
    pub max_hp_milli: i64,
    pub max_shield_milli: i64,
    pub weapon_cooldown: u32,
    pub order: Order,
    pub born_tick: u64,
    pub fitness: FitnessRecord,
    pub modulators: Modulators,
    pub actions: [i16; 6],
    pub harvest_remainder: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CompChunkDto {
    pub parameter_state: super::parameter_rules::ParameterState,
    pub buildings: Vec<Building>,
    pub units: Vec<UnitSnapshotDto>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FildChunkDto {
    pub width: u32,
    pub height: u32,
    pub steps: u64,
    pub values_q8: Vec<i16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct EconFactionDto {
    pub id: FactionId,
    pub ledger: ResourceLedger,
    pub techs: TechSet,
    pub surrendered: bool,
    pub eliminated: bool,
    pub nexus: Option<EntitySerial>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct EvolFactionDto {
    pub id: FactionId,
    pub genomes: BTreeMap<String, Genome>,
    pub bred_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct EvolChunkDto {
    pub factions: Vec<EvolFactionDto>,
    pub pve: Option<PveDirector>,
    pub pve_records: Vec<(u32, Genome, FitnessRecord)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FogFactionDto {
    pub id: FactionId,
    pub fog: FactionFog,
    pub ghosts: BTreeMap<EntitySerial, BuildingGhost>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RngsChunkDto {
    pub tick: u64,
    pub next_serial: u32,
    pub sudden_death_until: Option<u64>,
    pub outcome: Option<MatchOutcome>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ConfChunkDto {
    pub arena_format: String,
    pub authoring: super::authoring::AuthoringState,
    pub config: MatchConfig,
    pub map_width: u32,
    pub map_height: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MapChunkDto {
    width: u32,
    height: u32,
    seed: u64,
    tiles: Vec<TerrainTileKind>,
}

fn to_chunk<T: Serialize>(tag: [u8; 4], val: &T) -> Result<SnapshotCustomChunk, RynvekErrorCode> {
    let payload = serde_json::to_vec(val).map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
    Ok(SnapshotCustomChunk { tag, payload })
}

fn from_chunk<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, RynvekErrorCode> {
    serde_json::from_slice(bytes).map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)
}

impl ArenaWorld {
    /// 将当前完整世界编码为 `.rsnap` 字节流（涵盖全部 10 个权威分块与四模型 `NEUR` 动态态）
    pub fn encode_snapshot(
        &self,
        world_id: [u8; 16],
        world_revision: u64,
        parent_state_root: [u8; 32],
    ) -> Result<(Vec<u8>, SnapshotWriteOutput), RynvekErrorCode> {
        self.authoring.validate()?;
        self.parameter_state.validate(&self.authoring)?;
        if self.authoring.last_edit_revision > world_revision {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        if !self.validate_ecology() {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        let mut reg = EntityRegistry::new(u16::from_le_bytes([world_id[0], world_id[1]]).into());
        for _ in 0..(self.buildings.len() + self.units.len() + self.parameter_state.entities.len()) {
            reg.spawn(1)?;
        }
        let neur = self.to_neur_chunk();
        let canonical = self.canonical_state_with_parent(Blake3Hash(parent_state_root));

        let conf = ConfChunkDto {
            arena_format: "rynvek-arena/14".into(),
            authoring: self.authoring.clone(),
            config: self.config.clone(),
            map_width: self.map.width,
            map_height: self.map.height,
        };
        let comp = CompChunkDto {
            parameter_state: self.parameter_state.clone(),
            buildings: self.buildings.values().cloned().collect(),
            units: self
                .units
                .values()
                .map(|u| UnitSnapshotDto {
                    id: u.id,
                    faction: u.faction,
                    kind: u.kind,
                    genome: u.genome.clone(),
                    position: u.position,
                    velocity: u.velocity,
                    defense: u.defense,
                    max_hp_milli: u.max_hp_milli,
                    max_shield_milli: u.max_shield_milli,
                    weapon_cooldown: u.weapon_cooldown,
                    order: u.order,
                    born_tick: u.born_tick,
                    fitness: u.fitness,
                    modulators: u.modulators,
                    actions: u.actions,
                    harvest_remainder: u.harvest_remainder,
                })
                .collect(),
        };
        let fild = FildChunkDto {
            width: self.fields.width(),
            height: self.fields.height(),
            steps: self.fields.steps(),
            values_q8: self.fields.export_q8(),
        };
        let econ: Vec<EconFactionDto> = self
            .factions
            .iter()
            .map(|f| EconFactionDto {
                id: f.id,
                ledger: f.ledger.clone(),
                techs: f.techs,
                surrendered: f.surrendered,
                eliminated: f.eliminated,
                nexus: f.nexus,
            })
            .collect();
        let evol = EvolChunkDto {
            factions: self
                .factions
                .iter()
                .map(|f| EvolFactionDto {
                    id: f.id,
                    genomes: f.genomes.clone(),
                    bred_count: f.bred_count,
                })
                .collect(),
            pve: self.pve.clone(),
            pve_records: self.pve_records.clone(),
        };
        let fogw: Vec<FogFactionDto> = self
            .factions
            .iter()
            .map(|f| FogFactionDto {
                id: f.id,
                fog: f.fog.clone(),
                ghosts: f.ghosts.clone(),
            })
            .collect();
        let rngs = RngsChunkDto {
            tick: self.tick,
            next_serial: self.next_serial(),
            sudden_death_until: self.sudden_death_until,
            outcome: self.outcome.clone(),
        };

        let mut extra_chunks = vec![
            SnapshotCustomChunk {
                tag: CMIT_TAG,
                payload: self
                    .to_commit_manifest(world_id, world_revision, parent_state_root)
                    .encode(),
            },
            to_chunk(
                MAPT_TAG,
                &MapChunkDto {
                    width: self.map.width,
                    height: self.map.height,
                    seed: self.map.seed,
                    tiles: self.map.tiles.clone(),
                },
            )?,
            to_chunk(CONF_TAG, &conf)?,
            to_chunk(COMP_TAG, &comp)?,
            to_chunk(FILD_TAG, &fild)?,
            to_chunk(ECON_TAG, &econ)?,
            to_chunk(EVOL_TAG, &evol)?,
            to_chunk(FOGW_TAG, &fogw)?,
            to_chunk(RNGS_TAG, &rngs)?,
            to_chunk(ROOT_TAG, &canonical)?,
        ];

        if let Some(ecology) = &self.ecology {
            extra_chunks.push(to_chunk(super::ecology::ECOL_TAG, ecology)?);
        }
        SnapshotWriter::write_bytes_with_chunks(
            world_id,
            self.tick,
            world_revision,
            parent_state_root,
            &reg,
            Some(&neur),
            &extra_chunks,
        )
    }

    /// 原子保存完整世界快照到 `.rsnap` 文件
    pub fn save_snapshot(
        &self,
        path: impl AsRef<Path>,
        world_id: [u8; 16],
        world_revision: u64,
        parent_state_root: [u8; 32],
    ) -> Result<SnapshotWriteOutput, RynvekErrorCode> {
        let target = path.as_ref();
        let (bytes, out) = self.encode_snapshot(world_id, world_revision, parent_state_root)?;
        let tmp_path = target.with_extension("rsnap.tmp");
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp_path).map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
        file.write_all(&bytes)
            .map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
        file.sync_all().map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
        drop(file);
        std::fs::rename(&tmp_path, target).map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
        // 同步目录元数据，让文件替换在断电恢复后仍然可见。
        let parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::File::open(parent)
            .and_then(|dir| dir.sync_all())
            .map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
        Ok(out)
    }

    /// 从 `.rsnap` 字节流完整恢复 `ArenaWorld` 并强制核验 `CanonicalWorldState` 与 `state_hash()`
    pub fn decode_snapshot(bytes: &[u8]) -> Result<Self, RynvekErrorCode> {
        let read = SnapshotReader::read_bytes(bytes)?;
        // 旧通用快照保留在低层读取器；Arena 不把缺失地形和规范根的 v1 解释成完整恢复。
        if read.header.format_version != 2 {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let conf: ConfChunkDto = from_chunk(
            read.find_chunk(CONF_TAG)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?,
        )?;
        if conf.arena_format != "rynvek-arena/14" {
            return Err(RynvekErrorCode::InvalidFileMagicOrVersion);
        }
        let comp: CompChunkDto = from_chunk(
            read.find_chunk(COMP_TAG)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?,
        )?;
        let fild: FildChunkDto = from_chunk(
            read.find_chunk(FILD_TAG)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?,
        )?;
        let econ: Vec<EconFactionDto> = from_chunk(
            read.find_chunk(ECON_TAG)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?,
        )?;
        let evol: EvolChunkDto = from_chunk(
            read.find_chunk(EVOL_TAG)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?,
        )?;
        let fogw: Vec<FogFactionDto> = from_chunk(
            read.find_chunk(FOGW_TAG)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?,
        )?;
        let rngs: RngsChunkDto = from_chunk(
            read.find_chunk(RNGS_TAG)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?,
        )?;
        let expected_canonical: CanonicalWorldState = from_chunk(
            read.find_chunk(ROOT_TAG)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?,
        )?;

        let terrain: MapChunkDto = from_chunk(
            read.find_chunk(MAPT_TAG)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?,
        )?;
        if terrain.width == 0
            || terrain.height == 0
            || terrain.width != conf.map_width
            || terrain.height != conf.map_height
            || (terrain.width as usize).checked_mul(terrain.height as usize) != Some(terrain.tiles.len())
        {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        let map = ArenaMap {
            width: terrain.width,
            height: terrain.height,
            seed: terrain.seed,
            tiles: terrain.tiles,
        };
        let mut nav = NavGrid::from_terrain(map.width, map.height, &map.tiles);
        let mut buildings = BTreeMap::new();
        for b in comp.buildings {
            let rect = b.rect();
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    nav.set_occupied(x, y, true);
                }
            }
            buildings.insert(b.id, b);
        }
        let hpa = HpaGraph::build(&nav);

        let mut fields = FieldGrid::new(fild.width, fild.height);
        fields.restore_q8(fild.steps, &fild.values_q8)?;

        let mut units = BTreeMap::new();
        for u_dto in comp.units {
            let brain = UnitBrain::from_genome(&u_dto.genome)?;
            units.insert(
                u_dto.id,
                Unit {
                    id: u_dto.id,
                    faction: u_dto.faction,
                    kind: u_dto.kind,
                    genome: u_dto.genome,
                    brain,
                    position: u_dto.position,
                    velocity: u_dto.velocity,
                    defense: u_dto.defense,
                    max_hp_milli: u_dto.max_hp_milli,
                    max_shield_milli: u_dto.max_shield_milli,
                    weapon_cooldown: u_dto.weapon_cooldown,
                    order: u_dto.order,
                    born_tick: u_dto.born_tick,
                    fitness: u_dto.fitness,
                    modulators: u_dto.modulators,
                    actions: u_dto.actions,
                    harvest_remainder: u_dto.harvest_remainder,
                },
            );
        }

        if econ.len() != evol.factions.len() || econ.len() != fogw.len() {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        let mut factions = Vec::with_capacity(econ.len());
        for ((ec, ev), fg) in econ.into_iter().zip(evol.factions).zip(fogw) {
            if ec.id != ev.id || ec.id != fg.id {
                return Err(RynvekErrorCode::Blake3ChecksumMismatch);
            }
            factions.push(Faction {
                id: ec.id,
                ledger: ec.ledger,
                techs: ec.techs,
                fog: fg.fog,
                ghosts: fg.ghosts,
                genomes: ev.genomes,
                bred_count: ev.bred_count,
                surrendered: ec.surrendered,
                eliminated: ec.eliminated,
                nexus: ec.nexus,
            });
        }

        let ecology = read.find_chunk(super::ecology::ECOL_TAG).map(from_chunk).transpose()?;
        conf.authoring.validate()?;
        comp.parameter_state.validate(&conf.authoring)?;
        if conf.authoring.last_edit_revision > read.header.world_revision {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        let mut world = Self {
            authoring: conf.authoring,
            parameter_state: comp.parameter_state,
            ecology,
            config: conf.config,
            map,
            nav,
            hpa,
            fields,
            tick: rngs.tick,
            next_serial: rngs.next_serial,
            factions,
            buildings,
            units,
            flow_cache: BTreeMap::new(),
            flow_order: std::collections::VecDeque::new(),
            pve: evol.pve,
            pve_records: evol.pve_records,
            outcome: rngs.outcome,
            sudden_death_until: rngs.sudden_death_until,
            events: Vec::new(),
            command_log: None,
        };

        if world.config.mode == super::MatchMode::ParameterEntities
            && (!world.units.is_empty()
                || !world.buildings.is_empty()
                || !world.factions.is_empty()
                || world.pve.is_some()
                || world.ecology.is_some()
                || world.outcome.is_some())
        {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        if !world.validate_ecology() {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        if let Some(neur) = &read.neur {
            world.restore_from_neur(neur)?;
        }

        let actual_canonical = world.canonical_state_with_parent(expected_canonical.parent_commit_root);
        let expected_commit = world.to_commit_manifest(
            read.header.world_id,
            read.header.world_revision,
            expected_canonical.parent_commit_root.0,
        );
        let saved_commit = CommitManifest::decode(
            read.find_chunk(CMIT_TAG)
                .ok_or(RynvekErrorCode::Blake3ChecksumMismatch)?,
        )?;
        if actual_canonical != expected_canonical
            || actual_canonical.state_root().0 != read.state_root
            || expected_commit != saved_commit
            || world.tick != read.header.tick
        {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        Ok(world)
    }

    /// 从磁盘 `.rsnap` 文件恢复 `ArenaWorld`
    pub fn load_snapshot(path: impl AsRef<Path>) -> Result<Self, RynvekErrorCode> {
        let bytes = std::fs::read(path.as_ref()).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        Self::decode_snapshot(&bytes)
    }

    /// 生成用于写入 `.rjournal` 的 `CommitManifest`
    pub fn to_commit_manifest(
        &self,
        world_id: [u8; 16],
        world_revision: u64,
        parent_commit_hash: [u8; 32],
    ) -> CommitManifest {
        let c = self.canonical_state_with_parent(Blake3Hash(parent_commit_hash));
        CommitManifest::new_canonical(
            world_id,
            self.tick,
            world_revision,
            parent_commit_hash,
            c.commit_sub_roots(),
            c.commit_domains(),
        )
    }

    /// 将一条已接受的 `RtsCommand` 编码为 WAL `JournalRecord::CommandAccepted`
    pub fn encode_wal_command(
        tick: u64,
        world_revision: u64,
        faction: FactionId,
        cmd: &RtsCommand,
    ) -> Result<JournalRecord, RynvekErrorCode> {
        let command_bytes = serde_json::to_vec(cmd).map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
        Ok(JournalRecord::CommandAccepted(JournalCommandRecord {
            tick,
            world_revision,
            faction_id: faction.0,
            command_bytes,
        }))
    }

    /// 从 `.rsnap` 快照文件加载并在其上重放 `.rjournal` 中的全部命令与 Tick 提交，验证每步状态根一致
    pub fn recover_from_snapshot_and_journal(
        snapshot_path: impl AsRef<Path>,
        journal_path: impl AsRef<Path>,
        world_id: [u8; 16],
    ) -> Result<(Self, RecoveryOutput), RynvekErrorCode> {
        let snap_path = snapshot_path.as_ref().to_path_buf();
        let snap_read = SnapshotReader::read_path(&snap_path)?;
        let mut world = Self::load_snapshot(&snap_path)?;
        let mut parent_root = snap_read.state_root;
        let mut machine = CrashRecoveryStateMachine::new();
        let output = machine.run_with_replayer(
            &RecoveryInput {
                snapshot_path: Some(snap_path),
                journal_path: journal_path.as_ref().to_path_buf(),
                snapshot_state_root: snap_read.state_root,
                snapshot_tick: snap_read.header.tick,
                snapshot_revision: snap_read.header.world_revision,
            },
            |record| match record {
                JournalRecord::CommandAccepted(cmd_rec) => {
                    while world.tick < cmd_rec.tick {
                        world.step();
                    }
                    let cmd: RtsCommand = serde_json::from_slice(&cmd_rec.command_bytes)
                        .map_err(|_| RynvekErrorCode::InvalidFileMagicOrVersion)?;
                    world
                        .apply_command(FactionId(cmd_rec.faction_id), &cmd)
                        .map_err(|_| RynvekErrorCode::Blake3ChecksumMismatch)?;
                    Ok(None)
                }
                JournalRecord::TickCommit(commit) | JournalRecord::Checkpoint(commit) => {
                    while world.tick < commit.tick {
                        world.step();
                    }
                    let manifest = world.to_commit_manifest(world_id, commit.world_revision, parent_root);
                    parent_root = manifest.state_root_hash;
                    Ok(Some(manifest.state_root_hash))
                }
                _ => Ok(None),
            },
        )?;
        Ok((world, output))
    }

    /// 推进一个 Tick 并将本 Tick 的提交清单写入 `JournalWriter`
    pub fn step_with_journal(
        &mut self,
        writer: &mut JournalWriter,
        world_id: [u8; 16],
        world_revision: u64,
        parent_commit_hash: &mut [u8; 32],
    ) -> Result<CommitManifest, RynvekErrorCode> {
        self.step();
        let manifest = self.to_commit_manifest(world_id, world_revision, *parent_commit_hash);
        writer.append_commit(&manifest)?;
        *parent_commit_hash = manifest.state_root_hash;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::UnitSelector;
    use crate::ids::OrderKind;
    use tempfile::TempDir;

    #[test]
    fn arena_world_full_snapshot_and_wal_crash_recovery_preserves_all_four_brains() {
        let dir = TempDir::new().unwrap();
        let snap_path = dir.path().join("arena.rsnap");
        let journal_path = dir.path().join("arena.rjournal");
        let world_id = [0x55u8; 16];

        let mut world = ArenaWorld::new(MatchConfig::pve(42, 2));
        // 注入包含 CTRNN 与 Izhikevich 脑的单位以覆盖全部 4 种神经元模型
        for (idx, kind) in [UnitKind::Carapace, UnitKind::Weaver, UnitKind::Leviathan]
            .into_iter()
            .enumerate()
        {
            let pos = Vec2::from_int(38 + idx as i32 * 2, 38);
            world.spawn_unit(FactionId(0), kind, Genome::seed(kind), pos);
        }
        world.run(20);

        let snap_out = world.save_snapshot(&snap_path, world_id, 1, [0u8; 32]).unwrap();
        let loaded = ArenaWorld::load_snapshot(&snap_path).unwrap();
        assert_eq!(loaded.state_hash(), world.state_hash());
        assert_eq!(loaded.canonical_state(), world.canonical_state());

        // 从 Tick 20 继续运行到 Tick 32，期间通过 WAL 记录命令与每步提交，并在尾部注入撕裂写
        let mut writer = JournalWriter::create(&journal_path, world_id, 20, 1, 20).unwrap();
        let mut parent = snap_out.state_root;
        let mut rev = 1u64;

        for t in 20..32u64 {
            if t == 24 {
                let cmd = RtsCommand::IssueOrder {
                    units: UnitSelector::Archetype(Some(UnitKind::Striker)),
                    order: OrderKind::AttackMove,
                    target: Vec2::from_int(128, 128),
                };
                rev += 1;
                let wal_cmd = ArenaWorld::encode_wal_command(world.tick, rev, FactionId(0), &cmd).unwrap();
                writer.append_record(&wal_cmd).unwrap();
                world.apply_command(FactionId(0), &cmd).unwrap();
            }
            rev += 1;
            world
                .step_with_journal(&mut writer, world_id, rev, &mut parent)
                .unwrap();
        }
        writer.append_torn_bytes(&[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();

        let (recovered_world, recovery_out) =
            ArenaWorld::recover_from_snapshot_and_journal(&snap_path, &journal_path, world_id).unwrap();
        assert!(recovery_out.torn_write_detected);
        assert_eq!(recovered_world.tick, 32);
        assert_eq!(recovered_world.canonical_state(), world.canonical_state());
        assert_eq!(recovered_world.state_hash(), world.state_hash());
    }
}
