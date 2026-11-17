// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 路由 `04`–`09`：世界列表、创建、提交链、Copy-on-Write 分叉、WorldPatch 提交与实时流升级。

use axum::Json;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use rynvek_core::error::RynvekErrorCode;
use rynvek_engine::ArenaRuntime;
use rynvek_network::NetError;
use rynvek_network::link::{Link, TransportKind};
use rynvek_protocol::{CapabilityScope, WireUuid};
use rynvek_rts::{ArenaWorld, MatchConfig};
use rynvek_security::{Resource, Subject, Visibility, authorize};
use serde::Deserialize;

use crate::app::AppState;
use crate::error::{ApiError, ApiResult};
use crate::ids::{hex, new_uuid_v7};
use crate::patch::{LiveEditSafetyClass, WorldPatch};
use crate::store::{CommitRow, WorldRow};
use crate::stream::{parse_world_id, serve_link};

/// 单页上限
pub const MAX_PAGE: u32 = 100;
/// WorldPatch 请求体上限（1 MiB）
pub const MAX_PATCH_BYTES: usize = 1 << 20;

#[derive(Debug, Deserialize)]
pub struct Page {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    offset: Option<u32>,
}

fn world_json(w: &WorldRow) -> serde_json::Value {
    serde_json::json!({
        "world_id": w.world_id.to_string(),
        "org_id": w.org_id.to_string(),
        "owner_principal_id": w.owner_principal_id.to_string(),
        "title": w.title,
        "world_profile": w.world_profile,
        "game_mode": w.game_mode,
        "execution_profile": w.execution_profile,
        "visibility": w.visibility,
        "parent_world_id": w.parent_world_id.map(|p| p.to_string()),
        "forked_at_revision": w.forked_at_revision,
        "latest_tick": w.latest_tick,
        "latest_revision": w.latest_revision,
        "latest_state_root": hex(&w.latest_state_root),
    })
}

fn world_resource(w: &WorldRow) -> Resource {
    Resource::World {
        owner_principal_id: w.owner_principal_id,
        org_id: w.org_id,
        visibility: w.visibility,
    }
}

async fn load_world(app: &AppState, subject: &Subject, id: &str, scope: CapabilityScope) -> ApiResult<WorldRow> {
    let world_id = parse_world_id(id)?;
    let world = app
        .store()
        .get_world(world_id)
        .await?
        .ok_or_else(|| ApiError::not_found("世界不存在"))?;
    // 无读权限时与 "不存在" 同样返回 404，避免探测私有世界
    if authorize(subject, CapabilityScope::WorldRead, &world_resource(&world)).is_err() {
        return Err(ApiError::not_found("世界不存在"));
    }
    authorize(subject, scope, &world_resource(&world)).map_err(ApiError::from)?;
    Ok(world)
}

