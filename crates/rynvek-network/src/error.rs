// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 网络层错误。协议违例（第 2 防线）一律视为致命：连接以 `Disconnect` 关闭，不尝试 "猜测" 修复。

use core::fmt;

use rynvek_core::error::RynvekErrorCode;
use rynvek_protocol::wire::WireError;

/// 网络层错误
#[derive(Debug)]
pub enum NetError {
    /// 帧头非法
    Header(WireError),
    /// 帧长度与传输层消息长度不一致（消息型传输：WebSocket）
    LengthMismatch { declared: u32, actual: usize },
    /// 同一通道帧序号不连续（可靠有序流上出现即为篡改或实现缺陷）
    Sequence { channel: u8, expected: u32, got: u32 },
    /// LZ4 解压失败或解压后超过上限
    Decompress(String),
    /// 负载结构非法（Protobuf / 列式状态包 / JSON）
    Payload(String),
    /// 传输层 I/O 错误
    Transport(String),
    /// 对端已关闭
    Closed,
}

impl NetError {
    /// 映射到规范错误码：协议违例视为未授权帧（`0x8001`），负载 Schema 错误为 `0x5001`
    pub const fn code(&self) -> RynvekErrorCode {
        match self {
            Self::Payload(_) => RynvekErrorCode::RuleStaticTypeCheckFailed,
            _ => RynvekErrorCode::UnauthorizedCapabilityToken,
        }
    }
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Header(e) => write!(f, "帧头非法：{e}"),
            Self::LengthMismatch { declared, actual } => {
                write!(f, "帧声明负载 {declared} 字节，但消息实际负载 {actual} 字节")
            }
            Self::Sequence { channel, expected, got } => {
                write!(f, "通道 {channel} 帧序号应为 {expected}，收到 {got}")
            }
            Self::Decompress(e) => write!(f, "LZ4 解压失败：{e}"),
            Self::Payload(e) => write!(f, "负载非法：{e}"),
            Self::Transport(e) => write!(f, "传输层错误：{e}"),
            Self::Closed => write!(f, "连接已关闭"),
        }
    }
}

impl std::error::Error for NetError {}

impl From<WireError> for NetError {
    fn from(e: WireError) -> Self {
        Self::Header(e)
    }
}
