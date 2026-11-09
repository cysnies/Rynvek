// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 双栈传输回环：同一组帧（含 LZ4 压缩关键帧、Protobuf 控制消息）经 WebSocket 与 WebTransport 往返后逐字节一致。

use std::net::SocketAddr;

use futures_util::SinkExt;
use rynvek_network::control::{ControlBody, ControlMessage, Heartbeat};
use rynvek_network::frame::{Frame, FrameEncoder};
use rynvek_network::link::{Link, TransportKind};
use rynvek_network::{NetError, websocket, webtransport};
use rynvek_protocol::WireChannelId;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

fn sample_frames() -> Vec<Frame> {
    let mut enc = FrameEncoder::new(true);
    let hb = ControlMessage::new(ControlBody::Heartbeat(Heartbeat {
        nonce: 300,
        client_time_ms: 1,
    }))
    .to_bytes();
    let mut frames = vec![enc.encode(WireChannelId::Control, 0, &hb, false).unwrap()];
    for tick in 1..=12u64 {
        let state = vec![(tick % 7) as u8; 4096];
        frames.push(
            enc.encode(WireChannelId::StateStream, tick, &state, tick % 4 == 1)
                .unwrap(),
        );
        frames.push(
            enc.encode(
                WireChannelId::Event,
                tick,
                format!("{{\"t\":{tick}}}").as_bytes(),
                false,
            )
            .unwrap(),
        );
    }
    assert!(
        frames
            .iter()
            .any(|f| f.header.flags.contains(rynvek_protocol::WireFrameFlags::COMPRESSED_LZ4))
    );
    frames
}

async fn echo(mut link: Link) {
    while let Some(Ok(frame)) = link.recv().await {
        if link.send(frame).await.is_err() {
            break;
        }
    }
}

async fn roundtrip(mut client: Link) {
    let frames = sample_frames();
    for f in &frames {
        client.send(f.clone()).await.unwrap();
    }
    for f in &frames {
        let back = client.recv().await.unwrap().unwrap();
        assert_eq!(&back, f);
        assert_eq!(back.decoded_payload().unwrap(), f.decoded_payload().unwrap());
    }
    let hello = ControlMessage::from_bytes(&frames[0].decoded_payload().unwrap()).unwrap();
    assert!(matches!(hello, ControlBody::Heartbeat(Heartbeat { nonce: 300, .. })));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_loopback_preserves_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        echo(websocket::link_from_stream(ws)).await;
    });
    let client = websocket::connect(&format!(
        "ws://{addr}/api/v1/worlds/00000000-0000-7000-8000-000000000000/stream"
    ))
    .await
    .unwrap();
    assert_eq!(client.kind, TransportKind::WebSocket);
    roundtrip(client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_text_message_is_a_protocol_violation() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let mut link = websocket::link_from_stream(ws);
        link.recv().await
    });
    let (mut raw, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/")).await.unwrap();
    raw.send(Message::Text("{\"hello\":1}".into())).await.unwrap();
    let got = server.await.unwrap();
    assert!(matches!(got, Some(Err(NetError::Payload(_)))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webtransport_loopback_preserves_frames() {
    let identity = webtransport::self_signed_identity(&["localhost", "127.0.0.1"]).unwrap();
    let server = webtransport::WebTransportServer::bind(SocketAddr::from(([127, 0, 0, 1], 0)), identity).unwrap();
    let port = server.local_addr().unwrap().port();
    let pin = server.certificate_hash();
    let path = "/api/v1/worlds/00000000-0000-7000-8000-000000000000/stream";
    let accept = tokio::spawn(async move {
        let session = server.accept().await.unwrap();
        assert_eq!(session.path, path);
        let _conn = session.connection;
        echo(session.link).await;
        drop(server);
    });
    let client = webtransport::connect(&format!("https://127.0.0.1:{port}{path}"), Some(pin))
        .await
        .unwrap();
    assert_eq!(client.link.kind, TransportKind::WebTransport);
    let _conn = client.connection.clone();
    // 先发一帧让服务端 accept_bi 返回（QUIC 流在首次写入时才对端可见）
    roundtrip(client.link).await;
    accept.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webtransport_rejects_unpinned_certificate() {
    let identity = webtransport::self_signed_identity(&["localhost"]).unwrap();
    let server = webtransport::WebTransportServer::bind(SocketAddr::from(([127, 0, 0, 1], 0)), identity).unwrap();
    let port = server.local_addr().unwrap().port();
    let _accept = tokio::spawn(async move {
        let _ = server.accept().await;
    });
    let wrong = [0u8; 32];
    let res = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        webtransport::connect(&format!("https://127.0.0.1:{port}/api/v1/worlds/x/stream"), Some(wrong)),
    )
    .await
    .expect("握手应迅速失败");
    assert!(matches!(res, Err(NetError::Transport(_))));
}
