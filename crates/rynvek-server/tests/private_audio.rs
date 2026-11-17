// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
mod common;
use common::*;
use rynvek_server::{audio_package as audio, client, ids::hex};
use rynvek_world::package::{PackageInput, PackageInputEntry, PackageKind, write_package};
use serde_json::{Value, json};
fn pack(manifest: Value, recipes: Value) -> Vec<u8> {
    pack_samples(manifest, recipes, Vec::new())
}
fn pack_samples(manifest: Value, recipes: Value, samples: Vec<Vec<u8>>) -> Vec<u8> {
    write_package(&PackageInput {
        kind: PackageKind::Rpack,
        id: "owner.audio-test".into(),
        version: "1.0.0".into(),
        name: "有界自有配方".into(),
        license: "CC0-1.0".into(),
        engine_compat: "rynvek-audio-pcm16/2".into(),
        entries: [
            (
                "audio.json",
                json!({"schema":"rynvek.audio-pack.v2","runtime":"pcm16-recipes/2","role":"soundpack"}),
            ),
            ("manifest.json", manifest),
            ("recipes.json", recipes),
            ("dependencies.lock", json!({"lockfile_version":1,"packages":[]})),
        ]
        .into_iter()
        .map(|(path, v)| PackageInputEntry {
            path: path.into(),
            kind: "data".into(),
            media_type: "application/json".into(),
            data: serde_json::to_vec(&v).unwrap(),
        })
        .chain(samples.into_iter().map(|data| PackageInputEntry {
            path: format!("samples/{}.wav", hex(blake3::hash(&data).as_bytes())),
            kind: "data".into(),
            media_type: "application/octet-stream".into(),
            data,
        }))
        .collect(),
    })
    .unwrap()
}
#[test]
fn bounded_data_profile_rejects_code_urls_resources_bus_escape_and_corruption() {
    let empty = json!({"schema":"rynvek.audio-manifest.v1","events":{}});
    let valid = pack(empty.clone(), json!({"building.placed":{"freqStart":733}}));
    assert!(audio::parse(&valid).is_ok());
    for recipes in [
        json!({"building.placed":{"freqStart":8001}}),
        json!({"building.placed":{"attackMs":0}}),
        json!({"building.placed":{"releaseMs":501}}),
        json!({"building.placed":{"filterQ":100}}),
        json!({"building.placed":{"noiseMix":1}}),
        json!({"building.placed":{"bus":"voice_alert"}}),
        json!({"building.placed":{"oscType":"custom"}}),
        json!({"unknown.event":{}}),
        json!({"__proto__":{"freqStart":440}}),
        json!({"building.placed":{"fetch":"https://example.invalid"}}),
    ] {
        assert!(audio::parse(&pack(empty.clone(), recipes)).is_err());
    }
    for event in [
        json!({"status":"placeholder","bus":"voice_alert"}),
        json!({"status":"asset","bus":"sfx_world","src":"../../x.ogg","license":"CC0-1.0"}),
        json!({"status":"placeholder","bus":"sfx_world","max_voices":17}),
    ] {
        assert!(
            audio::parse(&pack(
                json!({"schema":"rynvek.audio-manifest.v1","events":{"building.placed":event}}),
                json!({})
            ))
            .is_err()
        );
    }
    let mut corrupt = valid.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(audio::parse(&corrupt).is_err());
    assert!(audio::parse(&vec![0; 524289]).is_err());
    assert_ne!(
        audio::signing_message(42, "owner.audio-test", "1.0.0", &valid),
        rynvek_server::agent_package::signing_message(42, "owner.audio-test", "1.0.0", &valid)
    );
}
#[tokio::test]
async fn signed_sdk_private_discovery_dependencies_and_actual_arena_audio_consumption() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "audio-owner", "human", 181).await;
    let bob = register(&base, "audio-other", "human", 182).await;
    let created = client::send_json(
        "POST",
        &format!("{base}/api/v1/worlds"),
        Some(&alice.token),
        &json!({"title":"私有音效真实事件快验","template":"arena_pve","map_seed":73,"visibility":"Private"}),
    )
    .await
    .unwrap();
    assert_eq!(created.status, 201);
    let world = created.json().unwrap()["world_id"].as_str().unwrap().to_string();
    let opened = client::send_json(
        "POST",
        &format!("{base}/api/v1/worlds/{world}/arena"),
        Some(&alice.token),
        &json!({}),
    )
    .await
    .unwrap();
    assert_eq!(opened.status, 200);
    let world_id = world.parse().unwrap();
    let row = server.state.store().get_world(world_id).await.unwrap().unwrap();
    let runtime = rynvek_server::transactions::restore_committed(
        server.state.store(),
        server.state.inner.world_snapshots.as_ref(),
        &row,
    )
    .await
    .unwrap();
    let origin = (0..runtime.map.height)
        .flat_map(|y| (0..runtime.map.width).map(move |x| (x, y)))
        .find(|o| {
            runtime
                .validate_placement(rynvek_rts::FactionId(0), rynvek_rts::BuildingKind::SignalBeacon, *o)
                .is_ok()
        })
        .unwrap();
    let host = server.state.registry().by_world(&world_id).unwrap();
    let driver = tokio::spawn(async move {
        for _ in 0..900 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let _ = host.advance(1).await;
        }
    });
    let input = json!({"base":base,"principalId":alice.principal_id.to_string(),"token":alice.token,"testSeed":181,"world":world,"origin":origin});
    let result = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut child = std::process::Command::new("node")
            .arg("packages/agent-sdk/test/private-audio-e2e.mjs")
            .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "Node真实消费失败：{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    })
    .await
    .unwrap();
    driver.abort();
    let _ = driver.await;
    assert_eq!(result["sampleFrames"], 1280);
    assert_eq!(result["sampleRate"], 16000);
    assert_eq!(result["actualPan"], 0.25);
    assert_eq!(result["ack"], "building_placed");
    assert_eq!(result["libraryRevocationEnforced"], true);
    let path = "/api/v1/private-hub/audio/owner.audio-main/1.0.0";
    for suffix in ["", "/artifact", "/resolved"] {
        let url = format!("{base}{path}{suffix}");
        assert_eq!(
            client::send_json("GET", &url, None, &json!({})).await.unwrap().status,
            401
        );
        assert_eq!(
            client::send_json("GET", &url, Some(&bob.token), &json!({}))
                .await
                .unwrap()
                .status,
            404
        );
    }
    let list = client::send_json(
        "GET",
        &format!("{base}/api/v1/private-hub/audio"),
        Some(&bob.token),
        &json!({}),
    )
    .await
    .unwrap();
    assert_eq!(list.json().unwrap()["packages"], json!([]));
    let cid = result["root"]["artifact_cid"].as_str().unwrap();
    assert_eq!(
        client::send_json("GET", &format!("{base}/api/v1/hub/blobs/{cid}"), None, &json!({}))
            .await
            .unwrap()
            .status,
        404
    );
    let public = client::send_json("GET", &format!("{base}/api/v1/hub/packages"), None, &json!({}))
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&public.body).contains("owner.audio"));
    let data = client::send_json(
        "POST",
        &format!("{base}/api/v1/user/data-export"),
        Some(&alice.token),
        &json!({}),
    )
    .await
    .unwrap();
    assert_eq!(data.json().unwrap()["private_audio"].as_array().unwrap().len(), 2);
    let mut bytes = pack(json!({"schema":"rynvek.audio-manifest.v1","events":{}}), json!({}));
    let signature = hex(&alice.key.sign(&audio::signing_message(
        alice.principal_id,
        "owner.audio-test",
        "1.0.0",
        &bytes,
    )));
    bytes[64] ^= 1;
    let response=client::send_json("PUT",&format!("{base}/api/v1/private-hub/audio/owner.audio-test/1.0.0"),Some(&alice.token),&json!({"artifact_b64":base64::Engine::encode(&base64::engine::general_purpose::STANDARD,&bytes),"signature":signature})).await.unwrap();
    assert_ne!(response.status, 201);
    server.shutdown().await;
}

