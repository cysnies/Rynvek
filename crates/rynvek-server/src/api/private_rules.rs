// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 原身份/私库上的规则发布、发现、下载与撤销；发布/下载绝不隐式安装或执行。
use crate::{
    app::AppState,
    error::{ApiError, ApiResult},
    ids::{hex, parse_hex},
    rule_package::{self as rules, RuleReleaseManifest, SignedRuleRelease},
    store::PrivateArtifactRow,
    transactions::conflict,
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use rynvek_protocol::CapabilityScope;
use serde::Deserialize;
use serde_json::{Value, json};
fn private(value: Value) -> Response {
    (
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::VARY, "Authorization"),
        ],
        Json(value),
    )
        .into_response()
}
fn metadata(row: &PrivateArtifactRow) -> ApiResult<Value> {
    Ok(
        json!({"release":rules::release_of(row)?,"dependencies":row.dependencies,"verification":row.verification,"revoked":row.revoked,"visibility":"Private"}),
    )
}
async fn subject(app: &AppState, headers: &HeaderMap, scope: CapabilityScope) -> ApiResult<u64> {
    let s = app.require(headers, scope).await?;
    if !app
        .store()
        .get_principal(s.principal_id)
        .await?
        .is_some_and(|p| p.status == rynvek_security::PrincipalStatus::Active)
    {
        return Err(ApiError::forbidden("私有规则主体已失效"));
    }
    Ok(s.principal_id)
}
async fn row(app: &AppState, owner: u64, id: String, version: String) -> ApiResult<PrivateArtifactRow> {
    app.store()
        .private_rule(owner, id, version)
        .await?
        .ok_or_else(|| ApiError::not_found("私有规则版本不存在"))
}
async fn slot(app: &AppState) -> ApiResult<tokio::sync::OwnedSemaphorePermit> {
    app.inner
        .agent_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| conflict("私有制品复验名额已满，请显式重试"))
}
pub async fn list(State(app): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let owner = subject(&app, &headers, CapabilityScope::WorldRead).await?;
    Ok(private(
        json!({"packages":app.store().private_rules(owner).await?.iter().map(metadata).collect::<ApiResult<Vec<_>>>()?,"visibility":"Private"}),
    ))
}
pub async fn get(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let owner = subject(&app, &headers, CapabilityScope::WorldRead).await?;
    Ok(private(metadata(&row(&app, owner, id, version).await?)?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Publish {
    artifact_b64: String,
    manifest: RuleReleaseManifest,
    signature: String,
}
pub async fn publish(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Response> {
    let owner = subject(&app, &headers, CapabilityScope::HubPublish).await?;
    if body.len() > 1536 * 1024 {
        return Err(ApiError::bad_request("规则发布请求超过1536KiB"));
    }
    let _slot = slot(&app).await?;
    let req: Publish = super::parse_json(&body)?;
    if req.manifest.package_id != id || req.manifest.version != version || req.manifest.owner_id != owner.to_string() {
        return Err(ApiError::bad_request("路径/私有主体与签名发布身份不符"));
    }
    let publisher = app
        .store()
        .get_principal(owner)
        .await?
        .ok_or_else(|| ApiError::forbidden("发布主体不存在"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(req.artifact_b64)
        .map_err(|_| ApiError::bad_request("规则制品Base64非法"))?;
    let signed = SignedRuleRelease {
        manifest: req.manifest,
        publisher_key: hex(&publisher.ed25519_pubkey),
        signature: req.signature,
    };
    let signed_copy = signed.clone();
    let key = publisher.ed25519_pubkey;
    let bytes = tokio::task::spawn_blocking(move || {
        rules::verify_download(owner, &key, &signed_copy, &bytes)?;
        Ok::<_, ApiError>(bytes)
    })
    .await
    .map_err(|_| ApiError::internal("规则制品复验失败"))??;
    let cid = app.inner.world_snapshots.put(&bytes)?;
    let mut row = PrivateArtifactRow {
        owner_id: owner,
        package_id: id.clone(),
        version: version.clone(),
        cid: cid.0,
        signature: parse_hex::<64>(&signed.signature)
            .ok_or_else(|| ApiError::bad_request("签名非法"))?
            .to_vec(),
        publisher_key: publisher.ed25519_pubkey,
        profile: serde_json::to_value(&signed.manifest).expect("定型清单"),
        dependencies: vec![],
        verification: json!({"graph_rir_equivalence":true,"ts_source_equivalence":false,"private_release_signature":true,"dependency_mode":"explicit-coinstall-exact-dag"}),
        revoked: false,
    };
    let resolved = rules::resolve(&app, owner, &signed.manifest.reference(), Some(row.clone())).await?;
    row.dependencies = resolved.pins.into_iter().filter(|p| p.id != id).collect();
    subject(&app, &headers, CapabilityScope::HubPublish).await?;
    // 原私库发布锁内复验全部依赖/Active/登记公钥/不可变墓碑；CAS孤儿不产生任何可发现版本。
    app.store().put_private_rule(row.clone()).await?;
    app.audit(
        Some(owner),
        "private_rule.publish",
        format!("rule:{id}@{version}"),
        json!({"visibility":"Private"}),
    )
    .await;
    Ok((StatusCode::CREATED, private(metadata(&row)?)).into_response())
}
pub async fn resolved(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let owner = subject(&app, &headers, CapabilityScope::WorldRead).await?;
    let _slot = slot(&app).await?;
    let row = row(&app, owner, id, version).await?;
    let root = rules::release_of(&row)?.manifest.reference();
    let resolved = rules::resolve(&app, owner, &root, None).await?;
    Ok(private(
        json!({"root":root,"releases":resolved.releases,"pins":resolved.pins,"visibility":"Private","installed":false}),
    ))
}
pub async fn artifact(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let owner = subject(&app, &headers, CapabilityScope::WorldRead).await?;
    let _slot = slot(&app).await?;
    let row = row(&app, owner, id, version).await?;
    rules::resolve(&app, owner, &rules::release_of(&row)?.manifest.reference(), None).await?;
    let bytes = app.inner.world_snapshots.get(&rynvek_core::Blake3Hash(row.cid))?;
    if rynvek_core::Blake3Hash::of(&bytes).0 != row.cid || bytes.len() > rules::MAX_CLOSURE_BYTES {
        return Err(conflict("下载制品摘要/大小不符"));
    }
    Ok((
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::VARY, "Authorization"),
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
}
pub async fn revoke(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let owner = subject(&app, &headers, CapabilityScope::HubPublish).await?;
    if !app
        .store()
        .revoke_private_rule(owner, id.clone(), version.clone())
        .await?
    {
        return Err(ApiError::not_found("私有规则版本不存在"));
    }
    app.audit(
        Some(owner),
        "private_rule.revoke",
        format!("rule:{id}@{version}"),
        json!({}),
    )
    .await;
    Ok(private(json!({"revoked":true,"package_id":id,"version":version})))
}
