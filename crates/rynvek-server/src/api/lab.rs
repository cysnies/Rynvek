// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 路由 `17`–`18`：Rynvek Lab 批量多种子演化实验。
//!
//! - `17` 校验规格与配额后登记任务（`queued`），后台执行器在并发槽可用时转为 `running`，
//!   调用 `rynvek_rts::lab::run_experiment`（run 级多线程并行），把 `runs` / `metrics` / `lineage` / `energy`
//!   四张表编码为 Parquet 写入 CAS 后转为 `completed`；
//! - `18` 在产出就绪前返回 `0x2002`（消息中带当前状态与进度），就绪后下载 `?table=`（缺省 `metrics`）。
//!
//! 实验结果完全确定：同一规格总是得到逐字节相同的 Parquet，CAS 标识可直接用于复现性比对。

use std::collections::BTreeMap;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rynvek_protocol::CapabilityScope;
use rynvek_rts::lab::arrow_io::{LabTable, table_batch, to_parquet};
use rynvek_rts::lab::{ExperimentSpec, VariantSpec, run_experiment};
use rynvek_security::{Resource, authorize};
use serde::Deserialize;

use crate::app::{AppState, Experiment};
use crate::error::{ApiError, ApiResult};
use crate::ids::{new_uuid_v7, parse_hex, unix_now_s};

/// 单个实验的 run 数上限（变体 × 种子）
pub const MAX_SEEDS: usize = rynvek_rts::lab::MAX_RUNS;
/// 单个 run 的 Tick 上限（30 Hz 下约 1 小时）
pub const MAX_TICKS: u64 = rynvek_rts::lab::MAX_TICKS;
/// 每个主体同时排队或执行中的实验上限
pub const MAX_QUEUED_PER_PRINCIPAL: usize = 8;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitRequest {
    name: String,
    seeds: Vec<u64>,
    ticks: u64,
    #[serde(default)]
    sample_every: Option<u64>,
    #[serde(default)]
    variants: Vec<VariantSpec>,
}

fn spec_of(e: &Experiment) -> ExperimentSpec {
    ExperimentSpec {
        name: e.name.clone(),
        seeds: e.seeds.clone(),
        ticks: e.ticks,
        sample_every: e.sample_every,
        variants: e.variants.clone(),
    }
}

/// `17 POST /api/v1/lab/experiments`（`lab.submit`）
pub async fn submit(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    let subject = app.require(&headers, CapabilityScope::LabSubmit).await?;
    let req: SubmitRequest = super::parse_json(&body)?;
    let spec = ExperimentSpec {
        name: req.name,
        seeds: req.seeds,
        ticks: req.ticks,
        sample_every: req
            .sample_every
            .unwrap_or(rynvek_rts::lab::DEFAULT_SAMPLE_EVERY.min(req.ticks.max(1))),
        variants: req.variants,
    };
    spec.validate().map_err(ApiError::bad_request)?;
    let exp = {
        let mut exps = app.experiments();
        let pending = exps
            .values()
            .filter(|e| {
                e.owner_principal_id == subject.principal_id && matches!(e.status.as_str(), "queued" | "running")
            })
            .count();
        if pending >= MAX_QUEUED_PER_PRINCIPAL {
            return Err(ApiError::new(
                rynvek_core::error::RynvekErrorCode::CommandRateLimitExceeded,
                format!("同时排队或执行中的实验不能超过 {MAX_QUEUED_PER_PRINCIPAL} 个"),
            ));
        }
        let exp = Experiment {
            exp_id: new_uuid_v7().to_string(),
            owner_principal_id: subject.principal_id,
            runs_total: spec.runs().len(),
            name: spec.name.clone(),
            seeds: spec.seeds.clone(),
            ticks: spec.ticks,
            sample_every: spec.sample_every,
            variants: spec.effective_variants(),
            status: "queued".to_owned(),
            submitted_at_unix_s: unix_now_s(),
            started_at_unix_s: None,
            completed_at_unix_s: None,
            runs_done: 0,
            parquet_cid: None,
            tables: BTreeMap::new(),
            error: None,
        };
        exps.insert(exp.exp_id.clone(), exp.clone());
        exp
    };
    app.audit(
        Some(subject.principal_id),
        "lab.submit",
        format!("experiment:{}", exp.exp_id),
        serde_json::json!({ "seeds": exp.seeds.len(), "ticks": exp.ticks, "runs": exp.runs_total }),
    )
    .await;
    tokio::spawn(execute(app.clone(), exp.exp_id.clone()));
    Ok((StatusCode::ACCEPTED, Json(exp)).into_response())
}

fn update(app: &AppState, id: &str, f: impl FnOnce(&mut Experiment)) {
    if let Some(e) = app.experiments().get_mut(id) {
        f(e);
    }
}

/// 后台执行器：等待并发槽 → 运行 → 编码 Parquet → 写入 CAS
async fn execute(app: AppState, id: String) {
    let Ok(_permit) = app.inner.lab_slots.clone().acquire_owned().await else {
        return;
    };
    let Some(spec) = app.experiments().get(&id).map(spec_of) else {
        return;
    };
    update(&app, &id, |e| {
        e.status = "running".into();
        e.started_at_unix_s = Some(unix_now_s());
    });
    let worker_app = app.clone();
    let worker_id = id.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<BTreeMap<String, String>, String> {
        let progress = |done: usize, _total: usize| update(&worker_app, &worker_id, |e| e.runs_done = done);
        let result = run_experiment(&spec, &progress);
        let mut tables = BTreeMap::new();
        for t in LabTable::ALL {
            let batch = table_batch(&result, t).map_err(|e| format!("Arrow 编码失败：{e}"))?;
            let bytes = to_parquet(&batch).map_err(|e| format!("Parquet 编码失败：{e}"))?;
            let cid = worker_app
                .inner
                .blobs
                .put(&bytes)
                .map_err(|e| format!("写入 CAS 失败：{e}"))?;
            tables.insert(t.name().to_owned(), cid.to_hex());
        }
        Ok(tables)
    })
    .await
    .unwrap_or_else(|e| Err(format!("执行器异常退出：{e}")));
    let now = unix_now_s();
    let owner = app.experiments().get(&id).map(|e| e.owner_principal_id);
    match result {
        Ok(tables) => update(&app, &id, |e| {
            e.status = "completed".into();
            e.completed_at_unix_s = Some(now);
            e.runs_done = e.runs_total;
            e.parquet_cid = tables.get("metrics").cloned();
            e.tables = tables;
        }),
        Err(msg) => update(&app, &id, |e| {
            e.status = "failed".into();
            e.completed_at_unix_s = Some(now);
            e.error = Some(msg);
        }),
    }
    app.audit(
        owner,
        "lab.complete",
        format!("experiment:{id}"),
        serde_json::json!({ "status": app.experiments().get(&id).map(|e| e.status.clone()) }),
    )
    .await;
}

// 状态与四表下载统一使用相同的所有者校验，不泄露其他主体的作业存在性。
async fn owned_experiment(app: &AppState, headers: &HeaderMap, id: &str) -> ApiResult<Experiment> {
    let subject = app.require(headers, CapabilityScope::LabRead).await?;
    let exp = app
        .experiments()
        .get(id)
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
    Ok(exp)
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
    let bytes = app.inner.blobs.get(&rynvek_core::hash::Blake3Hash(cid))?;
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
