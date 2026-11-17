// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_rts::world::ecology::EcologyVersion;
use rynvek_rts::{ArenaWorld, MatchConfig};

#[test]
fn new_world_config_root_distinguishes_actual_ecology_mode() {
    let mut v1 = MatchConfig::pve(57, 1);
    v1.ecology = Some(EcologyVersion::EnergyExchangeV1);
    let mut v2 = v1.clone();
    v2.ecology = Some(EcologyVersion::ForagerBudgetV2);
    let a = ArenaWorld::new(v1);
    let b = ArenaWorld::new(v2);
    assert_ne!(
        a.canonical_state().config_root,
        b.canonical_state().config_root,
        "新世界必须绑定实际模式，不能共用legacy标签"
    );
}

fn fixture(v2: bool) -> (Vec<u8>, MatchConfig) {
    let text = if v2 {
        include_str!("../../../tests/arena/forager-budget-v2.json")
    } else {
        include_str!("../../../tests/arena/energy-exchange-v1.json")
    };
    let f: serde_json::Value = serde_json::from_str(text).unwrap();
    let bytes = f["snapshot_hex"]
        .as_str()
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|x| u8::from_str_radix(std::str::from_utf8(x).unwrap(), 16).unwrap())
        .collect();
    // 固定旧夹具已知创世配置，绝不从待检CONF推导可信期望。
    let mut expected = MatchConfig::pve(57, 1);
    expected.ecology = Some(if v2 {
        EcologyVersion::ForagerBudgetV2
    } else {
        EcologyVersion::EnergyExchangeV1
    });
    (bytes, expected)
}

#[test]
fn legacy_requires_independent_mode_pin_and_migration_preserves_real_history() {
    use rynvek_journal::CommitManifest;
    use rynvek_rts::ArenaRuntime;
    use rynvek_snapshot::{CMIT_TAG, SnapshotReader};
    for v2 in [false, true] {
        let (bytes, expected) = fixture(v2);
        let read = SnapshotReader::read_bytes(&bytes).unwrap();
        assert!(ArenaWorld::decode_snapshot(&bytes).is_err());
        let (_, wrong) = fixture(!v2);
        assert!(ArenaWorld::decode_snapshot_with_config(&bytes, &wrong).is_err());
        let mut old = ArenaRuntime::restore_with_config(
            read.header.world_id,
            read.header.world_revision,
            read.state_root,
            &bytes,
            &expected,
        )
        .unwrap();
        let before = old.checkpoint().unwrap();
        assert!(old.clone().fork([91; 16]).is_err());
        assert!(
            old.transact_at(old.tick, |w| {
                w.config.ecology = wrong.ecology;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(old.checkpoint().unwrap().bytes, before.bytes);
        assert!(old.migrate_ecology_identity([91; 16], &wrong).is_err());
        let mut migrated = old.migrate_ecology_identity([91; 16], &expected).unwrap();
        assert_eq!(old.checkpoint().unwrap().bytes, before.bytes);
        assert_eq!(migrated.tick, old.tick);
        assert_eq!(migrated.revision(), 0);
        assert_ne!(migrated.state_root(), old.state_root());
        assert_eq!(migrated.config.ecology, old.config.ecology);
        assert!(migrated.config.ecology_identity.is_some());
        let cp = migrated.checkpoint().unwrap();
        let r = SnapshotReader::read_bytes(&cp.bytes).unwrap();
        assert_eq!(
            CommitManifest::decode(r.find_chunk(CMIT_TAG).unwrap())
                .unwrap()
                .parent_commit_hash,
            old.state_root()
        );
        let mut restored = ArenaRuntime::restore([91; 16], cp.revision, cp.state_root, &cp.bytes).unwrap();
        for _ in 0..12 {
            migrated.step().unwrap();
            restored.step().unwrap();
            assert_eq!(migrated.state_root(), restored.state_root());
        }
    }
}

#[test]
fn resealed_cross_mode_snapshot_and_downgrade_are_rejected() {
    use rynvek_snapshot::{CONF_TAG, SnapshotReader, SnapshotWriter};
    let mut c = MatchConfig::pve(57, 1);
    c.ecology = Some(EcologyVersion::EnergyExchangeV1);
    let w = ArenaWorld::new(c);
    let (bytes, _) = w.encode_snapshot([92; 16], 0, [0; 32]).unwrap();
    let read = SnapshotReader::read_bytes(&bytes).unwrap();
    for drop_identity in [false, true] {
        let mut chunks = read.custom_chunks.clone();
        let conf = chunks.iter_mut().find(|c| c.tag == CONF_TAG).unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(&conf.payload).unwrap();
        if drop_identity {
            v["config"].as_object_mut().unwrap().remove("ecology_identity");
        } else {
            v["config"]["ecology"] = serde_json::json!("forager_budget_v2");
        }
        conf.payload = serde_json::to_vec(&v).unwrap();
        let (forged, _) = SnapshotWriter::write_bytes_with_chunks(
            [92; 16],
            w.tick,
            0,
            [0; 32],
            &read.registry,
            read.neur.as_ref(),
            &chunks,
        )
        .unwrap();
        SnapshotReader::read_bytes(&forged).expect("通用容器合法，领域根仍须拒绝跨模式/降级");
        assert!(ArenaWorld::decode_snapshot(&forged).is_err());
        assert!(ArenaWorld::decode_snapshot_with_config(&forged, &w.config).is_err());
    }
}
