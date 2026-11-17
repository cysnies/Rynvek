// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 服务端启动入口：HTTP/1.1（含 WebSocket 升级）+ 可选 WebTransport（HTTP/3，自签名证书）。
//!
//! 两个监听共享同一个 [`AppState`]；WebTransport 只承载 `/api/v1/worlds/{world_id}/stream`，
//! 其他路径直接拒绝。每个 WebTransport 会话在独立任务中完成握手，慢客户端不会阻塞接入。

use std::net::SocketAddr;

use rynvek_network::webtransport::{self, WebTransportServer};
use rynvek_security::ServerSigningKey;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::app::{AppConfig, AppState, WebTransportInfo};
use crate::error::{ApiError, ApiResult};
use crate::store::Store;
use crate::stream::{parse_world_id, serve_link};

/// 启动参数
#[derive(Debug)]
pub struct ServerConfig {
    pub http_addr: SocketAddr,
    /// `None` 表示只启用 WebSocket
    pub webtransport_addr: Option<SocketAddr>,
    /// `public_base_url` 为空时自动取 `http://{实际绑定地址}`
    pub app: AppConfig,
    pub store: Store,
    pub signer: ServerSigningKey,
}

/// 运行中的服务端
#[derive(Debug)]
pub struct RunningServer {
    pub http_addr: SocketAddr,
    pub webtransport_addr: Option<SocketAddr>,
    pub certificate_sha256: Option<[u8; 32]>,
    pub state: AppState,
    shutdown: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl RunningServer {
    /// `http://host:port`
    pub fn base_url(&self) -> String {
        format!("http://{}", self.http_addr)
    }

    /// 优雅停机：停止接入新连接并等待监听任务退出
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        for t in self.tasks {
            let _ = t.await;
        }
    }

    /// 阻塞直到监听任务退出（CLI `rynvek server` 使用）
    pub async fn wait(self) {
        for t in self.tasks {
            let _ = t.await;
        }
    }
}

/// WebTransport 路径 → world_id
pub fn stream_path_world(path: &str) -> Option<&str> {
    let path = path.split('?').next().unwrap_or_default();
    path.strip_prefix("/api/v1/worlds/")?.strip_suffix("/stream")
}

/// 绑定监听并开始服务
pub async fn start(mut cfg: ServerConfig) -> ApiResult<RunningServer> {
    if let Some(origin) = &cfg.app.local_practice_origin {
        let uri: axum::http::Uri = origin.parse().map_err(|_| ApiError::bad_request("本地 Origin 无效"))?;
        if !cfg.http_addr.ip().is_loopback()
            || cfg.store.backend_name() != "memory"
            || uri.scheme_str() != Some("http")
            || !matches!(uri.host(), Some("127.0.0.1" | "[::1]" | "localhost"))
            || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
        {
            return Err(ApiError::forbidden(
                "本地练习只允许回环 HTTP Origin、回环监听和内存 Store",
            ));
        }
    }
    let listener = tokio::net::TcpListener::bind(cfg.http_addr)
        .await
        .map_err(|e| ApiError::internal(format!("HTTP 监听 {} 失败：{e}", cfg.http_addr)))?;
    let http_addr = listener.local_addr().map_err(|e| ApiError::internal(e.to_string()))?;
    if cfg.app.public_base_url.is_empty() {
        cfg.app.public_base_url = format!("http://{http_addr}");
    }
    let wt = match cfg.webtransport_addr {
        Some(addr) => {
            let identity = webtransport::self_signed_identity(&["localhost", "127.0.0.1"])
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let server = WebTransportServer::bind(addr, identity).map_err(|e| ApiError::internal(e.to_string()))?;
            let local = server.local_addr().map_err(|e| ApiError::internal(e.to_string()))?;
            cfg.app.webtransport = Some(WebTransportInfo {
                url_base: format!("https://{local}"),
                certificate_sha256: server.certificate_hash(),
            });
            Some((server, local))
        }
        None => None,
    };
    let state = AppState::new(cfg.app, cfg.store, cfg.signer)?;
    let (shutdown, rx) = watch::channel(false);
    let mut tasks = Vec::new();

    let router = crate::api::router(state.clone());
    let mut http_rx = rx.clone();
    tasks.push(tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = http_rx.wait_for(|v| *v).await;
            })
            .await;
    }));

    let (certificate_sha256, webtransport_addr) = match wt {
        Some((server, local)) => {
            let hash = server.certificate_hash();
            let app = state.clone();
            let mut wt_rx = rx;
            tasks.push(tokio::spawn(async move {
                loop {
                    let pending = tokio::select! {
                        p = server.accept() => p,
                        _ = wt_rx.wait_for(|v| *v) => break,
                    };
                    let app = app.clone();
                    tokio::spawn(async move {
                        let Ok(session) = pending.establish().await else { return };
                        let Some(Ok(world_id)) = stream_path_world(&session.path).map(parse_world_id) else {
                            session.connection.close(0x8001u32.into(), "unknown path".as_bytes());
                            return;
                        };
                        // 连接对象必须存活到流结束
                        let _connection = session.connection;
                        serve_link(app, world_id, session.link).await;
                    });
                }
            }));
            (Some(hash), Some(local))
        }
        None => (None, None),
    };

    Ok(RunningServer {
        http_addr,
        webtransport_addr,
        certificate_sha256,
        state,
        shutdown,
        tasks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_paths() {
        assert_eq!(stream_path_world("/api/v1/worlds/abc/stream"), Some("abc"));
        assert_eq!(stream_path_world("/api/v1/worlds/abc/stream?x=1"), Some("abc"));
        assert_eq!(stream_path_world("/api/v1/worlds/abc"), None);
    }
}
