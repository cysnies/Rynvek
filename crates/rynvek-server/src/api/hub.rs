// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 路由 `14`–`16`：Rynvek Hub 包检索、签名发布与 CAS Blob 下载（HTTP Range）。
//!
//! 发布签名消息（ERRATA）：`"rynvek-package/1\0" ‖ package_id ‖ "\0" ‖ semver ‖ "\0" ‖ BLAKE3(artifact)`，
//! 用发布者 Principal 在 `principals.ed25519_pubkey` 登记的公钥验签（失败 `0x9001`）。
//! `dependencies.lock`（规范 10.3.1）中的每个依赖都必须已在 Hub 发布、未吊销（`0x9002`）且产物哈希一致（`0x9003`）。
//! 之后执行服务端静态扫描与 1,000 Tick 沙箱压力测试（[`crate::verify`]），拒绝时 `0x9004`；
//! 通过的社区包授予 TrustLevel 2，官方组织保持 TrustLevel 1。

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;
use rynvek_protocol::CapabilityScope;
use rynvek_security::{Resource, authorize, verify_ed25519};
use serde::Deserialize;

use crate::app::AppState;
use crate::error::{ApiError, ApiResult};
use crate::ids::{hex, parse_hex};
use crate::store::{PACKAGE_KINDS, PackageRow, PackageVersionRow};

/// 产物上限（64 MiB）
pub const MAX_ARTIFACT_BYTES: usize = 64 << 20;

/// 发布签名覆盖的消息
pub fn package_signing_message(package_id: &str, semver: &str, artifact: &[u8]) -> Vec<u8> {
    let mut m = b"rynvek-package/1\0".to_vec();
    m.extend_from_slice(package_id.as_bytes());
    m.push(0);
    m.extend_from_slice(semver.as_bytes());
    m.push(0);
    m.extend_from_slice(blake3::hash(artifact).as_bytes());
    m
}