/// `04 GET /api/v1/worlds`（`world.list`）：自己拥有的 + 公开世界，按创建时间倒序分页
pub async fn list(State(app): State<AppState>, headers: HeaderMap, Query(page): Query<Page>) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldList).await?;
    authorize(
        &subject,
        CapabilityScope::WorldList,
        &Resource::Org {
            org_id: subject.principal_id,
        },
    )
    .map_err(ApiError::from)?;
    let limit = page.limit.unwrap_or(20).clamp(1, MAX_PAGE);
    let offset = page.offset.unwrap_or(0);
    let worlds = app
        .store()
        .list_worlds(Some(subject.principal_id), limit, offset)
        .await?;
    Ok(Json(serde_json::json!({
        "worlds": worlds.iter().map(world_json).collect::<Vec<_>>(),
        "limit": limit,
        "offset": offset,
    }))
    .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateWorld {
    title: String,
    /// 预设模板：`arena_duel`（竞技场 1v1）或 `arena_pve`（虫潮生存）
    template: String,
    #[serde(default)]
    org_id: Option<String>,
    #[serde(default)]
    visibility: Option<Visibility>,
    #[serde(default)]
    map_seed: Option<u64>,
}

/// `.rworld` 上传时通过查询串传递的元数据（请求体是二进制包）
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadQuery {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    org_id: Option<String>,
    #[serde(default)]
    visibility: Option<Visibility>,
}

/// `05 POST /api/v1/worlds`（`world.create`）
///
/// 两种请求体：
/// - `application/json`：预设模板 `{title, template, org_id?, visibility?, map_seed?}`；
/// - `application/vnd.rynvek.rworld`（或 `application/octet-stream`）：`.rworld` 初始包，
///   元数据走查询串 `?title=&org_id=&visibility=`（`title` 缺省取包清单 `name`）。
///   包经完整校验（容器 `0x7001` / `0x7002`、`world.json`），原始字节写入 CAS 并记录在创世清单中。
pub async fn create(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(upload): Query<UploadQuery>,
    body: Bytes,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldCreate).await?;
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let (title, org, visibility, config, mode, manifest) = if content_type.starts_with("application/json") {
        let req: CreateWorld = super::parse_json(&body)?;
        let seed = req.map_seed.unwrap_or(1);
        let (config, mode) = match req.template.as_str() {
            "arena_duel" => (MatchConfig::duel(seed), "PvP_Custom"),
            "arena_pve" => (MatchConfig::pve(seed, 1), "PvE_Survival"),
            other => return Err(ApiError::bad_request(format!("未知模板 `{other}`"))),
        };
        let manifest = serde_json::json!({ "template": req.template, "config": config });
        (req.title, req.org_id, req.visibility, config, mode, manifest)
    } else if content_type.starts_with(rynvek_world::rworld::RWORLD_MEDIA_TYPE)
        || content_type.starts_with("application/octet-stream")
    {
        let bundle = rynvek_world::WorldBundle::from_bytes(&body).map_err(|e| ApiError::new(e.code, e.detail))?;
        let package_cid = app.inner.world_snapshots.put(&body)?;
        let m = &bundle.package.manifest;
        let manifest = serde_json::json!({
            "template": bundle.config.template,
            "config": bundle.match_config,
            "rworld": {
                "id": m.id,
                "version": m.version,
                "license": m.license,
                "content_hash": hex(&bundle.package.content_hash),
                "package_cid": package_cid.to_hex(),
            },
        });
        (
            upload.title.unwrap_or_else(|| m.name.clone()),
            upload.org_id,
            upload.visibility,
            bundle.match_config.clone(),
            bundle.config.game_mode(),
            manifest,
        )
    } else {
        return Err(ApiError::new(
            RynvekErrorCode::InvalidFileMagicOrVersion,
            format!(
                "不支持的 Content-Type `{content_type}`：请使用 application/json（预设模板）或 {}（.rworld 包）",
                rynvek_world::rworld::RWORLD_MEDIA_TYPE
            ),
        ));
    };
    if title.is_empty() || title.chars().count() > 128 {
        return Err(ApiError::bad_request("title 长度必须在 1..=128"));
    }
    let org_id = match &org {
        Some(s) => s
            .parse()
            .map_err(|_| ApiError::bad_request("org_id 必须是十进制整数"))?,
        None => subject.principal_id,
    };
    authorize(&subject, CapabilityScope::WorldCreate, &Resource::Org { org_id }).map_err(ApiError::from)?;
    let initial_world = ArenaWorld::new(config.clone());
    let manifest = serde_json::to_vec(&manifest).expect("可序列化");
    let genesis = app.inner.world_snapshots.put(&manifest)?;
    let world_id = new_uuid_v7();
    let runtime = ArenaRuntime::new(world_id.0, initial_world);
    let checkpoint = runtime
        .checkpoint()
        .map_err(|e| ApiError::new(e, "无法编码创世检查点"))?;
    let root = checkpoint.state_root;
    let snapshot = app.inner.world_snapshots.put(&checkpoint.bytes)?;
    let row = WorldRow {
        world_id,
        org_id,
        owner_principal_id: subject.principal_id,
        title,
        world_profile: "RTS_Standard".to_owned(),
        game_mode: mode.to_owned(),
        execution_profile: "D0_Strict".to_owned(),
        visibility: visibility.unwrap_or(Visibility::Private),
        parent_world_id: None,
        forked_at_revision: None,
        latest_tick: 0,
        latest_revision: 0,
        latest_state_root: root,
        lease_holder_node: None,
        fencing_token: 1,
    };
    app.store()
        .insert_world(
            row.clone(),
            CommitRow {
                world_id,
                world_revision: 0,
                tick: 0,
                parent_revision: None,
                state_root_blake3: root,
                journal_segment_cid: genesis.0,
                snapshot_cid: Some(snapshot.0),
            },
        )
        .await?;
    app.audit(
        Some(subject.principal_id),
        "world.create",
        format!("world:{world_id}"),
        serde_json::json!({ "game_mode": mode, "org_id": org_id }),
    )
    .await;
    Ok((StatusCode::CREATED, Json(world_json(&row))).into_response())
}

