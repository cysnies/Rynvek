// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 需要网络的子命令：启动服务端、登录、查询源码要约 / SBOM / 健康状态。

use rynvek_security::ServerSigningKey;
use rynvek_server::app::AppConfig;
use rynvek_server::client::{self, LoginTarget};
use rynvek_server::host::TickDriver;
use rynvek_server::source::{SBOM_SPDX_JSON, SourceOffer};
use rynvek_server::store::{PostgresStore, Store};
use rynvek_server::{ServerConfig, start};

use crate::offline::remote_error;
use crate::{CliError, CliResult, LoginArgs, ServerArgs};

fn api(e: rynvek_server::ApiError) -> CliError {
    CliError::with_code(e.code.code(), e.message)
}

/// `server`：前台运行直到 Ctrl-C / SIGTERM，然后优雅停机
pub async fn serve(a: ServerArgs) -> CliResult {
    if a.tick_rate == 0 || a.tick_rate > 240 {
        return Err(CliError::new("--tick-rate 必须在 1..=240"));
    }
    if a.lab_max_concurrent == 0 || a.lab_max_concurrent > 64 {
        return Err(CliError::new("--lab-max-concurrent 必须在 1..=64"));
    }
    let store = match &a.postgres {
        Some(url) => {
            let pg = PostgresStore::connect(url).await.map_err(api)?;
            if pg.migrate().await.map_err(api)? {
                eprintln!("已在空数据库上执行规范 DDL（0001_initial_schema）");
            }
            Store::Postgres(pg)
        }
        None => {
            eprintln!("提示：未指定 --postgres，使用进程内存储（重启后数据丢失）");
            Store::memory()
        }
    };
    let signer = match &a.signing_key {
        Some(p) => crate::keys::load(p, a.key_id)?,
        None => {
            eprintln!("提示：未指定 --signing-key，本次进程使用随机签名密钥（重启后旧回放需用旧公钥验证）");
            ServerSigningKey::generate(a.key_id).map_err(|e| CliError::new(e.to_string()))?
        }
    };
    let public_key = rynvek_security::to_hex(&signer.public_key());
    let server = start(ServerConfig {
        http_addr: a.http,
        webtransport_addr: a.webtransport,
        app: AppConfig {
            public_base_url: a.public_url.unwrap_or_default(),
            source_tarball_override: a.source_tarball,
            tick_driver: TickDriver::RealTime { hz: a.tick_rate },
            blob_dir: a.blob_dir,
            lab_max_concurrent: a.lab_max_concurrent,
            local_practice_origin: a.local_practice_origin,
            ..AppConfig::default()
        },
        store,
        signer,
    })
    .await
    .map_err(api)?;
    // 首行为机器可读的就绪信息（测试与编排脚本据此获取实际端口）
    println!(
        "{}",
        serde_json::json!({
            "event": "listening",
            "http": format!("http://{}", server.http_addr),
            "webtransport": server.webtransport_addr.map(|w| format!("https://{w}")),
            "certificate_sha256": server.certificate_sha256.map(|h| rynvek_security::to_hex(&h)),
            "replay_signer": { "key_id": a.key_id, "ed25519_public_key": public_key },
            "source_offer": format!("http://{}/api/v1/meta/source-offer", server.http_addr),
        })
    );
    shutdown_signal().await;
    eprintln!("收到停机信号，正在优雅停机…");
    server.shutdown().await;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("注册 SIGTERM");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// `login`
pub async fn login(a: LoginArgs) -> CliResult {
    let key = crate::keys::load(&a.key, 0)?;
    let target = match (&a.principal_id, &a.register) {
        (Some(pid), _) => LoginTarget::Existing(*pid),
        (None, Some(handle)) => LoginTarget::Register { handle, kind: &a.kind },
        (None, None) => return Err(CliError::new("需要 --principal-id 或 --register HANDLE")),
    };
    let (pid, token) = client::login(a.server.trim_end_matches('/'), &key, target)
        .await
        .map_err(|e| {
            // client::login 的错误文本里带有服务端 JSON，尽量提取稳定错误码
            let code = e
                .find("\"code\":")
                .and_then(|i| e[i + 7..].split([',', '}']).next())
                .and_then(|c| c.trim().parse::<u16>().ok());
            match code {
                Some(c) => CliError::with_code(c, e),
                None => CliError::new(e),
            }
        })?;
    crate::print_json(&serde_json::json!({ "principal_id": pid.to_string(), "token": token }));
    Ok(())
}

async fn get_json(url: &str) -> CliResult<serde_json::Value> {
    let r = client::get(url, None).await.map_err(CliError::new)?;
    let v = r.json().map_err(CliError::new)?;
    if r.status != 200 {
        return Err(remote_error(&v));
    }
    Ok(v)
}

/// `source-offer`
pub async fn source_offer(server: Option<&str>) -> CliResult {
    let v = match server {
        Some(s) => get_json(&format!("{}/api/v1/meta/source-offer", s.trim_end_matches('/'))).await?,
        None => serde_json::to_value(SourceOffer::current("", None)).expect("可序列化"),
    };
    crate::print_json(&v);
    Ok(())
}

/// `sbom`
pub async fn sbom(server: Option<&str>) -> CliResult {
    match server {
        Some(s) => crate::print_json(
            &get_json(&format!(
                "{}{}",
                s.trim_end_matches('/'),
                rynvek_server::source::SBOM_PATH
            ))
            .await?,
        ),
        None => println!("{SBOM_SPDX_JSON}"),
    }
    Ok(())
}

/// `health`
pub async fn health(server: &str) -> CliResult {
    crate::print_json(&get_json(&format!("{}/api/v1/meta/health", server.trim_end_matches('/'))).await?);
    Ok(())
}
