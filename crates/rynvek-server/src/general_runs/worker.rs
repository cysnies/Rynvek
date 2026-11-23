// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 服务启动恢复Store中的明确运行意图；Weak不把进程缓存变成任务事实源。
use crate::{
    app::{AppInner, AppState},
    lab_jobs::{Claim, WorkerAction},
};
use std::{
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::watch;
pub fn start_workers(app: &AppState) {
    // 一个有限串行调度槽，多节点经原作业租约竞争；不是每个HTTP请求派生无界任务。
    let weak = Arc::downgrade(&app.inner);
    let stop = app.inner.general_run_stop.subscribe();
    let task = tokio::spawn(worker(weak, stop));
    app.inner
        .general_run_workers
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(task);
}
pub async fn stop_workers(app: &AppState) {
    let _ = app.inner.general_run_stop.send(true);
    let tasks = std::mem::take(&mut *app.inner.general_run_workers.lock().unwrap_or_else(|p| p.into_inner()));
    for t in tasks {
        let _ = t.await;
    }
}
async fn wait(stop: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    if *stop.borrow() {
        return true;
    }
    tokio::select! {_=tokio::time::sleep(duration)=>false,_=stop.changed()=>true}
}
async fn worker(weak: Weak<AppInner>, mut stop: watch::Receiver<bool>) {
    loop {
        if *stop.borrow() {
            return;
        }
        let Some(inner) = weak.upgrade() else { return };
        let app = AppState { inner };
        match app.store().general_run_claim(app.inner.ranked_node.clone()).await {
            Ok(Some(j)) => drive(&app, j.claim_of(), &mut stop).await,
            Ok(None) => {}
            Err(e) => eprintln!("General运行认领失败：{e}"),
        }
        drop(app);
        if wait(&mut stop, Duration::from_millis(250)).await {
            return;
        }
    }
}
async fn drive(app: &AppState, c: Claim, stop: &mut watch::Receiver<bool>) {
    loop {
        if *stop.borrow() {
            let _ = app.store().general_run_worker(c.clone(), WorkerAction::Release).await;
            return;
        }
        let j = match app.store().general_run_worker(c.clone(), WorkerAction::Heartbeat).await {
            Ok(j) => j,
            Err(_) => {
                let _ = app.store().general_run_worker(c.clone(), WorkerAction::Fail).await;
                return;
            }
        };
        match crate::general_host::run_step(app, c.clone()).await {
            Ok(_) => {}
            Err(_) => {
                // 末步提交可能已成功而回执过期；只按Store的现状态封闭，不重发已完成步。
                let _ = app.store().general_run_worker(c.clone(), WorkerAction::Fail).await;
                return;
            }
        }
        if wait(stop, Duration::from_millis(j.spec.interval_ms as u64)).await {
            let _ = app.store().general_run_worker(c.clone(), WorkerAction::Release).await;
            return;
        }
    }
}
