// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use rynvek_simulation::{
    engine::SimulationEngine,
    physics::{Body, ONE, PhysicsCommand as C},
};
fn body(x: i64, vx: i64) -> Body {
    Body {
        position_q32: [x * ONE, 64 * ONE],
        velocity_q32: [vx * ONE, 0],
        acceleration_q32: [0; 2],
        radius_q32: ONE / 2,
        mass_q32: ONE,
        fixed: false,
    }
}
#[test]
fn general_empty_world_has_no_implicit_entities_or_material() {
    let w = SimulationEngine::new([1; 16]);
    assert_eq!(w.physics().iter().count(), 0);
    assert_eq!((w.biomass, w.flux, w.neurogel), (0, 0, 0));
    assert_ne!(w.parent_commit_hash, SimulationEngine::new([2; 16]).parent_commit_hash);
}
#[test]
fn general_symplectic_euler_and_csr_follow_real_body() {
    let mut w = SimulationEngine::new([1; 16]);
    let mut b = body(10, 0);
    b.acceleration_q32[0] = 30 * ONE;
    w.tick_with_commands(&[C::Spawn { body: b }]).unwrap();
    let actual = w.physics().body(1).unwrap();
    assert_eq!(actual.velocity_q32[0], ONE);
    assert_eq!(
        actual.position_q32[0],
        10 * ONE + ONE / 4 / 120 + ONE / 2 / 120 + (3 * ONE / 4) / 120 + ONE / 120
    );
    assert_eq!(w.spatial.sorted_dense_indices, vec![0]);
    assert_eq!(w.phase_exec_counts, [1; 16]);
}
#[test]
fn general_head_on_elastic_contact_uses_mass() {
    let mut w = SimulationEngine::new([1; 16]);
    let a = body(10, 4);
    let mut b = body(11, 0);
    b.mass_q32 = 3 * ONE;
    w.tick_with_commands(&[C::Spawn { body: a }, C::Spawn { body: b }])
        .unwrap();
    let va = w.physics().body(1).unwrap().velocity_q32[0];
    let vb = w.physics().body(2).unwrap().velocity_q32[0];
    assert!((va + 2 * ONE).abs() < 64, "{va}");
    assert!((vb - 2 * ONE).abs() < 64, "{vb}");
    assert!((va + 3 * vb - 4 * ONE).abs() < 128);
}
#[test]
fn general_fixed_body_does_not_move_and_wall_reflects() {
    let mut w = SimulationEngine::new([1; 16]);
    let mut fixed = body(11, 0);
    fixed.fixed = true;
    let mut wall = body(2, -4);
    wall.position_q32[0] = ONE / 2;
    w.tick_with_commands(&[
        C::Spawn { body: body(10, 4) },
        C::Spawn { body: fixed },
        C::Spawn { body: wall },
    ])
    .unwrap();
    assert_eq!(w.physics().body(2), Some(&fixed));
    assert!(w.physics().body(1).unwrap().velocity_q32[0] < 0);
    assert!(w.physics().body(3).unwrap().velocity_q32[0] > 0);
}
#[test]
fn general_failure_rolls_back_clock_field_registry_and_root() {
    let mut w = SimulationEngine::new([1; 16]);
    let before = w.encode_snapshot().unwrap();
    assert!(
        w.tick_with_commands(&[C::Spawn { body: body(10, 0) }, C::Despawn { serial: 999 }])
            .is_err()
    );
    assert_eq!(w.encode_snapshot().unwrap(), before);
    w.diagnostics_nonzero_count = 1;
    let root = w.parent_commit_hash;
    let field = w.field.authority_bytes();
    assert!(w.tick_with_commands(&[C::Spawn { body: body(10, 1) }]).is_err());
    assert_eq!(w.clock.tick, 0);
    assert_eq!(w.parent_commit_hash, root);
    assert_eq!(w.physics().iter().count(), 0);
    assert_eq!(w.field.authority_bytes(), field);
}
#[test]
fn general_dense_fixed_overlap_and_invalid_inputs_fail_closed() {
    let mut w = SimulationEngine::new([1; 16]);
    let mut b = body(10, 0);
    b.fixed = true;
    assert!(
        w.tick_with_commands(&[C::Spawn { body: b }, C::Spawn { body: b }])
            .is_err()
    );
    b.radius_q32 = 0;
    assert!(w.tick_with_commands(&[C::Spawn { body: b }]).is_err());
    assert!(w.tick_with_commands(&vec![C::Spawn { body: body(10, 0) }; 65]).is_err());
    assert_eq!(w.clock.tick, 0);
}
#[test]
fn general_swap_remove_never_retargets_stale_serial_and_restores_allocator() {
    let mut w = SimulationEngine::new([1; 16]);
    w.tick_with_commands(&[
        C::Spawn { body: body(10, 0) },
        C::Spawn { body: body(20, 1) },
        C::Despawn { serial: 1 },
    ])
    .unwrap();
    assert!(w.physics().body(1).is_none());
    assert!(w.physics().body(2).is_some());
    let before = w.encode_snapshot().unwrap();
    assert!(
        w.tick_with_commands(&[C::SetAcceleration {
            serial: 1,
            acceleration_q32: [ONE, 0]
        }])
        .is_err()
    );
    assert_eq!(w.encode_snapshot().unwrap(), before);
    let mut r = SimulationEngine::decode_snapshot(&before).unwrap();
    for _ in 0..4 {
        let commands = [C::Spawn { body: body(30, 0) }, C::Despawn { serial: 3 }];
        if w.clock.tick == 1 {
            w.tick_with_commands(&commands).unwrap();
            r.tick_with_commands(&commands).unwrap();
        } else {
            w.tick().unwrap();
            r.tick().unwrap();
        }
        assert_eq!(w.parent_commit_hash, r.parent_commit_hash);
    }
}
#[test]
fn general_snapshot_preserves_field_sources_and_every_future_root() {
    let mut w = SimulationEngine::new([3; 16]);
    w.field
        .set_interior(rynvek_field::FieldChannelId::Nutrient, 32, 32, 8000);
    w.field.channel_mut(rynvek_field::FieldChannelId::SignalA).sources[10] = 100;
    w.tick_with_commands(&[C::Spawn { body: body(10, 4) }, C::Spawn { body: body(11, -4) }])
        .unwrap();
    let bytes = w.encode_snapshot().unwrap();
    let mut r = SimulationEngine::decode_snapshot(&bytes).unwrap();
    assert_eq!(r.encode_snapshot().unwrap(), bytes);
    for _ in 0..8 {
        assert_eq!(w.tick().unwrap().state_root_hash, r.tick().unwrap().state_root_hash);
    }
    let mut broken = bytes;
    let last = broken.len() - 1;
    broken[last] ^= 0x80;
    assert!(SimulationEngine::decode_snapshot(&broken).is_err());
}
#[test]
fn general_original_wal_replays_commands_lifecycle_and_each_commit() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path();
    let snap = base.join("genesis.rsnap");
    let wal = base.join("world.rjournal");
    let mut w = SimulationEngine::new([4; 16]);
    std::fs::write(&snap, w.encode_snapshot().unwrap()).unwrap();
    let mut writer = rynvek_journal::JournalWriter::create(&wal, w.world_id, 0, 0, 0).unwrap();
    w.tick_recorded(
        &[C::Spawn { body: body(10, 4) }, C::Spawn { body: body(11, -4) }],
        &mut writer,
    )
    .unwrap();
    w.tick_recorded(&[C::Despawn { serial: 1 }], &mut writer).unwrap();
    w.tick_recorded(&[], &mut writer).unwrap();
    drop(writer);
    let (r, out) = SimulationEngine::recover(&snap, &wal).unwrap();
    assert_eq!(out.replayed_commits.len(), 3);
    assert_eq!(r.parent_commit_hash, w.parent_commit_hash);
    assert_eq!(r.encode_snapshot().unwrap(), w.encode_snapshot().unwrap());
    // 模拟尾部撕裂，只能恢复最后完整提交，不执行未提交命令。
    let bytes = std::fs::read(&wal).unwrap();
    std::fs::write(&wal, &bytes[..bytes.len() - 9]).unwrap();
    let (r, out) = SimulationEngine::recover(&snap, &wal).unwrap();
    assert!(out.torn_write_detected);
    assert_eq!(r.clock.tick, 2);
}

