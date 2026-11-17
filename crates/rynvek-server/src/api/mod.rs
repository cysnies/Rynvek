// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 规范 11.3 HTTP REST / WebSocket 路由表（20 条，路径与方法逐字一致）+ 补充路由 SBOM 下载。

pub mod auth;
pub mod hub;
pub mod lab;
pub mod matches;
pub mod meta;
pub mod user;
pub mod worlds;

use axum::Router;
use axum::routing::{delete, get, post, put};

use crate::app::AppState;

/// 规范路由表（序号 → 方法、路径），供测试逐条核对
pub const ROUTES: [(&str, &str, &str); 20] = [
    ("01", "GET", "/api/v1/meta/health"),
    ("02", "GET", "/api/v1/meta/source-offer"),
    ("03", "POST", "/api/v1/auth/session"),
    ("04", "GET", "/api/v1/worlds"),
    ("05", "POST", "/api/v1/worlds"),
    ("06", "GET", "/api/v1/worlds/{world_id}/commits"),
    ("07", "POST", "/api/v1/worlds/{world_id}/fork"),
    ("08", "POST", "/api/v1/worlds/{world_id}/patches"),
    ("09", "WS", "/api/v1/worlds/{world_id}/stream"),
    ("10", "POST", "/api/v1/matches/queue"),
    ("11", "GET", "/api/v1/matches/{match_id}"),
    ("12", "GET", "/api/v1/matches/{match_id}/replay"),
    ("13", "GET", "/api/v1/rankings/{league}"),
    ("14", "GET", "/api/v1/hub/packages"),
    ("15", "PUT", "/api/v1/hub/packages/{id}/{ver}"),
    ("16", "GET", "/api/v1/hub/blobs/{blake3_hex}"),
    ("17", "POST", "/api/v1/lab/experiments"),
    ("18", "GET", "/api/v1/lab/experiments/{exp_id}/parquet"),
    ("19", "POST", "/api/v1/user/data-export"),
    ("20", "DELETE", "/api/v1/user/account"),
];

/// 构建路由
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/local/practice", post(crate::local::practice))
        .route("/api/v1/meta/health", get(meta::health))
        .route("/api/v1/meta/source-offer", get(meta::source_offer))
        .route(crate::source::SBOM_PATH, get(meta::sbom))
        .route("/api/v1/auth/session", post(auth::session))
        .route("/api/v1/worlds", get(worlds::list).post(worlds::create))
        .route("/api/v1/worlds/{world_id}/commits", get(worlds::commits))
        .route("/api/v1/worlds/{world_id}/fork", post(worlds::fork))
        .route("/api/v1/worlds/{world_id}/patches", post(worlds::patches))
        .route("/api/v1/worlds/{world_id}/stream", get(worlds::stream))
        .route("/api/v1/matches/queue", post(matches::queue))
        .route("/api/v1/matches/{match_id}", get(matches::get_match))
        .route("/api/v1/matches/{match_id}/replay", get(matches::replay))
        .route("/api/v1/rankings/{league}", get(matches::rankings))
        .route("/api/v1/hub/packages", get(hub::search))
        .route("/api/v1/hub/packages/{id}/{ver}", put(hub::publish))
        .route("/api/v1/hub/blobs/{blake3_hex}", get(hub::blob))
        .route("/api/v1/lab/experiments", post(lab::submit))
        .route("/api/v1/lab/experiments/{exp_id}", get(lab::status))
        .route("/api/v1/lab/experiments/{exp_id}/parquet", get(lab::parquet))
        .route("/api/v1/user/data-export", post(user::data_export))
        .route("/api/v1/user/account", delete(user::delete_account))
        .fallback(meta::not_found)
        .with_state(state)
}

/// 解析 JSON 请求体；失败统一返回 `0x5001`
pub(crate) fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> crate::error::ApiResult<T> {
    serde_json::from_slice(body).map_err(|e| crate::error::ApiError::bad_request(format!("请求体不符合契约：{e}")))
}

/// 接受 JSON 数字或十进制字符串的 `u64`（JavaScript 无法精确表示 > 2^53 的整数）
pub(crate) mod u64_text {
    use serde::{Deserialize, Deserializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either {
        N(u64),
        S(String),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        match Either::deserialize(d)? {
            Either::N(n) => Ok(n),
            Either::S(s) => s.parse().map_err(serde::de::Error::custom),
        }
    }
}