/// `06 GET /api/v1/worlds/{world_id}/commits`（`world.read`）
pub async fn commits(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldRead).await?;
    let world = load_world(&app, &subject, &id, CapabilityScope::WorldRead).await?;
    let commits = app.store().list_commits(world.world_id).await?;
    let latest_snapshot = commits.iter().rev().find_map(|c| c.snapshot_cid).map(|c| hex(&c));
    Ok(Json(serde_json::json!({
        "world_id": world.world_id.to_string(),
        "latest_revision": world.latest_revision,
        "latest_snapshot_cid": latest_snapshot,
        "commits": commits.iter().map(|c| serde_json::json!({
            "world_revision": c.world_revision,
            "tick": c.tick,
            "parent_revision": c.parent_revision,
            "state_root_blake3": hex(&c.state_root_blake3),
            "journal_segment_cid": hex(&c.journal_segment_cid),
            "snapshot_cid": c.snapshot_cid.map(|s| hex(&s)),
        })).collect::<Vec<_>>(),
    }))
    .into_response())
}

/// 分叉保留来源记录，但完整检查点必须重新绑定新世界身份与创世修订。
async fn fork_world(
    app: &AppState,
    subject: &Subject,
    source: &WorldRow,
    revision: u64,
    title: Option<String>,
) -> ApiResult<WorldRow> {
    let commits = app.store().list_commits(source.world_id).await?;
    let base = commits
        .iter()
        .find(|c| c.world_revision == revision)
        .ok_or_else(|| ApiError::not_found(format!("修订 {revision} 不存在")))?;
    let mut source_revision = source.clone();
    source_revision.latest_revision = base.world_revision;
    source_revision.latest_tick = base.tick;
    source_revision.latest_state_root = base.state_root_blake3;
    let runtime = restore_runtime(app, &source_revision).await?;
    let world_id = new_uuid_v7();
    let checkpoint = runtime
        .fork(world_id.0)
        .checkpoint()
        .map_err(|e| ApiError::new(e, "无法编码分叉检查点"))?;
    let snapshot = app.inner.world_snapshots.put(&checkpoint.bytes)?;
    let row = WorldRow {
        world_id,
        org_id: subject.principal_id,
        owner_principal_id: subject.principal_id,
        title: title.unwrap_or_else(|| format!("{}（分叉）", source.title)),
        visibility: Visibility::Private,
        parent_world_id: Some(source.world_id),
        forked_at_revision: Some(revision),
        latest_tick: base.tick,
        latest_revision: 0,
        latest_state_root: checkpoint.state_root,
        lease_holder_node: None,
        fencing_token: 1,
        ..source.clone()
    };
    app.store()
        .insert_world(
            row.clone(),
            CommitRow {
                world_id,
                world_revision: 0,
                tick: base.tick,
                parent_revision: None,
                state_root_blake3: checkpoint.state_root,
                journal_segment_cid: base.journal_segment_cid,
                snapshot_cid: Some(snapshot.0),
            },
        )
        .await?;
    app.audit(
        Some(subject.principal_id),
        "world.fork",
        format!("world:{world_id}"),
        serde_json::json!({ "parent": source.world_id.to_string(), "revision": revision }),
    )
    .await;
    Ok(row)
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ForkRequest {
    #[serde(default)]
    world_revision: Option<u64>,
    #[serde(default)]
    title: Option<String>,
}

/// `07 POST /api/v1/worlds/{world_id}/fork`（`world.fork`）
pub async fn fork(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldFork).await?;
    let source = load_world(&app, &subject, &id, CapabilityScope::WorldFork).await?;
    let req: ForkRequest = if body.is_empty() {
        ForkRequest::default()
    } else {
        super::parse_json(&body)?
    };
    let revision = req.world_revision.unwrap_or(source.latest_revision);
    let row = fork_world(&app, &subject, &source, revision, req.title).await?;
    Ok((StatusCode::CREATED, Json(world_json(&row))).into_response())
}

