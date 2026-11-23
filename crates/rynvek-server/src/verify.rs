// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Rynvek Hub 发布验证：静态扫描 + 1,000 Tick 沙箱压力测试（项目文档 7.2 TrustLevel 2 准入）。
//!
//! | 包类型 | 静态扫描 | 沙箱 |
//! |---|---|---|
//! | `.rview` | 容器结构与哈希；每个 `shader` 条目按目标信任等级做 WGSL 审查（与浏览器端同规则）+ naga 校验 | 不适用（纯表现层，无模拟副作用） |
//! | `.rpack` | 容器；`rule_module` 解码 `.rir`（魔数、版本、摘要）；`rule_source` 必须为 UTF-8 且 ≤ 256 KiB；`plugin` 编译并检查导入 / 导出；`genome` / JSON `data` 必须是合法 JSON | 每个插件绑定阵营 0，在 PvE 世界里驱动 1,000 Tick（500,000 Gas / 调用、64 MiB） |
//! | `.rworld` | 容器；`world.json` 与 `dependencies.lock` 结构；lockfile 必须与发布请求一致 | 由世界包创建世界并推进 1,000 Tick，重放核对状态根 |
//!
//! 任一项失败 → `0x9004 UnsafeRviewShaderRejected`（Hub 静态扫描 / 沙箱拒绝，消息中带具体原因与底层错误码）；
//! 容器结构 / 哈希错误保留 `0x7001` / `0x7002`。`audio` / `agent` / `dataset` 尚无容器格式，不做扫描，信任等级保持 3。
//! 通过扫描的非官方包授予 TrustLevel 2，官方组织保持 TrustLevel 1（ERRATA E-051）。

use rynvek_core::error::RynvekErrorCode;
use rynvek_engine::plugins::ArenaPluginDriver;
use rynvek_rts::{ArenaWorld, FactionId, MatchConfig};
use rynvek_rules::plugin::PluginRuntime;
use rynvek_world::package::{Package, PackageKind, read_package};
use rynvek_world::rworld::WorldBundle;
use rynvek_world::wgsl_review::{TrustLevel, review_rview_shader, validate_with_naga};

use crate::error::ApiError;

/// 沙箱压力测试的 Tick 数（项目文档 7.2）
pub const SANDBOX_TICKS: u64 = 1_000;
/// `rule_source` 条目上限
pub const MAX_RULE_SOURCE_BYTES: usize = 256 << 10;

/// 发布请求中与验证相关的字段
#[derive(Clone, Debug)]
pub struct VerifyInput<'a> {
    pub kind: &'a str,
    pub package_id: &'a str,
    pub semver: &'a str,
    pub license_spdx: &'a str,
    pub lockfile: &'a serde_json::Value,
    pub artifact: &'a [u8],
    /// 发布组织是否为官方（决定 WGSL 预算：官方不限，社区按 TrustLevel 2）
    pub official: bool,
}

/// 验证报告（写入发布响应的 `verification` 字段）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    /// `passed` 或 `not_applicable`
    pub static_scan: &'static str,
    pub sandbox: serde_json::Value,
    pub findings: Vec<serde_json::Value>,
    /// 授予的信任等级
    pub trust_level: u8,
}

fn reject(detail: impl Into<String>) -> ApiError {
    ApiError::new(RynvekErrorCode::UnsafeRviewShaderRejected, detail)
}

/// 执行验证（CPU 密集；调用方应在 `spawn_blocking` 中运行）
pub fn verify(input: &VerifyInput<'_>, plugins: &PluginRuntime) -> Result<VerifyReport, ApiError> {
    let Some(kind) = PackageKind::parse(input.kind) else {
        return Ok(VerifyReport {
            static_scan: "not_applicable",
            sandbox: serde_json::json!({ "status": "not_applicable" }),
            findings: Vec::new(),
            trust_level: if input.official { 1 } else { 3 },
        });
    };
    let pkg = read_package(input.artifact, Some(kind)).map_err(|e| ApiError::new(e.code, e.detail))?;
    let m = &pkg.manifest;
    if m.id != input.package_id || m.version != input.semver {
        return Err(ApiError::new(
            RynvekErrorCode::InvalidFileMagicOrVersion,
            format!(
                "包内清单 {}@{} 与发布路径 {}@{} 不一致",
                m.id, m.version, input.package_id, input.semver
            ),
        ));
    }
    if m.license != input.license_spdx {
        return Err(ApiError::new(
            RynvekErrorCode::InvalidFileMagicOrVersion,
            format!("包内许可证 {} 与 license_spdx {} 不一致", m.license, input.license_spdx),
        ));
    }
    let target = if input.official {
        TrustLevel::OfficialSigned
    } else {
        TrustLevel::VerifiedCreator
    };
    let mut findings = Vec::new();
    let sandbox = match kind {
        PackageKind::Rview => {
            scan_rview(&pkg, target, &mut findings)?;
            serde_json::json!({ "status": "not_applicable" })
        }
        PackageKind::Rpack => scan_rpack(&pkg, input.artifact, plugins, &mut findings)?,
        PackageKind::Rworld => scan_rworld(pkg, input.lockfile)?,
    };
    Ok(VerifyReport {
        static_scan: "passed",
        sandbox,
        findings,
        trust_level: target as u8,
    })
}

