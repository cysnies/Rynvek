// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Rynvek 浸泡测试（Soak Test）执行体。由 `cargo xtask soak --duration <时长>` 以 `opt-dev` 配置构建并启动。
//!
//! 在给定时长内循环执行混合负载，每个循环结束时检查不变量并采样进程资源：
//!
//! | 负载 | 不变量 |
//! |---|---|
//! | 竞技场对局（双人 / PvE，脚本策略 + WASM 插件阵营，命令录制） | 实时状态根检查点 == 按命令日志重放的状态根（D0 确定性） |
//! | 同种子重跑 | 最终状态根与首次运行逐位相同 |
//! | 故障插件（spin / bomb / sneaky-*） | 分别以 0x5004 / 0x5005 / 0x5006 隔离，世界继续推进 |
//! | Lab 小实验 + Parquet | 三张表的 Parquet 字节与首个循环逐字节相同 |
//! | `.rpack` Hub 验证（静态扫描 + 1,000 Tick 沙箱） | 报告与首个循环相同 |
//! | `.rworld` 打包 / 解析 + 文件系统 CAS 读写 / 篡改检测 | 读回内容一致，篡改报 0x7002 |
//! | 进程内 HTTP 服务端（健康检查、Hub 检索） | 全部 200 |
//!
//! 资源不变量：预热后 RSS 无单调增长（最小二乘斜率外推增长 ≤ 阈值，或末三分之一最小值不高于前三分之一最大值）、
//! 文件描述符数与线程数不增长。任一不变量失败即以非零退出码结束；panic 同样视为失败。

#![deny(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rynvek_core::error::RynvekErrorCode;
use rynvek_engine::plugins::ArenaPluginDriver;
use rynvek_rts::lab::policy::{PolicyKind, run_policy};
use rynvek_rts::lab::{ExperimentSpec, LabMode, VariantSpec, arrow_io, run_experiment};
use rynvek_rts::{ArenaWorld, FactionId, MatchConfig};
use rynvek_rules::plugin::{PluginModule, PluginRuntime};
use rynvek_storage::{BlobStore, FsBlobStore};
use rynvek_world::{PackageInput, PackageInputEntry, PackageKind, WorldBundle, write_package};
use serde_json::json;

/// 单局最大 Tick 数（30 Hz 下 1 分钟）
const MATCH_TICKS: u64 = 1_800;
/// 实时 / 重放状态根比对间隔
const CHECKPOINT_EVERY: u64 = 150;
/// 预热占比（之前的资源样本不参与增长判定）
const WARMUP_FRACTION: f64 = 0.2;
/// 允许的 RSS 外推增长上限
const RSS_GROWTH_LIMIT_BYTES: f64 = 48.0 * 1024.0 * 1024.0;
/// 允许的文件描述符 / 线程数波动
const FD_SLACK: i64 = 8;
const THREAD_SLACK: i64 = 4;

#[derive(Debug)]
struct Args {
    duration: Duration,
    report: PathBuf,
    root: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut duration = None;
    let mut report = None;
    let mut root = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--duration-secs" => {
                let v = it.next().ok_or("--duration-secs 需要参数")?;
                duration = Some(Duration::from_secs(v.parse().map_err(|_| format!("非法秒数 {v}"))?));
            }
            "--report" => report = Some(PathBuf::from(it.next().ok_or("--report 需要参数")?)),
            "--root" => root = Some(PathBuf::from(it.next().ok_or("--root 需要参数")?)),
            other => return Err(format!("未知参数 {other}")),
        }
    }
    Ok(Args {
        duration: duration.ok_or("缺少 --duration-secs")?,
        report: report.ok_or("缺少 --report")?,
        root: root.ok_or("缺少 --root")?,
    })
}

/// 一次资源采样
#[derive(Clone, Copy, Debug)]
struct Sample {
    elapsed_s: f64,
    rss_bytes: u64,
    fds: i64,
    threads: i64,
}