async fn restore_runtime(app: &AppState, world: &WorldRow) -> ApiResult<ArenaRuntime> {
    let commit = app
        .store()
        .list_commits(world.world_id)
        .await?
        .into_iter()
        .find(|c| c.world_revision == world.latest_revision)
        .ok_or_else(|| {
            ApiError::new(
                RynvekErrorCode::SchemaMigrationPathNotFound,
                "缺失权威提交，拒绝从模板猜测",
            )
        })?;
    let cid = commit.snapshot_cid.ok_or_else(|| {
        ApiError::new(
            RynvekErrorCode::SchemaMigrationPathNotFound,
            "旧世界没有完整检查点，需要显式迁移",
        )
    })?;
    let bytes = app.inner.world_snapshots.get(&rynvek_core::Blake3Hash(cid))?;
    let runtime = ArenaRuntime::restore(world.world_id.0, world.latest_revision, world.latest_state_root, &bytes)
        .map_err(|e| ApiError::new(e, "权威检查点与世界元数据不一致"))?;
    if runtime.world().tick != world.latest_tick
        || commit.tick != world.latest_tick
        || commit.state_root_blake3 != world.latest_state_root
    {
        return Err(ApiError::new(
            RynvekErrorCode::Blake3ChecksumMismatch,
            "检查点 Tick 或根不一致",
        ));
    }
    Ok(runtime)
}

