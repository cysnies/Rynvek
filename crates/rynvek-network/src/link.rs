// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 与传输无关的双向帧通道。
//!
//! 会话逻辑（握手、命令、复制）只面对 [`Link`]：发送 [`Frame`]、接收已校验的 [`Frame`]。
//! 传输适配器负责把具体连接 "泵" 成 `Link`：
//!
//! - [`Link::from_messages`]：消息型传输（WebSocket），一条二进制消息恰好一帧；
//! - [`Link::from_byte_stream`]：字节流传输（WebTransport 双向流、测试用 `tokio::io::duplex`），增量重组；
//! - [`Link::memory_pair`]：进程内直连（单元测试）。
//!
//! 入站方向统一做第 2 防线校验：帧头合法、长度一致、每通道序号严格 `+1`；首个违例即上报并终止该方向。

use futures_util::{Sink, SinkExt, Stream, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::error::NetError;
use crate::frame::{Frame, FrameDecoder, SequenceTracker, decode_message};

/// 每个方向缓冲的帧数（背压：缓冲满时发送方等待）
pub const LINK_BUFFER_FRAMES: usize = 256;

/// 传输种类
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportKind {
    WebSocket,
    WebTransport,
    Memory,
}

/// 帧发送端（可克隆，供多任务共享）
#[derive(Clone, Debug)]
pub struct LinkSender {
    tx: mpsc::Sender<Frame>,
}

impl LinkSender {
    pub async fn send(&self, frame: Frame) -> Result<(), NetError> {
        self.tx.send(frame).await.map_err(|_| NetError::Closed)
    }

    /// 非阻塞发送：缓冲已满即报错。实时宿主用它向慢消费者推送，满了就断开该连接而不是拖慢整局 Tick
    pub fn try_send(&self, frame: Frame) -> Result<(), NetError> {
        self.tx.try_send(frame).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => NetError::Transport("发送缓冲已满（慢消费者）".into()),
            mpsc::error::TrySendError::Closed(_) => NetError::Closed,
        })
    }

    /// 对端（或本地泵任务）是否已关闭
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

/// 帧接收端
#[derive(Debug)]
pub struct LinkReceiver {
    rx: mpsc::Receiver<Result<Frame, NetError>>,
}

impl LinkReceiver {
    /// 下一帧；`None` 表示对端正常关闭
    pub async fn recv(&mut self) -> Option<Result<Frame, NetError>> {
        self.rx.recv().await
    }
}

/// 双向帧通道
#[derive(Debug)]
pub struct Link {
    pub kind: TransportKind,
    pub sender: LinkSender,
    pub receiver: LinkReceiver,
}

impl Link {
    pub async fn send(&self, frame: Frame) -> Result<(), NetError> {
        self.sender.send(frame).await
    }

    pub async fn recv(&mut self) -> Option<Result<Frame, NetError>> {
        self.receiver.recv().await
    }

    pub fn split(self) -> (LinkSender, LinkReceiver) {
        (self.sender, self.receiver)
    }

    /// 进程内直连的一对 Link（同样经过序号校验）
    pub fn memory_pair() -> (Link, Link) {
        let (a_tx, a_rx) = mpsc::channel::<Frame>(LINK_BUFFER_FRAMES);
        let (b_tx, b_rx) = mpsc::channel::<Frame>(LINK_BUFFER_FRAMES);
        (Self::memory_end(a_tx, b_rx), Self::memory_end(b_tx, a_rx))
    }

    fn memory_end(out: mpsc::Sender<Frame>, mut incoming: mpsc::Receiver<Frame>) -> Link {
        let (in_tx, in_rx) = mpsc::channel(LINK_BUFFER_FRAMES);
        tokio::spawn(async move {
            let mut seq = SequenceTracker::new();
            while let Some(frame) = incoming.recv().await {
                let checked = seq.accept(&frame.header).map(|()| frame);
                let stop = checked.is_err();
                if in_tx.send(checked).await.is_err() || stop {
                    break;
                }
            }
        });
        Link {
            kind: TransportKind::Memory,
            sender: LinkSender { tx: out },
            receiver: LinkReceiver { rx: in_rx },
        }
    }