fn sample(start: Instant) -> Sample {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    // statm 第 2 列为常驻页数；Linux x86_64 / aarch64 常规页大小 4 KiB
    let pages: u64 = statm
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let fds = std::fs::read_dir("/proc/self/fd").map_or(-1, |d| d.count() as i64);
    let threads = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Threads:").map(|v| v.trim().parse().unwrap_or(-1)))
        })
        .unwrap_or(-1);
    Sample {
        elapsed_s: start.elapsed().as_secs_f64(),
        rss_bytes: pages * 4096,
        fds,
        threads,
    }
}

/// 不变量计数
#[derive(Debug, Default)]
struct Counters {
    cycles: u64,
    ticks_simulated: u64,
    state_root_checkpoints: u64,
    replays_verified: u64,
    determinism_reruns: u64,
    plugin_ticks: u64,
    plugin_isolations: u64,
    lab_experiments: u64,
    hub_verifications: u64,
    cas_roundtrips: u64,
    http_requests: u64,
}

/// 跨循环保持不变的参考结果
#[derive(Debug, Default)]
struct Reference {
    final_roots: BTreeMap<u64, [u8; 32]>,
    lab_parquet: Option<[Vec<u8>; 3]>,
    hub_report: Option<String>,
}

struct Ctx {
    runtime: PluginRuntime,
    probe: PluginModule,
    probe_bytes: Vec<u8>,
    cas: FsBlobStore,
    cas_dir: PathBuf,
    http: rynvek_server::RunningServer,
    tokio: tokio::runtime::Runtime,
    counters: Counters,
    reference: Reference,
}

fn check(cond: bool, what: impl FnOnce() -> String) -> Result<(), String> {
    if cond { Ok(()) } else { Err(what()) }
}

/// 对局配置：按循环编号轮换双人局 / 单人 PvE / 双人 PvE
fn match_config(seed: u64) -> MatchConfig {
    match seed % 3 {
        0 => MatchConfig::duel(seed),
        1 => MatchConfig::pve(seed, 1),
        _ => MatchConfig::pve(seed, 2),
    }
}

/// 一局的结果：（最终状态根, 检查点 `(tick, 状态根)`, 终局世界）
type MatchRun = ([u8; 32], Vec<(u64, [u8; 32])>, ArenaWorld);

/// 运行一局：阵营 0 在奇数循环由探针插件控制，其余阵营使用经济策略；返回（最终状态根, 检查点）
fn play_match(ctx: &mut Ctx, seed: u64) -> Result<MatchRun, String> {
    let config = match_config(seed);
    let mut world = ArenaWorld::new(config.clone());
    world.enable_command_log();
    let players: Vec<FactionId> = world.factions.iter().map(|f| f.id).collect();
    let mut driver = if seed % 2 == 1 {
        Some(
            ArenaPluginDriver::new(
                &ctx.runtime,
                &ctx.probe,
                r#"{"mode":"probe"}"#,
                config.world_seed,
                players[0],
            )
            .map_err(|e| format!("探针插件实例化失败：{e:?}"))?,
        )
    } else {
        None
    };
    let mut checkpoints = Vec::new();
    while world.tick < MATCH_TICKS && !world.is_over() {
        if world.tick % CHECKPOINT_EVERY == 0 {
            checkpoints.push((world.tick, world.state_hash().0));
        }
        for &f in &players {
            match (&mut driver, f == players[0]) {
                (Some(d), true) => {
                    d.tick(&mut world).map_err(|e| format!("探针插件被意外隔离：{e:?}"))?;
                    ctx.counters.plugin_ticks += 1;
                }
                _ => {
                    run_policy(&mut world, f, PolicyKind::Economy);
                }
            }
        }
        world.step();
        ctx.counters.ticks_simulated += 1;
    }
    Ok((world.state_hash().0, checkpoints, world))
}