fn valid_package_id(id: &str) -> bool {
    (3..=128).contains(&id.len())
        && id.split('.').all(|seg| {
            !seg.is_empty()
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

fn valid_semver(v: &str) -> bool {
    let core = v.split(['-', '+']).next().unwrap_or_default();
    v.len() <= 32
        && core.split('.').count() == 3
        && core
            .split('.')
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && (n == "0" || !n.starts_with('0')))
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    q: Option<String>,
}

/// `14 GET /api/v1/hub/packages`（Public）
pub async fn search(State(app): State<AppState>, Query(q): Query<SearchQuery>) -> ApiResult<Response> {
    if let Some(k) = &q.kind {
        if !PACKAGE_KINDS.contains(&k.as_str()) {
            return Err(ApiError::bad_request(format!("kind 必须是 {PACKAGE_KINDS:?} 之一")));
        }
    }
    let rows = app.store().search_packages(q.kind.as_deref(), q.q.as_deref()).await?;
    let mut out = Vec::new();
    for p in rows {
        let versions = app.store().package_versions(&p.package_id).await?;
        out.push(serde_json::json!({
            "package_id": p.package_id,
            "org_id": p.org_id.to_string(),
            "package_kind": p.package_kind,
            "trust_level": p.trust_level,
            "versions": versions.iter().map(|v| serde_json::json!({
                "semver": v.semver,
                "min_engine_version": v.min_engine_version,
                "license_spdx": v.license_spdx,
                "artifact_cid_blake3": hex(&v.artifact_cid_blake3),
                "artifact_size_bytes": v.artifact_size_bytes,
                "blob_url": format!("/api/v1/hub/blobs/{}", hex(&v.artifact_cid_blake3)),
            })).collect::<Vec<_>>(),
        }));
    }
    Ok(Json(serde_json::json!({ "packages": out })).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishRequest {
    org_id: String,
    package_kind: String,
    min_engine_version: String,
    license_spdx: String,
    /// 产物（标准 Base64）
    artifact_b64: String,
    /// 64 字节 Ed25519 签名（十六进制）
    signature: String,
    manifest: serde_json::Value,
    /// 规范 10.3.1 `dependencies.lock`
    lockfile: serde_json::Value,
}

#[derive(Deserialize)]
struct LockEntry {
    id: String,
    version: String,
    blake3_256: String,
}

#[derive(Deserialize)]
struct Lockfile {
    lockfile_version: u32,
    #[serde(default)]
    packages: Vec<LockEntry>,
}

/// `15 PUT /api/v1/hub/packages/{id}/{ver}`（`hub.publish`）
///
/// 顺序：签名验证 → 命名空间与组织授权 → 依赖锁校验 → 静态扫描与沙箱 → 产物入 CAS（版本不可覆盖）。
pub async fn publish(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path((id, ver)): Path<(String, String)>,
    body: Bytes,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::HubPublish).await?;
    if !valid_package_id(&id) {
        return Err(ApiError::bad_request(
            "包 ID 只允许以 `.` 分段的 [a-z0-9-]，长度 3..=128",
        ));
    }
    if !valid_semver(&ver) {
        return Err(ApiError::bad_request("版本号必须是 SemVer 2.0（MAJOR.MINOR.PATCH）"));
    }
    let req: PublishRequest = super::parse_json(&body)?;
    if !PACKAGE_KINDS.contains(&req.package_kind.as_str()) {
        return Err(ApiError::bad_request(format!(
            "package_kind 必须是 {PACKAGE_KINDS:?} 之一"
        )));
    }
    let org_id: u64 = req
        .org_id
        .parse()
        .map_err(|_| ApiError::bad_request("org_id 必须是十进制整数"))?;
    let org = app
        .store()
        .get_org(org_id)
        .await?
        .ok_or_else(|| ApiError::not_found("组织不存在"))?;
    authorize(
        &subject,
        CapabilityScope::HubPublish,
        &Resource::Package {
            package_id: id.clone(),
            org_id,
            org_is_official: org.is_official(),
        },
    )
    .map_err(ApiError::from)?;
    let existing = app.store().get_package(&id).await?;
    if let Some(p) = &existing {
        if p.revoked {
            return Err(ApiError::new(RynvekErrorCode::RevokedPackageBlocked, "该包已被吊销"));
        }
        if p.org_id != org_id {
            return Err(ApiError::forbidden("包 ID 已属于其他组织"));
        }
        if p.package_kind != req.package_kind {
            return Err(ApiError::bad_request("同一包的 package_kind 不可变"));
        }
    }
    let artifact = base64::engine::general_purpose::STANDARD
        .decode(&req.artifact_b64)
        .map_err(|_| ApiError::bad_request("artifact_b64 不是合法 Base64"))?;
    if artifact.is_empty() || artifact.len() > MAX_ARTIFACT_BYTES {
        return Err(ApiError::bad_request("产物大小必须在 1 字节..=64 MiB"));
    }
    let signature: [u8; 64] =
        parse_hex(&req.signature).ok_or_else(|| ApiError::bad_request("signature 必须是 64 字节十六进制"))?;
    let publisher = app
        .store()
        .get_principal(subject.principal_id)
        .await?
        .ok_or_else(|| ApiError::unauthenticated("发布者不存在"))?;
    verify_ed25519(
        &publisher.ed25519_pubkey,
        &package_signing_message(&id, &ver, &artifact),
        &signature,
    )
    .map_err(ApiError::from)?;
    // 依赖锁
    let lock: Lockfile = serde_json::from_value(req.lockfile.clone()).map_err(|e| {
        ApiError::new(
            RynvekErrorCode::DependencyLockConflict,
            format!("dependencies.lock 非法：{e}"),
        )
    })?;
    if lock.lockfile_version != 1 {
        return Err(ApiError::new(
            RynvekErrorCode::DependencyLockConflict,
            "lockfile_version 必须为 1",
        ));
    }
    for dep in &lock.packages {
        let pkg = app.store().get_package(&dep.id).await?.ok_or_else(|| {
            ApiError::new(
                RynvekErrorCode::DependencyLockConflict,
                format!("依赖 `{}` 未发布", dep.id),
            )
        })?;
        if pkg.revoked {
            return Err(ApiError::new(
                RynvekErrorCode::RevokedPackageBlocked,
                format!("依赖 `{}` 已被吊销", dep.id),
            ));
        }
        let versions = app.store().package_versions(&dep.id).await?;
        let hash = dep.blake3_256.trim_start_matches("blake3:");
        let ok = versions
            .iter()
            .any(|v| v.semver == dep.version && hex(&v.artifact_cid_blake3) == hash);
        if !ok {
            return Err(ApiError::new(
                RynvekErrorCode::DependencyLockConflict,
                format!("依赖 `{}@{}` 的版本或 BLAKE3 与 Hub 不一致", dep.id, dep.version),
            ));
        }
    }
    // 静态扫描与沙箱（CPU 密集，移出异步运行时）
    let plugins = app.plugin_runtime()?;
    let report = {
        let (kind, id, ver, license, lockfile) = (
            req.package_kind.clone(),
            id.clone(),
            ver.clone(),
            req.license_spdx.clone(),
            req.lockfile.clone(),
        );
        let official = org.is_official();
        let artifact = artifact.clone();
        tokio::task::spawn_blocking(move || {
            crate::verify::verify(
                &crate::verify::VerifyInput {
                    kind: &kind,
                    package_id: &id,
                    semver: &ver,
                    license_spdx: &license,
                    lockfile: &lockfile,
                    artifact: &artifact,
                    official,
                },
                &plugins,
            )
        })
        .await
        .map_err(|e| ApiError::internal(format!("验证任务异常退出：{e}")))??
    };
    let cid = app.inner.blobs.put(&artifact)?;
    let granted = i16::from(report.trust_level);
    let trust_level = match &existing {
        None => Some(granted),
        // 已有包：只会因新版本通过扫描而提升信任等级，不会降级
        Some(p) if granted < p.trust_level => Some(granted),
        Some(_) => None,
    };
    if let Some(trust_level) = trust_level {
        app.store()
            .upsert_package(PackageRow {
                package_id: id.clone(),
                org_id,
                package_kind: req.package_kind.clone(),
                trust_level,
                revoked: false,
            })
            .await?;
    }
    app.store()
        .insert_package_version(PackageVersionRow {
            package_id: id.clone(),
            semver: ver.clone(),
            min_engine_version: req.min_engine_version,
            license_spdx: req.license_spdx,
            artifact_cid_blake3: cid.0,
            artifact_size_bytes: artifact.len() as u64,
            ed25519_signature: signature,
            manifest_json: req.manifest,
            lockfile_json: req.lockfile,
        })
        .await?;
    app.audit(
        Some(subject.principal_id),
        "hub.publish",
        format!("package:{id}@{ver}"),
        serde_json::json!({ "artifact_cid": cid.to_hex(), "size": artifact.len() }),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "package_id": id,
            "semver": ver,
            "artifact_cid_blake3": cid.to_hex(),
            "trust_level": trust_level.or(existing.as_ref().map(|p| p.trust_level)).unwrap_or(granted),
            "verification": {
                "signature": "ok",
                "lockfile": "ok",
                "static_scan": report.static_scan,
                "sandbox": report.sandbox,
                "findings": report.findings,
            },
        })),
    )
        .into_response())
}

