// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 服务端共享状态：存储、令牌中心、质询、匹配队列、进行中对局注册表，以及终局持久化任务。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use axum::http::HeaderMap;
use base64::Engine as _;
use rynvek_core::error::RynvekErrorCode;
use rynvek_protocol::{CapabilityScope, WireUuid};
use rynvek_rts::{ArenaWorld, MatchConfig};
use rynvek_security::{
    AuthChallenges, CommandGateConfig, League, OrgRole, PrincipalKindCode, PrincipalStatus, ServerSigningKey, Subject,
    TokenAuthority, VerifiedToken,
};
use rynvek_storage::{BlobStore, MemoryBlobStore};
use tokio::sync::mpsc;

use crate::error::{ApiError, ApiResult};
use crate::glicko2;
use crate::host::{self, HostConfig, HostEvent, MatchHandle, MatchSpec, SeatSpec, TickDriver};
use crate::ids::{new_uuid_v7, unix_now_s};
use crate::matchmaking::{Matchmaker, Pairing};
use crate::source::SourceOffer;
use crate::store::{
    CommitRow, MatchRow, MatchState, NewAudit, OrgRow, ParticipantRow, PrincipalRow, RatingRow, Store, WorldRow,
};

/// 本服务器唯一支持的规则集标识；`ruleset_hash` = BLAKE3(此字符串)
pub const ARENA_RULESET_ID: &str = "rynvek.ruleset.arena-duel@1";

/// 内置规则集哈希
pub fn arena_ruleset_hash() -> [u8; 32] {
    *blake3::hash(ARENA_RULESET_ID.as_bytes()).as_bytes()
}

/// 运行配置
#[derive(Clone, Debug)]
pub struct AppConfig {
    /// 对外地址（拼接 SBOM、流地址等绝对链接）
    pub public_base_url: String,
    /// 运营方自托管的源码包地址（修改版部署必须提供）
    pub source_tarball_override: Option<String>,
    pub tick_driver: TickDriver,
    pub keyframe_interval_ticks: u64,
    pub lobby_timeout: Option<std::time::Duration>,
    pub token_ttl_s: u64,
    /// WebTransport 对外地址与证书指纹（未启用时为 `None`）
    pub webtransport: Option<WebTransportInfo>,
    /// 新建对局的地图种子；`None` 时由 match_id 派生
    pub fixed_map_seed: Option<u64>,
}

/// WebTransport 入口信息
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebTransportInfo {
    pub url_base: String,
    pub certificate_sha256: [u8; 32],
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            public_base_url: "http://127.0.0.1:8080".to_owned(),
            source_tarball_override: None,
            tick_driver: TickDriver::RealTime {
                hz: host::DEFAULT_TICK_RATE_HZ,
            },
            keyframe_interval_ticks: rynvek_network::replication::DEFAULT_KEYFRAME_INTERVAL_TICKS,
            lobby_timeout: Some(std::time::Duration::from_secs(120)),
            token_ttl_s: rynvek_security::token::DEFAULT_TOKEN_TTL_S,
            webtransport: None,
            fixed_map_seed: None,
        }
    }
}

/// 实验任务（M7 只登记，执行器在 M8）
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Experiment {
    pub exp_id: String,
    pub owner_principal_id: u64,
    pub name: String,
    pub seeds: Vec<u64>,
    pub ticks: u64,
    pub status: String,
    pub submitted_at_unix_s: u64,
    /// 产出 Parquet 的 CAS 标识（执行完成后才有）
    pub parquet_cid: Option<String>,
}

/// 进行中对局注册表
#[derive(Debug, Default)]
pub struct MatchRegistry {
    by_match: BTreeMap<WireUuid, MatchHandle>,
    by_world: BTreeMap<WireUuid, WireUuid>,
    /// Principal → 当前所在对局
    by_principal: BTreeMap<u64, WireUuid>,
}

