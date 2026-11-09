// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 集成测试公共夹具：内存存储 + 手动 Tick 驱动的服务端，以及最小的流客户端。

#![allow(dead_code)]

pub mod arena;

use std::net::SocketAddr;
use std::time::Duration;

use rynvek_network::control::{ClientHello, ControlBody, ControlMessage, WIRE_PROTOCOL};
use rynvek_network::link::Link;
use rynvek_network::{Frame, FrameEncoder};
use rynvek_protocol::{CommandEnvelope, IdempotencyKey, WireChannelId, WireUuid};
use rynvek_security::ServerSigningKey;
use rynvek_server::app::AppConfig;
use rynvek_server::client::{self, LoginTarget};
use rynvek_server::host::TickDriver;
use rynvek_server::store::Store;
use rynvek_server::{RunningServer, ServerConfig};

/// 测试服务端签名种子（测试需要用同一把钥匙伪造合法签名的篡改回放）
pub const SERVER_SEED: [u8; 32] = [7; 32];
pub const SERVER_KEY_ID: u16 = 1;

pub fn server_key() -> ServerSigningKey {
    ServerSigningKey::from_seed(SERVER_KEY_ID, SERVER_SEED)
}

pub async fn start_server(webtransport: bool) -> RunningServer {
    start_server_with(Store::memory(), webtransport).await
}

pub async fn start_server_with(store: Store, webtransport: bool) -> RunningServer {
    let loopback: SocketAddr = "127.0.0.1:0".parse().unwrap();
    rynvek_server::start(ServerConfig {
        http_addr: loopback,
        webtransport_addr: webtransport.then_some(loopback),
        app: AppConfig {
            public_base_url: String::new(),
            tick_driver: TickDriver::Manual,
            lobby_timeout: None,
            fixed_map_seed: Some(42),
            ..AppConfig::default()
        },
        store,
        signer: server_key(),
    })
    .await
    .expect("服务端启动")
}

/// 用户夹具
#[derive(Debug)]
pub struct User {
    pub principal_id: u64,
    pub token: String,
    pub key: ServerSigningKey,
}

impl User {
    pub fn token_bytes(&self) -> Vec<u8> {
        rynvek_server::app::decode_token_text(&self.token).unwrap()
    }
}

pub async fn register(base: &str, handle: &str, kind: &str, seed: u8) -> User {
    let key = ServerSigningKey::from_seed(0, [seed; 32]);
    let (principal_id, token) = client::login(base, &key, LoginTarget::Register { handle, kind })
        .await
        .expect("注册登录");
    User {
        principal_id,
        token,
        key,
    }
}

pub fn ruleset_hex() -> String {
    rynvek_server::ids::hex(&rynvek_server::app::arena_ruleset_hash())
}

/// 最小流客户端
#[derive(Debug)]
pub struct StreamClient {
    pub link: Link,
    pub enc: FrameEncoder,
    pub principal_id: u64,
}

impl StreamClient {
    pub fn new(link: Link, principal_id: u64) -> Self {
        Self {
            link,
            enc: FrameEncoder::new(true),
            principal_id,
        }
    }

    pub async fn send(&mut self, channel: WireChannelId, payload: &[u8]) {
        let frame = self.enc.encode(channel, 0, payload, false).unwrap();
        self.link.send(frame).await.unwrap();
    }

    pub async fn hello(&mut self, token: Vec<u8>, world_id: WireUuid, faction: u32) {
        let msg = ControlMessage::new(ControlBody::Hello(ClientHello {
            protocol: WIRE_PROTOCOL.to_owned(),
            capability_token: token,
            world_id: world_id.0.to_vec(),
            faction,
            resume_token: Vec::new(),
            client_version: "rynvek-test".to_owned(),
        }));
        self.send(WireChannelId::Control, &msg.to_bytes()).await;
    }

    pub async fn recv(&mut self) -> Option<Frame> {
        match tokio::time::timeout(Duration::from_secs(20), self.link.recv()).await {
            Ok(Some(Ok(f))) => Some(f),
            Ok(Some(Err(e))) => panic!("链路错误：{e}"),
            Ok(None) => None,
            Err(_) => panic!("等待下行帧超时"),
        }
    }

    /// 读到指定通道的下一帧（其他通道的帧交给 `other` 处理）
    pub async fn recv_on(&mut self, channel: WireChannelId, mut other: impl FnMut(&Frame)) -> Frame {
        loop {
            let f = self.recv().await.expect("连接意外关闭");
            if f.channel() == channel {
                return f;
            }
            other(&f);
        }
    }

    /// 发一条 `rynvek-arena/1` 命令并等待带同一幂等键的回复
    pub async fn command(&mut self, tick: u64, payload: serde_json::Value) -> serde_json::Value {
        let key = IdempotencyKey::new(rynvek_server::ids::new_uuid_v7()).unwrap();
        let env = CommandEnvelope {
            idempotency_key: key,
            principal_id: self.principal_id,
            issued_at_tick: tick,
            target_tick: tick,
            payload,
        };
        self.send(WireChannelId::Command, &serde_json::to_vec(&env).unwrap())
            .await;
        loop {
            let f = self.recv_on(WireChannelId::Command, |_| {}).await;
            let v: serde_json::Value = serde_json::from_slice(&f.decoded_payload().unwrap()).unwrap();
            if v["idempotency_key"] == key.to_string() {
                return v["message"].clone();
            }
            // 信封无法解析时服务端拿不到幂等键，回复不带键：直接视为本命令的失败
            if v.get("idempotency_key").is_none() && v["message"]["type"] == "error" {
                panic!("命令被拒绝（无幂等键）：{v}");
            }
        }
    }
}

pub fn control_body(f: &Frame) -> ControlBody {
    ControlMessage::from_bytes(&f.decoded_payload().unwrap()).unwrap()
}