/// 解析单段 `Range: bytes=…`，返回半开区间；多段范围不支持（按 RFC 9110 可忽略，返回整个实体）
fn parse_range(value: &str, len: u64) -> Option<Result<(u64, u64), ()>> {
    let spec = value.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let r = match (a.trim(), b.trim()) {
        ("", "") => return Some(Err(())),
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            if n == 0 {
                return Some(Err(()));
            }
            (len.saturating_sub(n), len)
        }
        (start, "") => (start.parse().ok()?, len),
        (start, end) => {
            let s: u64 = start.parse().ok()?;
            let e: u64 = end.parse().ok()?;
            if e < s {
                return Some(Err(()));
            }
            (s, (e + 1).min(len))
        }
    };
    Some(if r.0 >= len { Err(()) } else { Ok(r) })
}

/// `16 GET /api/v1/hub/blobs/{blake3_hex}`（Public，支持单段 HTTP Range）
pub async fn blob(State(app): State<AppState>, headers: HeaderMap, Path(h): Path<String>) -> ApiResult<Response> {
    let id: [u8; 32] = parse_hex(&h).ok_or_else(|| ApiError::not_found("CID 必须是 64 位十六进制"))?;
    let id = Blake3Hash(id);
    if !app.inner.blobs.contains(&id) {
        return Err(ApiError::not_found("Blob 不存在"));
    }
    let bytes = app.inner.blobs.get(&id)?;
    // 回放必须经过世界权限路由；即便旧部署曾把归档放入公共 CAS，也不按 CID 公开。
    if bytes.starts_with(&crate::replay::REPLAY_MAGIC) {
        return Err(ApiError::not_found("公开制品不存在"));
    }
    let len = bytes.len() as u64;
    let etag = format!("\"{}\"", id.to_hex());
    let common = [
        (header::ACCEPT_RANGES, "bytes".to_owned()),
        (header::ETAG, etag),
        (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
        (header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_owned()),
    ];
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    match range.and_then(|r| parse_range(r, len)) {
        None => Ok((StatusCode::OK, common, bytes).into_response()),
        Some(Err(())) => Ok((
            StatusCode::RANGE_NOT_SATISFIABLE,
            [(header::CONTENT_RANGE, format!("bytes */{len}"))],
        )
            .into_response()),
        Some(Ok((s, e))) => Ok((
            StatusCode::PARTIAL_CONTENT,
            common,
            [(header::CONTENT_RANGE, format!("bytes {s}-{}/{len}", e - 1))],
            bytes[s as usize..e as usize].to_vec(),
        )
            .into_response()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parsing_follows_rfc_9110() {
        assert_eq!(parse_range("bytes=0-3", 10), Some(Ok((0, 4))));
        assert_eq!(parse_range("bytes=5-", 10), Some(Ok((5, 10))));
        assert_eq!(parse_range("bytes=-3", 10), Some(Ok((7, 10))));
        assert_eq!(parse_range("bytes=8-100", 10), Some(Ok((8, 10))));
        assert_eq!(parse_range("bytes=10-", 10), Some(Err(())));
        assert_eq!(parse_range("bytes=4-2", 10), Some(Err(())));
        assert_eq!(parse_range("bytes=0-1,4-5", 10), None);
        assert_eq!(parse_range("items=0-1", 10), None);
    }

    #[test]
    fn ids_and_versions() {
        assert!(valid_package_id("rynvek.visual.industrial-default"));
        assert!(!valid_package_id("Rynvek.x"));
        assert!(!valid_package_id("a..b"));
        assert!(valid_semver("1.0.0") && valid_semver("0.12.3-beta.1"));
        assert!(!valid_semver("1.0") && !valid_semver("01.0.0"));
    }
}
