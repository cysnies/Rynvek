// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 服务端共享状态：存储、令牌中心、质询、匹配队列、进行中对局注册表，以及终局持久化任务。

use rynvek_core::hash::Blake3Hash;
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
use rynvek_storage::{BlobStore, FsBlobStore, MemoryBlobStore};
use tokio::sync::mpsc;

use crate::error::{ApiError, ApiResult};
use crate::host::{self, HostConfig, HostEvent, MatchHandle, MatchSpec, SeatSpec, TickDriver};
use crate::ids::{new_uuid_v7, unix_now_s};
use crate::source::SourceOffer;
use crate::store::{CommitRow, MatchRow, MatchState, NewAudit, OrgRow, ParticipantRow, PrincipalRow, Store, WorldRow};

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
    /// CAS Blob 目录；`None` 时使用进程内存（重启丢失，仅适合测试与开发）
    pub blob_dir: Option<std::path::PathBuf>,
    /// 同时执行的 Lab 实验数（每个实验内部再按 CPU 核数并行 run）
    pub lab_max_concurrent: usize,
    /// 显式开启且仅限回环内存开发服的 Web Origin；生产缺省关闭。
    pub local_practice_origin: Option<String>,
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
            blob_dir: None,
            lab_max_concurrent: 2,
            local_practice_origin: None,
        }
    }
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
    /// 完整世界状态、补丁输入与公开 Hub Blob 物理隔离，不允许通过公共 CID 路由下载隐藏状态。
    pub world_snapshots: Arc<dyn BlobStore>,
    pub tokens: RwLock<TokenAuthority>,
    pub challenges: Mutex<AuthChallenges>,
    pub ranked_node: String,
    pub agent_slots: Arc<tokio::sync::Semaphore>,
    /// 配对到宿主发布期间串行化查询/取消，避免报告虚假离队或重复建局。
    pub matchmaking_gate: tokio::sync::Mutex<()>,
    pub registry: Mutex<MatchRegistry>,
    pub general_hosts: tokio::sync::Mutex<BTreeMap<WireUuid, crate::general_host::GeneralHandle>>,
    pub lab_stop: tokio::sync::watch::Sender<bool>,
    pub lab_workers: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    pub practice_slots: Arc<tokio::sync::Semaphore>,
    /// 创作世界启动在本节点串行化；不持有同步注册表锁跨 await。
    pub authored_arena_start: tokio::sync::Mutex<()>,
    pub playback_slots: Arc<tokio::sync::Semaphore>,
    pub playbacks: Mutex<BTreeMap<WireUuid, crate::api::arena::PlaybackEntry>>,
    /// WASM 插件运行时（首次使用时创建）
    pub plugins: std::sync::OnceLock<rynvek_rules::plugin::PluginRuntime>,
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
                practice_slots: Arc::new(tokio::sync::Semaphore::new(4)),
                authored_arena_start: tokio::sync::Mutex::new(()),
                playback_slots: Arc::new(tokio::sync::Semaphore::new(2)),
                playbacks: Mutex::new(BTreeMap::new()),
                store,
                blobs: match &config.blob_dir {
                    Some(dir) => Arc::new(
                        FsBlobStore::open(dir)
                            .map_err(|e| ApiError::internal(format!("无法打开 Blob 目录 {}：{e}", dir.display())))?,
                    ),
                    None => Arc::new(MemoryBlobStore::new()),
                },
                world_snapshots: match &config.blob_dir {
                    Some(dir) => Arc::new(
                        FsBlobStore::open(dir.join("world-checkpoints"))
                            .map_err(|e| ApiError::internal(format!("无法打开私有检查点目录：{e}")))?,
                    ),
                    None => Arc::new(MemoryBlobStore::new()),
                },
                tokens: RwLock::new(TokenAuthority::new(signer.clone())),
                challenges: Mutex::new(AuthChallenges::new().map_err(ApiError::from)?),
                ranked_node: new_uuid_v7().to_string(),
                agent_slots: Arc::new(tokio::sync::Semaphore::new(2)),
                matchmaking_gate: tokio::sync::Mutex::new(()),
                registry: Mutex::new(MatchRegistry::default()),
                general_hosts: tokio::sync::Mutex::new(BTreeMap::new()),
                lab_stop: tokio::sync::watch::channel(false).0,
                lab_workers: Mutex::new(Vec::new()),
                plugins: std::sync::OnceLock::new(),
                signer,
                resume_secret: secret,
                started: Instant::now(),
                events,
                config,
            }),
        };
        tokio::spawn(finalizer(state.clone(), rx));
        crate::lab_jobs::start_workers(&state);
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

    pub fn challenges(&self) -> std::sync::MutexGuard<'_, AuthChallenges> {
        lock(&self.inner.challenges)
    }

    /// 共享的 WASM 插件运行时
    pub fn plugin_runtime(&self) -> ApiResult<rynvek_rules::plugin::PluginRuntime> {
        if let Some(rt) = self.inner.plugins.get() {
            return Ok(rt.clone());
        }
        let rt = rynvek_rules::plugin::PluginRuntime::new()
            .map_err(|e| ApiError::internal(format!("插件运行时初始化失败：{e}")))?;
        Ok(self.inner.plugins.get_or_init(|| rt).clone())
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
    /// 首提交只写私有CAS；配对事务成功前不会产生可发现的世界/对局/席位。
    pub fn prepare_ranked_genesis(&self) -> ApiResult<crate::ranked::RankedGenesis> {
        let match_id = new_uuid_v7();
        let world_id = new_uuid_v7();
        let map_seed = self
            .inner
            .config
            .fixed_map_seed
            .unwrap_or_else(|| u64::from_le_bytes(match_id.0[8..16].try_into().expect("8字节")) >> 1);
        let config = MatchConfig::duel(map_seed);
        let runtime = rynvek_rts::ArenaRuntime::new(world_id.0, ArenaWorld::new(config));
        let checkpoint = runtime.checkpoint().map_err(ApiError::from)?;
        let initial_root = checkpoint.state_root;
        let snapshot = self.inner.world_snapshots.put(&checkpoint.bytes)?;
        let empty_journal = self.inner.world_snapshots.put(b"")?;
        let owner = 0;
        Ok(crate::ranked::RankedGenesis {
            world: WorldRow {
                world_id,
                org_id: owner,
                owner_principal_id: owner,
                title: format!("Ranked {} {}", "human", match_id),
                world_profile: "RTS_Standard".to_owned(),
                game_mode: "PvP_Ranked".to_owned(),
                execution_profile: "D0_Strict".to_owned(),
                visibility: rynvek_security::Visibility::Public,
                parent_world_id: None,
                forked_at_revision: None,
                latest_tick: 0,
                latest_revision: 0,
                latest_state_root: initial_root,
                lease_holder_node: None,
                fencing_token: 1,
            },
            commit: CommitRow {
                world_id,
                world_revision: 0,
                tick: 0,
                parent_revision: None,
                state_root_blake3: initial_root,
                journal_segment_cid: empty_journal.0,
                snapshot_cid: Some(snapshot.0),
            },
            row: MatchRow {
                match_id,
                world_id,
                game_mode: "PvP_Ranked".to_owned(),
                ruleset_blake3: arena_ruleset_hash(),
                map_seed,
                match_state: MatchState::Created,
                winner_faction_id: None,
                win_reason: None,
                total_ticks: 0,
                replay_cid_blake3: None,
                started_at_unix_s: None,
                completed_at_unix_s: None,
            },
        })
    }
    /// 恢复已提交的真实排位预留；健康的远端宿主只返回发现结果，绝不抢占或转发Bearer。
    pub async fn ensure_ranked_host(&self, id: WireUuid) -> ApiResult<crate::ranked::HostLease> {
        let _gate = self.inner.matchmaking_gate.lock().await;
        let (row, parts) = self
            .store()
            .get_match(id)
            .await?
            .ok_or_else(|| ApiError::not_found("对局不存在"))?;
        if row.game_mode != "PvP_Ranked"
            || row.match_state.is_terminal()
            || parts.len() != 2
            || row.ruleset_blake3 != arena_ruleset_hash()
        {
            return Err(crate::transactions::conflict("不是可恢复的当前规则排位"));
        }
        let world = self
            .store()
            .get_world(row.world_id)
            .await?
            .ok_or_else(|| ApiError::not_found("世界不存在"))?;
        let existing = self.store().ranked_host(world.world_id).await?;
        if let Some(ref lease) = existing {
            if lease.fence == world.fencing_token && lease.expires_at_s > unix_now_s() {
                let active = self
                    .registry()
                    .get(&id)
                    .is_some_and(|h| !h.status().state.is_terminal() && h.fencing_token == Some(lease.fence));
                if lease.node != self.inner.ranked_node || active {
                    return Ok(lease.clone());
                }
            }
        }
        let mut seats = Vec::new();
        for p in &parts {
            let principal = self
                .store()
                .get_principal(p.principal_id)
                .await?
                .ok_or_else(|| ApiError::forbidden("主体不存在"))?;
            if principal.status != rynvek_security::PrincipalStatus::Active {
                return Err(ApiError::forbidden("参赛主体已失效"));
            }
            seats.push(SeatSpec {
                principal_id: p.principal_id,
                faction: u8::try_from(p.faction_id).map_err(|_| ApiError::internal("阵营无效"))?,
                kind: principal.kind,
            });
        }
        let runtime = crate::api::worlds::restore_runtime(self, &world).await?;
        if runtime.config != MatchConfig::duel(row.map_seed) {
            return Err(crate::transactions::conflict("恢复的真实配置不匹配"));
        }
        let lease = self
            .store()
            .claim_ranked_host(crate::ranked::HostClaim {
                world: world.world_id,
                revision: world.latest_revision,
                fence: world.fencing_token,
                node: self.inner.ranked_node.clone(),
                origin: self.config().public_base_url.clone(),
            })
            .await?;
        if lease.node != self.inner.ranked_node {
            return Ok(lease);
        }
        let spec = MatchSpec {
            custom: false,
            match_id: id,
            world_id: world.world_id,
            league: parts[0].controller_league,
            ruleset_hash: row.ruleset_blake3,
            config: runtime.config.clone(),
            seats: seats.clone(),
        };
        let handle = host::spawn_runtime(
            spec,
            self.host_config(),
            self.inner.events.clone(),
            runtime,
            Some(crate::transactions::TickCommitter {
                store: self.store().clone(),
                snapshots: self.inner.world_snapshots.clone(),
                world_id: world.world_id,
                fencing_token: lease.fence,
            }),
        )?;
        let mut registry = self.registry();
        registry.by_match.insert(id, handle);
        registry.by_world.insert(world.world_id, id);
        for seat in seats {
            registry.by_principal.insert(seat.principal_id, id);
        }
        Ok(lease)
    }

    /// 服务端启动/恢复既有创作世界；调用方须先建立对局及席位记录，不对外暴露任意认领接口。
    pub async fn attach_world_host(&self, spec: MatchSpec) -> ApiResult<MatchHandle> {
        if self
            .registry()
            .by_world(&spec.world_id)
            .is_some_and(|h| !h.status().state.is_terminal())
        {
            return Err(crate::transactions::conflict("世界已有本地活动宿主"));
        }
        let (row, participants) = self
            .store()
            .get_match(spec.match_id)
            .await?
            .ok_or_else(|| ApiError::not_found("对局不存在"))?;
        if row.world_id != spec.world_id
            || spec.seats.iter().any(|seat| {
                !participants
                    .iter()
                    .any(|p| p.principal_id == seat.principal_id && p.faction_id == i16::from(seat.faction))
            })
        {
            return Err(crate::transactions::conflict("对局记录与宿主席位不一致"));
        }
        let world = self
            .store()
            .get_world(spec.world_id)
            .await?
            .ok_or_else(|| ApiError::not_found("世界不存在"))?;
        if row.match_state.is_terminal()
            || row.game_mode != world.game_mode
            || row.ruleset_blake3 != spec.ruleset_hash
            || participants.len() != spec.seats.len()
            || spec.seats.len() != usize::from(spec.config.players)
        {
            return Err(crate::transactions::conflict("对局模式、规则或完整席位与世界不一致"));
        }
        let runtime = crate::api::worlds::restore_runtime(self, &world).await?;
        if runtime.config != spec.config {
            return Err(crate::transactions::conflict("不能用模板替换恢复的世界配置"));
        }
        let token = self
            .store()
            .claim_world(
                world.world_id,
                world.latest_revision,
                world.fencing_token,
                "rynvek-server".into(),
            )
            .await?;
        let handle = host::spawn_runtime(
            spec.clone(),
            self.host_config(),
            self.inner.events.clone(),
            runtime,
            Some(crate::transactions::TickCommitter {
                store: self.store().clone(),
                snapshots: self.inner.world_snapshots.clone(),
                world_id: world.world_id,
                fencing_token: token,
            }),
        )?;
        let mut reg = self.registry();
        reg.by_match.insert(spec.match_id, handle.clone());
        reg.by_world.insert(spec.world_id, spec.match_id);
        for seat in &spec.seats {
            reg.by_principal.insert(seat.principal_id, spec.match_id);
        }
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
                fencing_token,
            } => persist_state(&state, match_id, to, tick, fencing_token).await,
            HostEvent::Audit(row) => state.store().append_audit(row).await,
            HostEvent::Completed(c) => persist_completion(&state, *c).await,
        };
        if let Err(e) = r {
            eprintln!("对局持久化失败：{e}");
        }
    }
}

