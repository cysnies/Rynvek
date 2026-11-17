// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 私有数据型音效制品发布/发现/下载/撤销与有界配方解析；无源码/网络音源/自动播放。
use crate::{
    app::AppState,
    audio_package as agent,
    error::{ApiError, ApiResult},
    ids::{hex, parse_hex},
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
fn metadata(row: &PrivateArtifactRow) -> Value {
    json!({"owner_id":row.owner_id.to_string(),"package_id":row.package_id,"version":row.version,"artifact_cid":hex(&row.cid),"publisher_key":hex(&row.publisher_key),"signature":hex(&row.signature),"profile":row.profile,"dependencies":row.dependencies,"verification":row.verification,"revoked":row.revoked,"visibility":"Private"})
}
async fn row(app: &AppState, pid: u64, id: String, version: String) -> ApiResult<PrivateArtifactRow> {
    app.store()
        .private_audio(pid, id, version)
        .await?
        .ok_or_else(|| ApiError::not_found("私有音频版本不存在"))
}
async fn unchanged(app: &AppState, root: &PrivateArtifactRow, pins: &[PrivateArtifactRow]) -> ApiResult<()> {
    for old in std::iter::once(root).chain(pins) {
        let current = row(app, old.owner_id, old.package_id.clone(), old.version.clone()).await?;
        if current.revoked || current.cid != old.cid {
            return Err(agent::reject("音频或精确依赖已撤销/改变"));
        }
    }
    Ok(())
}
pub async fn list(State(app): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldRead).await?;
    let rows = app.store().private_audios(subject.principal_id).await?;
    Ok(private(
        json!({"packages":rows.iter().filter(|r|r.profile["schema"]=="rynvek.audio-pack.v2" && r.profile["runtime"]=="pcm16-recipes/2").map(metadata).collect::<Vec<_>>(),"visibility":"Private"}),
    ))
}
pub async fn get(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldRead).await?;
    Ok(private(metadata(&row(&app, subject.principal_id, id, version).await?)))
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publish {
    artifact_b64: String,
    signature: String,
}
pub async fn publish(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::HubPublish).await?;
    if body.len() > 768 << 10 {
        return Err(agent::reject("音频上传请求超过128KiB"));
    }
    let req: Publish = super::parse_json(&body)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&req.artifact_b64)
        .map_err(|_| agent::reject("制品Base64非法"))?;
    let package = agent::parse(&bytes)?;
    if package.id != id || package.version != version {
        return Err(agent::reject("签名容器与私有发布路径不一致"));
    }
    let signature: [u8; 64] = parse_hex(&req.signature).ok_or_else(|| agent::reject("发布签名必须64字节hex"))?;
    let publisher = app
        .store()
        .get_principal(subject.principal_id)
        .await?
        .ok_or_else(|| ApiError::unauthenticated("发布身份已失效"))?;
    rynvek_security::verify_ed25519(
        &publisher.ed25519_pubkey,
        &agent::signing_message(subject.principal_id, &id, &version, &bytes),
        &signature,
    )
    .map_err(ApiError::from)?;
    if app
        .store()
        .private_audio(subject.principal_id, id.clone(), version.clone())
        .await?
        .is_some()
    {
        return Err(conflict("私有版本不可覆盖，包括撤销墓碑"));
    }
    if app.store().private_audios(subject.principal_id).await?.len() >= 256 {
        return Err(conflict("私有版本配额已满"));
    }
    let _permit = app
        .inner
        .agent_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| conflict("音频扫描/执行名额已满，请稍后显式重试"))?;
    let profile = serde_json::to_value(&package.profile).unwrap();
    let resolved = agent::resolve(&app, subject.principal_id, package).await?;
    let pins = resolved.pins.clone();
    let _permit = _permit;
    let report = json!({"trust_level":3,"profile":"pcm16-recipes/2","code":false,"network":false,"arbitrary_media_decoders":false,"platform_codecs":false,"bounded_pcm16":true,"sample_count":resolved.samples.len(),"recipe_events":resolved.recipes.as_object().unwrap().len(),"manifest_events":resolved.manifest["events"].as_object().unwrap().len()});
    app.require(&headers, CapabilityScope::HubPublish).await?;
    let cid = app.inner.world_snapshots.put(&bytes)?;
    let row = PrivateArtifactRow {
        owner_id: subject.principal_id,
        package_id: id.clone(),
        version: version.clone(),
        cid: cid.0,
        signature: signature.to_vec(),
        publisher_key: publisher.ed25519_pubkey,
        profile,
        dependencies: agent::dependencies(&pins),
        verification: report,
        revoked: false,
    };
    app.store().put_private_audio(row.clone()).await?;
    app.audit(
        Some(subject.principal_id),
        "private_audio.publish",
        format!("audio:{id}@{version}"),
        json!({"visibility":"Private"}),
    )
    .await;
    unchanged(&app, &row, &pins).await?;
    let mut response = private(metadata(&row));
    *response.status_mut() = StatusCode::CREATED;
    Ok(response)
}
pub async fn artifact(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldRead).await?;
    let row = row(&app, subject.principal_id, id, version).await?;
    let bytes = agent::load_artifact(&app, &row)?;
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
    let subject = app.require(&headers, CapabilityScope::HubPublish).await?;
    if !app
        .store()
        .revoke_private_audio(subject.principal_id, id.clone(), version.clone())
        .await?
    {
        return Err(ApiError::not_found("私有版本不存在"));
    }
    app.audit(
        Some(subject.principal_id),
        "private_audio.revoke",
        format!("audio:{id}@{version}"),
        json!({}),
    )
    .await;
    Ok(private(json!({"status":"revoked"})))
}

pub async fn resolved(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::WorldRead).await?;
    let root = row(&app, subject.principal_id, id, version).await?;
    let package = agent::parse(&agent::load_artifact(&app, &root)?)?;
    if package.profile.role != "soundpack" {
        return Err(agent::reject("只能应用soundpack根包"));
    }
    let resolved = agent::resolve(&app, subject.principal_id, package).await?;
    unchanged(&app, &root, &resolved.pins).await?;
    app.require(&headers, CapabilityScope::WorldRead).await?;
    Ok(private(
        json!({"root":metadata(&root),"manifest":resolved.manifest,"recipes":resolved.recipes,"samples":resolved.samples.iter().map(|(k,v)|(k,base64::engine::general_purpose::STANDARD.encode(v))).collect::<std::collections::BTreeMap<_,_>>(),"visibility":"Private"}),
    ))
}