/// `08 POST /api/v1/worlds/{world_id}/patches`（`world.edit`）：请求体为规范 10.2 `WorldPatch`
///
/// 在私有检查点上运行真实领域事务，提交成功后才返回 applied。
/// 当前仅支持非实时托管世界；活动对局拒绝旁路编辑，不能制造第二份权威。
pub async fn patches(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldEdit).await?;
    let world = load_world(&app, &subject, &id, CapabilityScope::WorldEdit).await?;
    if body.len() > MAX_PATCH_BYTES {
        return Err(ApiError::bad_request("WorldPatch 超过 1 MiB"));
    }
    let patch: WorldPatch = serde_json::from_slice(&body).map_err(|e| {
        ApiError::new(
            RynvekErrorCode::SimulationInvariantViolated,
            format!("WorldPatch 格式错误：{e}"),
        )
        .with_status(StatusCode::UNPROCESSABLE_ENTITY)
    })?;
    let required = crate::patch::validate(&patch)?;
    if patch.base_authoring_revision != world.latest_revision {
        return Err(ApiError::new(
            RynvekErrorCode::StructuralMutationConflict,
            format!(
                "补丁基于修订 {}，世界已到 {}",
                patch.base_authoring_revision, world.latest_revision
            ),
        ));
    }
    if app
        .inner
        .registry
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .by_world(&world.world_id)
        .is_some()
        || world.lease_holder_node.is_some()
    {
        return Err(ApiError::new(
            RynvekErrorCode::StructuralMutationConflict,
            "活动宿主必须通过自己的权威队列编辑，拒绝旁路补丁",
        ));
    }
    if patch.safety_class == LiveEditSafetyClass::RequiresForkRestart {
        return Err(ApiError::new(
            RynvekErrorCode::PluginCapabilityDenied,
            "分叉重启操作尚未接通，不创建假成功分叉",
        )
        .with_status(StatusCode::UNPROCESSABLE_ENTITY));
    }
    let mut runtime = restore_runtime(&app, &world).await?;
    let (_, scheduled_tick) = crate::patch::schedule(patch.safety_class, world.latest_tick);
    let effective_tick = scheduled_tick.unwrap_or(world.latest_tick);
    let report = runtime
        .transact_at(effective_tick, |arena| patch.apply_at_boundary(arena, effective_tick))
        .map_err(|e| ApiError::new(e, "补丁未应用：整包事务已拒绝").with_status(StatusCode::UNPROCESSABLE_ENTITY))?;
    let checkpoint = runtime
        .checkpoint()
        .map_err(|e| ApiError::new(e, "无法编码事务检查点"))?;
    let snapshot = app.inner.world_snapshots.put(&checkpoint.bytes)?;
    let cid = app.inner.world_snapshots.put(&body)?;
    let revision = checkpoint.revision;
    // 持久化事务是唯一发布点；并发失败只留下不可达 CAS 对象，不更新任何运行中内存副本。
    app.store()
        .append_commit(
            CommitRow {
                world_id: world.world_id,
                world_revision: revision,
                tick: checkpoint.tick,
                parent_revision: Some(world.latest_revision),
                state_root_blake3: checkpoint.state_root,
                journal_segment_cid: cid.0,
                snapshot_cid: Some(snapshot.0),
            },
            world.fencing_token,
        )
        .await?;
    app.audit(
        Some(subject.principal_id),
        "world.patch",
        format!("world:{}", world.world_id),
        serde_json::json!({
            "revision": revision,
            "declared": patch.safety_class,
            "required": required,
            "operations": patch.operations.len(),
        }),
    )
    .await;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "applied",
            "state_root": crate::ids::hex(&checkpoint.state_root),
            "report": report,
            "world_id": world.world_id.to_string(),
            "world_revision": revision,
            "patch_cid": cid.to_hex(),
            "required_class": required,
            "scheduled_for": "applied_tick_boundary",
            "effective_tick": effective_tick,
            "forked_from": null,
        })),
    )
        .into_response())
}

/// `09 WS /api/v1/worlds/{world_id}/stream`（`world.connect`，令牌在首帧 ClientHello 中校验）
pub async fn stream(State(app): State<AppState>, Path(id): Path<String>, ws: WebSocketUpgrade) -> ApiResult<Response> {
    let world_id: WireUuid = parse_world_id(&id)?;
    let max = rynvek_protocol::wire::WIRE_FRAME_HEADER_LEN + rynvek_protocol::wire::MAX_WIRE_PAYLOAD_LEN as usize;
    Ok(ws
        .max_message_size(max)
        .on_upgrade(move |socket| serve_link(app, world_id, axum_link(socket)))
        .into_response())
}

/// axum WebSocket → [`Link`]（一条二进制消息恰好一帧）
pub fn axum_link(socket: WebSocket) -> Link {
    let (sink, stream) = socket.split();
    let sink = sink.with(|bytes: Vec<u8>| async move { Ok::<_, axum::Error>(Message::Binary(bytes.into())) });
    let stream = stream.map(|m| match m {
        Ok(Message::Binary(b)) => Ok(Some(b.to_vec())),
        Ok(Message::Ping(_) | Message::Pong(_)) => Ok(None),
        Ok(Message::Close(_)) => Err(NetError::Closed),
        Ok(Message::Text(_)) => Err(NetError::Payload("实时流只接受二进制 WebSocket 消息".into())),
        Err(e) => Err(NetError::Transport(e.to_string())),
    });
    Link::from_messages(TransportKind::WebSocket, Box::pin(sink), stream)
}
