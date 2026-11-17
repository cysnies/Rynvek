// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! HTTP 路由集成测试：AGPL 源码要约、SBOM、健康检查、登录、权限与错误码、世界 / Hub / Lab / 账户路由。

mod common;

use base64::Engine as _;
use common::*;
use rynvek_server::client::{self, HttpResponse};
use serde_json::{Value, json};

async fn get(base: &str, path: &str, token: Option<&str>) -> HttpResponse {
    client::get(&format!("{base}{path}"), token).await.unwrap()
}

async fn send(base: &str, method: &str, path: &str, token: Option<&str>, body: Value) -> HttpResponse {
    client::send_json(method, &format!("{base}{path}"), token, &body)
        .await
        .unwrap()
}

fn assert_error(r: &HttpResponse, status: u16, code: u16) {
    assert_eq!(r.status, status, "{}", String::from_utf8_lossy(&r.body));
    let v = r.json().unwrap();
    assert_eq!(v["code"], code, "{v}");
    assert_eq!(v["code_hex"], format!("0x{code:04X}"));
    assert!(v["name"].as_str().is_some_and(|n| !n.is_empty()));
}

#[tokio::test]
async fn source_offer_points_at_exact_commit_and_sbom() {
    let server = start_server(false).await;
    let base = server.base_url();
    let r = get(&base, "/api/v1/meta/source-offer", None).await;
    assert_eq!(r.status, 200);
    let v = r.json().unwrap();
    assert_eq!(v["license"], "AGPL-3.0-only");
    assert_eq!(v["name"], "rynvek-server");
    assert_eq!(v["repository"], "https://github.com/rynvek/rynvek");
    let commit = v["git_commit"].as_str().unwrap();
    assert_eq!(commit.len(), 40);
    assert!(commit.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(
        v["source_tarball_url"],
        format!("https://github.com/rynvek/rynvek/archive/{commit}.tar.gz")
    );
    let sbom_url = v["sbom_url"].as_str().unwrap();
    assert_eq!(sbom_url, format!("{base}/api/v1/meta/sbom.spdx.json"));
    assert_eq!(v["sbom_format"], "SPDX-2.3+json");
    assert!(v["agpl_section"].as_str().unwrap().contains("13"));
    // 无需登录即可获取 SBOM，且其中声明了本服务端与依赖
    let s = client::get(sbom_url, None).await.unwrap();
    assert_eq!(s.status, 200);
    assert_eq!(s.header("content-type"), Some("application/spdx+json"));
    let sbom = s.json().unwrap();
    assert_eq!(sbom["spdxVersion"], "SPDX-2.3");
    let pkgs = sbom["packages"].as_array().unwrap();
    let me = pkgs.iter().find(|p| p["name"] == "rynvek-server").unwrap();
    assert_eq!(me["licenseDeclared"], "AGPL-3.0-only");
    assert!(pkgs.iter().any(|p| p["name"] == "axum"));
    server.shutdown().await;
}

#[tokio::test]
async fn health_and_unknown_routes() {
    let server = start_server(false).await;
    let base = server.base_url();
    let v = get(&base, "/api/v1/meta/health", None).await.json().unwrap();
    assert_eq!(v["status"], "ok");
    assert_eq!(v["gpu_tier"], "cpu_reference");
    assert_eq!(v["active_worlds"], 0);
    assert_error(&get(&base, "/api/v1/nope", None).await, 404, 0x2002);
    // 规范 11.3 的路由表恰好 20 条且编号连续
    let routes = rynvek_server::api::ROUTES;
    for (i, (n, _, path)) in routes.iter().enumerate() {
        assert_eq!(*n, format!("{:02}", i + 1));
        assert!(path.starts_with("/api/v1/"));
    }
    server.shutdown().await;
}

#[tokio::test]
async fn auth_flow_scopes_and_error_codes() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "alice", "human", 1).await;
    assert_eq!(alice.token_bytes().len(), 96);

    // 缺少 / 损坏 / 方案错误的令牌 → 0x8001 / 401
    assert_error(&get(&base, "/api/v1/worlds", None).await, 401, 0x8001);
    assert_error(&get(&base, "/api/v1/worlds", Some("AAAA")).await, 401, 0x8001);
    let mut forged = alice.token_bytes();
    forged[40] ^= 1;
    let forged = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(forged);
    assert_error(&get(&base, "/api/v1/worlds", Some(&forged)).await, 401, 0x8001);

    // 错误签名 → 0x9001
    let url = "/api/v1/auth/session";
    let c = send(
        &base,
        "POST",
        url,
        None,
        json!({ "principal_id": alice.principal_id.to_string() }),
    )
    .await
    .json()
    .unwrap();
    let challenge = c["challenge"].as_str().unwrap().to_owned();
    let bad = send(
        &base,
        "POST",
        url,
        None,
        json!({ "principal_id": alice.principal_id.to_string(), "challenge": challenge, "signature": "00".repeat(64) }),
    )
    .await;
    assert_eq!(bad.error_code(), Some(0x9001));
    // OAuth 未配置
    assert_error(
        &send(&base, "POST", url, None, json!({ "oauth_token": "x" })).await,
        401,
        0x8001,
    );
    // 请求体不合契约 → 0x5001
    assert_error(
        &send(&base, "POST", url, None, json!({ "bogus": 1 })).await,
        400,
        0x5001,
    );
    // 二进制令牌
    let (pid, _) = client::login(&base, &alice.key, client::LoginTarget::Existing(alice.principal_id))
        .await
        .unwrap();
    assert_eq!(pid, alice.principal_id);
    // 重复 handle → 冲突；service 不可自注册
    let dup = client::login(
        &base,
        &rynvek_security::ServerSigningKey::from_seed(0, [9; 32]),
        client::LoginTarget::Register {
            handle: "alice",
            kind: "human",
        },
    )
    .await;
    assert!(dup.is_err());
    let svc = client::login(
        &base,
        &rynvek_security::ServerSigningKey::from_seed(0, [10; 32]),
        client::LoginTarget::Register {
            handle: "svc",
            kind: "service",
        },
    )
    .await;
    assert!(svc.unwrap_err().contains("0x8001"));

    // Agent 主体缺省没有 hub.publish / account.owner
    let bot = register(&base, "bot", "agent", 2).await;
    let r = send(
        &base,
        "PUT",
        "/api/v1/hub/packages/bot.pkg/1.0.0",
        Some(&bot.token),
        json!({}),
    )
    .await;
    assert_error(&r, 403, 0x8001);
    assert_error(
        &send(&base, "POST", "/api/v1/user/data-export", Some(&bot.token), json!({})).await,
        403,
        0x8001,
    );
    // 未知赛道 → 0x5001；规则集不符 → 0x9003
    let q = "/api/v1/matches/queue";
    assert_error(
        &send(
            &base,
            "POST",
            q,
            Some(&alice.token),
            json!({ "league": "chess", "ruleset_hash": ruleset_hex() }),
        )
        .await,
        400,
        0x5001,
    );
    assert_error(
        &send(
            &base,
            "POST",
            q,
            Some(&alice.token),
            json!({ "league": "human", "ruleset_hash": "11".repeat(32) }),
        )
        .await,
        409,
        0x9003,
    );
    // Agent 不能进入人类赛道
    assert_eq!(
        send(
            &base,
            "POST",
            q,
            Some(&bot.token),
            json!({ "league": "human", "ruleset_hash": ruleset_hex() })
        )
        .await
        .error_code(),
        Some(0x8001)
    );
    server.shutdown().await;
}

