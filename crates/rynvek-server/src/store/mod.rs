// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 服务端权威数据存储。行类型与规范 11.1 PostgreSQL 17 DDL（`rynvek-storage/migrations/postgres/0001_initial_schema.sql`）
//! 一一对应，两种后端行为一致：
//!
//! - [`MemoryStore`]：进程内实现，单机开发与全部核心测试默认使用，无需任何外部服务；
//! - [`PostgresStore`]：`tokio-postgres` 实现，直接执行规范 DDL；测试会自动拉起一次性本地实例（见 `tests/postgres_store.rs`）。
//!
//! Blob（回放、包产物、提交日志段）不入关系库，统一走 `rynvek-storage` 的内容寻址 [`BlobStore`](rynvek_storage::BlobStore)。

mod memory;
mod postgres;

pub use memory::MemoryStore;
pub use postgres::{POSTGRES_SCHEMA_SQL, PostgresStore};

use rynvek_protocol::WireUuid;
use rynvek_security::{League, PrincipalKindCode, PrincipalStatus, Visibility};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;

/// `principals`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalRow {
    pub principal_id: u64,
    pub handle: String,
    pub kind: PrincipalKindCode,
    pub ed25519_pubkey: [u8; 32],
    pub email_hash: Option<Vec<u8>>,
    pub status: PrincipalStatus,
}

/// `organizations`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgRow {
    pub org_id: u64,
    pub slug: String,
    pub display_name: String,
    pub owner_principal_id: u64,
    pub quota_max_worlds: i32,
    pub quota_storage_bytes: i64,
}

/// 官方组织 slug：`rynvek.*` 包命名空间的唯一合法发布者（TrustLevel 1）
pub const OFFICIAL_ORG_SLUG: &str = "rynvek";

impl OrgRow {
    pub fn is_official(&self) -> bool {
        self.slug == OFFICIAL_ORG_SLUG
    }
}

/// `worlds`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldRow {
    pub world_id: WireUuid,
    pub org_id: u64,
    pub owner_principal_id: u64,
    pub title: String,
    pub world_profile: String,
    pub game_mode: String,
    pub execution_profile: String,
    pub visibility: Visibility,
    pub parent_world_id: Option<WireUuid>,
    pub forked_at_revision: Option<u64>,
    pub latest_tick: u64,
    pub latest_revision: u64,
    pub latest_state_root: [u8; 32],
    pub lease_holder_node: Option<String>,
    pub fencing_token: u64,
}

impl WorldRow {
    /// 明确的单Chunk受限World服务器剖面，不借用排位/无限资源沙盒语义。
    pub fn is_controlled_general(&self) -> bool {
        self.world_profile == "General_Circle_v1"
            && self.game_mode == "General_Controlled"
            && self.execution_profile == "D0_Strict"
            && self.visibility == rynvek_security::Visibility::Private
    }
}

/// `world_commits`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitRow {
    pub world_id: WireUuid,
    pub world_revision: u64,
    pub tick: u64,
    pub parent_revision: Option<u64>,
    pub state_root_blake3: [u8; 32],
    pub journal_segment_cid: [u8; 32],
    pub snapshot_cid: Option<[u8; 32]>,
}

/// `packages`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageRow {
    pub package_id: String,
    pub org_id: u64,
    pub package_kind: String,
    /// 0 Builtin / 1 Official / 2 Verified / 3 Community
    pub trust_level: i16,
    pub revoked: bool,
}

/// `packages.package_kind` 合法取值（DDL CHECK）
pub const PACKAGE_KINDS: [&str; 6] = ["rworld", "rpack", "rview", "audio", "agent", "dataset"];

/// `package_versions`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PackageVersionRow {
    pub package_id: String,
    pub semver: String,
    pub min_engine_version: String,
    pub license_spdx: String,
    pub artifact_cid_blake3: [u8; 32],
    pub artifact_size_bytes: u64,
    #[serde(with = "serde_sig")]
    pub ed25519_signature: [u8; 64],
    pub manifest_json: serde_json::Value,
    pub lockfile_json: serde_json::Value,
}

mod serde_sig {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&rynvek_security::to_hex(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        let text = String::deserialize(d)?;
        rynvek_security::from_hex(&text)
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| serde::de::Error::custom("签名必须为 64 字节十六进制"))
    }
}

/// `matches.match_state`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MatchState {
    Created,
    Lobby,
    Ready,
    Running,
    Completed,
    Aborted,
}

