// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 路由 `17`–`18`：Rynvek Lab 批量多种子演化实验。
//!
//! M7 完成任务登记、配额校验、权限与所有权检查；批量执行器与 Parquet 产出属于 M8，
//! 因此 `18` 在产出就绪前返回 `0x2002`（任务存在但尚无产出），就绪后从 CAS 流式下载。

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rynvek_protocol::CapabilityScope;
use rynvek_security::{Resource, authorize};
use serde::Deserialize;

use crate::app::{AppState, Experiment};
use crate::error::{ApiError, ApiResult};
use crate::ids::{new_uuid_v7, parse_hex, unix_now_s};

/// 单个实验的种子数上限
pub const MAX_SEEDS: usize = 256;
/// 单个种子的 Tick 上限（30 Hz 下约 1 小时）
pub const MAX_TICKS: u64 = 108_000;
/// 每个主体同时排队的实验上限
pub const MAX_QUEUED_PER_PRINCIPAL: usize = 8;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitRequest {
    name: String,
    seeds: Vec<u64>,
    ticks: u64,
}

/// `17 POST /api/v1/lab/experiments`（`lab.submit`）
pub async fn submit(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::LabSubmit).await?;
    let req: SubmitRequest = super::parse_json(&body)?;
    if req.name.is_empty() || req.name.chars().count() > 128 {
        return Err(ApiError::bad_request("name 长度必须在 1..=128"));
    }
    if req.seeds.is_empty() || req.seeds.len() > MAX_SEEDS {
        return Err(ApiError::bad_request(format!("seeds 数量必须在 1..={MAX_SEEDS}")));
    }
    if req.ticks == 0 || req.ticks > MAX_TICKS {
        return Err(ApiError::bad_request(format!("ticks 必须在 1..={MAX_TICKS}")));
    }
    let exp = {
        let mut exps = app.experiments();
        let queued = exps
            .values()
            .filter(|e| e.owner_principal_id == subject.principal_id && e.status == "queued")
            .count();
        if queued >= MAX_QUEUED_PER_PRINCIPAL {
            return Err(ApiError::new(
                rynvek_core::error::RynvekErrorCode::CommandRateLimitExceeded,
                format!("同时排队的实验不能超过 {MAX_QUEUED_PER_PRINCIPAL} 个"),
            ));
        }
        let exp = Experiment {
            exp_id: new_uuid_v7().to_string(),
            owner_principal_id: subject.principal_id,
            name: req.name,
            seeds: req.seeds,
            ticks: req.ticks,
            status: "queued".to_owned(),
            submitted_at_unix_s: unix_now_s(),
            parquet_cid: None,
        };
        exps.insert(exp.exp_id.clone(), exp.clone());
        exp
    };
    app.audit(
        Some(subject.principal_id),
        "lab.submit",
        format!("experiment:{}", exp.exp_id),
        serde_json::json!({ "seeds": exp.seeds.len(), "ticks": exp.ticks }),
    )
    .await;
    Ok((StatusCode::ACCEPTED, Json(exp)).into_response())
}

/// `18 GET /api/v1/lab/experiments/{exp_id}/parquet`（`lab.read`）
pub async fn parquet(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::LabRead).await?;
    let exp = app
        .experiments()
        .get(&id)
        .cloned()
        .ok_or_else(|| ApiError::not_found("实验不存在"))?;
    // 非所有者与 "不存在" 返回相同错误
    if authorize(
        &subject,
        CapabilityScope::LabRead,
        &Resource::Experiment {
            owner_principal_id: exp.owner_principal_id,
        },
    )
    .is_err()
    {
        return Err(ApiError::not_found("实验不存在"));
    }
    let cid = exp
        .parquet_cid
        .as_deref()
        .and_then(parse_hex::<32>)
        .ok_or_else(|| ApiError::not_found(format!("实验处于 `{}`，尚无 Parquet 产出", exp.status)))?;
    let bytes = app.inner.blobs.get(&rynvek_core::hash::Blake3Hash(cid))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.apache.parquet".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}.parquet\"", exp.exp_id),
            ),
        ],
        bytes,
    )
        .into_response())
}
