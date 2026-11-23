// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 本地确定性构建，不替代Hub签名发布/发现/安装授权。
use crate::{
    CliError, CliResult,
    general::{read, write_new},
};
use clap::{Args, Subcommand};
use rynvek_rules::package::{
    MAX_GRAPH_BYTES, MAX_TIER0_PACKAGE_BYTES, TIER0_BUILD_PROFILE, Tier0RulePackage, build_tier0_package,
};
use std::path::PathBuf;
#[derive(Args)]
pub struct Connection {
    #[arg(long)]
    pub server: String,
    #[arg(long)]
    pub owner: u64,
    /// 登记/既有可信公钥，不从下载响应自动信任。
    #[arg(long)]
    pub trusted_key: String,
    #[arg(long, env = "RYNVEK_TOKEN", hide_env_values = true)]
    pub token: String,
}
impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}
impl Connection {
    fn client(self) -> Result<rynvek_server::private_rule_client::PrivateRuleClient, CliError> {
        let key = rynvek_server::ids::parse_hex::<32>(&self.trusted_key)
            .ok_or_else(|| CliError::new("可信公钥须32字节hex"))?;
        rynvek_server::private_rule_client::PrivateRuleClient::new(&self.server, self.owner, key, self.token)
            .map_err(CliError::new)
    }
}
#[derive(Debug, Subcommand)]
pub enum RuleCommand {
    /// 同owner精确锁签名发布，不执行/安装。
    Publish {
        #[command(flatten)]
        connection: Connection,
        #[arg(long)]
        package: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        dependencies: Option<PathBuf>,
    },
    List {
        #[command(flatten)]
        connection: Connection,
    },
    /// 完整锁逐级可信验签，复用唯一RYPK/IR复验；仅成功后wx写根制品。
    Download {
        #[command(flatten)]
        connection: Connection,
        #[arg(long)]
        id: String,
        #[arg(long)]
        version: String,
        #[arg(long)]
        cid: String,
        #[arg(long)]
        out: PathBuf,
    },
    Observe {
        #[command(flatten)]
        connection: Connection,
        #[arg(long)]
        world: String,
    },
    Connect {
        #[command(flatten)]
        connection: Connection,
        #[arg(long)]
        world: String,
    },
    Acquire {
        #[command(flatten)]
        connection: Connection,
        #[arg(long)]
        world: String,
        #[arg(long)]
        host_fence: u64,
    },
    /// 明确的一个真实Tick，不自动补Tick或获取控制权。
    Tick {
        #[command(flatten)]
        connection: Connection,
        #[arg(long)]
        world: String,
        #[arg(long)]
        host_fence: u64,
        #[arg(long)]
        control_fence: u64,
        #[arg(long)]
        tick: u64,
        #[arg(long)]
        idempotency_key: String,
    },
    /// 单次显式Epoch安装；--recover-tick为撤销后的一个有源隔离维护Tick，不激活包。
    Install {
        #[command(flatten)]
        connection: Connection,
        #[arg(long)]
        world: String,
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        host_fence: u64,
        #[arg(long)]
        control_fence: u64,
        #[arg(long)]
        tick: u64,
        #[arg(long)]
        idempotency_key: String,
        #[arg(long)]
        recover_tick: bool,
        #[arg(long, requires = "recover_tick")]
        accept_maintenance_q_per_body: Option<u64>,
    },
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
pub async fn execute(command: RuleCommand) -> CliResult {
    let err = |e: rynvek_core::package::PackageError| CliError::with_code(e.code.code(), e.detail);
    match command {
        RuleCommand::Publish {
            connection,
            package,
            key,
            dependencies,
        } => {
            let client = connection.client()?;
            let bytes = read(&package, MAX_TIER0_PACKAGE_BYTES)?;
            let signer = crate::keys::load(&key, 0)?;
            let deps = match dependencies {
                Some(p) => {
                    serde_json::from_slice(&read(&p, 64 * 1024)?).map_err(|_| CliError::new("精确依赖JSON非法"))?
                }
                None => vec![],
            };
            let release = client.publish(&bytes, deps, &signer).await.map_err(CliError::new)?;
            crate::print_json(&serde_json::json!({"release":release,"installed":false}));
            Ok(())
        }
        RuleCommand::List { connection } => {
            crate::print_json(&connection.client()?.list().await.map_err(CliError::new)?);
            Ok(())
        }
        RuleCommand::Download {
            connection,
            id,
            version,
            cid,
            out,
        } => {
            let pin = rynvek_server::rule_package::ExactRuleRef {
                id,
                version,
                blake3_256: cid,
            };
            let packages = connection.client()?.download(&pin).await.map_err(CliError::new)?;
            let bytes = &packages
                .iter()
                .find(|(r, _)| r.manifest.reference() == pin)
                .ok_or_else(|| CliError::new("缺少根制品"))?
                .1;
            write_new(&out, bytes)?;
            crate::print_json(
                &serde_json::json!({"verified":true,"packages":packages.iter().map(|(r,_)|&r.manifest).collect::<Vec<_>>(),"installed":false}),
            );
            Ok(())
        }
        RuleCommand::Observe { connection, world } => {
            crate::print_json(&connection.client()?.observe(&world).await.map_err(CliError::new)?);
            Ok(())
        }
        RuleCommand::Connect { connection, world } => {
            crate::print_json(
                &connection
                    .client()?
                    .control(&world, &serde_json::json!({"action":"connect"}))
                    .await
                    .map_err(CliError::new)?,
            );
            Ok(())
        }
        RuleCommand::Acquire {
            connection,
            world,
            host_fence,
        } => {
            crate::print_json(
                &connection
                    .client()?
                    .control(
                        &world,
                        &serde_json::json!({"action":"acquire","host_fence":host_fence,"level":"Sovereign"}),
                    )
                    .await
                    .map_err(CliError::new)?,
            );
            Ok(())
        }
        RuleCommand::Tick {
            connection,
            world,
            host_fence,
            control_fence,
            tick,
            idempotency_key,
        } => {
            let owner = connection.owner;
            let key: rynvek_protocol::IdempotencyKey = serde_json::from_value(serde_json::json!(idempotency_key))
                .map_err(|_| CliError::new("须UUIDv7幂等键"))?;
            let input = rynvek_server::general_host::GeneralInput {
                host_fence,
                control_fence,
                envelope: rynvek_protocol::CommandEnvelope {
                    idempotency_key: key,
                    principal_id: owner,
                    issued_at_tick: tick,
                    target_tick: tick.checked_add(1).ok_or_else(|| CliError::new("Tick溢出"))?,
                    payload: vec![],
                },
                install: None,
                recovery: None,
            };
            crate::print_json(
                &connection
                    .client()?
                    .control(&world, &serde_json::json!({"action":"tick","input":input}))
                    .await
                    .map_err(CliError::new)?,
            );
            Ok(())
        }
        RuleCommand::Install {
            connection,
            world,
            request,
            host_fence,
            control_fence,
            tick,
            idempotency_key,
            recover_tick,
            accept_maintenance_q_per_body,
        } => {
            let request = serde_json::from_slice(&read(&request, 64 * 1024)?)
                .map_err(|_| CliError::new("显式安装映射JSON非法"))?;
            let client = connection.client()?;
            if recover_tick {
                let observed = client.observe(&world).await.map_err(CliError::new)?;
                if accept_maintenance_q_per_body.is_none()
                    || observed["recovery_maintenance_q_per_body"].as_u64() != accept_maintenance_q_per_body
                {
                    return Err(CliError::new("须观察并明确接受每身体隔离维护EnergyQ，不是指令gas"));
                }
            }
            let key = serde_json::from_value(serde_json::json!(idempotency_key))
                .map_err(|_| CliError::new("须UUIDv7幂等键"))?;
            crate::print_json(
                &client
                    .install(&world, host_fence, control_fence, tick, key, request, recover_tick)
                    .await
                    .map_err(CliError::new)?,
            );
            Ok(())
        }
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