impl MatchState {
    pub const ALL: [Self; 6] = [
        Self::Created,
        Self::Lobby,
        Self::Ready,
        Self::Running,
        Self::Completed,
        Self::Aborted,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "Created",
            Self::Lobby => "Lobby",
            Self::Ready => "Ready",
            Self::Running => "Running",
            Self::Completed => "Completed",
            Self::Aborted => "Aborted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == s)
    }

    /// 合法状态迁移（D645–D657 对局状态机）：`Created → Lobby → Ready → Running → Completed`，
    /// 任意未终结状态都可 `→ Aborted`
    pub const fn can_transition(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Created, Self::Lobby)
                | (Self::Lobby, Self::Ready)
                | (Self::Ready, Self::Running)
                | (Self::Running, Self::Completed)
                | (Self::Created | Self::Lobby | Self::Ready | Self::Running, Self::Aborted)
        )
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Aborted)
    }
}

/// `matches`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchRow {
    pub match_id: WireUuid,
    pub world_id: WireUuid,
    pub game_mode: String,
    pub ruleset_blake3: [u8; 32],
    pub map_seed: u64,
    pub match_state: MatchState,
    pub winner_faction_id: Option<i16>,
    pub win_reason: Option<String>,
    pub total_ticks: u64,
    pub replay_cid_blake3: Option<[u8; 32]>,
    pub started_at_unix_s: Option<u64>,
    pub completed_at_unix_s: Option<u64>,
}

/// `match_participants`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParticipantRow {
    pub match_id: WireUuid,
    pub faction_id: i16,
    pub principal_id: u64,
    pub controller_league: League,
    pub final_score: i64,
    pub rating_delta: Option<f64>,
}

/// `glicko2_ratings`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RatingRow {
    pub principal_id: u64,
    pub league: League,
    pub rating: f64,
    pub deviation: f64,
    pub volatility: f64,
    pub matches_played: i32,
}

impl RatingRow {
    pub fn fresh(principal_id: u64, league: League) -> Self {
        let r = crate::glicko2::Rating::default();
        Self {
            principal_id,
            league,
            rating: r.rating,
            deviation: r.deviation,
            volatility: r.volatility,
            matches_played: 0,
        }
    }

    pub fn glicko(&self) -> crate::glicko2::Rating {
        crate::glicko2::Rating {
            rating: self.rating,
            deviation: self.deviation,
            volatility: self.volatility,
        }
    }
}

/// `audit_logs`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuditRow {
    pub log_id: i64,
    pub occurred_at_unix_s: u64,
    pub principal_id: Option<u64>,
    pub action_type: String,
    pub target_resource: String,
    pub client_ip: Option<std::net::IpAddr>,
    pub details_json: serde_json::Value,
}

/// 写入审计日志的输入
#[derive(Clone, Debug, PartialEq)]
pub struct NewAudit {
    pub principal_id: Option<u64>,
    pub action_type: String,
    pub target_resource: String,
    pub client_ip: Option<std::net::IpAddr>,
    pub details_json: serde_json::Value,
}

/// 后端错误统一转为 API 错误
pub type StoreResult<T> = Result<T, ApiError>;

/// 持久观战准入，不是控制席位，也不授予世界源码/归档访问权。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserverGrant {
    pub world_id: WireUuid,
    pub principal_id: u64,
    pub faction: u8,
    pub grant_id: WireUuid,
    pub active: bool,
}

/// 每个自创PvP世界一份冻结席位生命周期；关闭后须显式fork/新建，不热换玩家或抹历史。
/// 定向Studio源码/创作准入，不授予WorldRead、控制、分叉或归档。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EditorGrant { pub world_id: WireUuid, pub principal_id: u64, pub grant_id: WireUuid, pub active: bool }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorPermit { pub principal_id: u64, pub grant_id: Option<WireUuid> }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomRoom {
    pub world_id: WireUuid,
    pub version: WireUuid,
    pub guest_id: u64,
    pub match_id: WireUuid,
    pub accepted: bool,
    pub active: bool,
    pub closed: bool,
}

/// 私有不可变制品版本；与公共Hub和公共Blob索引隔离，owner为唯一读取/撤销者。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrivateArtifactRow {
    pub owner_id: u64,
    pub package_id: String,
    pub version: String,
    pub cid: [u8; 32],
    pub signature: Vec<u8>,
    pub publisher_key: [u8; 32],
    pub profile: serde_json::Value,
    pub dependencies: Vec<crate::agent_package::AgentDependency>,
    pub verification: serde_json::Value,
    pub revoked: bool,
}

/// 存储后端（枚举分发，避免 `dyn` + async trait 的装箱开销与额外依赖）
#[derive(Clone, Debug)]
pub enum Store {
    Memory(MemoryStore),
    Postgres(PostgresStore),
}

