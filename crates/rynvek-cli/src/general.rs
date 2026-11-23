// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 本地文件所有者工具；不会注册公开房间、伪造 Actor lease 或执行包内脚本。
use crate::{CliError, CliResult};
use clap::Subcommand;
use rynvek_simulation::engine::SimulationEngine;
use rynvek_world::general::GeneralWorldConfig;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
#[derive(Debug, Subcommand)]
pub enum WorldCommand {
    Build {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "1.0.0")]
        version: String,
        /// 本地创世显式安装：模块ID=原Tier0.rpack路径，可重复；不是私库签名/依赖解析。
        #[arg(long = "rule-pack")]
        rule_packs: Vec<String>,
    },
    Run {
        #[arg(long)]
        package: PathBuf,
        /// 32 位十六进制世界 ID。
        #[arg(long)]
        world_id: String,
        #[arg(long, default_value_t = 30)]
        ticks: u32,
        /// 创世快照，必须为新文件；与 journal 一起用于恢复。
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long)]
        journal: PathBuf,
    },
    Recover {
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long)]
        journal: PathBuf,
    },
}
fn fail(e: impl std::fmt::Debug) -> CliError {
    CliError::new(format!("通用 World：{e:?}"))
}
pub(crate) fn read(path: &Path, max: usize) -> CliResult<Vec<u8>> {
    let f = std::fs::File::open(path).map_err(fail)?;
    let mut bytes = Vec::new();
    f.take((max + 1) as u64).read_to_end(&mut bytes).map_err(fail)?;
    if bytes.len() > max {
        return Err(CliError::new("文件超出通用 World 上限"));
    }
    Ok(bytes)
}
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> CliResult {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(fail)?;
    f.write_all(bytes).map_err(fail)?;
    f.sync_all().map_err(fail)?;
    Ok(())
}
fn summary(world: &SimulationEngine, torn: bool) -> CliResult {
    use rynvek_render::{FrameScene, NeuralLod, RenderLayerMask, VisualizationProvider};
    let mut scene = FrameScene::new(world.clock.tick, RenderLayerMask::ALL);
    rynvek_engine::provider::EntityProvider::default().populate(world, NeuralLod::L2, &mut scene);
    let root: String = world.parent_commit_hash.iter().map(|b| format!("{b:02x}")).collect();
    let bodies: Vec<_> = world
        .physics()
        .iter()
        .map(|(serial, body)| serde_json::json!({"serial":serial,"body":body}))
        .collect();
    // 真实复用引擎 FrameScene；不是另写一套物理坐标到图像的代理投影。
    let instances: Vec<_> = scene
        .ordered_instances()
        .iter()
        .map(|i| serde_json::json!({"position":i.position,"pickId":i.pick_id}))
        .collect();
    let life = world.life().map(|store| {
        let accounts: Vec<_> = store
            .accounts()
            .map(|a| {
                serde_json::json!({
                    "received": a.received_q,
                    "stored": a.stored_q,
                    "dissipated": a.dissipated_q,
                    "returned": a.returned_q,
                })
            })
            .collect();
        serde_json::json!({
            "profile": store.profile().profile,
            "genesisEnergy": store.genesis_energy_q(),
            "accounts": accounts,
        })
    });
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"profile":rynvek_simulation::persistence::PHYSICS_PROFILE,"tick":world.clock.tick,"stateRoot":root,"tornWrite":torn,"bodies":bodies,"frameScene":instances,"gpuVerified":false,"life":life,"components":world.component_rules()})).map_err(fail)?);
    Ok(())
}
pub fn execute(command: WorldCommand) -> CliResult {
    match command {
        WorldCommand::Build {
            config,
            out,
            id,
            version,
            rule_packs,
        } => {
            let mut config: GeneralWorldConfig = serde_json::from_slice(&read(&config, 128 * 1024)?).map_err(fail)?;
            if rule_packs.len() > 16 {
                return Err(CliError::new("显式安装最多16个规则包"));
            }
            let mut seen = std::collections::BTreeSet::new();
            for spec in rule_packs {
                let (module, path) = spec
                    .split_once('=')
                    .ok_or_else(|| CliError::new("rule-pack须为模块ID=路径"))?;
                if !seen.insert(module.to_owned()) {
                    return Err(CliError::new("同模块不能重复安装"));
                }
                let rules = config
                    .components
                    .as_mut()
                    .ok_or_else(|| CliError::new("World必须已声明组件与绑定，不隐式创建"))?;
                if !rules.bindings.values().any(|b| b.module_id == module) || rules.tier1_rules.contains_key(module) {
                    return Err(CliError::new("安装模块必须有声明绑定且不能覆盖Tier1"));
                }
                let p = rynvek_rules::package::Tier0RulePackage::parse(&read(
                    Path::new(path),
                    rynvek_rules::package::MAX_TIER0_PACKAGE_BYTES,
                )?)
                .map_err(fail)?;
                // 显式本地安装替换命名模块的原图；原World校验再次核完整组件/宿主/价格。
                rules.rule_graphs.insert(module.to_owned(), p.graph_json);
            }
            write_new(&out, &config.build_package(&id, &version).map_err(fail)?)
        }
        WorldCommand::Run {
            package,
            world_id,
            ticks,
            snapshot,
            journal,
        } => {
            if ticks > 10_000 {
                return Err(CliError::new("离线单次推进最多 10000 Tick"));
            }
            let id = rynvek_engine::protocol::parse_world_id(&world_id)
                .ok_or_else(|| CliError::new("world-id 必须为32位十六进制"))?;
            let config = GeneralWorldConfig::from_package_bytes(&read(&package, 128 * 1024)?).map_err(fail)?;
            let mut world = config.instantiate(id).map_err(fail)?;
            let mut writer = rynvek_journal::JournalWriter::create_new(&journal, id, 0, 0, 0).map_err(fail)?;
            write_new(&snapshot, &world.encode_snapshot().map_err(fail)?)?;
            for _ in 0..ticks {
                world.tick_recorded(&[], &mut writer).map_err(fail)?;
            }
            summary(&world, false)
        }
        WorldCommand::Recover { snapshot, journal } => {
            let (world, out) = SimulationEngine::recover(&snapshot, &journal).map_err(fail)?;
            summary(&world, out.torn_write_detected)
        }
    }
}
