// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! WebTransport 传输（双栈首选通道，HTTP/3 + QUIC，天然 TLS 1.3）。
//!
//! 每个会话由客户端打开**一条**双向流承载全部通道的帧（首尾相接，[`Link::from_byte_stream`] 重组），
//! 保证跨通道的发送顺序与 WebSocket 一致，会话逻辑无需区分传输。
//! 会话路径必须是规范 11.3 路由 `09`：`/api/v1/worlds/{world_id}/stream`。
//!
//! 本地开发与测试使用自签名证书，客户端以证书 SHA-256 指纹固定（W3C `serverCertificateHashes` 语义，
//! 证书有效期须 ≤ 14 天）；生产部署应使用公共 CA 证书。

use std::net::SocketAddr;

use wtransport::tls::Sha256Digest;
use wtransport::{ClientConfig, Endpoint, Identity, ServerConfig};

use crate::error::NetError;
use crate::link::{Link, TransportKind};

fn transport_err(e: impl std::fmt::Display) -> NetError {
    NetError::Transport(e.to_string())
}

/// 生成自签名身份（有效期 14 天）
pub fn self_signed_identity(names: &[&str]) -> Result<Identity, NetError> {
    Identity::self_signed(names.iter().copied()).map_err(transport_err)
}

/// 身份中叶证书的 SHA-256 指纹
pub fn certificate_hash(identity: &Identity) -> [u8; 32] {
    let digest = identity.certificate_chain().as_slice()[0].hash();
    *digest.as_ref()
}

/// 已接受的会话
#[derive(Debug)]
pub struct AcceptedSession {
    /// 请求路径（含查询串）
    pub path: String,
    pub link: Link,
    /// 保持 QUIC 连接存活
    pub connection: wtransport::Connection,
}

/// WebTransport 服务端
pub struct WebTransportServer {
    endpoint: Endpoint<wtransport::endpoint::endpoint_side::Server>,
    certificate_hash: [u8; 32],
}

impl std::fmt::Debug for WebTransportServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebTransportServer")
            .field("local_addr", &self.endpoint.local_addr().ok())
            .finish_non_exhaustive()
    }
}

impl WebTransportServer {
    /// 绑定 UDP 端口
    pub fn bind(addr: SocketAddr, identity: Identity) -> Result<Self, NetError> {
        let certificate_hash = certificate_hash(&identity);
        let config = ServerConfig::builder()
            .with_bind_address(addr)
            .with_identity(identity)
            .keep_alive_interval(Some(std::time::Duration::from_secs(3)))
            .build();
        Ok(Self {
            endpoint: Endpoint::server(config).map_err(transport_err)?,
            certificate_hash,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, NetError> {
        self.endpoint.local_addr().map_err(transport_err)
    }

    pub const fn certificate_hash(&self) -> [u8; 32] {
        self.certificate_hash
    }

    /// 接受下一个会话；路径由调用方路由（不匹配时可直接丢弃 `AcceptedSession` 以关闭）
    pub async fn accept(&self) -> Result<AcceptedSession, NetError> {
        let request = self.endpoint.accept().await.await.map_err(transport_err)?;
        let path = request.path().to_owned();
        let connection = request.accept().await.map_err(transport_err)?;
        let (send, recv) = connection.accept_bi().await.map_err(transport_err)?;
        Ok(AcceptedSession {
            path,
            link: Link::from_byte_stream(TransportKind::WebTransport, recv, send),
            connection,
        })
    }
}

/// 已建立的客户端会话
pub struct ClientSession {
    pub link: Link,
    pub connection: wtransport::Connection,
    _endpoint: Endpoint<wtransport::endpoint::endpoint_side::Client>,
}

impl std::fmt::Debug for ClientSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientSession")
            .field("link", &self.link)
            .finish_non_exhaustive()
    }
}

/// 连接 `https://host:port/api/v1/worlds/{id}/stream`；`pinned_hash` 为自签名证书指纹
pub async fn connect(url: &str, pinned_hash: Option<[u8; 32]>) -> Result<ClientSession, NetError> {
    let builder = ClientConfig::builder().with_bind_default();
    let config = match pinned_hash {
        Some(hash) => builder
            .with_server_certificate_hashes([Sha256Digest::new(hash)])
            .build(),
        None => builder.with_native_certs().build(),
    };
    let endpoint = Endpoint::client(config).map_err(transport_err)?;
    let connection = endpoint.connect(url).await.map_err(transport_err)?;
    let (send, recv) = connection
        .open_bi()
        .await
        .map_err(transport_err)?
        .await
        .map_err(transport_err)?;
    Ok(ClientSession {
        link: Link::from_byte_stream(TransportKind::WebTransport, recv, send),
        connection,
        _endpoint: endpoint,
    })
}
