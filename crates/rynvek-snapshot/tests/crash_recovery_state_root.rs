// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! M1 验收：100,000 实体增删改 + 两阶段 Tombstone + 断电重放后状态根 BLAKE3 一致。

use rynvek_core::hash::Blake3Hash;
use rynvek_ecs::EntityRegistry;
use rynvek_journal::{
    CommitManifest, CommitSubRoots, CrashRecoveryState, CrashRecoveryStateMachine, JournalWriter, RecoveryInput,
};
use rynvek_snapshot::{NeurChunk, NeurNeuronRow, SnapshotReader, SnapshotWriter};
use tempfile::TempDir;

#[test]
fn one_hundred_thousand_entities_snapshot_journal_crash_recovery() {
    let dir = TempDir::new().unwrap();
    let world_id = [0x42u8; 16];

    let mut registry = EntityRegistry::new(0x42);
    let mut handles = Vec::with_capacity(100_000);
    for i in 0..100_000u32 {
        handles.push(registry.spawn((i % 16) as u16).unwrap().0);
    }
    assert_eq!(registry.alive_count(), 100_000);

    for h in handles.iter().take(25_000) {
        registry.despawn(*h, 12).unwrap();
    }
    assert_eq!(registry.tombstone_count(), 25_000);
    assert_eq!(registry.recycle_tombstones(4), 25_000);
    assert_eq!(registry.alive_count(), 75_000);

    for i in 0..25_000u32 {
        registry.spawn((i % 5) as u16).unwrap();
    }
    assert_eq!(registry.alive_count(), 100_000);
    let live: Vec<_> = registry.iter_alive().map(|(_, h, _)| h).collect();
    for h in live.iter().take(1_000) {
        let _ = registry.despawn(*h, 16);
        registry.spawn(99).unwrap();
    }
    registry.recycle_tombstones(5);
    assert_eq!(registry.alive_count(), 100_000);

    let entity_root = registry.state_root_blake3();

    let snap_path = dir.path().join("world.rsnap");
    let neur = NeurChunk {
        rows: vec![NeurNeuronRow {
            dense_index: 0,
            spike_history_ring: 0xDEAD_BEEF,
            weight_deltas: vec![0, 3, 0, 0, -1],
            ..NeurNeuronRow::default()
        }],
    };
    let snap = SnapshotWriter::write_path(&snap_path, world_id, 20, 1, [0; 32], &registry, Some(&neur)).unwrap();
    assert_eq!(snap.state_root, snap.commit.state_root_hash);

    let journal_path = dir.path().join("world.rjournal");
    let mut writer = JournalWriter::create(&journal_path, world_id, 20, 1, 20).unwrap();
    let mut parent = snap.state_root;
    let mut last_root = parent;
    for rev in 2..=3u64 {
        let mut entity = entity_root;
        entity[0] ^= rev as u8;
        let commit = CommitManifest::new(
            world_id,
            20 + rev,
            rev,
            parent,
            CommitSubRoots {
                entity_root_hash: entity,
                field_root_hash: Blake3Hash::of(b"field-empty").0,
                neural_root_hash: Blake3Hash::of(&neur.encode()).0,
                economy_root_hash: Blake3Hash::of(b"economy-empty").0,
                rng_root_hash: Blake3Hash::of(b"rng-empty").0,
            },
        );
        writer.append_commit(&commit).unwrap();
        parent = commit.state_root_hash;
        last_root = commit.state_root_hash;
    }
    writer.append_torn_bytes(&[0xFF, 0xEE, 0xDD]).unwrap();

    let mut machine = CrashRecoveryStateMachine::new();
    let recovered = machine
        .run(&RecoveryInput {
            snapshot_path: Some(snap_path.clone()),
            journal_path: journal_path.clone(),
            snapshot_state_root: snap.state_root,
            snapshot_tick: 20,
            snapshot_revision: 1,
        })
        .unwrap();
    assert_eq!(recovered.final_state, CrashRecoveryState::Completed);
    assert!(recovered.torn_write_detected);
    assert_eq!(recovered.replayed_commits.len(), 2);
    assert_eq!(recovered.final_state_root, last_root);

    let loaded = SnapshotReader::read_path(&snap_path).unwrap();
    assert_eq!(loaded.state_root, snap.state_root);
    assert_eq!(loaded.registry.alive_count(), 100_000);
    assert_eq!(loaded.registry.state_root_blake3(), entity_root);
    assert_eq!(loaded.neur.unwrap().rows[0].spike_history_ring, 0xDEAD_BEEF);
}