#[tokio::test]
async fn worlds_commits_fork_and_patches() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "alice", "human", 1).await;
    let eve = register(&base, "eve", "human", 3).await;
    let t = Some(alice.token.as_str());

    let r = send(
        &base,
        "POST",
        "/api/v1/worlds",
        t,
        json!({ "title": "沙盒", "template": "arena_duel" }),
    )
    .await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let w = r.json().unwrap();
    let wid = w["world_id"].as_str().unwrap().to_owned();
    assert_eq!(w["latest_revision"], 0);
    assert_eq!(w["visibility"], "Private");
    // 非 JSON 创建（.rworld 上传）→ 0x7001
    let raw = client::request(
        "POST",
        &format!("{base}/api/v1/worlds"),
        &[
            ("Authorization", &format!("Bearer {}", alice.token)),
            ("Content-Type", "application/octet-stream"),
        ],
        Some(b"RYWD"),
    )
    .await
    .unwrap();
    assert_error(&raw, 400, 0x7001);

    // 私有世界对其他人不可见（列表过滤；直接访问与 "不存在" 同为 0x2002，防枚举）
    let list = get(&base, "/api/v1/worlds", Some(&eve.token)).await.json().unwrap();
    assert!(list["worlds"].as_array().unwrap().is_empty());
    let commits_path = format!("/api/v1/worlds/{wid}/commits");
    assert_error(&get(&base, &commits_path, Some(&eve.token)).await, 404, 0x2002);
    let commits = get(&base, &commits_path, t).await.json().unwrap();
    assert_eq!(commits["commits"].as_array().unwrap().len(), 1);

    // 补丁：Class 1 合法
    let patch = |rev: u64, class: &str, op: Value| {
        json!({
            "patch_id": [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16],
            "base_authoring_revision": rev,
            "hlc": { "wall_time_ms": 1, "logical_counter": 0, "author_node_id": 7 },
            "safety_class": class,
            "operations": [op],
        })
    };
    let econ = json!({ "SetWorldParam": { "key_path": "economy.grant_biomass_milli", "new_value_json": "1000" } });
    let p = format!("/api/v1/worlds/{wid}/patches");
    let r = send(&base, "POST", &p, t, patch(0, "TickBoundarySafe", econ.clone())).await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let v = r.json().unwrap();
    assert_eq!(v["status"], "applied");
    assert_eq!(v["world_revision"], 1);
    assert_eq!(v["required_class"], "TickBoundarySafe");
    // 修订过期 → 0x2005
    assert_error(
        &send(&base, "POST", &p, t, patch(0, "TickBoundarySafe", econ.clone())).await,
        409,
        0x2005,
    );
    // 声明等级低于所需 → 0x3003
    assert_error(
        &send(&base, "POST", &p, t, patch(1, "HotSafePresentation", econ.clone())).await,
        422,
        0x3003,
    );
    // 格式错误 → 0x3003
    assert_error(&send(&base, "POST", &p, t, json!({ "x": 1 })).await, 422, 0x3003);
    // 未接通的重启参数拒绝，不创建只记账不执行的假分叉。
    let core = json!({ "SetWorldParam": { "key_path": "map.size", "new_value_json": "\"large\"" } });
    assert_error(
        &send(&base, "POST", &p, t, patch(1, "RequiresForkRestart", core)).await,
        422,
        0x5006,
    );

    // 显式分叉（他人对私有世界无权）
    let f = format!("/api/v1/worlds/{wid}/fork");
    assert_error(&send(&base, "POST", &f, Some(&eve.token), json!({})).await, 404, 0x2002);
    let r = send(&base, "POST", &f, t, json!({ "world_revision": 1, "title": "分支" })).await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let fork = r.json().unwrap();
    assert_eq!(fork["parent_world_id"], wid);
    assert_eq!(fork["forked_at_revision"], 1);
    let fork_path = format!("/api/v1/worlds/{}/patches", fork["world_id"].as_str().unwrap());
    let applied = send(&base, "POST", &fork_path, t, patch(0, "TickBoundarySafe", econ.clone())).await;
    assert_eq!(
        applied.status,
        200,
        "分叉检查点必须重新绑定身份后可继续：{}",
        String::from_utf8_lossy(&applied.body)
    );
    // 不存在的世界 / 非法 UUID → 0x2002
    assert_error(
        &get(&base, "/api/v1/worlds/0190a000-0000-7000-8000-000000000000/commits", t).await,
        404,
        0x2002,
    );
    assert_error(&get(&base, "/api/v1/worlds/not-a-uuid/commits", t).await, 404, 0x2002);
    server.shutdown().await;
}

