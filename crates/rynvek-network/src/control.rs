// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Control 通道（`WireChannelId::Control`）的 Protobuf v3 信令。
//!
//! 消息定义的权威来源是 `protocols/proto/rynvek_wire.proto`（Apache-2.0 WITH LLVM-exception，供第三方客户端
//! 直接 `protoc` 生成）。这里用 `prost` 派生宏手写等价结构（免去构建期依赖 `protoc`），
//! 测试逐字段比对 `.proto` 文本与本模块的标签表，并以手算字节断言线上编码。

use prost::Message;

use crate::error::NetError;

/// 线上协议标识
pub const WIRE_PROTOCOL: &str = "rynvek-wire/1";

#[derive(Clone, PartialEq, Message)]
pub struct ClientHello {
    #[prost(string, tag = "1")]
    pub protocol: String,
    #[prost(bytes = "vec", tag = "2")]
    pub capability_token: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    pub world_id: Vec<u8>,
    #[prost(uint32, tag = "4")]
    pub faction: u32,
    #[prost(bytes = "vec", tag = "5")]
    pub resume_token: Vec<u8>,
    #[prost(string, tag = "6")]
    pub client_version: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ServerHello {
    #[prost(bytes = "vec", tag = "1")]
    pub session_id: Vec<u8>,
    #[prost(uint64, tag = "2")]
    pub principal_id: u64,
    #[prost(uint32, tag = "3")]
    pub faction: u32,
    #[prost(uint64, tag = "4")]
    pub current_tick: u64,
    #[prost(bytes = "vec", tag = "5")]
    pub resume_token: Vec<u8>,
    #[prost(uint32, tag = "6")]
    pub tick_rate_hz: u32,
    #[prost(uint32, tag = "7")]
    pub keyframe_interval_ticks: u32,
    #[prost(string, tag = "8")]
    pub source_commit: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Heartbeat {
    #[prost(uint64, tag = "1")]
    pub nonce: u64,
    #[prost(uint64, tag = "2")]
    pub client_time_ms: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct HeartbeatAck {
    #[prost(uint64, tag = "1")]
    pub nonce: u64,
    #[prost(uint64, tag = "2")]
    pub server_tick: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct Disconnect {
    #[prost(uint32, tag = "1")]
    pub error_code: u32,
    #[prost(string, tag = "2")]
    pub reason: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct KeyframeRequest {
    #[prost(uint64, tag = "1")]
    pub last_applied_tick: u64,
}

/// `ControlMessage.body` 的 oneof
#[derive(Clone, PartialEq, prost::Oneof)]
pub enum ControlBody {
    #[prost(message, tag = "1")]
    Hello(ClientHello),
    #[prost(message, tag = "2")]
    Welcome(ServerHello),
    #[prost(message, tag = "3")]
    Heartbeat(Heartbeat),
    #[prost(message, tag = "4")]
    HeartbeatAck(HeartbeatAck),
    #[prost(message, tag = "5")]
    Disconnect(Disconnect),
    #[prost(message, tag = "6")]
    KeyframeRequest(KeyframeRequest),
}

#[derive(Clone, PartialEq, Message)]
pub struct ControlMessage {
    #[prost(oneof = "ControlBody", tags = "1, 2, 3, 4, 5, 6")]
    pub body: Option<ControlBody>,
}

impl ControlMessage {
    pub fn new(body: ControlBody) -> Self {
        Self { body: Some(body) }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode_to_vec()
    }

    /// 解码；空 oneof 视为非法
    pub fn from_bytes(bytes: &[u8]) -> Result<ControlBody, NetError> {
        Self::decode(bytes)
            .map_err(|e| NetError::Payload(format!("Control 负载不是合法 Protobuf：{e}")))?
            .body
            .ok_or_else(|| NetError::Payload("ControlMessage.body 为空".into()))
    }
}

/// `(消息, 字段, 类型, 标签)`：与 `.proto` 文本逐项比对
pub const FIELD_TABLE: &[(&str, &str, &str, u32)] = &[
    ("ClientHello", "protocol", "string", 1),
    ("ClientHello", "capability_token", "bytes", 2),
    ("ClientHello", "world_id", "bytes", 3),
    ("ClientHello", "faction", "uint32", 4),
    ("ClientHello", "resume_token", "bytes", 5),
    ("ClientHello", "client_version", "string", 6),
    ("ServerHello", "session_id", "bytes", 1),
    ("ServerHello", "principal_id", "uint64", 2),
    ("ServerHello", "faction", "uint32", 3),
    ("ServerHello", "current_tick", "uint64", 4),
    ("ServerHello", "resume_token", "bytes", 5),
    ("ServerHello", "tick_rate_hz", "uint32", 6),
    ("ServerHello", "keyframe_interval_ticks", "uint32", 7),
    ("ServerHello", "source_commit", "string", 8),
    ("Heartbeat", "nonce", "uint64", 1),
    ("Heartbeat", "client_time_ms", "uint64", 2),
    ("HeartbeatAck", "nonce", "uint64", 1),
    ("HeartbeatAck", "server_tick", "uint64", 2),
    ("Disconnect", "error_code", "uint32", 1),
    ("Disconnect", "reason", "string", 2),
    ("KeyframeRequest", "last_applied_tick", "uint64", 1),
    ("ControlMessage", "hello", "ClientHello", 1),
    ("ControlMessage", "welcome", "ServerHello", 2),
    ("ControlMessage", "heartbeat", "Heartbeat", 3),
    ("ControlMessage", "heartbeat_ack", "HeartbeatAck", 4),
    ("ControlMessage", "disconnect", "Disconnect", 5),
    ("ControlMessage", "keyframe_request", "KeyframeRequest", 6),
];

#[cfg(test)]
mod tests {
    use super::*;

    const PROTO: &str = include_str!("../../../protocols/proto/rynvek_wire.proto");

    /// 极简 `.proto` 解析：提取 `(消息, 字段, 类型, 标签)`
    fn parse_proto(src: &str) -> Vec<(String, String, String, u32)> {
        let mut out = Vec::new();
        let mut message = String::new();
        for line in src.lines().map(str::trim) {
            if line.starts_with("//") || line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("message ") {
                message = rest.trim_end_matches('{').trim().to_owned();
                continue;
            }
            if let Some((lhs, rhs)) = line.split_once('=') {
                let parts: Vec<&str> = lhs.split_whitespace().collect();
                let tag = rhs.trim().trim_end_matches(';').trim();
                if let ([ty, name], Ok(tag)) = (parts.as_slice(), tag.parse::<u32>()) {
                    out.push((message.clone(), (*name).to_owned(), (*ty).to_owned(), tag));
                }
            }
        }
        out
    }

    #[test]
    fn proto_file_matches_prost_field_table() {
        let parsed = parse_proto(PROTO);
        let table: Vec<(String, String, String, u32)> = FIELD_TABLE
            .iter()
            .map(|(m, f, t, n)| ((*m).into(), (*f).into(), (*t).into(), *n))
            .collect();
        assert_eq!(parsed, table);
        assert!(PROTO.contains("syntax = \"proto3\";"));
        assert!(PROTO.contains("package rynvek.wire.v1;"));
    }

    #[test]
    fn heartbeat_has_reference_wire_bytes() {
        let msg = ControlMessage::new(ControlBody::Heartbeat(Heartbeat {
            nonce: 300,
            client_time_ms: 1,
        }));
        // field 3 (LEN) → 0x1A, len 5 ; nonce: 0x08 0xAC 0x02 ; client_time_ms: 0x10 0x01
        assert_eq!(msg.to_bytes(), [0x1A, 0x05, 0x08, 0xAC, 0x02, 0x10, 0x01]);
        assert_eq!(
            ControlMessage::from_bytes(&msg.to_bytes()).unwrap(),
            ControlBody::Heartbeat(Heartbeat {
                nonce: 300,
                client_time_ms: 1
            })
        );
    }

    #[test]
    fn hello_roundtrip_and_empty_body_is_rejected() {
        let hello = ClientHello {
            protocol: WIRE_PROTOCOL.into(),
            capability_token: vec![1; 96],
            world_id: vec![2; 16],
            faction: 1,
            resume_token: vec![],
            client_version: "test".into(),
        };
        let bytes = ControlMessage::new(ControlBody::Hello(hello.clone())).to_bytes();
        assert_eq!(ControlMessage::from_bytes(&bytes).unwrap(), ControlBody::Hello(hello));
        assert!(ControlMessage::from_bytes(&[]).is_err());
        assert!(ControlMessage::from_bytes(&[0xFF, 0xFF]).is_err());
    }
}
