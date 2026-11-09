// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! PostgreSQL 17 存储一致性测试：不依赖任何外部已启动的实例。
//!
//! 实例来源（按顺序）：
//! 1. `RYNVEK_TEST_POSTGRES_URL`：使用给定的数据库（调用方保证可写、可丢弃）；
//! 2. 自动拉起一次性实例：在 `RYNVEK_PG_BIN`、`PATH`、`/usr/lib/postgresql/*/bin`、`/usr/local/pgsql/bin`
//!    中寻找 `initdb` / `pg_ctl`，于临时目录 `initdb` 后以 Unix 套接字 + 随机回环端口启动，测试结束即停机删除；
//! 3. 都找不到时跳过并打印原因（核心路径已由内存实现覆盖）；设置 `RYNVEK_REQUIRE_POSTGRES=1` 则改为失败。
//!
//! 每个测试都同时在内存与 PostgreSQL 两种后端上执行同一组断言，确保行为一致。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::arena::{Transport, full_match};
use rynvek_protocol::WireUuid;
use rynvek_security::{League, PrincipalKindCode, PrincipalStatus, Visibility};
use rynvek_server::ids::new_uuid_v7;
use rynvek_server::store::*;

/// 一次性 PostgreSQL 实例（Drop 时立即停机，临时目录随之删除）
struct DisposablePostgres {
    bin: PathBuf,
    data: tempfile::TempDir,
    url: String,
}

impl Drop for DisposablePostgres {
    fn drop(&mut self) {
        let _ = Command::new(self.bin.join("pg_ctl"))
            .arg("-D")
            .arg(self.data.path().join("pgdata"))
            .args(["-m", "immediate", "-w", "stop"])
            .output();
    }
}