fn sign_package(user: &User, id: &str, ver: &str, artifact: &[u8]) -> String {
    let msg = rynvek_server::api::hub::package_signing_message(id, ver, artifact);
    rynvek_security::to_hex(&user.key.sign(&msg))
}

#[tokio::test]
async fn hub_publish_lockfile_and_ranged_blob() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "alice", "human", 1).await;
    let t = Some(alice.token.as_str());
    let org = alice.principal_id.to_string();
    let artifact = rview_pack("alice.theme", "1.0.0", OK_SHADER);
    let body = |sig: String, lock: Value, art: &[u8]| {
        json!({
            "org_id": org,
            "package_kind": "rview",
            "min_engine_version": "0.1.0",
            "license_spdx": "AGPL-3.0-only",
            "artifact_b64": base64::engine::general_purpose::STANDARD.encode(art),
            "signature": sig,
            "manifest": { "name": "theme" },
            "lockfile": lock,
        })
    };
    let empty_lock = json!({ "lockfile_version": 1, "packages": [] });
    let path = "/api/v1/hub/packages/alice.theme/1.0.0";
    // 签名错误 → 0x9001
    let r = send(
        &base,
        "PUT",
        path,
        t,
        body("00".repeat(64), empty_lock.clone(), &artifact),
    )
    .await;
    assert_error(&r, 400, 0x9001);
    let sig = sign_package(&alice, "alice.theme", "1.0.0", &artifact);
    let r = send(&base, "PUT", path, t, body(sig.clone(), empty_lock.clone(), &artifact)).await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let published = r.json().unwrap();
    // 静态扫描通过：社区包授予 TrustLevel 2，报告着色器指令数
    assert_eq!(published["verification"]["static_scan"], "passed");
    assert_eq!(published["trust_level"], 2);
    assert_eq!(published["verification"]["findings"][0]["budget"], 2048);
    let cid = published["artifact_cid_blake3"].as_str().unwrap().to_owned();
    assert_eq!(cid, blake3::hash(&artifact).to_hex().to_string());
    // 版本不可覆盖 → 0x9003
    assert_error(
        &send(&base, "PUT", path, t, body(sig, empty_lock, &artifact)).await,
        409,
        0x9003,
    );

    // 依赖锁：引用不一致的哈希 → 0x9003；正确 → 201
    let art2 = rview_pack("alice.child", "0.1.0", OK_SHADER);
    let p2 = "/api/v1/hub/packages/alice.child/0.1.0";
    let sig2 = sign_package(&alice, "alice.child", "0.1.0", &art2);
    let bad_lock = json!({ "lockfile_version": 1, "packages": [
        { "id": "alice.theme", "version": "1.0.0", "kind": "rview", "blake3_256": "ab".repeat(32) }
    ]});
    assert_error(
        &send(&base, "PUT", p2, t, body(sig2.clone(), bad_lock, &art2)).await,
        409,
        0x9003,
    );
    let good_lock = json!({ "lockfile_version": 1, "packages": [
        { "id": "alice.theme", "version": "1.0.0", "kind": "rview", "blake3_256": format!("blake3:{cid}") }
    ]});
    assert_eq!(
        send(&base, "PUT", p2, t, body(sig2, good_lock, &art2)).await.status,
        201
    );

    // 他人组织 → 0x8001
    let eve = register(&base, "eve", "human", 3).await;
    let sig3 = sign_package(&eve, "eve.x", "1.0.0", &artifact);
    let r = send(
        &base,
        "PUT",
        "/api/v1/hub/packages/eve.x/1.0.0",
        Some(&eve.token),
        body(sig3, json!({ "lockfile_version": 1 }), &artifact),
    )
    .await;
    assert_error(&r, 403, 0x8001);

    // 检索
    let s = get(&base, "/api/v1/hub/packages?kind=rview&q=theme", None)
        .await
        .json()
        .unwrap();
    let pkgs = s["packages"].as_array().unwrap();
    assert_eq!(pkgs.len(), 1);
    assert_eq!(pkgs[0]["trust_level"], 2);

    // Blob 与 Range
    let blob = format!("/api/v1/hub/blobs/{cid}");
    let full = get(&base, &blob, None).await;
    assert_eq!(full.status, 200);
    assert_eq!(full.body, artifact);
    assert_eq!(full.header("accept-ranges"), Some("bytes"));
    assert_eq!(full.header("etag"), Some(format!("\"{cid}\"").as_str()));
    let ranged = |range: &'static str| {
        let url = format!("{base}{blob}");
        async move { client::request("GET", &url, &[("Range", range)], None).await.unwrap() }
    };
    let r = ranged("bytes=0-5").await;
    assert_eq!(r.status, 206);
    assert_eq!(r.body, artifact[..6]);
    assert_eq!(
        r.header("content-range"),
        Some(format!("bytes 0-5/{}", artifact.len()).as_str())
    );
    let r = ranged("bytes=-4").await;
    assert_eq!(r.body, artifact[artifact.len() - 4..]);
    assert_eq!(ranged("bytes=999-").await.status, 416);
    assert_error(
        &get(&base, &format!("/api/v1/hub/blobs/{}", "00".repeat(32)), None).await,
        404,
        0x2002,
    );
    server.shutdown().await;
}