/// 负载 1：对局 + 命令日志重放 + 同种子重跑
fn arena_cycle(ctx: &mut Ctx, seed: u64) -> Result<(), String> {
    let (root, checkpoints, world) = play_match(ctx, seed)?;
    let mut replayed = Vec::new();
    let end = world.tick;
    let replay = ArenaWorld::replay(match_config(seed), world.command_log(), end, |w| {
        // 实时循环只在推进前记录检查点（不含终局 Tick），重放侧对齐
        if w.tick % CHECKPOINT_EVERY == 0 && w.tick < end {
            replayed.push((w.tick, w.state_hash().0));
        }
    })
    .map_err(|(i, e)| format!("种子 {seed}：重放第 {i} 条命令失败：{e}"))?;
    ctx.counters.ticks_simulated += replay.tick;
    check(replayed == checkpoints, || {
        format!("种子 {seed}：实时与重放检查点状态根不一致")
    })?;
    check(replay.state_hash().0 == root, || {
        format!("种子 {seed}：重放最终状态根不一致")
    })?;
    ctx.counters.state_root_checkpoints += checkpoints.len() as u64 + 1;
    ctx.counters.replays_verified += 1;
    // 同种子重跑：种子空间只取 0..12，后续循环与首次结果逐位比对
    if let Some(prev) = ctx.reference.final_roots.get(&seed) {
        check(*prev == root, || format!("种子 {seed}：同种子重跑状态根漂移"))?;
        ctx.counters.determinism_reruns += 1;
    } else {
        ctx.reference.final_roots.insert(seed, root);
    }
    Ok(())
}

/// 负载 2：故障插件隔离
fn plugin_fault_cycle(ctx: &mut Ctx, seed: u64) -> Result<(), String> {
    let cases = [
        ("spin", RynvekErrorCode::SandboxGasBudgetExhausted),
        ("bomb", RynvekErrorCode::SandboxMemoryLimitExceeded),
        ("sneaky-command", RynvekErrorCode::PluginCapabilityDenied),
    ];
    for (mode, expected) in cases {
        let mut world = ArenaWorld::new(MatchConfig::pve(seed, 1));
        let cfg = json!({ "mode": mode }).to_string();
        let fault = match ArenaPluginDriver::new(&ctx.runtime, &ctx.probe, &cfg, seed, FactionId(0)) {
            Err(f) => f,
            Ok(mut d) => d
                .tick(&mut world)
                .err()
                .ok_or_else(|| format!("故障插件 {mode} 未被隔离"))?,
        };
        check(fault.code() == expected, || {
            format!("故障插件 {mode}：期望 {expected:?}，实际 {:?}", fault.code())
        })?;
        world.run(30);
        check(world.tick == 30, || format!("隔离 {mode} 后世界未继续推进"))?;
        ctx.counters.plugin_isolations += 1;
    }
    Ok(())
}

/// 负载 3：Lab 小实验 + Parquet 逐字节稳定
fn lab_cycle(ctx: &mut Ctx) -> Result<(), String> {
    let spec = ExperimentSpec {
        name: "soak".into(),
        seeds: vec![1, 2],
        ticks: 300,
        sample_every: 60,
        variants: vec![
            VariantSpec::baseline(),
            VariantSpec {
                label: "duel".into(),
                mode: LabMode::Duel,
                players: None,
                policy: PolicyKind::Economy,
            },
        ],
    };
    let result = run_experiment(&spec, &|_, _| {});
    let mut out: [Vec<u8>; 3] = Default::default();
    for (i, t) in [
        arrow_io::LabTable::Runs,
        arrow_io::LabTable::Metrics,
        arrow_io::LabTable::Lineage,
    ]
    .into_iter()
    .enumerate()
    {
        let batch = arrow_io::table_batch(&result, t).map_err(|e| e.to_string())?;
        out[i] = arrow_io::to_parquet(&batch).map_err(|e| e.to_string())?;
    }
    match &ctx.reference.lab_parquet {
        Some(prev) => check(*prev == out, || "Lab Parquet 输出跨循环漂移".into())?,
        None => ctx.reference.lab_parquet = Some(out),
    }
    ctx.counters.lab_experiments += 1;
    Ok(())
}

fn package(
    kind: PackageKind,
    id: &str,
    license: &str,
    entries: Vec<(&str, &str, &str, Vec<u8>)>,
) -> Result<Vec<u8>, String> {
    write_package(&PackageInput {
        kind,
        id: id.into(),
        version: "1.0.0".into(),
        name: format!("{id} soak"),
        license: license.into(),
        engine_compat: ">=1.0.0 <2.0.0".into(),
        entries: entries
            .into_iter()
            .map(|(path, kind, media_type, data)| PackageInputEntry {
                path: path.into(),
                kind: kind.into(),
                media_type: media_type.into(),
                data,
            })
            .collect(),
    })
    .map_err(|e| format!("写包失败：{e:?}"))
}