impl MatchRegistry {
    pub fn get(&self, match_id: &WireUuid) -> Option<MatchHandle> {
        self.by_match.get(match_id).cloned()
    }

    pub fn by_world(&self, world_id: &WireUuid) -> Option<MatchHandle> {
        self.by_world.get(world_id).and_then(|m| self.get(m))
    }

    /// 某 Principal 仍未终结的对局
    pub fn active_for(&self, principal_id: u64) -> Option<MatchHandle> {
        self.by_principal
            .get(&principal_id)
            .and_then(|m| self.get(m))
            .filter(|h| !h.status().state.is_terminal())
    }

    pub fn active_count(&self) -> usize {
        self.by_match
            .values()
            .filter(|h| !h.status().state.is_terminal())
            .count()
    }
}

/// 共享状态（`Clone` 廉价）
#[derive(Clone, Debug)]
pub struct AppState {
    pub inner: Arc<AppInner>,
}

/// 共享状态本体
pub struct AppInner {
    pub config: AppConfig,
    pub store: Store,
    pub blobs: Arc<dyn BlobStore>,
    pub tokens: RwLock<TokenAuthority>,
    pub challenges: Mutex<AuthChallenges>,
    pub matchmaker: Mutex<Matchmaker>,
    pub registry: Mutex<MatchRegistry>,
    pub experiments: Mutex<BTreeMap<String, Experiment>>,
    pub signer: ServerSigningKey,
    pub resume_secret: [u8; 32],
    pub started: Instant,
    events: mpsc::UnboundedSender<HostEvent>,
}