#[test]
fn pcm_profile_rejects_malformed_compressed_stereo_chunks_and_missing_credits() {
    let mut wav = vec![0u8; 76];
    wav[..4].copy_from_slice(b"RIFF");
    wav[4..8].copy_from_slice(&68u32.to_le_bytes());
    wav[8..16].copy_from_slice(b"WAVEfmt ");
    wav[16..20].copy_from_slice(&16u32.to_le_bytes());
    wav[20..22].copy_from_slice(&1u16.to_le_bytes());
    wav[22..24].copy_from_slice(&1u16.to_le_bytes());
    wav[24..28].copy_from_slice(&8000u32.to_le_bytes());
    wav[28..32].copy_from_slice(&16000u32.to_le_bytes());
    wav[32..34].copy_from_slice(&2u16.to_le_bytes());
    wav[34..36].copy_from_slice(&16u16.to_le_bytes());
    wav[36..40].copy_from_slice(b"data");
    wav[40..44].copy_from_slice(&32u32.to_le_bytes());
    let path = format!("samples/{}.wav", hex(blake3::hash(&wav).as_bytes()));
    let event = json!({"status":"asset","bus":"sfx_world","src":path,"license":"CC0-1.0"});
    let manifest = json!({"schema":"rynvek.audio-manifest.v1","events":{"building.placed":event}});
    let parsed = audio::parse(&pack_samples(manifest.clone(), json!({}), vec![wav.clone()])).unwrap();
    assert_eq!(parsed.samples[&path], wav);
    assert_eq!(audio::pcm_frames(&wav).unwrap(), 16);
    for (offset, value) in [(20, 3), (22, 2), (34, 32), (40, 255), (4, 0), (16, 18)] {
        let mut bad = wav.clone();
        bad[offset] = value;
        assert!(audio::pcm_frames(&bad).is_err());
        assert!(audio::parse(&pack_samples(manifest.clone(), json!({}), vec![bad])).is_err());
    }
    let mut extra = wav.clone();
    extra.extend_from_slice(b"JUNK0000");
    assert!(audio::pcm_frames(&extra).is_err());
    let mut by = manifest.clone();
    by["events"]["building.placed"]["license"] = json!("CC-BY-4.0");
    assert!(audio::parse(&pack_samples(by.clone(), json!({}), vec![wav.clone()])).is_err());
    by["events"]["building.placed"]["credit"] = json!("self generated test");
    assert!(audio::parse(&pack_samples(by, json!({}), vec![wav])).is_ok());
}