    /// 消息型传输：`stream` 产出 `Ok(Some(bytes))` 为二进制消息，`Ok(None)` 为可忽略的控制消息（Ping/Pong 等），
    /// `Err(NetError::Closed)` 表示对端正常关闭
    pub fn from_messages<S, K>(kind: TransportKind, sink: K, stream: S) -> Link
    where
        S: Stream<Item = Result<Option<Vec<u8>>, NetError>> + Send + Unpin + 'static,
        K: Sink<Vec<u8>> + Send + Unpin + 'static,
    {
        let (out_tx, mut out_rx) = mpsc::channel::<Frame>(LINK_BUFFER_FRAMES);
        let (in_tx, in_rx) = mpsc::channel(LINK_BUFFER_FRAMES);
        let mut sink = sink;
        tokio::spawn(async move {
            while let Some(frame) = out_rx.recv().await {
                if sink.send(frame.to_bytes()).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });
        let mut stream = stream;
        tokio::spawn(async move {
            let mut seq = SequenceTracker::new();
            while let Some(item) = stream.next().await {
                let checked = match item {
                    Ok(None) => continue,
                    Ok(Some(bytes)) => decode_message(&bytes).and_then(|f| seq.accept(&f.header).map(|()| f)),
                    // 对端正常关闭（WebSocket Close 帧）：结束而不是上报违例
                    Err(NetError::Closed) => break,
                    Err(e) => Err(e),
                };
                let stop = checked.is_err();
                if in_tx.send(checked).await.is_err() || stop {
                    break;
                }
            }
        });
        Link {
            kind,
            sender: LinkSender { tx: out_tx },
            receiver: LinkReceiver { rx: in_rx },
        }
    }

    /// 字节流传输：帧首尾相接写入 `writer`，从 `reader` 增量重组
    pub fn from_byte_stream<R, W>(kind: TransportKind, reader: R, writer: W) -> Link
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (out_tx, mut out_rx) = mpsc::channel::<Frame>(LINK_BUFFER_FRAMES);
        let (in_tx, in_rx) = mpsc::channel(LINK_BUFFER_FRAMES);
        let mut writer = writer;
        tokio::spawn(async move {
            while let Some(frame) = out_rx.recv().await {
                if writer.write_all(&frame.to_bytes()).await.is_err() || writer.flush().await.is_err() {
                    break;
                }
            }
            let _ = writer.shutdown().await;
        });
        let mut reader = reader;
        tokio::spawn(async move {
            let mut dec = FrameDecoder::new();
            let mut seq = SequenceTracker::new();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => {
                        let _ = in_tx.send(Err(NetError::Transport(e.to_string()))).await;
                        break;
                    }
                };
                dec.push(&buf[..n]);
                loop {
                    let next = dec
                        .next_frame()
                        .and_then(|f| f.map(|f| seq.accept(&f.header).map(|()| f)).transpose());
                    match next {
                        Ok(None) => break,
                        Ok(Some(frame)) => {
                            if in_tx.send(Ok(frame)).await.is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = in_tx.send(Err(e)).await;
                            return;
                        }
                    }
                }
            }
        });
        Link {
            kind,
            sender: LinkSender { tx: out_tx },
            receiver: LinkReceiver { rx: in_rx },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameEncoder;
    use rynvek_protocol::WireChannelId;

    #[tokio::test]
    async fn byte_stream_link_roundtrip_over_duplex() {
        let (a, b) = tokio::io::duplex(37); // 刻意很小的缓冲，迫使帧被切碎
        let (ar, aw) = tokio::io::split(a);
        let (br, bw) = tokio::io::split(b);
        let left = Link::from_byte_stream(TransportKind::Memory, ar, aw);
        let mut right = Link::from_byte_stream(TransportKind::Memory, br, bw);
        let mut enc = FrameEncoder::new(true);
        let frames: Vec<Frame> = (0..20)
            .map(|i| {
                enc.encode(
                    WireChannelId::StateStream,
                    i,
                    &vec![i as u8; 300 * i as usize],
                    i % 5 == 0,
                )
                .unwrap()
            })
            .collect();
        for f in &frames {
            left.send(f.clone()).await.unwrap();
        }
        for f in &frames {
            assert_eq!(&right.recv().await.unwrap().unwrap(), f);
        }
    }

    #[tokio::test]
    async fn try_send_reports_backpressure_instead_of_blocking() {
        let (a, _b) = Link::memory_pair();
        let mut enc = FrameEncoder::new(false);
        let mut full = false;
        for i in 0..(LINK_BUFFER_FRAMES * 4) {
            let f = enc.encode(WireChannelId::Event, i as u64, b"x", false).unwrap();
            if a.sender.try_send(f).is_err() {
                full = true;
                break;
            }
        }
        assert!(full, "对端不读时缓冲必然写满");
    }

    #[tokio::test]
    async fn sequence_violation_is_reported_and_terminates() {
        let (a, mut b) = Link::memory_pair();
        let mut enc = FrameEncoder::new(false);
        let f0 = enc.encode(WireChannelId::Command, 0, b"x", false).unwrap();
        a.send(f0.clone()).await.unwrap();
        a.send(f0).await.unwrap(); // 重放
        assert!(b.recv().await.unwrap().is_ok());
        assert!(matches!(b.recv().await.unwrap(), Err(NetError::Sequence { .. })));
        assert!(b.recv().await.is_none());
    }
}
