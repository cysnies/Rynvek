// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! WebSocket 传输（双栈之一，浏览器兜底通道）：每条二进制消息恰好承载一个 16 字节帧头 + 负载。
//!
//! 文本消息不属于本协议，按第 2 防线视为违例；Ping / Pong / Frame 由 tungstenite 自动处理后忽略。
//! 服务端（`rynvek-server`）用 axum 的 WebSocket 升级，再经 [`Link::from_messages`] 接入同一会话逻辑。

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::error::NetError;
use crate::link::{Link, TransportKind};

/// 把 tungstenite 消息映射为 Link 消息
pub fn map_message(msg: Result<Message, tokio_tungstenite::tungstenite::Error>) -> Result<Option<Vec<u8>>, NetError> {
    match msg {
        Ok(Message::Binary(b)) => Ok(Some(b.to_vec())),
        Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => Ok(None),
        Ok(Message::Close(_)) => Err(NetError::Closed),
        Ok(Message::Text(_)) => Err(NetError::Payload("实时流只接受二进制 WebSocket 消息".into())),
        Err(e) => Err(NetError::Transport(e.to_string())),
    }
}

/// 把已建立的 tungstenite 连接接成 Link
pub fn link_from_stream<S>(ws: WebSocketStream<S>) -> Link
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (sink, stream) = ws.split();
    let sink = sink.with(|bytes: Vec<u8>| async move {
        Ok::<_, tokio_tungstenite::tungstenite::Error>(Message::Binary(bytes.into()))
    });
    Link::from_messages(TransportKind::WebSocket, Box::pin(sink), stream.map(map_message))
}

/// 客户端连接 `ws://` / `wss://` 实时流地址
pub async fn connect(url: &str) -> Result<Link, NetError> {
    let (ws, _response): (WebSocketStream<MaybeTlsStream<TcpStream>>, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| NetError::Transport(e.to_string()))?;
    Ok(link_from_stream(ws))
}