/// 轮询路由 18 直到产出就绪（未就绪时为 404 / 0x2002）
async fn wait_parquet(base: &str, path: &str, token: Option<&str>) -> Vec<u8> {
    for _ in 0..600 {
        let r = get(base, path, token).await;
        if r.status == 200 {
            return r.body;
        }
        assert_error(&r, 404, 0x2002);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("实验 30 秒内未完成");
}

#[tokio::test]
async fn lab_and_account_lifecycle() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "alice", "human", 1).await;
    let t = Some(alice.token.as_str());
    let r = send(
        &base,
        "POST",
        "/api/v1/lab/experiments",
        t,
        json!({ "name": "种子扫描", "seeds": [1, 2, 3], "ticks": 600 }),
    )
    .await;
    assert_eq!(r.status, 202);
    let exp = r.json().unwrap();
    assert_eq!(exp["status"], "queued");
    let id = exp["exp_id"].as_str().unwrap();
    assert_error(
        &send(
            &base,
            "POST",
            "/api/v1/lab/experiments",
            t,
            json!({ "name": "x", "seeds": [], "ticks": 1 }),
        )
        .await,
        400,
        0x5001,
    );
    // 后台执行器完成后可下载三张表；他人不可见（与不存在同为 0x2002）
    let pq = format!("/api/v1/lab/experiments/{id}/parquet");
    let metrics = wait_parquet(&base, &pq, t).await;
    assert_eq!(&metrics[..4], b"PAR1");
    for table in ["runs", "lineage"] {
        let r = get(&base, &format!("{pq}?table={table}"), t).await;
        assert_eq!(r.status, 200);
        assert_eq!(&r.body[..4], b"PAR1");
    }
    assert_error(&get(&base, &format!("{pq}?table=nope"), t).await, 400, 0x5001);
    let eve = register(&base, "eve", "human", 3).await;
    assert_error(&get(&base, &pq, Some(&eve.token)).await, 404, 0x2002);

    // 数据导出
    let r = send(&base, "POST", "/api/v1/user/data-export", t, json!({})).await;
    assert_eq!(r.status, 200);
    let archive = r.json().unwrap();
    assert_eq!(archive["format"], "rynvek-data-export/1");
    assert_eq!(archive["principal"]["handle"], "alice");
    assert_eq!(archive["experiments"].as_array().unwrap().len(), 1);
    assert!(!archive["audit_logs"].as_array().unwrap().is_empty());

    // 注销：令牌立即失效，无法再登录
    let r = client::request(
        "DELETE",
        &format!("{base}/api/v1/user/account"),
        &[("Authorization", &format!("Bearer {}", alice.token))],
        None,
    )
    .await
    .unwrap();
    assert_eq!(r.status, 202);
    let v = r.json().unwrap();
    assert_eq!(v["tokens_revoked"], true);
    assert_eq!(
        v["cold_purge_deadline_unix_s"].as_u64().unwrap() - v["hot_purge_deadline_unix_s"].as_u64().unwrap(),
        29 * 24 * 3600
    );
    assert_error(&get(&base, "/api/v1/worlds", t).await, 401, 0x8001);
    assert!(
        client::login(&base, &alice.key, client::LoginTarget::Existing(alice.principal_id))
            .await
            .is_err()
    );
    server.shutdown().await;
}

