// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 路由 `01` 健康检查、`02` AGPLv3 §13 源码要约与补充的 SBOM 下载。

use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;

use crate::app::AppState;
use crate::error::ApiError;

/// `GET /api/v1/meta/health` → `{ status: "ok", uptime_s, active_worlds, gpu_tier }`
///
/// 服务端权威仿真只走 CPU 参考后端（D0_Strict 确定性），故 `gpu_tier` 恒为 `"cpu_reference"`。
pub async fn health(State(app): State<AppState>) -> impl IntoResponse {
    let active = app.registry().active_count();
    Json(serde_json::json!({
        "status": "ok",
        "uptime_s": app.inner.started.elapsed().as_secs(),
        "active_worlds": active,
        "gpu_tier": "cpu_reference",
    }))
}

/// `GET /api/v1/meta/source-offer`（Public，AGPLv3 §13）
pub async fn source_offer(State(app): State<AppState>) -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(app.source_offer()),
    )
}

/// `GET /api/v1/meta/sbom.spdx.json`：构建时生成的 SPDX 2.3 SBOM
pub async fn sbom() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/spdx+json"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        crate::source::SBOM_SPDX_JSON,
    )
}

pub async fn not_found() -> ApiError {
    ApiError::not_found("路由不存在").with_status(StatusCode::NOT_FOUND)
}
