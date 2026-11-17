// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::app::AppState;
use rynvek_rts::lab::arrow_io::{LabTable, table_batch, to_parquet};
use rynvek_rts::lab::{ExperimentResult, run_one_interruptible};
use rynvek_storage::BlobStore;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Serialize, Deserialize)]
struct Checkpoint {
    profile: String,
    spec_digest: String,
    run: u32,
    result: ExperimentResult,
}
fn merge(to: &mut ExperimentResult, mut from: ExperimentResult) {
    to.runs.append(&mut from.runs);
    to.metrics.append(&mut from.metrics);
    to.lineage.append(&mut from.lineage);
    to.energy.append(&mut from.energy);
}
#[derive(Debug)]
enum WorkError {
    Stopped,
    LeaseLost,
    Execution,
}
fn compute(
    job: LabJob,
    cas: Arc<dyn BlobStore>,
    store: crate::store::Store,
    stop: Arc<AtomicBool>,
    rt: tokio::runtime::Handle,
) -> Result<BTreeMap<String, String>, WorkError> {
    let check = || stop.load(Ordering::Acquire);
    let mut all = ExperimentResult::default();
    let spec = job.experiment.spec();
    let claim = job.claim_ref();
    for plan in spec.runs() {
        if check() {
            return Err(WorkError::Stopped);
        }
        let result = if let Some(cid) = job.chunks.get(&plan.run) {
            let bytes = cas
                .get(&rynvek_core::hash::Blake3Hash(
                    crate::ids::parse_hex::<32>(cid).ok_or(WorkError::Execution)?,
                ))
                .map_err(|_| WorkError::Execution)?;
            // 直接反序列化为权威类型，不经serde_json::Value，避免u128能量计数截断。
            let cp: Checkpoint = serde_json::from_slice(&bytes).map_err(|_| WorkError::Execution)?;
            if cp.profile != KERNEL_PROFILE
                || cp.spec_digest != job.experiment.spec_digest
                || cp.run != plan.run
                || cp.result.runs.len() != 1
                || cp.result.runs[0].run != plan.run
                || cp.result.runs[0].seed != plan.seed
            {
                return Err(WorkError::Execution);
            }
            cp.result
        } else {
            let result =
                run_one_interruptible(&plan, spec.ticks, spec.sample_every, &check).ok_or(WorkError::Stopped)?;
            let cp = Checkpoint {
                profile: KERNEL_PROFILE.into(),
                spec_digest: job.experiment.spec_digest.clone(),
                run: plan.run,
                result,
            };
            let bytes = serde_json::to_vec(&cp).map_err(|_| WorkError::Execution)?;
            let cid = cas.put(&bytes).map_err(|_| WorkError::Execution)?.to_hex();
            rt.block_on(store.lab_worker(claim.clone(), WorkerAction::Checkpoint { run: plan.run, cid }))
                .map_err(|_| WorkError::LeaseLost)?;
            cp.result
        };
        merge(&mut all, result);
    }
    let mut tables = BTreeMap::new();
    for table in LabTable::ALL {
        if check() {
            return Err(WorkError::Stopped);
        }
        let batch = table_batch(&all, table).map_err(|_| WorkError::Execution)?;
        let bytes = to_parquet(&batch).map_err(|_| WorkError::Execution)?;
        let cid = cas.put(&bytes).map_err(|_| WorkError::Execution)?.to_hex();
        tables.insert(table.name().to_owned(), cid);
    }
    Ok(tables)
}
async fn execute(app: &AppState, job: LabJob, shutdown: &mut tokio::sync::watch::Receiver<bool>) {
    let claim = job.claim_ref();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let cas = app.inner.world_snapshots.clone();
    let store = app.store().clone();
    let rt = tokio::runtime::Handle::current();
    let mut work = tokio::task::spawn_blocking(move || compute(job, cas, store, flag, rt));
    let mut beat = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        tokio::select! {biased;
         _=shutdown.changed()=>{stop.store(true,Ordering::Release);let _=work.await;let _=app.store().lab_worker(claim,WorkerAction::Release).await;return},
         result=&mut work=>{
          let action=match result{Ok(Ok(tables))=>WorkerAction::Complete{tables},Ok(Err(WorkError::Stopped|WorkError::LeaseLost))=>return,_=>WorkerAction::Fail};
          if let Ok(job)=app.store().lab_worker(claim.clone(),action).await {app.audit(Some(claim.owner),"lab.finish",format!("experiment:{}",claim.id),serde_json::json!({"status":job.experiment.status,"generation":job.experiment.generation,"attempt":job.experiment.attempt})).await;}return
         },
         _=beat.tick()=>{if app.store().lab_worker(claim.clone(),WorkerAction::Heartbeat).await.is_err(){stop.store(true,Ordering::Release);let _=work.await;return}},
        }
    }
}
pub fn start_workers(app: &AppState) {
    for _ in 0..app.config().lab_max_concurrent.clamp(1, 16) {
        let weak = Arc::downgrade(&app.inner);
        let mut shutdown = app.inner.lab_stop.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                if *shutdown.borrow() {
                    return;
                }
                let Some(inner) = weak.upgrade() else { return };
                let app = AppState { inner };
                match app.store().lab_claim(app.inner.ranked_node.clone()).await {
                    Ok(Some(job)) => {
                        execute(&app, job, &mut shutdown).await;
                        continue;
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
                drop(app);
                tokio::select! {_=shutdown.changed()=>return,_=tokio::time::sleep(std::time::Duration::from_millis(250))=>{}}
            }
        });
        app.inner
            .lab_workers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(handle);
    }
}
pub async fn stop_workers(app: &AppState) {
    app.inner.lab_stop.send_replace(true);
    let handles = std::mem::take(&mut *app.inner.lab_workers.lock().unwrap_or_else(|p| p.into_inner()));
    for handle in handles {
        let _ = handle.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoint_preserves_exact_u128_and_interruptible_kernel_is_original() {
        let spec = ExperimentSpec {
            name: "checkpoint".into(),
            seeds: vec![57],
            ticks: 3,
            sample_every: 1,
            variants: vec![],
        };
        let plan = spec.runs().remove(0);
        assert!(run_one_interruptible(&plan, 3, 1, &|| true).is_none());
        let mut result = run_one_interruptible(&plan, 3, 1, &|| false).unwrap();
        assert_eq!(result, rynvek_rts::lab::run_one(&plan, 3, 1));
        let huge = (1u128 << 110) + 9;
        result.energy[0].received_q = huge;
        result.energy[0].stored_q = huge;
        let cp = Checkpoint {
            profile: KERNEL_PROFILE.into(),
            spec_digest: "ab".repeat(32),
            run: 0,
            result,
        };
        let bytes = serde_json::to_vec(&cp).unwrap();
        let restored: Checkpoint = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.result, cp.result);
        assert_eq!(restored.result.energy[0].received_q, huge);
    }
}
