// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `rynvek` 命令行（Tier 4）。
//!
//! | 子命令 | 作用 |
//! |---|---|
//! | `server` | 启动权威服务端（HTTP + WebSocket，可选 WebTransport；内存或 PostgreSQL 17 存储） |
//! | `keygen` | 生成 Ed25519 种子文件（权限 0600），输出公钥 |
//! | `login` | Ed25519 质询登录（可自注册），输出 96 字节能力令牌的 Base64url 文本 |
//! | `replay verify` | 结构 → BLAKE3 → 签名 → 重新仿真，完整验证 `.rreplay`（本地文件或从服务器下载） |
//! | `replay inspect` | 只解析文件头与正文摘要（不验签） |
//! | `frame decode` | 解码 `rynvek-wire/1` 16 字节帧头（及可选负载） |
//! | `source-offer` / `sbom` / `health` | 查询 AGPLv3 §13 源码要约、SPDX SBOM 与健康状态 |
//!
//! 失败时以退出码 1 结束，stderr 打印规范 11.4 稳定错误码（如 `错误 0x7002 ...`），便于脚本判断。

mod general;
mod keys;
mod offline;
mod remote;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

/// 命令执行失败：可选的规范错误码 + 说明
#[derive(Debug)]
pub struct CliError {
    pub code: Option<u16>,
    pub message: String,
}

impl CliError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            code: None,
            message: message.into(),
        }
    }

    pub fn with_code(code: u16, message: impl Into<String>) -> Self {
        Self {
            code: Some(code),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Some(c) => {
                let name = rynvek_core::error::RynvekErrorCode::from_code(c)
                    .map(|e| format!("{e:?}"))
                    .unwrap_or_else(|| "Unknown".to_owned());
                write!(f, "错误 0x{c:04X} {name}：{}", self.message)
            }
            None => write!(f, "错误：{}", self.message),
        }
    }
}

pub type CliResult<T = ()> = Result<T, CliError>;

#[derive(Debug, Parser)]
#[command(
    name = "rynvek",
    version,
    about = "Rynvek 命令行：服务端、登录、回放验证、帧解码与 AGPL 源码要约"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 原 RYWL 通用物理 World 打包、运行与 WAL 恢复
    #[command(subcommand)]
    World(general::WorldCommand),
    /// 启动权威服务端
    Server(ServerArgs),
    /// 生成 Ed25519 种子文件
    Keygen {
        /// 输出路径（已存在时拒绝覆盖）
        #[arg(long)]
        out: PathBuf,
    },
    /// Ed25519 质询登录，输出能力令牌
    Login(LoginArgs),
    /// `.rreplay` 工具
    #[command(subcommand)]
    Replay(ReplayCommand),
    /// `rynvek-wire/1` 帧工具
    #[command(subcommand)]
    Frame(FrameCommand),
    /// 查询服务器的 AGPLv3 §13 源码要约（缺省打印本二进制内置的要约）
    SourceOffer(RemoteArgs),
    /// 输出 SPDX 2.3 SBOM（缺省打印本二进制内置的 SBOM）
    Sbom(RemoteArgs),
    /// 查询服务器健康状态
    Health {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        server: String,
    },
}