fn find_pg_bin() -> Option<PathBuf> {
    let has = |d: &Path| d.join("initdb").is_file() && d.join("pg_ctl").is_file() && d.join("postgres").is_file();
    if let Some(d) = std::env::var_os("RYNVEK_PG_BIN").map(PathBuf::from) {
        return has(&d).then_some(d);
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Ok(rd) = std::fs::read_dir("/usr/lib/postgresql") {
        let mut versions: Vec<PathBuf> = rd.flatten().map(|e| e.path().join("bin")).collect();
        versions.sort();
        dirs.extend(versions.into_iter().rev());
    }
    dirs.push("/usr/local/pgsql/bin".into());
    dirs.into_iter().find(|d| has(d))
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start_disposable() -> Result<DisposablePostgres, String> {
    let bin = find_pg_bin().ok_or("未找到 initdb / pg_ctl / postgres")?;
    let data = tempfile::tempdir().map_err(|e| e.to_string())?;
    let pgdata = data.path().join("pgdata");
    let out = Command::new(bin.join("initdb"))
        .arg("-D")
        .arg(&pgdata)
        .args(["-U", "rynvek", "--auth=trust", "-E", "UTF8", "--no-locale", "--no-sync"])
        .output()
        .map_err(|e| format!("initdb 启动失败：{e}"))?;
    if !out.status.success() {
        return Err(format!("initdb 失败：{}", String::from_utf8_lossy(&out.stderr)));
    }
    let port = free_port();
    let opts = format!(
        "-p {port} -k {} -c listen_addresses=127.0.0.1 -c fsync=off -c synchronous_commit=off -c full_page_writes=off",
        data.path().display()
    );
    let out = Command::new(bin.join("pg_ctl"))
        .arg("-D")
        .arg(&pgdata)
        .args(["-o", &opts, "-l"])
        .arg(data.path().join("server.log"))
        .args(["-w", "-t", "60", "start"])
        .output()
        .map_err(|e| format!("pg_ctl 启动失败：{e}"))?;
    if !out.status.success() {
        let log = std::fs::read_to_string(data.path().join("server.log")).unwrap_or_default();
        return Err(format!(
            "pg_ctl start 失败：{}\n{log}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(DisposablePostgres {
        bin,
        data,
        url: format!("host=127.0.0.1 port={port} user=rynvek dbname=postgres"),
    })
}

/// 返回 (连接串, 实例守卫)；`None` 表示跳过
fn postgres() -> Option<(String, Option<DisposablePostgres>)> {
    if let Ok(url) = std::env::var("RYNVEK_TEST_POSTGRES_URL") {
        return Some((url, None));
    }
    match start_disposable() {
        Ok(pg) => Some((pg.url.clone(), Some(pg))),
        Err(e) => {
            if std::env::var("RYNVEK_REQUIRE_POSTGRES").is_ok_and(|v| v == "1") {
                panic!("RYNVEK_REQUIRE_POSTGRES=1 但无法拉起 PostgreSQL：{e}");
            }
            eprintln!("跳过 PostgreSQL 测试：{e}（内存实现已覆盖核心路径）");
            None
        }
    }
}

async fn pg_store(url: &str) -> Store {
    let pg = PostgresStore::connect(url).await.expect("连接 PostgreSQL");
    // 首次迁移执行规范 DDL；重复调用为幂等空操作
    assert!(pg.migrate().await.unwrap());
    assert!(!pg.migrate().await.unwrap());
    Store::Postgres(pg)
}

fn principal(id: u64, handle: &str) -> PrincipalRow {
    PrincipalRow {
        principal_id: id,
        handle: handle.to_owned(),
        kind: PrincipalKindCode::Human,
        ed25519_pubkey: [id as u8; 32],
        email_hash: None,
        status: PrincipalStatus::Active,
    }
}

fn org(id: u64, slug: &str, owner: u64, max_worlds: i32) -> OrgRow {
    OrgRow {
        org_id: id,
        slug: slug.to_owned(),
        display_name: slug.to_owned(),
        owner_principal_id: owner,
        quota_max_worlds: max_worlds,
        quota_storage_bytes: 1 << 30,
    }
}

fn world(id: WireUuid, org_id: u64, owner: u64, visibility: Visibility) -> (WorldRow, CommitRow) {
    (
        WorldRow {
            world_id: id,
            org_id,
            owner_principal_id: owner,
            title: "世界".to_owned(),
            world_profile: "Competitive_RTS".to_owned(),
            game_mode: "PvP_Custom".to_owned(),
            execution_profile: "D0_Strict".to_owned(),
            visibility,
            parent_world_id: None,
            forked_at_revision: None,
            latest_tick: 0,
            latest_revision: 0,
            latest_state_root: [1; 32],
            lease_holder_node: None,
            fencing_token: 1,
        },
        CommitRow {
            world_id: id,
            world_revision: 0,
            tick: 0,
            parent_revision: None,
            state_root_blake3: [1; 32],
            journal_segment_cid: [2; 32],
            snapshot_cid: None,
        },
    )
}

fn code(r: StoreResult<impl std::fmt::Debug>) -> u16 {
    r.unwrap_err().code.code()
}

/// 两种后端共用的一致性断言
async fn conformance(s: &Store) {
    // principals / organizations：唯一约束与外键
    s.insert_principal(principal(1, "alice")).await.unwrap();
    s.insert_principal(principal(2, "bob")).await.unwrap();
    assert_eq!(code(s.insert_principal(principal(3, "alice")).await), 0x2005);
    assert_eq!(s.get_principal(1).await.unwrap().unwrap(), principal(1, "alice"));
    s.insert_org(org(1, "u-alice", 1, 2)).await.unwrap();
    s.insert_org(org(2, "u-bob", 2, 100)).await.unwrap();
    assert_eq!(code(s.insert_org(org(9, "u-alice", 1, 1)).await), 0x2005);
    assert!(s.insert_org(org(10, "ghost", 999, 1)).await.is_err(), "owner 外键");
    assert_eq!(s.orgs_owned_by(1).await.unwrap().len(), 1);

    // worlds：配额、可见性过滤、提交与围栏令牌
    let w1 = new_uuid_v7();
    let w2 = new_uuid_v7();
    let w3 = new_uuid_v7();
    let (row, genesis) = world(w1, 1, 1, Visibility::Private);
    s.insert_world(row.clone(), genesis).await.unwrap();
    let (r2, g2) = world(w2, 1, 1, Visibility::Public);
    s.insert_world(r2, g2).await.unwrap();
    let (r3, g3) = world(w3, 1, 1, Visibility::Public);
    assert_eq!(code(s.insert_world(r3, g3).await), 0x2004, "超出 quota_max_worlds");
    assert_eq!(s.get_world(w1).await.unwrap().unwrap(), row);
    assert_eq!(s.list_worlds(Some(1), 50, 0).await.unwrap().len(), 2);
    let bob_view = s.list_worlds(Some(2), 50, 0).await.unwrap();
    assert_eq!(bob_view.iter().map(|w| w.world_id).collect::<Vec<_>>(), vec![w2]);
    assert_eq!(s.list_worlds(None, 50, 0).await.unwrap().len(), 1);
    let next = CommitRow {
        world_id: w1,
        world_revision: 1,
        tick: 30,
        parent_revision: Some(0),
        state_root_blake3: [5; 32],
        journal_segment_cid: [6; 32],
        snapshot_cid: Some([7; 32]),
    };
    assert_eq!(code(s.append_commit(next.clone(), 99).await), 0x7005, "围栏令牌过期");
    s.append_commit(next.clone(), 1).await.unwrap();
    assert_eq!(code(s.append_commit(next.clone(), 1).await), 0x2005, "修订号重复");
    let commits = s.list_commits(w1).await.unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[1], next);
    let w = s.get_world(w1).await.unwrap().unwrap();
    assert_eq!(
        (w.latest_revision, w.latest_tick, w.latest_state_root),
        (1, 30, [5; 32])
    );

    // registry：版本唯一
    s.upsert_package(PackageRow {
        package_id: "alice.theme".into(),
        org_id: 1,
        package_kind: "rview".into(),
        trust_level: 3,
        revoked: false,
    })
    .await
    .unwrap();
    let v = PackageVersionRow {
        package_id: "alice.theme".into(),
        semver: "1.0.0".into(),
        min_engine_version: "0.1.0".into(),
        license_spdx: "AGPL-3.0-only".into(),
        artifact_cid_blake3: [8; 32],
        artifact_size_bytes: 10,
        ed25519_signature: [9; 64],
        manifest_json: serde_json::json!({ "a": 1 }),
        lockfile_json: serde_json::json!({ "lockfile_version": 1 }),
    };
    s.insert_package_version(v.clone()).await.unwrap();
    assert_eq!(code(s.insert_package_version(v.clone()).await), 0x9003);
    assert_eq!(s.package_versions("alice.theme").await.unwrap(), vec![v]);
    assert_eq!(s.search_packages(Some("rview"), Some("theme")).await.unwrap().len(), 1);
    assert!(s.search_packages(Some("rpack"), None).await.unwrap().is_empty());

    // matches / participants / ratings
    let m = MatchRow {
        match_id: new_uuid_v7(),
        world_id: w2,
        game_mode: "PvP_Ranked".into(),
        ruleset_blake3: [3; 32],
        map_seed: u64::MAX >> 1,
        match_state: MatchState::Created,
        winner_faction_id: None,
        win_reason: None,
        total_ticks: 0,
        replay_cid_blake3: None,
        started_at_unix_s: None,
        completed_at_unix_s: None,
    };
    let parts = |id: WireUuid| {
        vec![
            ParticipantRow {
                match_id: id,
                faction_id: 0,
                principal_id: 1,
                controller_league: League::Human,
                final_score: 0,
                rating_delta: None,
            },
            ParticipantRow {
                match_id: id,
                faction_id: 1,
                principal_id: 2,
                controller_league: League::Human,
                final_score: 0,
                rating_delta: None,
            },
        ]
    };
    s.insert_match(m.clone(), parts(m.match_id)).await.unwrap();
    let mut done = m.clone();
    done.match_state = MatchState::Completed;
    done.winner_faction_id = Some(1);
    done.win_reason = Some("Surrender".into());
    done.total_ticks = 120;
    done.replay_cid_blake3 = Some([4; 32]);
    done.started_at_unix_s = Some(1_700_000_000);
    done.completed_at_unix_s = Some(1_700_000_004);
    s.update_match(done.clone()).await.unwrap();
    let mut p = parts(m.match_id)[1].clone();
    p.final_score = 42;
    p.rating_delta = Some(12.5);
    s.update_participant(p.clone()).await.unwrap();
    let (got, got_parts) = s.get_match(m.match_id).await.unwrap().unwrap();
    assert_eq!(got, done);
    assert_eq!(got_parts[1], p);
    assert_eq!(s.matches_of(2).await.unwrap().len(), 1);
    assert!(s.get_match(new_uuid_v7()).await.unwrap().is_none());

    assert_eq!(
        s.get_rating(1, League::Human).await.unwrap(),
        RatingRow::fresh(1, League::Human)
    );
    let mut r = RatingRow::fresh(2, League::Human);
    r.rating = 1662.31;
    r.deviation = 290.32;
    r.matches_played = 1;
    s.put_rating(r.clone()).await.unwrap();
    s.put_rating(RatingRow::fresh(1, League::Human)).await.unwrap();
    let board = s.leaderboard(League::Human, 10).await.unwrap();
    assert_eq!(board[0], (r, "bob".to_owned()));
    assert!(s.leaderboard(League::AgentHybrid, 10).await.unwrap().is_empty());

    // audit_logs：只追加，按主体检索；client_ip 为 INET
    s.append_audit(NewAudit {
        principal_id: Some(1),
        action_type: "test".into(),
        target_resource: "world:x".into(),
        client_ip: Some("127.0.0.1".parse().unwrap()),
        details_json: serde_json::json!({ "k": "v" }),
    })
    .await
    .unwrap();
    let audit = s.audit_of(1).await.unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].client_ip, Some("127.0.0.1".parse().unwrap()));

    // 注销：匿名化且不可登录
    s.tombstone_principal(1).await.unwrap();
    let t = s.get_principal(1).await.unwrap().unwrap();
    assert_eq!(t.status, PrincipalStatus::Deleted);
    assert_ne!(t.handle, "alice");
}

#[tokio::test]
async fn memory_store_conformance() {
    conformance(&Store::memory()).await;
}

#[tokio::test]
async fn postgres_store_conformance() {
    let Some((url, _guard)) = postgres() else { return };
    let s = pg_store(&url).await;
    conformance(&s).await;
}

#[tokio::test]
async fn postgres_backed_match_end_to_end() {
    let Some((url, _guard)) = postgres() else { return };
    full_match(Transport::WebSocket, pg_store(&url).await).await;
}
