// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 真实CLI子进程、临时测试身份、HTTP权限Actor及文件CAS；不是PG/OS断电实证。
use rynvek_rules::package::build_tier0_package;
use rynvek_security::ServerSigningKey;
use rynvek_server::{
    AppConfig, RunningServer, ServerConfig,
    client::{self, LoginTarget},
    ids::{hex, new_uuid_v7},
    rule_package::{RuleInstallRequest, RuleReleaseManifest},
    store::Store,
};
use rynvek_world::general::GeneralWorldConfig;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    process::{Command, Output},
};
async fn start(store: Store, path: &Path) -> RunningServer {
    rynvek_server::start(ServerConfig {
        http_addr: "127.0.0.1:0".parse().unwrap(),
        webtransport_addr: None,
        app: AppConfig {
            blob_dir: Some(path.to_owned()),
            tick_driver: rynvek_server::host::TickDriver::Manual,
            lobby_timeout: None,
            ..AppConfig::default()
        },
        store,
        signer: ServerSigningKey::from_seed(1, [7; 32]),
    })
    .await
    .unwrap()
}
fn run(base: &str, owner: u64, key: &ServerSigningKey, token: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rynvek"))
        .arg("rule")
        .args(args)
        .args([
            "--server",
            base,
            "--owner",
            &owner.to_string(),
            "--trusted-key",
            &hex(&key.public_key()),
        ])
        .env("RYNVEK_TOKEN", token)
        .output()
        .unwrap()
}
fn ok(output: Output) -> Value {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_cli_publish_discover_verify_install_paid_quarantine_epoch_resume_and_private_restore() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory();
    let s = start(store.clone(), dir.path()).await;
    let key = ServerSigningKey::from_seed(0, [111; 32]);
    let (owner, token) = client::login(
        &s.base_url(),
        &key,
        LoginTarget::Register {
            handle: "rule-cli",
            kind: "human",
        },
    )
    .await
    .unwrap();
    let mut cfg: GeneralWorldConfig =
        serde_json::from_str(include_str!("../../../examples/worlds/general-rules.world.json")).unwrap();
    let components = cfg.components.as_mut().unwrap();
    let mut binding = components.bindings["steer.step"].clone();
    binding.module_id = "support".into();
    components.bindings.insert("support.step".into(), binding);
    components.rule_graphs.insert(
        "support".into(),
        components.rule_graphs["steer"].replace("state.accel_x", "state.accel_y"),
    );
    let worldbytes = cfg.build_package("world.cli.rules", "1.0.0").unwrap();
    let r = client::request(
        "POST",
        &format!("{}/api/v1/worlds/general", s.base_url()),
        &[("Authorization", &format!("Bearer {token}"))],
        Some(&worldbytes),
    )
    .await
    .unwrap();
    assert_eq!(r.status, 201);
    let world = r.json().unwrap()["world_id"].as_str().unwrap().to_owned();
    let keypath = dir.path().join("test-signing-key");
    std::fs::write(&keypath, hex(&key.seed())).unwrap();
    let graph = &cfg.components.as_ref().unwrap().rule_graphs["steer"];
    let mut releases: Vec<(RuleReleaseManifest, Vec<u8>)> = vec![];
    for id in [
        "rules.cli.old-dep",
        "rules.cli.old",
        "rules.cli.new-dep",
        "rules.cli.new",
    ] {
        let selected = if id.ends_with("-dep") {
            &cfg.components.as_ref().unwrap().rule_graphs["support"]
        } else {
            graph
        };
        let bytes = build_tier0_package(id, "1.0.0", "MIT", selected).unwrap();
        let path = dir.path().join(format!("{id}.rpack"));
        std::fs::write(&path, &bytes).unwrap();
        let depsfile = dir.path().join("exact-deps.json");
        let deps = if id.ends_with("-dep") {
            vec![]
        } else {
            vec![releases.last().unwrap().0.reference()]
        };
        std::fs::write(&depsfile, serde_json::to_vec(&deps).unwrap()).unwrap();
        let receipt = ok(run(
            &s.base_url(),
            owner,
            &key,
            &token,
            &[
                "publish",
                "--package",
                path.to_str().unwrap(),
                "--key",
                keypath.to_str().unwrap(),
                "--dependencies",
                depsfile.to_str().unwrap(),
            ],
        ));
        assert_eq!(receipt["installed"], false);
        let manifest: RuleReleaseManifest = serde_json::from_value(receipt["release"]["manifest"].clone()).unwrap();
        releases.push((manifest, bytes));
    }
    assert_eq!(
        ok(run(&s.base_url(), owner, &key, &token, &["list"]))["packages"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let download = dir.path().join("verified.rpack");
    let m = &releases[1].0;
    let args = [
        "download",
        "--id",
        m.package_id.as_str(),
        "--version",
        "1.0.0",
        "--cid",
        m.artifact_cid.as_str(),
        "--out",
        download.to_str().unwrap(),
    ];
    assert_eq!(ok(run(&s.base_url(), owner, &key, &token, &args))["verified"], true);
    assert_eq!(std::fs::read(&download).unwrap(), releases[1].1);
    assert!(
        !run(&s.base_url(), owner, &key, &token, &args).status.success(),
        "不覆盖原下载资料"
    );
    let badkey = ServerSigningKey::from_seed(0, [112; 32]);
    let badout = dir.path().join("untrusted.rpack");
    assert!(
        !run(
            &s.base_url(),
            owner,
            &badkey,
            &token,
            &[
                "download",
                "--id",
                m.package_id.as_str(),
                "--version",
                "1.0.0",
                "--cid",
                m.artifact_cid.as_str(),
                "--out",
                badout.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    assert!(!badout.exists());
    let h = ok(run(&s.base_url(), owner, &key, &token, &["connect", "--world", &world]))["host_fence"]
        .as_u64()
        .unwrap()
        .to_string();
    let l = ok(run(
        &s.base_url(),
        owner,
        &key,
        &token,
        &["acquire", "--world", &world, "--host-fence", &h],
    ))["lease"]["fencing_token"]
        .as_u64()
        .unwrap()
        .to_string();
    for tick in 0..3 {
        ok(run(
            &s.base_url(),
            owner,
            &key,
            &token,
            &[
                "tick",
                "--world",
                &world,
                "--host-fence",
                &h,
                "--control-fence",
                &l,
                "--tick",
                &tick.to_string(),
                "--idempotency-key",
                &new_uuid_v7().to_string(),
            ],
        ));
    }
    let request = dir.path().join("install.json");
    for (n, tick) in [(1, 3), (3, 5), (3, 6), (3, 7)] {
        let m = &releases[n].0;
        std::fs::write(
            &request,
            serde_json::to_vec(&RuleInstallRequest {
                root: m.reference(),
                modules: BTreeMap::from([
                    (m.package_id.clone(), "steer".into()),
                    (releases[n - 1].0.package_id.clone(), "support".into()),
                ]),
            })
            .unwrap(),
        )
        .unwrap();
        let nonce = new_uuid_v7().to_string();
        let t = tick.to_string();
        let mut args = vec![
            "install",
            "--world",
            &world,
            "--host-fence",
            &h,
            "--control-fence",
            &l,
            "--tick",
            &t,
            "--idempotency-key",
            &nonce,
            "--request",
            request.to_str().unwrap(),
        ];
        if tick == 5 || tick == 6 {
            let mut no_consent = args.clone();
            no_consent.push("--recover-tick");
            assert!(!run(&s.base_url(), owner, &key, &token, &no_consent).status.success());
            args.extend(["--recover-tick", "--accept-maintenance-q-per-body", "4"]);
        }
        let result = ok(run(&s.base_url(), owner, &key, &token, &args));
        assert_eq!(result["commit"]["tick"], tick + 1);
        if tick == 3 {
            let nonce = new_uuid_v7().to_string();
            ok(run(
                &s.base_url(),
                owner,
                &key,
                &token,
                &[
                    "tick",
                    "--world",
                    &world,
                    "--host-fence",
                    &h,
                    "--control-fence",
                    &l,
                    "--tick",
                    "4",
                    "--idempotency-key",
                    &nonce,
                ],
            ));
            let m = &releases[1].0;
            let r = client::send_json(
                "DELETE",
                &format!(
                    "{}/api/v1/private-hub/rules/{}/{}",
                    s.base_url(),
                    m.package_id,
                    m.version
                ),
                Some(&token),
                &json!({}),
            )
            .await
            .unwrap();
            assert_eq!(r.status, 200);
        }
    }
    let expected = ok(run(&s.base_url(), owner, &key, &token, &["observe", "--world", &world]));
    assert_eq!(expected["commit"]["tick"], 8);
    assert_eq!(expected["rule_recovery"], Value::Null);
    assert!(expected["rule_meter"]["paid_q"].as_u64().unwrap() > 0);
    assert_eq!(
        expected["life"]["genesis_energy_q"],
        expected["life"]["universe_energy_q"]
    );
    s.shutdown().await;
    let s = start(store, dir.path()).await;
    assert_eq!(
        ok(run(&s.base_url(), owner, &key, &token, &["observe", "--world", &world])),
        expected
    );
    s.shutdown().await;
}