impl std::fmt::Debug for AppInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppInner")
            .field("config", &self.config)
            .field("store", &self.store.backend_name())
            .field("blobs", &self.blobs.len())
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl AppState {
    /// 创建状态并启动终局持久化任务（需在 Tokio 运行时内调用）
    pub fn new(config: AppConfig, store: Store, signer: ServerSigningKey) -> ApiResult<Self> {
        let mut secret = [0u8; 32];
        getrandom::getrandom(&mut secret).map_err(|e| ApiError::internal(format!("系统随机源不可用：{e}")))?;
        let (events, rx) = mpsc::unbounded_channel();
        let state = Self {
            inner: Arc::new(AppInner {
                store,
                blobs: Arc::new(MemoryBlobStore::new()),
                tokens: RwLock::new(TokenAuthority::new(signer.clone())),
                challenges: Mutex::new(AuthChallenges::new().map_err(ApiError::from)?),
                matchmaker: Mutex::new(Matchmaker::new()),
                registry: Mutex::new(MatchRegistry::default()),
                experiments: Mutex::new(BTreeMap::new()),
                signer,
                resume_secret: secret,
                started: Instant::now(),
                events,
                config,
            }),
        };
        tokio::spawn(finalizer(state.clone(), rx));
        Ok(state)
    }

    pub fn config(&self) -> &AppConfig {
        &self.inner.config
    }

    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    pub fn source_offer(&self) -> SourceOffer {
        SourceOffer::current(
            &self.inner.config.public_base_url,
            self.inner.config.source_tarball_override.as_deref(),
        )
    }

    pub fn registry(&self) -> std::sync::MutexGuard<'_, MatchRegistry> {
        lock(&self.inner.registry)
    }

    pub fn matchmaker(&self) -> std::sync::MutexGuard<'_, Matchmaker> {
        lock(&self.inner.matchmaker)
    }

    pub fn challenges(&self) -> std::sync::MutexGuard<'_, AuthChallenges> {
        lock(&self.inner.challenges)
    }

    pub fn experiments(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Experiment>> {
        lock(&self.inner.experiments)
    }

    pub fn tokens(&self) -> std::sync::RwLockReadGuard<'_, TokenAuthority> {
        self.inner.tokens.read().unwrap_or_else(|p| p.into_inner())
    }

    pub fn tokens_mut(&self) -> std::sync::RwLockWriteGuard<'_, TokenAuthority> {
        self.inner.tokens.write().unwrap_or_else(|p| p.into_inner())
    }

    /// 签发令牌（线上 96 字节）
    pub fn issue_token(&self, principal_id: u64, kind: PrincipalKindCode) -> rynvek_protocol::CapabilityToken {
        let ttl = self.inner.config.token_ttl_s;
        self.tokens_mut()
            .issue(principal_id, kind.default_scopes(), unix_now_s(), ttl)
    }

    /// 注册 Principal 并创建其个人组织（`org_id = principal_id`，slug = `u-{handle}`）
    pub async fn register_principal(
        &self,
        principal_id: u64,
        handle: &str,
        kind: PrincipalKindCode,
        ed25519_pubkey: [u8; 32],
    ) -> ApiResult<()> {
        if !(1..=64).contains(&handle.len())
            || !handle
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(ApiError::bad_request("handle 只允许 1..=64 个 [A-Za-z0-9_-] 字符"));
        }
        if principal_id == 0 || principal_id > i64::MAX as u64 {
            return Err(ApiError::bad_request("principal_id 必须在 [1, 2^63)"));
        }
        let mut ring = rynvek_security::Keyring::new();
        ring.insert(0, ed25519_pubkey).map_err(ApiError::from)?;
        let store = self.store();
        store
            .insert_principal(PrincipalRow {
                principal_id,
                handle: handle.to_owned(),
                kind,
                ed25519_pubkey,
                email_hash: None,
                status: PrincipalStatus::Active,
            })
            .await?;
        store
            .insert_org(OrgRow {
                org_id: principal_id,
                slug: format!("u-{handle}"),
                display_name: handle.to_owned(),
                owner_principal_id: principal_id,
                quota_max_worlds: 10,
                quota_storage_bytes: 10_737_418_240,
            })
            .await?;
        store
            .append_audit(NewAudit {
                principal_id: Some(principal_id),
                action_type: "principal.register".to_owned(),
                target_resource: format!("principal:{principal_id}"),
                client_ip: None,
                details_json: serde_json::json!({ "handle": handle, "kind": kind }),
            })
            .await
    }

    /// 创建（官方或团队）组织
    pub async fn create_org(&self, org_id: u64, slug: &str, display_name: &str, owner: u64) -> ApiResult<()> {
        self.store()
            .insert_org(OrgRow {
                org_id,
                slug: slug.to_owned(),
                display_name: display_name.to_owned(),
                owner_principal_id: owner,
                quota_max_worlds: 100,
                quota_storage_bytes: 10_737_418_240,
            })
            .await
    }

    /// 从 `Authorization: Bearer <base64>` 验证令牌
    pub fn authenticate(&self, headers: &HeaderMap) -> ApiResult<VerifiedToken> {
        let value = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::unauthenticated("缺少 Authorization: Bearer 令牌"))?;
        let b64 = value
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError::unauthenticated("Authorization 必须是 Bearer 方案"))?;
        let bytes = decode_token_text(b64.trim())?;
        self.tokens()
            .verify_bytes(&bytes, unix_now_s())
            .map_err(|e| ApiError::from(e).with_status(axum::http::StatusCode::UNAUTHORIZED))
    }

    /// 令牌 → 授权主体（合并数据库中的身份属性与组织角色）
    pub async fn subject(&self, token: &VerifiedToken) -> ApiResult<Subject> {
        let p = self
            .store()
            .get_principal(token.principal_id())
            .await?
            .ok_or_else(|| ApiError::unauthenticated("令牌对应的 Principal 不存在"))?;
        let memberships = self
            .store()
            .orgs_owned_by(p.principal_id)
            .await?
            .into_iter()
            .map(|o| (o.org_id, OrgRole::Owner))
            .collect();
        Ok(Subject {
            principal_id: p.principal_id,
            kind: p.kind,
            status: p.status,
            scopes: token.scopes(),
            memberships,
        })
    }

    /// 认证 + 主体 + 要求某权限位（ABAC 由各路由再判）
    pub async fn require(&self, headers: &HeaderMap, scope: CapabilityScope) -> ApiResult<Subject> {
        let token = self.authenticate(headers)?;
        token.require(scope).map_err(ApiError::from)?;
        self.subject(&token).await
    }

    pub async fn audit(&self, principal_id: Option<u64>, action: &str, target: String, details: serde_json::Value) {
        // 审计写入失败不影响主流程，但不能静默吞掉：降级到标准错误输出
        if let Err(e) = self
            .store()
            .append_audit(NewAudit {
                principal_id,
                action_type: action.to_owned(),
                target_resource: target,
                client_ip: None,
                details_json: details,
            })
            .await
        {
            eprintln!("审计日志写入失败：{e}");
        }
    }

    pub fn host_config(&self) -> HostConfig {
        HostConfig {
            driver: self.inner.config.tick_driver,
            keyframe_interval_ticks: self.inner.config.keyframe_interval_ticks,
            lobby_timeout: self.inner.config.lobby_timeout,
            gate: CommandGateConfig::default(),
            resume_secret: self.inner.resume_secret,
            signer: self.inner.signer.clone(),
        }
    }

    /// 为一次配对创建世界、对局记录并启动宿主
    pub async fn start_match(&self, pairing: &Pairing) -> ApiResult<MatchHandle> {
        let match_id = new_uuid_v7();
        let world_id = new_uuid_v7();
        let map_seed = self
            .inner
            .config
            .fixed_map_seed
            .unwrap_or_else(|| u64::from_le_bytes(match_id.0[8..16].try_into().expect("8 字节")) >> 1);
        let config = MatchConfig::duel(map_seed);
        let initial_root = *ArenaWorld::new(config.clone()).state_hash().as_bytes();
        let owner = pairing.players[0].principal_id;
        let store = self.store();
        let mut seats = Vec::new();
        for (faction, q) in pairing.players.iter().enumerate() {
            let p = store
                .get_principal(q.principal_id)
                .await?
                .ok_or_else(|| ApiError::not_found("参赛者不存在"))?;
            seats.push(SeatSpec {
                faction: faction as u8,
                principal_id: q.principal_id,
                kind: p.kind,
            });
        }
        let empty_journal = self.inner.blobs.put(b"")?;
        store
            .insert_world(
                WorldRow {
                    world_id,
                    org_id: owner,
                    owner_principal_id: owner,
                    title: format!("Ranked {} {}", pairing.league.as_str(), match_id),
                    world_profile: "RTS_Standard".to_owned(),
                    game_mode: "PvP_Ranked".to_owned(),
                    execution_profile: "D0_Strict".to_owned(),
                    visibility: rynvek_security::Visibility::Public,
                    parent_world_id: None,
                    forked_at_revision: None,
                    latest_tick: 0,
                    latest_revision: 0,
                    latest_state_root: initial_root,
                    lease_holder_node: Some("rynvek-server".to_owned()),
                    fencing_token: 1,
                },
                CommitRow {
                    world_id,
                    world_revision: 0,
                    tick: 0,
                    parent_revision: None,
                    state_root_blake3: initial_root,
                    journal_segment_cid: empty_journal.0,
                    snapshot_cid: None,
                },
            )
            .await?;
        store
            .insert_match(
                MatchRow {
                    match_id,
                    world_id,
                    game_mode: "PvP_Ranked".to_owned(),
                    ruleset_blake3: pairing.ruleset_hash,
                    map_seed,
                    match_state: MatchState::Created,
                    winner_faction_id: None,
                    win_reason: None,
                    total_ticks: 0,
                    replay_cid_blake3: None,
                    started_at_unix_s: None,
                    completed_at_unix_s: None,
                },
                seats
                    .iter()
                    .map(|s| ParticipantRow {
                        match_id,
                        faction_id: i16::from(s.faction),
                        principal_id: s.principal_id,
                        controller_league: pairing.league,
                        final_score: 0,
                        rating_delta: None,
                    })
                    .collect(),
            )
            .await?;
        let handle = host::spawn_match(
            MatchSpec {
                match_id,
                world_id,
                league: pairing.league,
                ruleset_hash: pairing.ruleset_hash,
                config,
                seats: seats.clone(),
            },
            self.host_config(),
            self.inner.events.clone(),
        );
        {
            let mut reg = self.registry();
            reg.by_match.insert(match_id, handle.clone());
            reg.by_world.insert(world_id, match_id);
            for s in &seats {
                reg.by_principal.insert(s.principal_id, match_id);
            }
        }
        self.audit(
            None,
            "match.created",
            format!("match:{match_id}"),
            serde_json::json!({ "league": pairing.league, "principals": seats.iter().map(|s| s.principal_id).collect::<Vec<_>>() }),
        )
        .await;
        Ok(handle)
    }

    /// 对外的实时流地址
    pub fn stream_urls(&self, world_id: &WireUuid) -> serde_json::Value {
        let path = format!("/api/v1/worlds/{world_id}/stream");
        let base = self.inner.config.public_base_url.trim_end_matches('/');
        let ws = base.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
        let wt = self.inner.config.webtransport.as_ref().map(|w| {
            serde_json::json!({
                "url": format!("{}{path}", w.url_base.trim_end_matches('/')),
                "certificate_sha256": crate::ids::hex(&w.certificate_sha256),
            })
        });
        serde_json::json!({ "websocket": format!("{ws}{path}"), "webtransport": wt })
    }
}

