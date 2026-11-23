// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use std::process::Command;
fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rynvek")).args(args).output().unwrap()
}
#[test]
fn general_world_cli_build_run_recover_matches_root_and_actual_scene() {
    let tmp = tempfile::tempdir().unwrap();
    let config = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/worlds/general-collision.world.json"
    );
    let package = tmp.path().join("collision.rworld");
    let package = package.to_str().unwrap();
    let snapshot = tmp.path().join("genesis.rsnap");
    let snapshot = snapshot.to_str().unwrap();
    let journal = tmp.path().join("world.rjournal");
    let journal = journal.to_str().unwrap();
    let build = run(&[
        "world",
        "build",
        "--config",
        config,
        "--out",
        package,
        "--id",
        "world.general.collision",
    ]);
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let result = run(&[
        "world",
        "run",
        "--package",
        package,
        "--world-id",
        "01010101010101010101010101010101",
        "--ticks",
        "4",
        "--snapshot",
        snapshot,
        "--journal",
        journal,
    ]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["tick"], 4);
    assert_eq!(value["frameScene"].as_array().unwrap().len(), 3);
    assert_eq!(value["gpuVerified"], false);
    assert!(value["bodies"][0]["body"]["velocity_q32"][0].as_i64().unwrap() < 0);
    let recovered = run(&["world", "recover", "--snapshot", snapshot, "--journal", journal]);
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&recovered.stdout).unwrap(),
        value
    );
    let original = std::fs::read(journal).unwrap();
    assert!(
        !run(&[
            "world",
            "run",
            "--package",
            package,
            "--world-id",
            "01010101010101010101010101010101",
            "--ticks",
            "4",
            "--snapshot",
            snapshot,
            "--journal",
            journal
        ])
        .status
        .success()
    );
    assert_eq!(std::fs::read(journal).unwrap(), original);
    assert!(
        !run(&[
            "world",
            "build",
            "--config",
            config,
            "--out",
            package,
            "--id",
            "world.general.collision"
        ])
        .status
        .success()
    );
}

#[test]
fn general_life_cli_build_run_recover_keeps_sourced_accounts() {
    let tmp = tempfile::tempdir().unwrap();
    let config = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/worlds/general-life.world.json"
    );
    let package = tmp.path().join("life.rworld");
    let package = package.to_str().unwrap();
    let snapshot = tmp.path().join("genesis.rsnap");
    let snapshot = snapshot.to_str().unwrap();
    let journal = tmp.path().join("world.rjournal");
    let journal = journal.to_str().unwrap();
    let build = run(&[
        "world",
        "build",
        "--config",
        config,
        "--out",
        package,
        "--id",
        "world.general.life",
    ]);
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let result = run(&[
        "world",
        "run",
        "--package",
        package,
        "--world-id",
        "02020202020202020202020202020202",
        "--ticks",
        "4",
        "--snapshot",
        snapshot,
        "--journal",
        journal,
    ]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["tick"], 4);
    assert_eq!(value["life"]["profile"], "rynvek.general-sourced-life.v1");
    assert!(value["life"]["accounts"][0]["received"].as_u64().unwrap() > 0);
    assert_eq!(value["life"]["accounts"].as_array().unwrap().len(), 1);
    let recovered = run(&["world", "recover", "--snapshot", snapshot, "--journal", journal]);
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&recovered.stdout).unwrap(),
        value
    );
}