macro_rules! dispatch {
    ($self:ident . $m:ident ( $($a:expr),* )) => {
        match $self {
            Store::Memory(s) => s.$m($($a),*),
            Store::Postgres(s) => s.$m($($a),*).await,
        }
    };
}

impl Store {
    pub async fn agent_run_create(&self, row: crate::agent_runs::AgentRun) -> StoreResult<crate::agent_runs::AgentRun> {
        dispatch!(self.agent_run_create(row))
    }
    pub async fn agent_run_get(&self, owner: u64, id: String) -> StoreResult<Option<crate::agent_runs::AgentRun>> {
        dispatch!(self.agent_run_get(owner, id))
    }
    pub async fn agent_run_list(&self, owner: u64) -> StoreResult<Vec<crate::agent_runs::AgentRun>> {
        dispatch!(self.agent_run_list(owner))
    }
    pub async fn agent_run_change(
        &self,
        owner: u64,
        id: String,
        change: crate::agent_runs::Change,
    ) -> StoreResult<(crate::agent_runs::AgentRun, bool)> {
        dispatch!(self.agent_run_change(owner, id, change))
    }

    pub async fn lab_submit(&self, job: crate::lab_jobs::LabJob) -> StoreResult<crate::lab_jobs::Experiment> {
        dispatch!(self.lab_submit(job))
    }
    pub async fn lab_get(&self, owner: u64, id: String) -> StoreResult<Option<crate::lab_jobs::LabJob>> {
        dispatch!(self.lab_get(owner, id))
    }
    pub async fn lab_list(&self, owner: u64, limit: u32, offset: u32) -> StoreResult<Vec<crate::lab_jobs::Experiment>> {
        dispatch!(self.lab_list(owner, limit, offset))
    }
    pub async fn lab_claim(&self, node: String) -> StoreResult<Option<crate::lab_jobs::LabJob>> {
        dispatch!(self.lab_claim(node))
    }
    pub async fn lab_control(
        &self,
        owner: u64,
        id: String,
        op: crate::lab_jobs::OwnerAction,
    ) -> StoreResult<crate::lab_jobs::Experiment> {
        dispatch!(self.lab_control(owner, id, op))
    }
    pub async fn lab_worker(
        &self,
        claim: crate::lab_jobs::Claim,
        action: crate::lab_jobs::WorkerAction,
    ) -> StoreResult<crate::lab_jobs::LabJob> {
        dispatch!(self.lab_worker(claim, action))
    }

    pub async fn put_private_agent(&self, row: PrivateArtifactRow) -> StoreResult<()> {
        dispatch!(self.put_private_agent(row))
    }
    pub async fn private_agent(&self, owner: u64, id: String, version: String) -> StoreResult<Option<PrivateArtifactRow>> {
        dispatch!(self.private_agent(owner, id, version))
    }
    pub async fn private_agents(&self, owner: u64) -> StoreResult<Vec<PrivateArtifactRow>> {
        dispatch!(self.private_agents(owner))
    }
    pub async fn revoke_private_agent(&self, owner: u64, id: String, version: String) -> StoreResult<bool> {
        dispatch!(self.revoke_private_agent(owner, id, version))
    }
    pub async fn put_private_audio(&self, row: PrivateArtifactRow) -> StoreResult<()> {
        dispatch!(self.put_private_audio(row))
    }
    pub async fn private_audio(&self, owner: u64, id: String, version: String) -> StoreResult<Option<PrivateArtifactRow>> {
        dispatch!(self.private_audio(owner, id, version))
    }
    pub async fn private_audios(&self, owner: u64) -> StoreResult<Vec<PrivateArtifactRow>> {
        dispatch!(self.private_audios(owner))
    }
    pub async fn revoke_private_audio(&self, owner: u64, id: String, version: String) -> StoreResult<bool> {
        dispatch!(self.revoke_private_audio(owner, id, version))
    }
    pub async fn put_private_visual(&self, row: PrivateArtifactRow) -> StoreResult<()> {
        dispatch!(self.put_private_visual(row))
    }
    pub async fn private_visual(&self, owner: u64, id: String, version: String) -> StoreResult<Option<PrivateArtifactRow>> {
        dispatch!(self.private_visual(owner, id, version))
    }
    pub async fn private_visuals(&self, owner: u64) -> StoreResult<Vec<PrivateArtifactRow>> {
        dispatch!(self.private_visuals(owner))
    }
    pub async fn revoke_private_visual(&self, owner: u64, id: String, version: String) -> StoreResult<bool> {
        dispatch!(self.revoke_private_visual(owner, id, version))
    }

