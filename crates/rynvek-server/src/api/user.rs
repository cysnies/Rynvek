// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 路由 `19`–`20`：GDPR / CCPA 数据导出与账户注销（`account.owner`，只能作用于令牌主体自身）。

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rynvek_protocol::CapabilityScope;
use rynvek_security::{League, Resource, authorize};

use crate::app::AppState;
use crate::error::{ApiError, ApiResult};
use crate::ids::{hex, unix_now_s};

/// 热库抹除时限（24 h）与冷备滚动清理时限（30 d）
pub const HOT_PURGE_S: u64 = 24 * 3600;
pub const COLD_PURGE_S: u64 = 30 * 24 * 3600;

async fn owner(app: &AppState, headers: &HeaderMap) -> ApiResult<rynvek_security::Subject> {
    let subject = app.require(headers, CapabilityScope::AccountOwner).await?;
    authorize(
        &subject,
        CapabilityScope::AccountOwner,
        &Resource::Account {
            principal_id: subject.principal_id,
        },
    )
    .map_err(ApiError::from)?;
    Ok(subject)
}

/// `19 POST /api/v1/user/data-export`：一次性导出该主体的全部记录（JSON 归档）
pub async fn data_export(State(app): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let subject = owner(&app, &headers).await?;
    let pid = subject.principal_id;
    let store = app.store();
    let principal = store
        .get_principal(pid)
        .await?
        .ok_or_else(|| ApiError::not_found("Principal 不存在"))?;
    let orgs = store.orgs_owned_by(pid).await?;
    // 世界：分页取完自己拥有的全部
    let mut worlds = Vec::new();
    let mut offset = 0;
    loop {
        let page = store.list_worlds(Some(pid), 100, offset).await?;
        let n = page.len();
        worlds.extend(page.into_iter().filter(|w| w.owner_principal_id == pid));
        if n < 100 {
            break;
        }
        offset += 100;
    }
    let matches = store.matches_of(pid).await?;
    let mut ratings = Vec::new();
    for league in League::ALL {
        ratings.push(store.get_rating(pid, league).await?);
    }
    let mut packages = Vec::new();
    for org in &orgs {
        for p in store.search_packages(None, None).await? {
            if p.org_id == org.org_id {
                packages.push(
                    serde_json::json!({ "package": p, "versions": store.package_versions(&p.package_id).await? }),
                );
            }
        }
    }
    let audit = store.audit_of(pid).await?;
    let experiments: Vec<_> = app
        .experiments()
        .values()
        .filter(|e| e.owner_principal_id == pid)
        .cloned()
        .collect();
    let archive = serde_json::json!({
        "format": "rynvek-data-export/1",
        "generated_at_unix_s": unix_now_s(),
        "principal": {
            "principal_id": pid.to_string(),
            "handle": principal.handle,
            "kind": principal.kind,
            "status": principal.status,
            "ed25519_pubkey": hex(&principal.ed25519_pubkey),
        },
        "organizations": orgs,
        "worlds": worlds,
        "matches": matches,
        "ratings": ratings,
        "packages": packages,
        "experiments": experiments,
        "audit_logs": audit,
    });
    app.audit(
        Some(pid),
        "account.data_export",
        format!("principal:{pid}"),
        serde_json::json!({}),
    )
    .await;
    Ok((
        [(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"rynvek-export-{pid}.json\""),
        )],
        Json(archive),
    )
        .into_response())
}

/// `20 DELETE /api/v1/user/account`：立即停用并匿名化主体、吊销全部令牌；返回抹除时间表
pub async fn delete_account(State(app): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let subject = owner(&app, &headers).await?;
    let pid = subject.principal_id;
    app.store().tombstone_principal(pid).await?;
    app.tokens_mut().revoke_principal(pid);
    let now = unix_now_s();
    app.audit(
        Some(pid),
        "account.delete",
        format!("principal:{pid}"),
        serde_json::json!({ "hot_purge_deadline_unix_s": now + HOT_PURGE_S }),
    )
    .await;
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "scheduled",
            "principal_id": pid.to_string(),
            "tokens_revoked": true,
            "hot_purge_deadline_unix_s": now + HOT_PURGE_S,
            "cold_purge_deadline_unix_s": now + COLD_PURGE_S,
        })),
    )
        .into_response())
}