#[tokio::test]
async fn ryn1_http_patch_commits_real_simulation_or_rejects_without_changes() {
    let server = start_server(false).await;
    let base = server.base_url();
    let alice = register(&base, "ryn1-owner", "human", 51).await;
    let eve = register(&base, "ryn1-outsider", "human", 52).await;
    let t = Some(alice.token.as_str());
    let created = send(
        &base,
        "POST",
        "/api/v1/worlds",
        t,
        json!({"title":"权威链回归", "template":"arena_duel", "map_seed":42}),
    )
    .await
    .json()
    .unwrap();
    let wid = created["world_id"].as_str().unwrap();
    let path = format!("/api/v1/worlds/{wid}/patches");
    let commits_path = format!("/api/v1/worlds/{wid}/commits");
    let paint = json!({"PaintTerrainRect":{"x0":100,"y0":100,"width":2,"height":2,"tile_kind":4}});
    let patch = |ops: Vec<Value>| {
        json!({"patch_id":vec![1;16],"base_authoring_revision":0,
        "hlc":{"wall_time_ms":1,"logical_counter":0,"author_node_id":1},
        "safety_class":"TickBoundarySafe","operations":ops})
    };
    // 无权用户连世界存在性都不应获知，更不能触发推进。
    assert_error(
        &send(&base, "POST", &path, Some(&eve.token), patch(vec![paint.clone()])).await,
        404,
        0x2002,
    );
    let before = get(&base, &commits_path, t).await.json().unwrap();
    let unsupported = json!({"SetWorldParam":{"key_path":"field.diffusion","new_value_json":"1"}});
    let rejected = send(&base, "POST", &path, t, patch(vec![paint.clone(), unsupported])).await;
    assert!(
        rejected.status >= 400,
        "不能把未实现补丁排队当作成功：{}",
        rejected.status
    );
    assert_eq!(get(&base, &commits_path, t).await.json().unwrap(), before);
    let result = send(&base, "POST", &path, t, patch(vec![paint])).await;
    assert_eq!(result.status, 200, "{}", String::from_utf8_lossy(&result.body));
    let applied = result.json().unwrap();
    assert_eq!(applied["status"], "applied");
    assert_eq!(applied["effective_tick"], 1);
    let patch_cid = applied["patch_cid"].as_str().unwrap();
    assert_eq!(
        get(&base, &format!("/api/v1/hub/blobs/{patch_cid}"), None).await.status,
        404
    );
    let after = get(&base, &commits_path, t).await.json().unwrap();
    assert_eq!(after["latest_revision"], 1);
    assert_ne!(
        after["commits"][1]["state_root_blake3"],
        before["commits"][0]["state_root_blake3"]
    );
    assert!(after["latest_snapshot_cid"].is_string());
    let world_id: rynvek_protocol::WireUuid = wid.parse().unwrap();
    let commits = server.state.store().list_commits(world_id).await.unwrap();
    let latest = commits.last().unwrap();
    let cid = latest.snapshot_cid.unwrap();
    let bytes = server
        .state
        .inner
        .world_snapshots
        .get(&rynvek_core::Blake3Hash(cid))
        .unwrap();
    let mut restored = rynvek_engine::ArenaRuntime::restore(world_id.0, 1, latest.state_root_blake3, &bytes).unwrap();
    assert_eq!(
        restored.world().map.tile(100, 100),
        rynvek_rts::TerrainTileKind::ImperviousWall
    );
    // 和通过引擎公开接口执行的相同初态、同一补丁、同一边界逐根对照。
    let mut engine = rynvek_engine::ArenaRuntime::new(
        world_id.0,
        rynvek_rts::ArenaWorld::new(rynvek_rts::MatchConfig::duel(42)),
    );
    let operation: rynvek_world::WorldPatch = serde_json::from_value(patch(vec![
        json!({"PaintTerrainRect":{"x0":100,"y0":100,"width":2,"height":2,"tile_kind":4}}),
    ]))
    .unwrap();
    engine
        .transact_at(1, |arena| operation.apply_at_boundary(arena, 1))
        .unwrap();
    assert_eq!(engine.state_root(), latest.state_root_blake3);
    for _ in 0..4 {
        engine.step().unwrap();
        restored.step().unwrap();
        assert_eq!(engine.state_root(), restored.state_root());
    }
    // 知道 CID 也不能从 Public Hub 获取包含敌方隐藏状态的完整快照。
    assert_eq!(
        get(
            &base,
            &format!("/api/v1/hub/blobs/{}", rynvek_server::ids::hex(&cid)),
            None
        )
        .await
        .status,
        404
    );
    let next_patch = |kind| {
        json!({"patch_id":vec![kind;16],"base_authoring_revision":1,
        "hlc":{"wall_time_ms":2,"logical_counter":0,"author_node_id":1},
        "safety_class":"TickBoundarySafe","operations":[{"PaintTerrainRect":{"x0":101,"y0":101,"width":1,"height":1,"tile_kind":kind}}]})
    };
    let (one, two) = tokio::join!(
        send(&base, "POST", &path, t, next_patch(1)),
        send(&base, "POST", &path, t, next_patch(2))
    );
    let mut statuses = [one.status, two.status];
    statuses.sort();
    assert_eq!(statuses, [200, 409], "并发补丁只能发布一个前驱修订");
    // 提交层也必须拒绝跳修订和时钟倒退，不能只依靠路由的先检查。
    let stable = server.state.store().get_world(world_id).await.unwrap().unwrap();
    let stable_commits = server.state.store().list_commits(world_id).await.unwrap();
    let mut bad = stable_commits.last().unwrap().clone();
    bad.parent_revision = Some(stable.latest_revision);
    bad.world_revision = stable.latest_revision + 2;
    assert!(
        server
            .state
            .store()
            .append_commit(bad.clone(), stable.fencing_token)
            .await
            .is_err()
    );
    bad.world_revision = stable.latest_revision + 1;
    bad.tick = stable.latest_tick - 1;
    assert!(
        server
            .state
            .store()
            .append_commit(bad, stable.fencing_token)
            .await
            .is_err()
    );
    assert_eq!(server.state.store().get_world(world_id).await.unwrap().unwrap(), stable);
    // 旧世界无检查点时拒绝迁移；有活动宿主租约时拒绝创建旁路权威。
    for leased in [false, true] {
        let mut old = stable.clone();
        old.world_id = rynvek_server::ids::new_uuid_v7();
        old.latest_revision = 0;
        old.latest_tick = 0;
        old.lease_holder_node = leased.then(|| "existing-host".into());
        let mut initial = stable_commits[0].clone();
        initial.world_id = old.world_id;
        initial.snapshot_cid = None;
        server.state.store().insert_world(old.clone(), initial).await.unwrap();
        let rejected = send(
            &base,
            "POST",
            &format!("/api/v1/worlds/{}/patches", old.world_id),
            t,
            patch(vec![
                json!({"PaintTerrainRect":{"x0":100,"y0":100,"width":1,"height":1,"tile_kind":4}}),
            ]),
        )
        .await;
        assert!(rejected.status >= 400);
        assert_eq!(
            server.state.store().get_world(old.world_id).await.unwrap().unwrap(),
            old
        );
    }
    server.shutdown().await;
}