async fn persist_state(
    state: &AppState,
    match_id: WireUuid,
    to: MatchState,
    tick: u64,
    fencing_token: Option<u64>,
) -> ApiResult<()> {
    let store = state.store();
    let Some((mut row, _)) = store.get_match(match_id).await? else {
        return Err(ApiError::not_found("对局不存在"));
    };
    if to == MatchState::Completed {
        // 由 persist_completion 原子地写入终局字段
        return Ok(());
    }
    // 恢复宿主重新经历 Lobby/Ready/Running，持久状态不倒退、不重写 started_at。
    if matches!(
        (row.match_state, to),
        (
            MatchState::Running,
            MatchState::Lobby | MatchState::Ready | MatchState::Running
        ) | (MatchState::Ready, MatchState::Lobby | MatchState::Ready)
            | (MatchState::Lobby, MatchState::Lobby)
    ) {
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
        row.started_at_unix_s.get_or_insert_with(unix_now_s);
    }
    if to == MatchState::Aborted {
        row.completed_at_unix_s = Some(unix_now_s());
    }
    store
        .update_match_fenced(
            row,
            fencing_token.ok_or_else(|| crate::transactions::conflict("持久宿主缺少fence"))?,
        )
        .await?;
    if to == MatchState::Aborted {
        state.registry().by_principal.retain(|_, m| *m != match_id);
    }
    Ok(())
}