/// 负载 4：Hub 验证（`.rpack` 静态扫描 + 1,000 Tick 沙箱）
fn hub_cycle(ctx: &mut Ctx) -> Result<(), String> {
    let artifact = package(
        PackageKind::Rpack,
        "community.soak-probe",
        "Apache-2.0 WITH LLVM-exception",
        vec![(
            "plugins/main.wasm",
            "plugin",
            "application/wasm",
            ctx.probe_bytes.clone(),
        )],
    )?;
    let lock = json!({});
    let report = rynvek_server::verify::verify(
        &rynvek_server::verify::VerifyInput {
            kind: "rpack",
            package_id: "community.soak-probe",
            semver: "1.0.0",
            license_spdx: "Apache-2.0 WITH LLVM-exception",
            lockfile: &lock,
            artifact: &artifact,
            official: false,
        },
        &ctx.runtime,
    )
    .map_err(|e| format!("Hub 验证失败：{e:?}"))?;
    let digest = format!(
        "{}|{}|{}|{:?}",
        report.static_scan, report.trust_level, report.sandbox, report.findings
    );
    match &ctx.reference.hub_report {
        Some(prev) => check(*prev == digest, || "Hub 验证报告跨循环漂移".into())?,
        None => ctx.reference.hub_report = Some(digest),
    }
    ctx.counters.hub_verifications += 1;
    Ok(())
}

/// 负载 5：`.rworld` 打包 / 解析 + 文件系统 CAS
fn cas_cycle(ctx: &mut Ctx, seed: u64) -> Result<(), String> {
    let world_json =
        json!({ "schema": "rynvek.world-config.v1", "template": "arena_pve", "map_seed": seed }).to_string();
    let bytes = package(
        PackageKind::Rworld,
        "community.soak-world",
        "CC0-1.0",
        vec![(
            "world.json",
            "world_config",
            "application/json",
            world_json.into_bytes(),
        )],
    )?;
    let bundle = WorldBundle::from_bytes(&bytes).map_err(|e| format!("解析 .rworld 失败：{e:?}"))?;
    let w = bundle.instantiate();
    check(
        w.state_hash() == ArenaWorld::new(MatchConfig::pve(seed, 1)).state_hash(),
        || "`.rworld` 实例化状态根与模板不一致".into(),
    )?;
    let id = ctx.cas.put(&bytes).map_err(|e| format!("CAS 写入失败：{e:?}"))?;
    let back = ctx.cas.get(&id).map_err(|e| format!("CAS 读取失败：{e:?}"))?;
    check(back == bytes, || "CAS 读回内容不一致".into())?;
    // 篡改检测：改写文件后读取必须报 0x7002，然后删除（下次循环重新写入）
    let hex = rynvek_security::to_hex(&id.0);
    let path = ctx.cas_dir.join(&hex[..2]).join(&hex);
    let mut corrupted = bytes.clone();
    corrupted[0] ^= 0xFF;
    std::fs::write(&path, &corrupted).map_err(|e| e.to_string())?;
    let err = ctx.cas.get(&id).err();
    check(err == Some(RynvekErrorCode::Blake3ChecksumMismatch), || {
        format!("CAS 篡改未被检测：{err:?}")
    })?;
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    ctx.counters.cas_roundtrips += 1;
    Ok(())
}

/// 负载 6：进程内 HTTP 服务端
fn http_cycle(ctx: &mut Ctx) -> Result<(), String> {
    let base = format!("http://{}", ctx.http.http_addr);
    let paths = [
        "/api/v1/meta/health",
        "/api/v1/hub/packages?kind=rpack",
        "/api/v1/meta/source-offer",
    ];
    let n = ctx.tokio.block_on(async {
        let mut n = 0u64;
        for _ in 0..10 {
            for p in paths {
                let r = rynvek_server::client::get(&format!("{base}{p}"), None).await?;
                if r.status != 200 {
                    return Err(format!("GET {p} 返回 {}", r.status));
                }
                n += 1;
            }
        }
        Ok::<_, String>(n)
    })?;
    ctx.counters.http_requests += n;
    Ok(())
}