#[tokio::test]
async fn ryn1_private_checkpoint_survives_host_recreation() {
    async fn boot(store: rynvek_server::store::Store, dir: &std::path::Path) -> rynvek_server::RunningServer {
        rynvek_server::start(rynvek_server::ServerConfig {
            http_addr: "127.0.0.1:0".parse().unwrap(),
            webtransport_addr: None,
            app: rynvek_server::app::AppConfig {
                blob_dir: Some(dir.to_owned()),
                tick_driver: rynvek_server::host::TickDriver::Manual,
                lobby_timeout: None,
                ..Default::default()
            },
            store,
            signer: common::server_key(),
        })
        .await
        .unwrap()
    }
    let dir = tempfile::TempDir::new().unwrap();
    let store = rynvek_server::store::Store::memory();
    let first = boot(store.clone(), dir.path()).await;
    let base = first.base_url();
    let owner = register(&base, "restart-owner", "human", 71).await;
    let t = Some(owner.token.as_str());
    let created = send(
        &base,
        "POST",
        "/api/v1/worlds",
        t,
        json!({"title":"恢复","template":"arena_duel","map_seed":42}),
    )
    .await
    .json()
    .unwrap();
    let id: rynvek_protocol::WireUuid = created["world_id"].as_str().unwrap().parse().unwrap();
    let patch = |rev| {
        json!({"patch_id":vec![8;16],"base_authoring_revision":rev,
        "hlc":{"wall_time_ms":1,"logical_counter":0,"author_node_id":1},"safety_class":"EpochBoundarySafe",
        "operations":[{"PaintTerrainRect":{"x0":100,"y0":100,"width":1,"height":1,"tile_kind":4}}]})
    };
    let path = format!("/api/v1/worlds/{id}/patches");
    let applied = send(&base, "POST", &path, t, patch(0)).await;
    assert_eq!(applied.status, 200);
    assert_eq!(applied.json().unwrap()["effective_tick"], 4);
    let old = store.get_world(id).await.unwrap().unwrap();
    let commits = store.list_commits(id).await.unwrap();
    let cid = commits.last().unwrap().snapshot_cid.unwrap();
    let bytes = first
        .state
        .inner
        .world_snapshots
        .get(&rynvek_core::Blake3Hash(cid))
        .unwrap();
    let mut expected = rynvek_engine::ArenaRuntime::restore(id.0, 1, old.latest_state_root, &bytes).unwrap();
    first.shutdown().await;
    let second = boot(store.clone(), dir.path()).await;
    assert_eq!(
        second
            .state
            .inner
            .world_snapshots
            .get(&rynvek_core::Blake3Hash(cid))
            .unwrap(),
        bytes
    );
    let operation: rynvek_world::WorldPatch = serde_json::from_value(patch(1)).unwrap();
    expected
        .transact_at(8, |world| operation.apply_at_boundary(world, 8))
        .unwrap();
    let applied = send(&second.base_url(), "POST", &path, t, patch(1)).await;
    assert_eq!(applied.status, 200, "{}", String::from_utf8_lossy(&applied.body));
    let current = store.get_world(id).await.unwrap().unwrap();
    assert_eq!(current.latest_tick, 8);
    assert_eq!(current.latest_revision, 2);
    assert_eq!(current.latest_state_root, expected.state_root());
    second.shutdown().await;
}
