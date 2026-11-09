// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `rynvek` 二进制的黑盒测试：离线源码要约 / SBOM / 帧解码 / 回放验证，以及真实启动服务端后的健康、要约与登录。

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Output, Stdio};

use rynvek_network::FrameEncoder;
use rynvek_protocol::WireChannelId;
use rynvek_security::ServerSigningKey;
use rynvek_server::replay;
use serde_json::Value;

fn rynvek(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rynvek")).args(args).output().unwrap()
}

fn ok_json(args: &[&str]) -> Value {
    let out = rynvek(args);
    assert!(
        out.status.success(),
        "{args:?} 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn fails_with(args: &[&str], code: &str) {
    let out = rynvek(args);
    assert_eq!(out.status.code(), Some(1), "{args:?} 应失败");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(code), "{args:?} 应报 {code}，实际：{err}");
}

#[test]
fn offline_source_offer_and_sbom() {
    let v = ok_json(&["source-offer"]);
    assert_eq!(v["license"], "AGPL-3.0-only");
    assert_eq!(v["git_commit"].as_str().unwrap().len(), 40);
    let sbom = ok_json(&["sbom"]);
    assert_eq!(sbom["spdxVersion"], "SPDX-2.3");
    assert!(
        sbom["packages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "rynvek-cli" && p["licenseDeclared"] == "AGPL-3.0-only")
    );
}

#[test]
fn frame_decode_reports_header_fields() {
    let mut enc = FrameEncoder::new(false);
    let f = enc
        .encode(WireChannelId::Command, 0x1_0000_0005, br#"{"x":2}"#, false)
        .unwrap();
    let hex = rynvek_security::to_hex(&f.to_bytes());
    // 帧头恰好 16 字节：version | channel | flags(u16 LE) | payload_len | sequence | tick_low32
    assert_eq!(&hex[..8], "01010000");
    let v = ok_json(&["frame", "decode", &hex]);
    let fr = &v["frames"][0];
    assert_eq!(fr["channel"], "Command");
    assert_eq!(fr["payload_len"], 7);
    assert_eq!(fr["tick_low32"], 5);
    assert_eq!(fr["payload_json"]["x"], 2);
    fails_with(&["frame", "decode", &hex[..30]], "0x8001");
    fails_with(&["frame", "decode", &format!("09{}", &hex[2..])], "0x8001");
}

#[test]
fn replay_verify_inspect_and_tamper_detection() {
    let dir = tempfile::tempdir().unwrap();
    let mut world = rynvek_rts::ArenaWorld::new(rynvek_rts::MatchConfig::duel(9));
    world.enable_command_log();
    for _ in 0..320 {
        world.step();
    }
    let body = replay::record(&world, "m-1".into(), "w-1".into(), "rs".into(), Vec::new()).unwrap();
    let signer = ServerSigningKey::from_seed(4, [5; 32]);
    let bytes = replay::encode(&body, [1; 16], &signer).unwrap();
    let path = dir.path().join("m.rreplay");
    std::fs::write(&path, &bytes).unwrap();
    let p = path.to_str().unwrap();
    let trusted = format!("4:{}", rynvek_security::to_hex(&signer.public_key()));

    let v = ok_json(&["replay", "verify", p, "--signer", &trusted]);
    assert_eq!(v["verified"], true);
    assert_eq!(v["final_tick"], 320);
    assert_eq!(v["match_id"], "m-1");
    let v = ok_json(&["replay", "inspect", p]);
    assert_eq!(v["header"]["tick"], 320);
    assert_eq!(v["signature_checked"], false);

    // 未提供受信公钥 / 公钥不符 → 拒绝
    fails_with(&["replay", "verify", p], "--signer");
    let other = format!(
        "4:{}",
        rynvek_security::to_hex(&ServerSigningKey::from_seed(4, [6; 32]).public_key())
    );
    fails_with(&["replay", "verify", p, "--signer", &other], "0x9001");
    // 负载篡改 → 0x7002；魔数 → 0x7001
    let mut b = bytes.clone();
    b[70] ^= 0x40;
    std::fs::write(&path, &b).unwrap();
    fails_with(&["replay", "verify", p, "--signer", &trusted], "0x7002");
    let mut b = bytes.clone();
    b[1] = b'?';
    std::fs::write(&path, &b).unwrap();
    fails_with(&["replay", "verify", p, "--signer", &trusted], "0x7001");
    fails_with(&["replay", "inspect", p], "0x7001");
}

/// 后台服务端进程（Drop 时强制结束）
struct ServerProc(Child);

impl Drop for ServerProc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn server_health_source_offer_and_login() {
    let dir = tempfile::tempdir().unwrap();
    let server_key = dir.path().join("server.seed");
    let user_key = dir.path().join("user.seed");
    let k = ok_json(&["keygen", "--out", server_key.to_str().unwrap()]);
    assert_eq!(k["ed25519_public_key"].as_str().unwrap().len(), 64);
    ok_json(&["keygen", "--out", user_key.to_str().unwrap()]);
    fails_with(&["keygen", "--out", user_key.to_str().unwrap()], "无法创建");

    let mut child = Command::new(env!("CARGO_BIN_EXE_rynvek"))
        .args(["server", "--http", "127.0.0.1:0", "--key-id", "7", "--signing-key"])
        .arg(&server_key)
        .env_remove("RYNVEK_POSTGRES_URL")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let mut proc = ServerProc(child);
    let ready: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("就绪行非法 `{line}`：{e}"));
    assert_eq!(ready["event"], "listening");
    assert_eq!(ready["replay_signer"]["key_id"], 7);
    assert_eq!(ready["replay_signer"]["ed25519_public_key"], k["ed25519_public_key"]);
    let base = ready["http"].as_str().unwrap().to_owned();

    let h = ok_json(&["health", "--server", &base]);
    assert_eq!(h["status"], "ok");
    let offer = ok_json(&["source-offer", "--server", &base]);
    assert_eq!(offer["license"], "AGPL-3.0-only");
    assert_eq!(offer["sbom_url"], format!("{base}/api/v1/meta/sbom.spdx.json"));
    let sbom = ok_json(&["sbom", "--server", &base]);
    assert_eq!(sbom["spdxVersion"], "SPDX-2.3");

    let uk = user_key.to_str().unwrap();
    let v = ok_json(&["login", "--server", &base, "--key", uk, "--register", "carol"]);
    let pid = v["principal_id"].as_str().unwrap().to_owned();
    let token = v["token"].as_str().unwrap().to_owned();
    assert!(!token.is_empty());
    let v = ok_json(&["login", "--server", &base, "--key", uk, "--principal-id", &pid]);
    assert_eq!(v["principal_id"], pid);
    // 用错误私钥登录 → 0x9001
    let sk = server_key.to_str().unwrap();
    fails_with(
        &["login", "--server", &base, "--key", sk, "--principal-id", &pid],
        "0x9001",
    );
    // 不存在的对局 → 0x2002（回放下载路径）
    fails_with(
        &[
            "replay",
            "verify",
            "--match",
            "0190a000-0000-7000-8000-000000000000",
            "--server",
            &base,
            "--token",
            &token,
        ],
        "0x2002",
    );

    // SIGTERM → 优雅停机，退出码 0
    #[cfg(unix)]
    {
        let pid = proc.0.id().to_string();
        assert!(Command::new("kill").args(["-TERM", &pid]).status().unwrap().success());
        let status = proc.0.wait().unwrap();
        assert!(status.success(), "{status:?}");
    }
    drop(proc);
}