    pub async fn settle_ranked(&self, row: MatchRow, fence: u64, parts: Vec<ParticipantRow>) -> StoreResult<()> {
        dispatch!(self.settle_ranked(row, fence, parts))
    }

    pub async fn ranked_queue(
        &self,
        request: crate::ranked::QueueCommand,
        genesis: Option<crate::ranked::RankedGenesis>,
    ) -> StoreResult<crate::ranked::QueueDecision> {
        dispatch!(self.ranked_queue(request, genesis))
    }
    pub async fn ranked_host(&self, world: WireUuid) -> StoreResult<Option<crate::ranked::HostLease>> {
        dispatch!(self.ranked_host(world))
    }
    pub async fn claim_ranked_host(&self, claim: crate::ranked::HostClaim) -> StoreResult<crate::ranked::HostLease> {
        dispatch!(self.claim_ranked_host(claim))
    }
    pub async fn renew_ranked_host(&self, world: WireUuid, fence: u64) -> StoreResult<bool> {
        dispatch!(self.renew_ranked_host(world, fence))
    }
    pub async fn release_ranked_hosts(&self, node: String) -> StoreResult<()> {
        dispatch!(self.release_ranked_hosts(node))
    }

    pub fn memory() -> Self {
        Self::Memory(MemoryStore::new())
    }