/// Bearer 文本 → 96 字节（接受 base64url 无填充与标准 Base64）
pub fn decode_token_text(text: &str) -> ApiResult<Vec<u8>> {
    use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
    URL_SAFE_NO_PAD
        .decode(text)
        .or_else(|_| STANDARD.decode(text))
        .map_err(|_| ApiError::unauthenticated("令牌不是合法 Base64"))
}

/// 96 字节令牌 → Bearer 文本（base64url 无填充）
pub fn encode_token_text(token: &rynvek_protocol::CapabilityToken) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(token.encode())
}

/// 终局持久化：状态迁移、审计、回放入库、Glicko-2 更新
async fn finalizer(state: AppState, mut rx: mpsc::UnboundedReceiver<HostEvent>) {
    while let Some(ev) = rx.recv().await {
        let r = match ev {
            HostEvent::State {
                match_id,
                state: to,
                tick,
            } => persist_state(&state, match_id, to, tick).await,
            HostEvent::Audit(row) => state.store().append_audit(row).await,
            HostEvent::Completed(c) => persist_completion(&state, *c).await,
        };
        if let Err(e) = r {
            eprintln!("对局持久化失败：{e}");
        }
    }
}

async fn persist_state(state: &AppState, match_id: WireUuid, to: MatchState, tick: u64) -> ApiResult<()> {
    let store = state.store();
    let Some((mut row, _)) = store.get_match(match_id).await? else {
        return Err(ApiError::not_found("对局不存在"));
    };
    if to == MatchState::Completed {
        // 由 persist_completion 原子地写入终局字段
        return Ok(());
    }
    if !row.match_state.can_transition(to) {
        return Err(ApiError::new(
            RynvekErrorCode::StructuralMutationConflict,
            format!("非法状态迁移 {:?} → {to:?}", row.match_state),
        ));
    }
    row.match_state = to;
    row.total_ticks = tick;
    if to == MatchState::Running {
        row.started_at_unix_s = Some(unix_now_s());
    }
    if to == MatchState::Aborted {
        row.completed_at_unix_s = Some(unix_now_s());
        let mut reg = state.registry();
        reg.by_principal.retain(|_, m| *m != match_id);
    }
    store.update_match(row).await
}