fn scan_rview(pkg: &Package, target: TrustLevel, findings: &mut Vec<serde_json::Value>) -> Result<(), ApiError> {
    for (e, data) in pkg.entries_of("shader") {
        let src = std::str::from_utf8(data).map_err(|_| reject(format!("着色器 {} 不是 UTF-8", e.path)))?;
        let r = review_rview_shader(src, target).map_err(|d| reject(format!("{}：{d}", e.path)))?;
        validate_with_naga(src).map_err(|d| reject(format!("{}：{d}", e.path)))?;
        findings.push(serde_json::json!({
            "path": e.path,
            "entry_point": r.entry_point,
            "instruction_count": r.instruction_count,
            "budget": r.budget,
        }));
    }
    for kind in ["palette", "theme"] {
        for (e, data) in pkg.entries_of(kind) {
            serde_json::from_slice::<serde_json::Value>(data)
                .map_err(|err| reject(format!("{} 不是合法 JSON：{err}", e.path)))?;
        }
    }
    Ok(())
}

fn check_json_entries(pkg: &Package) -> Result<(), ApiError> {
    for e in &pkg.manifest.entries {
        if e.media_type == "application/json" {
            let data = pkg.file(&e.path).unwrap_or_default();
            serde_json::from_slice::<serde_json::Value>(data)
                .map_err(|err| reject(format!("{} 不是合法 JSON：{err}", e.path)))?;
        }
    }
    Ok(())
}

fn scan_rpack(
    pkg: &Package,
    artifact: &[u8],
    plugins: &PluginRuntime,
    findings: &mut Vec<serde_json::Value>,
) -> Result<serde_json::Value, ApiError> {
    check_json_entries(pkg)?;
    if pkg
        .file("rule.json")
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
        .is_some_and(|v| v["profile"] == rynvek_rules::package::TIER0_BUILD_PROFILE)
    {
        let built = rynvek_rules::package::Tier0RulePackage::parse(artifact)
            .map_err(|e| reject(format!("Tier0构建剖面拒绝：{}", e.detail)))?;
        findings.push(
            serde_json::json!({"build_profile":rynvek_rules::package::TIER0_BUILD_PROFILE,
            "rule_module":built.module.name,"graph_rir_recompiled_equal":true,"ts_source_equivalence":false}),
        );
    }
    for (e, data) in pkg.entries_of("rule_module") {
        let module = rynvek_rules::rir::decode_rir(data).map_err(|err| reject(format!("{}：{err}", e.path)))?;
        findings.push(serde_json::json!({ "path": e.path, "rule_module": module.name }));
    }
    for (e, data) in pkg.entries_of("rule_source") {
        if data.len() > MAX_RULE_SOURCE_BYTES || std::str::from_utf8(data).is_err() {
            return Err(reject(format!(
                "{} 必须是不超过 {MAX_RULE_SOURCE_BYTES} 字节的 UTF-8 源码",
                e.path
            )));
        }
    }
    let mut reports = Vec::new();
    for (e, data) in pkg.entries_of("plugin") {
        let module = plugins.compile(data).map_err(|f| reject(format!("{}：{f}", e.path)))?;
        let config = MatchConfig::pve(1, 1);
        let mut world = ArenaWorld::new(config.clone());
        let mut driver = ArenaPluginDriver::new(plugins, &module, "{}", config.world_seed, FactionId(0))
            .map_err(|f| reject(format!("{}：插件初始化失败 {f}", e.path)))?;
        for _ in 0..SANDBOX_TICKS {
            if let Err(f) = driver.tick(&mut world) {
                if f.isolates() {
                    return Err(reject(format!(
                        "{}：1,000 Tick 沙箱压力测试在第 {} Tick 失败（{}）",
                        e.path, world.tick, f
                    )));
                }
            }
            world.step();
        }
        let stats = driver.instance().stats();
        reports.push(serde_json::json!({
            "path": e.path,
            "plugin_id": driver.instance().manifest().id,
            "ticks": SANDBOX_TICKS,
            "max_gas_per_call": stats.gas_max_call,
            "commands_emitted": stats.commands_emitted,
            "commands_accepted": driver.stats().accepted,
            "guest_errors": stats.guest_errors,
        }));
    }
    Ok(if reports.is_empty() {
        serde_json::json!({ "status": "not_applicable" })
    } else {
        serde_json::json!({ "status": "passed", "plugins": reports })
    })
}

fn scan_rworld(pkg: Package, lockfile: &serde_json::Value) -> Result<serde_json::Value, ApiError> {
    check_json_entries(&pkg)?;
    let bundle = WorldBundle::from_package(pkg).map_err(|e| ApiError::new(e.code, e.detail))?;
    if let Some(inner) = &bundle.lockfile {
        if inner != lockfile {
            return Err(ApiError::new(
                RynvekErrorCode::DependencyLockConflict,
                "包内 dependencies.lock 与发布请求中的 lockfile 不一致",
            ));
        }
    }
    let mut world = bundle.instantiate();
    world.enable_command_log();
    world.run(SANDBOX_TICKS);
    let live = world.state_hash();
    let replayed = ArenaWorld::replay(bundle.match_config.clone(), world.command_log(), SANDBOX_TICKS, |_| {})
        .map_err(|(i, e)| reject(format!("1,000 Tick 沙箱：重放第 {i} 条命令失败：{e:?}")))?
        .state_hash();
    if live != replayed {
        return Err(reject("1,000 Tick 沙箱：重放状态根与实时状态根不一致"));
    }
    Ok(serde_json::json!({
        "status": "passed",
        "ticks": SANDBOX_TICKS,
        "state_root": live.to_hex(),
    }))
}