async fn persist_completion(state: &AppState, c: crate::host::CompletedMatch) -> ApiResult<()> {
    let store = state.store();
    // 世界已经由 Actor 的 Tick CAS 提交；终局任务只能确认，不能再造无检查点的假修订。
    let world = store
        .get_world(c.world_id)
        .await?
        .ok_or_else(|| ApiError::not_found("世界不存在"))?;
    if c.fencing_token != Some(world.fencing_token) {
        return Err(crate::transactions::conflict("旧宿主终局fence已失效"));
    }
    let commits = store.list_commits(c.world_id).await?;
    if world.latest_tick != c.total_ticks
        || world.latest_state_root != c.final_state_root
        || !commits
            .last()
            .is_some_and(|commit| commit.world_revision == world.latest_revision && commit.snapshot_cid.is_some())
    {
        return Err(crate::transactions::conflict("终局与已提交权威检查点不一致"));
    }
    // 从真实已提交Studio起点覆盖全部运行期/恢复前日志；不把开局前创作伪装成运行帧。
    let snapshots = state.inner.world_snapshots.clone();
    let signer = state.inner.signer.clone();
    let expected_tick = c.total_ticks;
    let expected_root = c.final_state_root;
    let world_id = c.world_id;
    let replay = c.replay.clone();
    let replay_bytes = tokio::task::spawn_blocking(move || -> ApiResult<Vec<u8>> {
        let mut keys = rynvek_security::Keyring::new();
        keys.insert(signer.key_id(), signer.public_key())
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let current = crate::replay::open(&replay, Some(&keys)).map_err(|e| ApiError::new(e.code, e.message))?;
        let mut origin = commits
            .first()
            .filter(|c| c.world_revision == 0)
            .ok_or_else(|| crate::transactions::conflict("缺少世界genesis"))?;
        let mut ticks = Vec::new();
        for commit in commits.iter().skip(1) {
            let bytes = snapshots.get(&Blake3Hash(commit.journal_segment_cid))?;
            if Blake3Hash::of(&bytes).0 != commit.journal_segment_cid {
                return Err(crate::transactions::conflict("历史WAL CID不符"));
            }
            if let Ok(record) = serde_json::from_slice::<crate::transactions::TickRecord>(&bytes) {
                if record.revision != commit.world_revision
                    || record.state_root != commit.state_root_blake3
                    || record.tick != commit.tick
                    || Some(record.base_revision) != commit.parent_revision
                {
                    return Err(crate::transactions::conflict("历史WAL与提交元数据不一致"));
                }
                ticks.push(record);
            } else {
                // 当前未挂Actor的Studio事务直接形成完整创作锚点，不是可伪装成Tick的帧。
                // 选择最后真实创作快照为起点；运行期一旦开始，只接受连续TickRecord，绝不截断旧命令。
                let patch: rynvek_world::WorldPatch = serde_json::from_slice(&bytes)
                    .map_err(|_| crate::transactions::conflict("未知或损坏的权威日志"))?;
                if !ticks.is_empty()
                    || Some(patch.base_authoring_revision) != commit.parent_revision
                    || commit.snapshot_cid.is_none()
                {
                    return Err(crate::transactions::conflict(
                        "运行日志中出现非Actor创作或缺少真实锚点，不允许截断历史",
                    ));
                }
                origin = commit;
            }
        }
        let cid = origin
            .snapshot_cid
            .ok_or_else(|| crate::transactions::conflict("运行起点没有完整快照"))?;
        let initial_snapshot = snapshots.get(&Blake3Hash(cid))?;
        if Blake3Hash::of(&initial_snapshot).0 != cid {
            return Err(crate::transactions::conflict("起点CID不符"));
        }
        let history = crate::replay::RuntimeReplay {
            world_id: world_id.0,
            initial_revision: origin.world_revision,
            initial_root: origin.state_root_blake3,
            initial_snapshot,
            ticks,
        };
        let body = crate::replay::record_world_history(
            history,
            expected_tick,
            expected_root,
            current.body.match_id,
            current.body.ruleset_hash,
            current.body.participants,
        )
        .map_err(|e| ApiError::new(e.code, e.message))?;
        crate::replay::encode(&body, world_id.0, &signer).map_err(|e| ApiError::new(e.code, e.message))
    })
    .await
    .map_err(|_| ApiError::internal("完整世界归档线程失败"))??;
    // 完整回放包含私有基因和规则，绝不能进入 Hub 公共 CAS。
    let replay_cid = state.inner.world_snapshots.put(&replay_bytes)?;
    let Some((mut row, participants)) = store.get_match(c.match_id).await? else {
        return Err(ApiError::not_found("对局不存在"));
    };
    if row.match_state == MatchState::Completed && row.replay_cid_blake3 == Some(replay_cid.0) {
        return Ok(());
    }
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

    if row.game_mode == "PvP_Ranked" {
        let parts = participants
            .iter()
            .map(|p| ParticipantRow {
                final_score: c
                    .scores
                    .iter()
                    .find(|s| i16::from(s.0) == p.faction_id)
                    .map_or(0, |s| s.2),
                ..p.clone()
            })
            .collect();
        store
            .settle_ranked(
                row.clone(),
                c.fencing_token
                    .ok_or_else(|| crate::transactions::conflict("终局缺少fence"))?,
                parts,
            )
            .await?;
    } else {
        // 自定义世界可以创作编辑，但不能据此制造排位分数。
        for participant in &participants {
            let final_score = c
                .scores
                .iter()
                .find(|s| i16::from(s.0) == participant.faction_id)
                .map_or(0, |s| s.2);
            store
                .update_participant(ParticipantRow {
                    final_score,
                    rating_delta: None,
                    ..participant.clone()
                })
                .await?;
        }
    }
    if row.game_mode != "PvP_Ranked" {
        store
            .update_match_fenced(
                row,
                c.fencing_token
                    .ok_or_else(|| crate::transactions::conflict("终局缺少fence"))?,
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