    pub const fn backend_name(&self) -> &'static str {
        match self {
            Self::Memory(_) => "memory",
            Self::Postgres(_) => "postgres",
        }
    }

    // ---- principals
    pub async fn insert_principal(&self, row: PrincipalRow) -> StoreResult<()> {
        dispatch!(self.insert_principal(row))
    }
    pub async fn get_principal(&self, id: u64) -> StoreResult<Option<PrincipalRow>> {
        dispatch!(self.get_principal(id))
    }
    /// 账户注销：状态置 `Deleted`、匿名化 handle、清空邮箱哈希、公钥置零
    pub async fn tombstone_principal(&self, id: u64) -> StoreResult<()> {
        dispatch!(self.tombstone_principal(id))
    }

    // ---- organizations
    pub async fn insert_org(&self, row: OrgRow) -> StoreResult<()> {
        dispatch!(self.insert_org(row))
    }
    pub async fn get_org(&self, id: u64) -> StoreResult<Option<OrgRow>> {
        dispatch!(self.get_org(id))
    }
    pub async fn orgs_owned_by(&self, principal_id: u64) -> StoreResult<Vec<OrgRow>> {
        dispatch!(self.orgs_owned_by(principal_id))
    }

    // ---- worlds
    pub async fn insert_world(&self, row: WorldRow, genesis: CommitRow) -> StoreResult<()> {
        dispatch!(self.insert_world(row, genesis))
    }
    pub async fn get_world(&self, id: WireUuid) -> StoreResult<Option<WorldRow>> {
        dispatch!(self.get_world(id))
    }
    /// 列出 `viewer` 可见的世界：自己拥有的 + `Public`（`Unlisted` 不出现在列表里）；按 `world_id` 降序（UUIDv7 ≈ 时间倒序）
    pub async fn list_worlds(&self, viewer: Option<u64>, limit: u32, offset: u32) -> StoreResult<Vec<WorldRow>> {
        dispatch!(self.list_worlds(viewer, limit, offset))
    }
    pub async fn list_commits(&self, id: WireUuid) -> StoreResult<Vec<CommitRow>> {
        dispatch!(self.list_commits(id))
    }
    /// 追加提交并推进世界的 `latest_*`；`expected_fencing_token` 不符时返回 `0x7005`
    /// 精确修订与 fencing CAS 认领宿主；在途的旧 HTTP 候选将无法再发布。
    pub async fn claim_world(&self, id: WireUuid, revision: u64, token: u64, holder: String) -> StoreResult<u64> {
        dispatch!(self.claim_world(id, revision, token, holder))
    }

    pub async fn append_commit(&self, commit: CommitRow, expected_fencing_token: u64) -> StoreResult<()> {
        dispatch!(self.append_commit(commit, expected_fencing_token))
    }

    pub async fn custom_live(&self, world: WireUuid, match_id: WireUuid, fence: u64) -> StoreResult<bool> {
        dispatch!(self.custom_live(world, match_id, fence))
    }
    pub async fn custom_room(&self, world: WireUuid) -> StoreResult<Option<CustomRoom>> {
        dispatch!(self.custom_room(world))
    }
    /// CAS房间；首次激活同时插入完整MatchRow/participants，关闭同时轮换世界fence。
    pub async fn save_custom_room(
        &self,
        room: CustomRoom,
        expected: Option<WireUuid>,
        activation: Option<(MatchRow, Vec<ParticipantRow>)>,
    ) -> StoreResult<()> {
        dispatch!(self.save_custom_room(room, expected, activation))
    }

    pub async fn editor_grants(&self, world: WireUuid) -> StoreResult<Vec<EditorGrant>> {dispatch!(self.editor_grants(world))}
    pub async fn change_editor(&self, owner:u64, grant:EditorGrant, expected:Option<WireUuid>) -> StoreResult<()> {dispatch!(self.change_editor(owner,grant,expected))}
    pub async fn check_editor(&self,world:WireUuid,permit:EditorPermit)->StoreResult<()> {dispatch!(self.check_editor(world,permit))}
    pub async fn append_commit_checked(&self,commit:CommitRow,fence:u64,permits:Vec<EditorPermit>)->StoreResult<()> {dispatch!(self.append_commit_checked(commit,fence,permits))}

    pub async fn observer_grants(&self, world: WireUuid) -> StoreResult<Vec<ObserverGrant>> {
        dispatch!(self.observer_grants(world))
    }
    pub async fn set_observer_grant(&self, grant: ObserverGrant) -> StoreResult<()> {
        dispatch!(self.set_observer_grant(grant))
    }
    pub async fn revoke_observer(&self, world: WireUuid, principal: u64, grant: WireUuid) -> StoreResult<bool> {
        dispatch!(self.revoke_observer(world, principal, grant))
    }

    // ---- packages
    pub async fn upsert_package(&self, row: PackageRow) -> StoreResult<()> {
        dispatch!(self.upsert_package(row))
    }
    pub async fn get_package(&self, id: &str) -> StoreResult<Option<PackageRow>> {
        dispatch!(self.get_package(id))
    }
    /// 写入新版本；同一 `(package_id, semver)` 已存在时返回 `0x9003`
    pub async fn insert_package_version(&self, row: PackageVersionRow) -> StoreResult<()> {
        dispatch!(self.insert_package_version(row))
    }
    pub async fn package_versions(&self, id: &str) -> StoreResult<Vec<PackageVersionRow>> {
        dispatch!(self.package_versions(id))
    }
    pub async fn search_packages(&self, kind: Option<&str>, query: Option<&str>) -> StoreResult<Vec<PackageRow>> {
        dispatch!(self.search_packages(kind, query))
    }

    // ---- matches
    pub async fn insert_match(&self, row: MatchRow, participants: Vec<ParticipantRow>) -> StoreResult<()> {
        dispatch!(self.insert_match(row, participants))
    }
    pub async fn get_match(&self, id: WireUuid) -> StoreResult<Option<(MatchRow, Vec<ParticipantRow>)>> {
        dispatch!(self.get_match(id))
    }
    /// 整行更新（状态机迁移由调用方校验）
    pub async fn update_match(&self, row: MatchRow) -> StoreResult<()> {
        dispatch!(self.update_match(row))
    }
    pub async fn update_match_fenced(&self, row: MatchRow, fencing_token: u64) -> StoreResult<()> {
        dispatch!(self.update_match_fenced(row, fencing_token))
    }
    pub async fn update_participant(&self, row: ParticipantRow) -> StoreResult<()> {
        dispatch!(self.update_participant(row))
    }
    pub async fn matches_of(&self, principal_id: u64) -> StoreResult<Vec<MatchRow>> {
        dispatch!(self.matches_of(principal_id))
    }

    // ---- ratings
    pub async fn get_rating(&self, principal_id: u64, league: League) -> StoreResult<RatingRow> {
        dispatch!(self.get_rating(principal_id, league))
    }
    pub async fn put_rating(&self, row: RatingRow) -> StoreResult<()> {
        dispatch!(self.put_rating(row))
    }
    /// 排行榜：按 `rating` 降序，同分按 `principal_id` 升序；只含状态为 Active 的主体
    pub async fn leaderboard(&self, league: League, limit: u32) -> StoreResult<Vec<(RatingRow, String)>> {
        dispatch!(self.leaderboard(league, limit))
    }

    // ---- audit
    pub async fn append_audit(&self, row: NewAudit) -> StoreResult<()> {
        dispatch!(self.append_audit(row))
    }
    pub async fn audit_of(&self, principal_id: u64) -> StoreResult<Vec<AuditRow>> {
        dispatch!(self.audit_of(principal_id))
    }
}