#[derive(Debug, Args)]
struct ServerArgs {
    /// HTTP / WebSocket 监听地址
    #[arg(long, default_value = "127.0.0.1:8080")]
    http: SocketAddr,
    /// WebTransport（UDP）监听地址；缺省不启用
    #[arg(long)]
    webtransport: Option<SocketAddr>,
    /// PostgreSQL 连接串；缺省使用进程内存储（重启即丢失）
    #[arg(long, env = "RYNVEK_POSTGRES_URL")]
    postgres: Option<String>,
    /// 对外地址（源码要约与流地址中的绝对链接）；缺省取实际监听地址
    #[arg(long)]
    public_url: Option<String>,
    /// 修改版部署必须指向自己的源码包（AGPLv3 §13）
    #[arg(long)]
    source_tarball: Option<String>,
    /// 服务端签名种子文件（`keygen` 生成）；缺省每次启动随机生成
    #[arg(long)]
    signing_key: Option<PathBuf>,
    /// 签名密钥编号（写入回放尾部，供轮换）
    #[arg(long, default_value_t = 1)]
    key_id: u16,
    /// 权威 Tick 频率
    #[arg(long, default_value_t = 30)]
    tick_rate: u32,
    /// 内容寻址 Blob 目录（文件系统 CAS，持久化包与 Parquet）；缺省使用进程内 Blob 存储
    #[arg(long, env = "RYNVEK_BLOB_DIR")]
    blob_dir: Option<PathBuf>,
    /// Lab 实验最大并发执行数
    #[arg(long, default_value_t = 2)]
    lab_max_concurrent: usize,
    /// 仅回环内存开发服：允许此精确 Web Origin 创建本地 PvE（不适用于公网）。
    #[arg(long)]
    local_practice_origin: Option<String>,
}

#[derive(Debug, Args)]
struct LoginArgs {
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    server: String,
    /// 种子文件
    #[arg(long)]
    key: PathBuf,
    /// 已注册主体的 principal_id
    #[arg(long, conflicts_with = "register")]
    principal_id: Option<u64>,
    /// 以该 handle 自注册
    #[arg(long)]
    register: Option<String>,
    /// 自注册的主体类别
    #[arg(long, default_value = "human", value_parser = ["human", "agent"])]
    kind: String,
}

#[derive(Debug, Subcommand)]
enum ReplayCommand {
    /// 完整验证
    Verify(VerifyArgs),
    /// 只解析不验签
    Inspect { file: PathBuf },
}

#[derive(Debug, Args)]
struct VerifyArgs {
    /// 本地 `.rreplay` 文件
    #[arg(required_unless_present = "match_id")]
    file: Option<PathBuf>,
    /// 受信签名公钥 `KEY_ID:HEX`（可重复）
    #[arg(long = "signer")]
    signers: Vec<String>,
    /// 从服务器下载对局回放并信任其公布的签名公钥
    #[arg(long = "match", requires = "server")]
    match_id: Option<String>,
    #[arg(long)]
    server: Option<String>,
    /// 下载回放所需的能力令牌
    #[arg(long, env = "RYNVEK_TOKEN")]
    token: Option<String>,
}

#[derive(Debug, Subcommand)]
enum FrameCommand {
    /// 解码十六进制帧（一个或多个首尾相接）
    Decode {
        /// 十六进制文本；与 `--file` 二选一
        #[arg(required_unless_present = "file")]
        hex: Option<String>,
        /// 原始二进制帧文件
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

#[derive(Debug, Args)]
struct RemoteArgs {
    /// 服务器地址；缺省输出本二进制内置的数据
    #[arg(long)]
    server: Option<String>,
}

async fn run(cli: Cli) -> CliResult {
    match cli.command {
        Command::World(command) => general::execute(command),
        Command::Server(a) => remote::serve(a).await,
        Command::Keygen { out } => keys::keygen(&out),
        Command::Login(a) => remote::login(a).await,
        Command::Replay(ReplayCommand::Verify(a)) => offline::verify(a).await,
        Command::Replay(ReplayCommand::Inspect { file }) => offline::inspect(&file),
        Command::Frame(FrameCommand::Decode { hex, file }) => offline::frame_decode(hex.as_deref(), file.as_deref()),
        Command::SourceOffer(a) => remote::source_offer(a.server.as_deref()).await,
        Command::Sbom(a) => remote::sbom(a.server.as_deref()).await,
        Command::Health { server } => remote::health(&server).await,
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// 打印 JSON（美化）
pub fn print_json(v: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(v).expect("可序列化"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn error_display_carries_stable_code() {
        let e = CliError::with_code(0x7002, "x");
        assert!(e.to_string().starts_with("错误 0x7002 "));
    }
}
