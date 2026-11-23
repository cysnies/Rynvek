// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! C# SDK真实注册/挑战登录/私有世界HTTP/WS/原Actor/fog/命令/工具/重连。
//! .NET10与显式build是集成前提；不访问外部服务/用户真实私钥、不冒称浏览器/PG。
mod common;
use common::*;
use serde_json::{Value, json};
#[test]
fn csharp_auth_fixture_matches_authoritative_protocol() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../tests/golden_vectors/csharp-wire-v1.json")).unwrap();
    let challenge = std::array::from_fn(|i| i as u8);
    assert_eq!(
        rynvek_server::ids::hex(&rynvek_security::challenge_message(u64::MAX, &challenge)),
        fixture["auth_message_hex"]
    );
    let schema: Value = serde_json::from_str(include_str!("../../../protocols/schemas/agent-tools.v1.json")).unwrap();
    assert_eq!(
        fixture["tool_names"],
        Value::Array(schema.as_array().unwrap().iter().map(|v| v["name"].clone()).collect())
    );
}
#[tokio::test]
#[ignore = "需先用.NET10构建Rynvek.Client.Smoke；显式--ignored，见sdks/csharp/README.md"]
async fn csharp_real_identity_world_fog_commands_tools_and_reconnect() {
    let server = start_server(false).await;
    let base = server.base_url();
    let owner = rynvek_security::ServerSigningKey::from_seed(0, [171; 32]);
    let other = rynvek_security::ServerSigningKey::from_seed(0, [172; 32]);
    let (send, mut phases) = tokio::sync::mpsc::channel::<Value>(8);
    let program = tokio::task::spawn_blocking(move || {
        use std::io::{BufRead as _, Write as _};
        let dotnet = std::env::var("RYNVEK_DOTNET").unwrap_or_else(|_| "dotnet".into());
        let mut child = std::process::Command::new(dotnet)
            .arg("sdks/csharp/Rynvek.Client.Smoke/bin/Debug/net10.0/Rynvek.Client.Smoke.dll")
            .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect(".NET10 C#集成程序");
        let mut input = child.stdin.take().unwrap();
        let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
        writeln!(input,"{}",json!({"base":base,"public_key":rynvek_server::ids::hex(&owner.public_key()),"other_public_key":rynvek_server::ids::hex(&other.public_key())})).unwrap();
        let mut result = None;
        let mut signatures = 0;
        loop {
            let mut line = String::new();
            if output.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let value: Value = serde_json::from_str(&line).expect("C#只输出结构化非凭据状态");
            if let Some(message) = value.get("sign").and_then(Value::as_str) {
                let bytes = (0..message.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&message[i..i + 2], 16).unwrap())
                    .collect::<Vec<_>>();
                assert!(
                    bytes.starts_with(b"rynvek-register/1\0") || bytes.starts_with(b"rynvek-auth/1\0"),
                    "只签名真实协议域，不作为任意签名oracle"
                );
                let key = match value["key"].as_str().unwrap() {
                    "owner" => &owner,
                    "other" => &other,
                    _ => panic!("未知测试签名身份"),
                };
                signatures += 1;
                writeln!(
                    input,
                    "{}",
                    json!({"signature":rynvek_server::ids::hex(&key.sign(&bytes))})
                )
                .unwrap();
            } else if value.get("phase").is_some() {
                send.blocking_send(value).unwrap();
            } else if value["done"] == true {
                result = Some(value);
            } else {
                panic!("未知C#输出状态")
            }
        }
        drop(input);
        let exit = child.wait_with_output().unwrap();
        assert!(
            exit.status.success(),
            "C#真实程序失败：{}",
            String::from_utf8_lossy(&exit.stderr)
        );
        assert_eq!(signatures, 5, "C#自己注册/签名取会话与重登录，不借Rust登录后的令牌");
        result.expect("真实场景结果")
    });
    let driver = async {
        let mut steps = 0;
        while let Some(event) = phases.recv().await {
            let world = event["world_id"].as_str().unwrap().parse().unwrap();
            let host = server.state.registry().by_world(&world).expect("C#创建并连接的原Actor");
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                host.wait_for(|s| s.pending_inputs > 0),
            )
            .await
            .expect("真实命令必须实际入队");
            match event["phase"].as_str().unwrap() {
                "move" => {
                    host.advance(31).await.unwrap();
                }
                "query" => {
                    host.advance(1).await.unwrap();
                    host.advance(30).await.unwrap();
                }
                "strategic" | "finish" => {
                    host.advance(1).await.unwrap();
                }
                _ => panic!("未知驱动阶段"),
            }
            steps += 1;
        }
        assert_eq!(steps, 4);
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(55), async {
        tokio::join!(program, driver)
    })
    .await
    .expect("C#完整场景超时");
    let result = result.unwrap();
    for key in ["moved", "reconnect", "outsider_denied", "completed"] {
        assert_eq!(result[key], true, "{result}")
    }
    assert_eq!(result["exact_seed"], 9_007_199_254_740_993u64);
    assert_eq!(result["query"], "query_visible_clusters");
    assert_eq!(result["strategy"], "issue_strategic_intent");
    let world_id = result["world_id"].as_str().unwrap().parse().unwrap();
    let world = server.state.store().get_world(world_id).await.unwrap().unwrap();
    assert_eq!(
        world.owner_principal_id,
        result["principal_id"].as_str().unwrap().parse::<u64>().unwrap()
    );
    assert!(world.latest_tick >= 64);
    server.shutdown().await;
}