#[test]
fn general_entity_and_velocity_hard_limits_fail_without_partial_tick() {
    let bodies: Vec<_> = (0..128)
        .map(|i| {
            let mut b = body(4 + (i % 16) * 4, 0);
            b.position_q32[1] = (4 + (i / 16) * 4) * ONE;
            b
        })
        .collect();
    let mut w = SimulationEngine::with_bodies([6; 16], &bodies).unwrap();
    w.tick().unwrap();
    let before = w.encode_snapshot().unwrap();
    assert!(w.tick_with_commands(&[C::Spawn { body: body(100, 0) }]).is_err());
    assert_eq!(w.encode_snapshot().unwrap(), before);
    let mut oversized = bodies;
    oversized.push(body(100, 0));
    assert!(SimulationEngine::with_bodies([6; 16], &oversized).is_err());
    let mut bad = body(10, 8);
    bad.acceleration_q32 = [32 * ONE, 0];
    let mut w = SimulationEngine::with_bodies([6; 16], &[bad]).unwrap();
    let before = w.encode_snapshot().unwrap();
    assert!(w.tick().is_err());
    assert_eq!(w.encode_snapshot().unwrap(), before);
}
#[test]
fn general_snapshot_recomputes_component_and_source_roots_not_only_envelope() {
    use rynvek_snapshot::{CMIT_TAG, SnapshotReader, SnapshotWriter};
    let w = SimulationEngine::with_bodies([7; 16], &[body(10, 1)]).unwrap();
    let original = w.encode_snapshot().unwrap();
    assert!(SnapshotReader::read_bytes_bounded(&original, 1).is_err());
    let mut trailing = original.clone();
    trailing.push(0);
    assert!(SimulationEngine::decode_snapshot(&trailing).is_err());
    for tag in [*b"COMP", *b"FILD"] {
        let mut read = SnapshotReader::read_bytes(&original).unwrap();
        let commit = rynvek_journal::CommitManifest::decode(read.find_chunk(CMIT_TAG).unwrap()).unwrap();
        let chunk = read.custom_chunks.iter_mut().find(|c| c.tag == tag).unwrap();
        if tag == *b"COMP" {
            let mut values: Vec<Body> = serde_json::from_slice(&chunk.payload).unwrap();
            values[0].mass_q32 += ONE;
            chunk.payload = serde_json::to_vec(&values).unwrap();
        } else {
            let last = chunk.payload.len() - 1;
            chunk.payload[last] ^= 1;
        }
        // 重新写入完全有效的压缩容器，仍须因实际权威子根不匹配而拒绝。
        let forged = SnapshotWriter::write_bytes_with_chunks(
            w.world_id,
            0,
            0,
            commit.parent_commit_hash,
            &read.registry,
            None,
            &read.custom_chunks,
        )
        .unwrap()
        .0;
        assert!(SimulationEngine::decode_snapshot(&forged).is_err());
    }
}
#[test]
fn general_uncommitted_wal_tail_is_not_applied_and_wrong_world_header_rejected() {
    use rynvek_journal::{JournalCommandRecord, JournalReader, JournalRecord, JournalWriter};
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path();
    let snap = base.join("genesis.rsnap");
    let wal = base.join("world.rjournal");
    let mut w = SimulationEngine::new([8; 16]);
    std::fs::write(&snap, w.encode_snapshot().unwrap()).unwrap();
    let mut writer = JournalWriter::create_new(&wal, w.world_id, 0, 0, 0).unwrap();
    w.tick_recorded(&[C::Spawn { body: body(10, 1) }], &mut writer).unwrap();
    writer
        .append_record(&JournalRecord::CommandAccepted(JournalCommandRecord {
            tick: 2,
            world_revision: 2,
            faction_id: 0,
            command_bytes: serde_json::to_vec(&[C::Despawn { serial: 1 }]).unwrap(),
        }))
        .unwrap();
    drop(writer);
    let (r, out) = SimulationEngine::recover(&snap, &wal).unwrap();
    assert_eq!(r.parent_commit_hash, w.parent_commit_hash);
    assert_eq!(out.replayed_commits.len(), 1);
    assert!(r.physics().body(1).is_some());
    assert!(JournalReader::read_path_bounded(&wal, 63).is_err());
    assert!(JournalWriter::create_new(&wal, w.world_id, 0, 0, 0).is_err());
    let mut bytes = std::fs::read(&wal).unwrap();
    bytes[16] ^= 1;
    std::fs::write(&wal, bytes).unwrap();
    assert!(SimulationEngine::recover(&snap, &wal).is_err());
}
