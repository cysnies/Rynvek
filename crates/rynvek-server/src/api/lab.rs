// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 真实持久Lab作业入口。Store是唯一状态源；私有CAS结果经fenced事务一次发布。
use crate::{
    app::AppState,
    error::{ApiError, ApiResult},
    ids::parse_hex,
    lab_jobs::{Experiment, LabJob, OwnerAction, uuid_text},
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use rynvek_protocol::CapabilityScope;
use rynvek_rts::lab::{ExperimentSpec, VariantSpec, arrow_io::LabTable};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitRequest {
    submission_id: String,
    name: String,
    seeds: Vec<u64>,
    ticks: u64,
    #[serde(default)]
    sample_every: Option<u64>,
    #[serde(default)]
    variants: Vec<VariantSpec>,
}
pub async fn submit(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::LabSubmit).await?;
    let req: SubmitRequest = super::parse_json(&body)?;
    let job = LabJob::new(
        subject.principal_id,
        req.submission_id,
        ExperimentSpec {
            name: req.name,
            seeds: req.seeds,
            ticks: req.ticks,
            sample_every: req
                .sample_every
                .unwrap_or(rynvek_rts::lab::DEFAULT_SAMPLE_EVERY.min(req.ticks.max(1))),
            variants: req.variants,
        },
    )?;
    let exp = app.store().lab_submit(job).await?;
    app.audit(
        Some(subject.principal_id),
        "lab.submit",
        format!("experiment:{}", exp.exp_id),
        serde_json::json!({"runs":exp.runs_total,"generation":exp.generation}),
    )
    .await;
    Ok((
        StatusCode::ACCEPTED,
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(exp),
    )
        .into_response())
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    limit: Option<u32>,
    offset: Option<u32>,
}
pub async fn list(State(app): State<AppState>, headers: HeaderMap, Query(q): Query<ListQuery>) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::LabRead).await?;
    let limit = q.limit.unwrap_or(25);
    let offset = q.offset.unwrap_or(0);
    if !(1..=100).contains(&limit) || offset > 512 {
        return Err(ApiError::bad_request("limit必须1..100、offset不得超过512"));
    }
    let jobs = app.store().lab_list(subject.principal_id, limit, offset).await?;
    Ok((
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(serde_json::json!({"experiments":jobs,"offset":offset,"limit":limit})),
    )
        .into_response())
}
pub async fn control(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::LabSubmit).await?;
    let mut op: OwnerAction = super::parse_json(&body)?;
    op.operation_id = uuid_text(&op.operation_id)?;
    let exp = app
        .store()
        .lab_control(subject.principal_id, uuid_text(&id)?, op)
        .await?;
    app.audit(
        Some(subject.principal_id),
        "lab.control",
        format!("experiment:{}", exp.exp_id),
        serde_json::json!({"status":exp.status,"generation":exp.generation}),
    )
    .await;
    Ok(([(header::CACHE_CONTROL, "private, no-store")], Json(exp)).into_response())
}
async fn owned_experiment(app: &AppState, headers: &HeaderMap, id: &str) -> ApiResult<Experiment> {
    let subject = app.require(headers, CapabilityScope::LabRead).await?;
    app.store()
        .lab_get(subject.principal_id, uuid_text(id)?)
        .await?
        .map(|j| j.experiment)
        .ok_or_else(|| ApiError::not_found("实验不存在"))
}

pub async fn status(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let exp = owned_experiment(&app, &headers, &id).await?;
    Ok(([(header::CACHE_CONTROL, "private, no-store")], Json(exp)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct ParquetQuery {
    #[serde(default)]
    table: Option<String>,
}

/// `18 GET /api/v1/lab/experiments/{exp_id}/parquet`（`lab.read`；`?table=runs|metrics|lineage|energy`）
pub async fn parquet(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ParquetQuery>,
) -> ApiResult<Response> {
    let exp = owned_experiment(&app, &headers, &id).await?;
    let table = match q.table.as_deref() {
        None => LabTable::Metrics,
        Some(t) => {
            LabTable::parse(t).ok_or_else(|| ApiError::bad_request("table 必须是 runs / metrics / lineage / energy"))?
        }
    };
    let cid = exp
        .tables
        .get(table.name())
        .and_then(|h| parse_hex::<32>(h))
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "实验处于 `{}`（{}/{} run），尚无 Parquet 产出{}",
                exp.status,
                exp.runs_done,
                exp.runs_total,
                exp.error.as_deref().map(|e| format!("：{e}")).unwrap_or_default()
            ))
        })?;
    let bytes = app.inner.world_snapshots.get(&rynvek_core::hash::Blake3Hash(cid))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.apache.parquet".to_owned()),
            (header::CACHE_CONTROL, "private, no-store".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}.{}.parquet\"", exp.exp_id, table.name()),
            ),
            (header::ETAG, format!("\"{}\"", crate::ids::hex(&cid))),
            (
                header::HeaderName::from_static("x-rynvek-experiment-status"),
                exp.status.clone(),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct RowsQuery {
    pub table: String,
    #[serde(default)]
    pub offset: u32,
    pub limit: Option<usize>,
}
/// 私有工作台页直接解码同一CID的Parquet，不重跑仿真、不维护影子结果。
pub async fn rows(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<RowsQuery>,
) -> ApiResult<Response> {
    let exp = owned_experiment(&app, &headers, &id).await?;
    let table = LabTable::parse(&q.table).ok_or_else(|| ApiError::bad_request("未知结果表"))?;
    let limit = q.limit.unwrap_or(250);
    if !(1..=500).contains(&limit) {
        return Err(ApiError::bad_request("limit必须在1..=500"));
    }
    let cid = exp
        .tables
        .get(table.name())
        .and_then(|h| parse_hex::<32>(h))
        .ok_or_else(|| ApiError::not_found("结果尚未产出"))?;
    let bytes = app.inner.world_snapshots.get(&rynvek_core::hash::Blake3Hash(cid))?;
    let page =
        tokio::task::spawn_blocking(move || rynvek_rts::lab::arrow_io::parquet_page(bytes, q.offset as usize, limit))
            .await
            .map_err(|e| ApiError::bad_request(format!("读取结果失败：{e}")))?
            .map_err(ApiError::bad_request)?;
    Ok((
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(serde_json::json!({
            "exp_id":id,"table":table.name(),"cid":crate::ids::hex(&cid),"page":page
        })),
    )
        .into_response())
}