#[tokio::test]
#[ignore = "需.NET10构建新General C#主体，显式--include-ignored；非浏览器或PG恢复"]
async fn csharp_real_general_durable_login_budget_pause_resume_cancel_relogin_and_paid_completion() {
    use rynvek_server::{AppConfig, ServerConfig, client, store::Store};
    let dir = tempfile::tempdir().unwrap();
    let server = rynvek_server::start(ServerConfig {
        http_addr: "127.0.0.1:0".parse().unwrap(),
        webtransport_addr: None,
        app: AppConfig {
            blob_dir: Some(dir.path().to_owned()),
            tick_driver: rynvek_server::host::TickDriver::Manual,
            lobby_timeout: None,
            ..AppConfig::default()
        },
        store: Store::memory(),
        signer: server_key(),
    })
    .await
    .unwrap();
    let owner = register(&server.base_url(), "general-csharp-owner", "human", 173).await;
    let cfg: rynvek_world::general::GeneralWorldConfig =
        serde_json::from_str(include_str!("../../../examples/worlds/general-rules.world.json")).unwrap();
    let bytes = cfg.build_package("world.csharp.durable", "1.0.0").unwrap();
    let created = client::request(
        "POST",
        &format!("{}/api/v1/worlds/general", server.base_url()),
        &[("Authorization", &format!("Bearer {}", owner.token))],
        Some(&bytes),
    )
    .await
    .unwrap();
    assert_eq!(created.status, 201);
    let world = created.json().unwrap()["world_id"].as_str().unwrap().to_owned();
    let config = json!({"base":server.base_url(),"world":world,"principal":owner.principal_id.to_string()});
    let result = tokio::task::spawn_blocking(move || {
        use std::io::{BufRead as _, Write as _};
        let dotnet = std::env::var("RYNVEK_DOTNET").unwrap_or_else(|_| "dotnet".into());
        let mut child = std::process::Command::new(dotnet)
            .args([
                "sdks/csharp/Rynvek.Client.Smoke/bin/Debug/net10.0/Rynvek.Client.Smoke.dll",
                "--general-run",
            ])
            .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
        writeln!(input, "{config}").unwrap();
        let mut signatures = 0;
        let mut result = None;
        loop {
            let mut line = String::new();
            if output.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let value: Value = serde_json::from_str(&line).unwrap();
            if let Some(message) = value["sign"].as_str() {
                let bytes = (0..message.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&message[i..i + 2], 16).unwrap())
                    .collect::<Vec<_>>();
                assert!(bytes.starts_with(b"rynvek-auth/1\0"));
                assert_eq!(value["key"], "owner");
                writeln!(
                    input,
                    "{}",
                    json!({"signature":rynvek_server::ids::hex(&owner.key.sign(&bytes))})
                )
                .unwrap();
                signatures += 1;
            } else {
                assert_eq!(value["done"], true);
                result = Some(value);
            }
        }
        drop(input);
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(signatures, 2, "C#两次挑战登录，没借夹具Bearer");
        result.unwrap()
    })
    .await
    .unwrap();
    let id = world.parse().unwrap();
    let row = server.state.store().get_world(id).await.unwrap().unwrap();
    assert!(row.latest_tick >= 3);
    assert_eq!(result["tick"], row.latest_tick);
    assert_eq!(result["root"], rynvek_server::ids::hex(&row.latest_state_root));
    assert!(result["paid"].as_u64().unwrap() > 0);
    assert_eq!(result["storage"], "memory");
    server.shutdown().await;
}