fn cycle(ctx: &mut Ctx) -> Result<(), String> {
    let c = ctx.counters.cycles;
    let seed = c % 12;
    arena_cycle(ctx, seed)?;
    if c % 4 == 0 {
        plugin_fault_cycle(ctx, seed)?;
    }
    if c % 6 == 0 {
        lab_cycle(ctx)?;
        hub_cycle(ctx)?;
    }
    cas_cycle(ctx, seed)?;
    http_cycle(ctx)?;
    ctx.counters.cycles += 1;
    Ok(())
}

/// 资源增长判定；返回（是否通过, 说明）
fn resource_verdict(samples: &[Sample], duration_s: f64) -> (bool, serde_json::Value) {
    let warm_s = duration_s * WARMUP_FRACTION;
    let steady: Vec<&Sample> = samples.iter().filter(|s| s.elapsed_s >= warm_s).collect();
    if steady.len() < 6 {
        return (
            false,
            json!({ "reason": format!("预热后的资源样本不足（{} 个，至少 6 个）；请延长 --duration", steady.len()) }),
        );
    }
    let n = steady.len() as f64;
    let mx = steady.iter().map(|s| s.elapsed_s).sum::<f64>() / n;
    let my = steady.iter().map(|s| s.rss_bytes as f64).sum::<f64>() / n;
    let sxx: f64 = steady.iter().map(|s| (s.elapsed_s - mx).powi(2)).sum();
    let sxy: f64 = steady
        .iter()
        .map(|s| (s.elapsed_s - mx) * (s.rss_bytes as f64 - my))
        .sum();
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    let window = steady.last().map_or(0.0, |s| s.elapsed_s) - steady[0].elapsed_s;
    let projected = slope * window;
    let third = steady.len() / 3;
    let first_max = steady[..third].iter().map(|s| s.rss_bytes).max().unwrap_or(0);
    let last_min = steady[steady.len() - third..]
        .iter()
        .map(|s| s.rss_bytes)
        .min()
        .unwrap_or(0);
    // 单调增长判定：外推增长超过阈值，且末三分之一的最低点高于前三分之一的最高点
    let monotonic_growth = projected > RSS_GROWTH_LIMIT_BYTES && last_min > first_max;
    let fd0 = steady[0].fds;
    let th0 = steady[0].threads;
    let fd_max = steady.iter().map(|s| s.fds).max().unwrap_or(0);
    let th_max = steady.iter().map(|s| s.threads).max().unwrap_or(0);
    let fd_ok = fd_max <= fd0 + FD_SLACK;
    let th_ok = th_max <= th0 + THREAD_SLACK;
    (
        !monotonic_growth && fd_ok && th_ok,
        json!({
            "steady_samples": steady.len(),
            "rss_first_bytes": steady[0].rss_bytes,
            "rss_last_bytes": steady.last().map(|s| s.rss_bytes),
            "rss_max_bytes": steady.iter().map(|s| s.rss_bytes).max(),
            "rss_slope_bytes_per_s": slope,
            "rss_projected_growth_bytes": projected,
            "rss_growth_limit_bytes": RSS_GROWTH_LIMIT_BYTES,
            "rss_first_third_max": first_max,
            "rss_last_third_min": last_min,
            "rss_monotonic_growth": monotonic_growth,
            "fds_start": fd0, "fds_max": fd_max, "fds_ok": fd_ok,
            "threads_start": th0, "threads_max": th_max, "threads_ok": th_ok,
        }),
    )
}

