// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-network
//!
//! Rynvek 的 Tier 4 实时网络层（规范 11、D124–D129、D202–D208）：
//!
//! - [`frame`]：16 字节 [`WireFrameHeader`](rynvek_protocol::WireFrameHeader) 帧同步、按通道序号与 LZ4 Frame 压缩；
//! - [`control`]：Control 通道的 Protobuf v3 信令（`protocols/proto/rynvek_wire.proto`）；
//! - [`visibility`]：服务端 `VisibilityGrid` 物理裁剪快照（GAP-12）；
//! - [`replication`]：StateStream 通道的 16B 对齐列式二进制 Delta 复制与客户端副本；
//! - [`link`]：与传输无关的双向帧通道 [`Link`](link::Link)；
//! - [`websocket`] / [`webtransport`]：双栈传输适配（WebSocket 一消息一帧；WebTransport 双向流上增量重组）。

#![deny(unsafe_code)]

pub mod control;
pub mod error;
pub mod frame;
pub mod visibility;

pub use error::NetError;
pub use frame::{Frame, FrameDecoder, FrameEncoder, SequenceTracker, decode_message};
pub use visibility::VisibilityGrid;
