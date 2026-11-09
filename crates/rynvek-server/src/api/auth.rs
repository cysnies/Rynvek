// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 路由 `03`：`POST /api/v1/auth/session`，Ed25519 质询-应答登录，签发 96 字节 `CapabilityToken`。
//!
//! 同一路由两步完成（ERRATA）：
//!
//! 0. 自注册（规范 11.3 未定义注册路由，ERRATA）：`{ "handle", "kind": "human|agent", "ed25519_pubkey": hex32,
//!    "registration_signature": hex64 }`，签名消息 `"rynvek-register/1\0" ‖ handle ‖ "\0" ‖ pubkey`（持有证明），
//!    成功后与第 1 步一样返回挑战，并附带新分配的 `principal_id`；
//! 1. `{ "principal_id": 42 }` → `{ "challenge": hex32, "expires_in_s": 60, "signing_message": "rynvek-auth/1\0 ‖ challenge ‖ principal_id_le" }`
//! 2. `{ "principal_id": 42, "challenge": hex32, "signature": hex64 }` → 令牌：
//!    - 缺省 `application/json`：`{ "token": base64url, "principal_id", "scopes", "expires_at_unix_s" }`；
//!    - `Accept: application/octet-stream`：直接返回 96 字节二进制令牌。
//!
//! `{ "oauth_token": … }` 为规范保留的 OAuth 入口；本部署未配置任何 OAuth 提供方，一律 `0x8001`。

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use rynvek_security::{AUTH_CHALLENGE_TTL_S, PrincipalKindCode, PrincipalStatus};
use serde::Deserialize;

use crate::app::{AppState, encode_token_text};
use crate::error::{ApiError, ApiResult};
use crate::ids::{hex, new_principal_id, parse_hex, unix_now_s};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionRequest {
    #[serde(default, with = "opt_u64_text")]
    principal_id: Option<u64>,
    #[serde(default)]
    challenge: Option<String>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    oauth_token: Option<String>,
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    ed25519_pubkey: Option<String>,
    #[serde(default)]
    registration_signature: Option<String>,
}

/// 自注册签名消息
pub fn registration_message(handle: &str, pubkey: &[u8; 32]) -> Vec<u8> {
    let mut m = b"rynvek-register/1\0".to_vec();
    m.extend_from_slice(handle.as_bytes());
    m.push(0);
    m.extend_from_slice(pubkey);
    m
}

fn parse_kind(text: &str) -> ApiResult<PrincipalKindCode> {
    match text {
        "human" => Ok(PrincipalKindCode::Human),
        "agent" => Ok(PrincipalKindCode::Agent),
        // Service 主体拥有全部权限，只能由运营方离线开通，不允许自注册
        "service" => Err(ApiError::forbidden("service 主体不可自注册")),
        _ => Err(ApiError::bad_request("kind 必须是 human / agent")),
    }
}

/// 第 0 步：自注册，返回新 principal_id
async fn register(app: &AppState, req: &SessionRequest) -> ApiResult<u64> {
    let handle = req.handle.as_deref().unwrap_or_default();
    let kind = parse_kind(req.kind.as_deref().unwrap_or("human"))?;
    let pubkey: [u8; 32] = req
        .ed25519_pubkey
        .as_deref()
        .and_then(parse_hex)
        .ok_or_else(|| ApiError::bad_request("ed25519_pubkey 必须是 32 字节十六进制"))?;
    let sig: [u8; 64] = req
        .registration_signature
        .as_deref()
        .and_then(parse_hex)
        .ok_or_else(|| ApiError::bad_request("registration_signature 必须是 64 字节十六进制"))?;
    rynvek_security::verify_ed25519(&pubkey, &registration_message(handle, &pubkey), &sig).map_err(ApiError::from)?;
    let pid = new_principal_id();
    app.register_principal(pid, handle, kind, pubkey).await?;
    app.audit(
        Some(pid),
        "auth.principal_registered",
        format!("principal:{pid}"),
        serde_json::json!({ "handle": handle, "kind": req.kind }),
    )
    .await;
    Ok(pid)
}

mod opt_u64_text {
    use serde::Deserializer;

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
        super::super::u64_text::deserialize(d).map(Some)
    }
}

pub async fn session(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    let req: SessionRequest = super::parse_json(&body)?;
    if req.oauth_token.is_some() {
        return Err(ApiError::unauthenticated(
            "本服务器未配置 OAuth 提供方，请使用 Ed25519 质询登录",
        ));
    }
    let registering = req.principal_id.is_none() && req.handle.is_some();
    let principal_id = match req.principal_id {
        Some(pid) => pid,
        None if registering => register(&app, &req).await?,
        None => return Err(ApiError::bad_request("缺少 principal_id（或自注册所需的 handle）")),
    };
    let principal = app
        .store()
        .get_principal(principal_id)
        .await?
        .filter(|p| p.status == PrincipalStatus::Active)
        // 不区分 "不存在" 与 "已停用"，避免账户枚举
        .ok_or_else(|| ApiError::unauthenticated("Principal 不存在或不可登录"))?;
    let now = unix_now_s();
    match (req.challenge, req.signature) {
        (None, None) => {
            let challenge = app.challenges().issue(principal_id, now);
            Ok(Json(serde_json::json!({
                "principal_id": principal_id.to_string(),
                "registered": registering,
                "challenge": hex(&challenge),
                "expires_in_s": AUTH_CHALLENGE_TTL_S,
                "signing_message": "rynvek-auth/1\\0 ‖ challenge(32B) ‖ principal_id(u64 LE)",
            }))
            .into_response())
        }
        (Some(c), Some(s)) => {
            let challenge: [u8; 32] =
                parse_hex(&c).ok_or_else(|| ApiError::bad_request("challenge 必须是 32 字节十六进制"))?;
            let signature: [u8; 64] =
                parse_hex(&s).ok_or_else(|| ApiError::bad_request("signature 必须是 64 字节十六进制"))?;
            let redeemed =
                app.challenges()
                    .redeem(principal_id, &challenge, &principal.ed25519_pubkey, &signature, now);
            if let Err(e) = redeemed {
                app.audit(
                    Some(principal_id),
                    "auth.session_rejected",
                    format!("principal:{principal_id}"),
                    serde_json::json!({ "code": e.code.code(), "message": e.message }),
                )
                .await;
                return Err(e.into());
            }
            let token = app.issue_token(principal_id, principal.kind);
            app.audit(
                Some(principal_id),
                "auth.session_issued",
                format!("principal:{principal_id}"),
                serde_json::json!({ "nonce": token.nonce, "expires_at_unix_s": token.expires_at_unix_s }),
            )
            .await;
            let wants_binary = headers
                .get(header::ACCEPT)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("application/octet-stream"));
            if wants_binary {
                return Ok((
                    [(header::CONTENT_TYPE, "application/octet-stream")],
                    token.encode().to_vec(),
                )
                    .into_response());
            }
            Ok(Json(serde_json::json!({
                "token": encode_token_text(&token),
                "token_bytes": rynvek_protocol::capability::CAPABILITY_TOKEN_LEN,
                "principal_id": principal_id.to_string(),
                "scopes": token.scopes.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                "expires_at_unix_s": token.expires_at_unix_s,
            }))
            .into_response())
        }
        _ => Err(ApiError::bad_request("challenge 与 signature 必须同时提供")),
    }
}