fn run(args: &Args) -> Result<serde_json::Value, String> {
    let start = Instant::now();
    let probe_path = args
        .root
        .join("crates/rynvek-rules/tests/fixtures/plugin/probe.component.wasm");
    let probe_bytes = std::fs::read(&probe_path).map_err(|e| format!("读取 {}：{e}", probe_path.display()))?;
    let runtime = PluginRuntime::new().map_err(|e| format!("{e:?}"))?;
    let probe = runtime.compile(&probe_bytes).map_err(|e| format!("{e:?}"))?;
    let cas_dir = std::env::temp_dir().join(format!("rynvek-soak-{}", std::process::id()));
    let cas = FsBlobStore::open(&cas_dir).map_err(|e| e.to_string())?;
    let tokio = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let http = tokio
        .block_on(rynvek_server::start(rynvek_server::ServerConfig {
            http_addr: "127.0.0.1:0".parse().expect("合法地址"),
            webtransport_addr: None,
            app: rynvek_server::app::AppConfig::default(),
            store: rynvek_server::store::Store::memory(),
            signer: rynvek_security::ServerSigningKey::generate(1).map_err(|e| e.to_string())?,
        }))
        .map_err(|e| format!("服务端启动失败：{e:?}"))?;
    let mut ctx = Ctx {
        runtime,
        probe,
        probe_bytes,
        cas,
        cas_dir: cas_dir.clone(),
        http,
        tokio,
        counters: Counters::default(),
        reference: Reference::default(),
    };
    let duration_s = args.duration.as_secs_f64();
    let mut samples = vec![sample(start)];
    let mut failure = None;
    let mut last_log = Instant::now();
    while start.elapsed() < args.duration {
        if let Err(e) = cycle(&mut ctx) {
            failure = Some(e);
            break;
        }
        samples.push(sample(start));
        if last_log.elapsed() >= Duration::from_secs(60) {
            last_log = Instant::now();
            let s = samples.last().expect("至少一个样本");
            eprintln!(
                "[soak] {:>8.0}s 循环 {} · Tick {} · RSS {:.1} MiB · fd {} · 线程 {}",
                s.elapsed_s,
                ctx.counters.cycles,
                ctx.counters.ticks_simulated,
                s.rss_bytes as f64 / 1048576.0,
                s.fds,
                s.threads
            );
        }
    }
    let Ctx {
        http, tokio, counters, ..
    } = ctx;
    tokio.block_on(http.shutdown());
    drop(tokio);
    let _ = std::fs::remove_dir_all(&cas_dir);
    let (resources_ok, resources) = resource_verdict(&samples, duration_s);
    let passed = failure.is_none() && resources_ok;
    let c = &counters;
    Ok(json!({
        "passed": passed,
        "failure": failure,
        "requested_duration_s": duration_s,
        "actual_duration_s": start.elapsed().as_secs_f64(),
        "counters": {
            "cycles": c.cycles,
            "ticks_simulated": c.ticks_simulated,
            "state_root_checkpoints": c.state_root_checkpoints,
            "replays_verified": c.replays_verified,
            "determinism_reruns": c.determinism_reruns,
            "plugin_ticks": c.plugin_ticks,
            "plugin_isolations": c.plugin_isolations,
            "lab_experiments": c.lab_experiments,
            "hub_verifications": c.hub_verifications,
            "cas_roundtrips": c.cas_roundtrips,
            "http_requests": c.http_requests,
        },
        "resources_ok": resources_ok,
        "resources": resources,
        "samples": samples.iter().map(|s| json!([s.elapsed_s, s.rss_bytes, s.fds, s.threads])).collect::<Vec<_>>(),
    }))
}

fn write_report(path: &Path, report: &serde_json::Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn main() -> std::process::ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("错误：{e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let report = match run(&args) {
        Ok(r) => r,
        Err(e) => json!({ "passed": false, "failure": e }),
    };
    if let Err(e) = write_report(&args.report, &report) {
        eprintln!("错误：写报告失败：{e}");
        return std::process::ExitCode::FAILURE;
    }
    println!(
        "{}",
        serde_json::to_string(&json!({
            "passed": report["passed"],
            "failure": report["failure"],
            "actual_duration_s": report["actual_duration_s"],
            "counters": report["counters"],
            "resources": report["resources"],
            "report": args.report.display().to_string(),
        }))
        .unwrap_or_default()
    );
    if report["passed"] == true {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
