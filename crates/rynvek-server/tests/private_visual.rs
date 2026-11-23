// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
mod common;
use common::*;
use rynvek_server::{client, ids::hex, visual_package as visual};
use rynvek_world::package::{PackageInput, PackageInputEntry, PackageKind, write_package};
use serde_json::{Value, json};
fn pack(id: &str, version: &str, license: &str, entries: Vec<(&str, &str, Vec<u8>)>) -> Vec<u8> {
    write_package(&PackageInput {
        kind: PackageKind::Rview,
        id: id.into(),
        version: version.into(),
        name: id.into(),
        license: license.into(),
        engine_compat: "rynvek-private-theme/1".into(),
        entries: entries
            .into_iter()
            .map(|(path, kind, data)| PackageInputEntry {
                path: path.into(),
                kind: kind.into(),
                media_type: "application/json".into(),
                data,
            })
            .collect(),
    })
    .unwrap()
}
fn valid() -> Vec<u8> {
    pack(
        "owner.visual-test",
        "1.0.0",
        "CC0-1.0",
        vec![(
            "theme.json",
            "theme",
            serde_json::to_vec(&json!({"tokens":{"--rv-world-bg":"#102040","--rv-faction-1":"#e01080"}})).unwrap(),
        )],
    )
}
fn profile() -> visual::Profile {
    visual::Profile {
        schema: "rynvek.private-visual.v1".into(),
        runtime: "opaque-palette/1".into(),
        role: "theme".into(),
        credit: String::new(),
        dependencies: vec![],
    }
}
#[test]
fn private_profile_rejects_shaders_textures_bad_tokens_and_licenses() {
    let ok = valid();
    assert!(visual::parse(&ok).is_ok());
    visual::validate_license(&visual::parse(&ok).unwrap(), &profile()).unwrap();
    let shader = common::rview_pack("owner.visual-test", "1.0.0", common::OK_SHADER);
    assert!(visual::parse(&shader).is_err());
    let atlas = pack(
        "owner.visual-test",
        "1.0.0",
        "CC0-1.0",
        vec![("sprites.json", "sprite_atlas", b"{\"frames\":{}}".to_vec())],
    );
    assert!(visual::parse(&atlas).is_err());
    for (path, kind, data) in [
        ("theme.json", "theme", serde_json::to_vec(&json!({"tokens":{"--rv-color-accent":"#ffffff"}})).unwrap()),
        ("theme.json", "theme", serde_json::to_vec(&json!({"tokens":{"--rv-world-bg":"#10204080"}})).unwrap()),
        ("theme.json", "theme", serde_json::to_vec(&json!({"tokens":{"--rv-world-bg":"url(#x)"}})).unwrap()),
        ("palette.json", "palette", serde_json::to_vec(&json!({"colors":{"rock":"#123456"}})).unwrap()),
        ("theme/main.json", "theme", serde_json::to_vec(&json!({"tokens":{"--rv-world-bg":"#102040"}})).unwrap()),
    ] {
        assert!(visual::parse(&pack("owner.visual-test", "1.0.0", "CC0-1.0", vec![(path, kind, data)])).is_err());
    }
    let agpl = pack(
        "owner.visual-test",
        "1.0.0",
        "AGPL-3.0-only",
        vec![("theme.json", "theme", serde_json::to_vec(&json!({"tokens":{"--rv-world-bg":"#102040"}})).unwrap())],
    );
    assert!(visual::parse(&agpl).is_ok());
    assert!(visual::validate_license(&visual::parse(&agpl).unwrap(), &profile()).is_err());
    let mut ccby = profile();
    ccby.credit.clear();
    let by = pack(
        "owner.visual-test",
        "1.0.0",
        "CC-BY-4.0",
        vec![("theme.json", "theme", serde_json::to_vec(&json!({"tokens":{"--rv-world-bg":"#102040"}})).unwrap())],
    );
    assert!(visual::validate_license(&visual::parse(&by).unwrap(), &ccby).is_err());
    ccby.credit = "Test credit".into();
    visual::validate_license(&visual::parse(&by).unwrap(), &ccby).unwrap();
    assert!(visual::parse(&vec![0; 65537]).is_err());
    let mut corrupt = ok.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(visual::parse(&corrupt).is_err());
    assert_ne!(
        visual::signing_message(42, "owner.visual-test", "1.0.0", &ok, &profile()),
        rynvek_server::audio_package::signing_message(42, "owner.visual-test", "1.0.0", &ok)
    );
}
#[tokio::test]
async fn signed_sdk_private_discovery_dependencies_and_actual_renderer_theme() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "visual-owner", "human", 191).await;
    let bob = register(&base, "visual-other", "human", 192).await;
    let input = json!({"base":base,"principalId":alice.principal_id.to_string(),"token":alice.token,"testSeed":191});
    let result = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut child = std::process::Command::new("node")
            .arg("packages/agent-sdk/test/private-visual-e2e.mjs")
            .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "Node真实视觉消费失败：{}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    })
    .await
    .unwrap();
    assert_eq!(result["background"].as_u64(), Some(0xFF40_2010));
    assert_eq!(result["factions"][0].as_u64(), Some(0xFF80_10E0));
    assert_eq!(result["factions"][6].as_u64(), Some(0xFF03_0201));
    assert_eq!(result["libraryRevocationEnforced"], true);
    let path = "/api/v1/private-hub/visual/owner.visual-main/1.0.0";
    for suffix in ["", "/artifact", "/resolved"] {
        let url = format!("{base}{path}{suffix}");
        assert_eq!(client::send_json("GET", &url, None, &json!({})).await.unwrap().status, 401);
        assert_eq!(client::send_json("GET", &url, Some(&bob.token), &json!({})).await.unwrap().status, 404);
    }
    let list = client::send_json("GET", &format!("{base}/api/v1/private-hub/visual"), Some(&bob.token), &json!({})).await.unwrap();
    assert_eq!(list.json().unwrap()["packages"], json!([]));
    let cid = result["root"]["artifact_cid"].as_str().unwrap();
    assert_eq!(client::send_json("GET", &format!("{base}/api/v1/hub/blobs/{cid}"), None, &json!({})).await.unwrap().status, 404);
    let public = client::send_json("GET", &format!("{base}/api/v1/hub/packages"), None, &json!({})).await.unwrap();
    assert!(!String::from_utf8_lossy(&public.body).contains("owner.visual"));
    let data = client::send_json("POST", &format!("{base}/api/v1/user/data-export"), Some(&alice.token), &json!({})).await.unwrap();
    assert_eq!(data.json().unwrap()["private_visual"].as_array().unwrap().len(), 2);
    let mut bytes = valid();
    let signature = hex(&alice.key.sign(&visual::signing_message(alice.principal_id, "owner.visual-test", "1.0.0", &bytes, &profile())));
    bytes[64] ^= 1;
    let response = client::send_json(
        "PUT",
        &format!("{base}/api/v1/private-hub/visual/owner.visual-test/1.0.0"),
        Some(&alice.token),
        &json!({"artifact_b64":base64::Engine::encode(&base64::engine::general_purpose::STANDARD,&bytes),"signature":signature,"profile":profile()}),
    )
    .await
    .unwrap();
    assert_ne!(response.status, 201);
    server.shutdown().await;
}
