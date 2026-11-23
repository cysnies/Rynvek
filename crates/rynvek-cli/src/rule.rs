// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 本地确定性构建，不替代Hub签名发布/发现/安装授权。
use crate::{
    CliError, CliResult,
    general::{read, write_new},
};
use clap::Subcommand;
use rynvek_rules::package::{
    MAX_GRAPH_BYTES, MAX_TIER0_PACKAGE_BYTES, TIER0_BUILD_PROFILE, Tier0RulePackage, build_tier0_package,
};
use std::path::PathBuf;
#[derive(Debug, Subcommand)]
pub enum RuleCommand {
    /// 原图编译为可复验RYIR并写入原RYPK；图可由rynvek-rule-ast真实TS前端生成。
    Build {
        #[arg(long)]
        graph: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "1.0.0")]
        version: String,
        #[arg(long, default_value = "CC0-1.0")]
        license: String,
    },
    /// 原容器/图/IR逐字节复验后报告；不执行包或声明其可信/已签名。
    Inspect {
        #[arg(long)]
        package: PathBuf,
    },
}
pub fn execute(command: RuleCommand) -> CliResult {
    let err = |e: rynvek_core::package::PackageError| CliError::with_code(e.code.code(), e.detail);
    match command {
        RuleCommand::Build {
            graph,
            out,
            id,
            version,
            license,
        } => {
            let bytes = read(&graph, MAX_GRAPH_BYTES)?;
            let text = std::str::from_utf8(&bytes).map_err(|_| CliError::new("图不是UTF8"))?;
            write_new(&out, &build_tier0_package(&id, &version, &license, text).map_err(err)?)
        }
        RuleCommand::Inspect { package } => {
            let bytes = read(&package, MAX_TIER0_PACKAGE_BYTES)?;
            let p = Tier0RulePackage::parse(&bytes).map_err(err)?;
            println!(
                "{}",
                serde_json::json!({"profile":TIER0_BUILD_PROFILE,"id":p.id,"version":p.version,"artifact_cid":rynvek_core::Blake3Hash::of(&bytes).to_hex(),"rir_hash":rynvek_core::Blake3Hash::of(&rynvek_rules::encode_rir(&p.module)).to_hex(),"gas_estimate":p.module.gas_estimate,"signed":false,"source_equivalence":"graph-to-rir-verified; TS-not-in-package"})
            );
            Ok(())
        }
    }
}