async fn persist_completion(state: &AppState, c: crate::host::CompletedMatch) -> ApiResult<()> {
    let store = state.store();
    let replay_cid = state.inner.blobs.put(&c.replay)?;
    let Some((mut row, participants)) = store.get_match(c.match_id).await? else {
        return Err(ApiError::not_found("对局不存在"));
    };
    // Lobby/Ready 迁移事件可能晚于 Running → Completed：按状态机补齐
    for step in [MatchState::Lobby, MatchState::Ready, MatchState::Running] {
        if row.match_state.can_transition(step) {
            row.match_state = step;
        }
    }
    if !row.match_state.can_transition(MatchState::Completed) {
        return Err(ApiError::new(
            RynvekErrorCode::StructuralMutationConflict,
            format!("对局处于 {:?}，不能完成", row.match_state),
        ));
    }
    row.match_state = MatchState::Completed;
    row.winner_faction_id = c.winner_faction.map(i16::from);
    row.win_reason = c.win_reason.clone();
    row.total_ticks = c.total_ticks;
    row.replay_cid_blake3 = Some(replay_cid.0);
    row.completed_at_unix_s = Some(unix_now_s());
    row.started_at_unix_s = row.started_at_unix_s.or(row.completed_at_unix_s);

    // Glicko-2：两人局，胜 1 / 平 0.5 / 负 0
    let mut ratings = Vec::new();
    for p in &participants {
        ratings.push(store.get_rating(p.principal_id, c.league).await?);
    }
    if let [a, b] = ratings.as_slice() {
        let score_a = match c.winner_faction {
            Some(w) if i16::from(w) == participants[0].faction_id => 1.0,
            Some(_) => 0.0,
            None => 0.5,
        };
        let (na, nb) = glicko2::update_pair(a.glicko(), b.glicko(), score_a, glicko2::DEFAULT_TAU);
        for ((p, old), new) in participants.iter().zip([a, b]).zip([na, nb]) {
            let final_score = c
                .scores
                .iter()
                .find(|s| i16::from(s.0) == p.faction_id)
                .map(|s| s.2)
                .unwrap_or_default();
            store
                .update_participant(ParticipantRow {
                    final_score,
                    rating_delta: Some(new.rating - old.rating),
                    ..p.clone()
                })
                .await?;
            store
                .put_rating(RatingRow {
                    principal_id: p.principal_id,
                    league: c.league,
                    rating: new.rating,
                    deviation: new.deviation,
                    volatility: new.volatility,
                    matches_played: old.matches_played + 1,
                })
                .await?;
        }
    }
    store.update_match(row).await?;
    // 世界提交链：revision 1 = 终局状态根，日志段指向签名回放
    if let Some(world) = store.get_world(c.world_id).await? {
        store
            .append_commit(
                CommitRow {
                    world_id: c.world_id,
                    world_revision: world.latest_revision + 1,
                    tick: c.total_ticks,
                    parent_revision: Some(world.latest_revision),
                    state_root_blake3: c.final_state_root,
                    journal_segment_cid: replay_cid.0,
                    snapshot_cid: None,
                },
                world.fencing_token,
            )
            .await?;
    }
    {
        let mut reg = state.registry();
        reg.by_principal.retain(|_, m| *m != c.match_id);
    }
    state
        .audit(
            None,
            "match.completed",
            format!("match:{}", c.match_id),
            serde_json::json!({
                "winner_faction": c.winner_faction,
                "win_reason": c.win_reason,
                "total_ticks": c.total_ticks,
                "replay_cid": replay_cid.to_hex(),
            }),
        )
        .await;
    Ok(())
}

/// 赛道准入（ABAC）：便于路由复用
pub fn league_resource(league: League) -> rynvek_security::Resource {
    rynvek_security::Resource::League(league)
}
