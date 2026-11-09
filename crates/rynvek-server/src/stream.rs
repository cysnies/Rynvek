// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 规范 11.3 路由 `09`：`/api/v1/worlds/{world_id}/stream`（WebTransport / WebSocket 双栈共用）。
//!
//! 握手（第 1 防线）：首帧必须在 [`HELLO_TIMEOUT`] 内到达，且是 `Control` 通道的 Protobuf `ClientHello`：
//!
//! 1. `protocol == "rynvek-wire/1"`，`world_id` 与路径一致；
//! 2. 96 字节 `CapabilityToken` 验签、未过期、未吊销，且含 `world.connect`；
//! 3. ABAC：主体处于 Active 且对该世界有读取 / 连接权；
//! 4. 该世界有进行中的对局，且主体是参赛者、请求的阵营与席位一致（ResumeToken 若提供必须匹配）。
//!
//! 任一失败都以 `Control/Disconnect{error_code, reason}` 回复后关闭；成功后连接交给对局宿主。
//! 浏览器的 WebSocket / WebTransport API 都无法自定义请求头，因此令牌放在首帧而不是 `Authorization` 头。

use std::time::Duration;

use rynvek_core::error::RynvekErrorCode;
use rynvek_network::FrameEncoder;
use rynvek_network::control::{ControlBody, ControlMessage, Disconnect, WIRE_PROTOCOL};
use rynvek_network::link::{Link, LinkSender};
use rynvek_protocol::{CapabilityScope, WireChannelId, WireUuid};
use rynvek_security::{Resource, authorize};

use crate::app::AppState;
use crate::error::ApiError;
use crate::ids::unix_now_s;

/// 首帧超时
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

async fn refuse(sender: &LinkSender, e: &ApiError) {
    let msg = ControlMessage::new(ControlBody::Disconnect(Disconnect {
        error_code: e.code.code().into(),
        reason: e.message.clone(),
    }));
    if let Ok(frame) = FrameEncoder::new(false).encode(WireChannelId::Control, 0, &msg.to_bytes(), false) {
        let _ = sender.send(frame).await;
    }
}

async fn handshake(
    app: &AppState,
    world_id: WireUuid,
    link: &mut Link,
) -> Result<(crate::host::MatchHandle, crate::host::JoinAccepted), ApiError> {
    let violation = |m: &str| ApiError::forbidden(m.to_owned());
    let first = tokio::time::timeout(HELLO_TIMEOUT, link.recv())
        .await
        .map_err(|_| violation("握手超时：未收到 ClientHello"))?
        .ok_or_else(|| violation("连接在握手前关闭"))?
        .map_err(|e| violation(&e.to_string()))?;
    if first.channel() != WireChannelId::Control {
        return Err(violation("首帧必须在 Control 通道"));
    }
    let payload = first.decoded_payload().map_err(|e| violation(&e.to_string()))?;
    let ControlBody::Hello(hello) = ControlMessage::from_bytes(&payload).map_err(|e| violation(&e.to_string()))? else {
        return Err(violation("首帧必须是 ClientHello"));
    };
    if hello.protocol != WIRE_PROTOCOL {
        return Err(violation(&format!(
            "不支持的协议 `{}`，需要 `{WIRE_PROTOCOL}`",
            hello.protocol
        )));
    }
    if hello.world_id.as_slice() != world_id.0.as_slice() {
        return Err(violation("ClientHello.world_id 与连接路径不一致"));
    }
    let token = app
        .tokens()
        .verify_bytes(&hello.capability_token, unix_now_s())
        .map_err(ApiError::from)?;
    token.require(CapabilityScope::WorldConnect).map_err(ApiError::from)?;
    let subject = app.subject(&token).await?;
    let world = app
        .store()
        .get_world(world_id)
        .await?
        .ok_or_else(|| ApiError::not_found("世界不存在"))?;
    authorize(
        &subject,
        CapabilityScope::WorldConnect,
        &Resource::World {
            owner_principal_id: world.owner_principal_id,
            org_id: world.org_id,
            visibility: world.visibility,
        },
    )
    .map_err(ApiError::from)?;
    let handle = app
        .registry()
        .by_world(&world_id)
        .ok_or_else(|| ApiError::not_found("该世界没有进行中的对局"))?;
    let accepted = handle
        .join(
            subject.principal_id,
            hello.faction,
            hello.resume_token,
            link.sender.clone(),
        )
        .await?;
    Ok((handle, accepted))
}

/// 服务一条已建立的传输连接（直到任一方关闭）
pub async fn serve_link(app: AppState, world_id: WireUuid, mut link: Link) {
    let (handle, accepted) = match handshake(&app, world_id, &mut link).await {
        Ok(ok) => ok,
        Err(e) => {
            app.audit(
                None,
                "stream.handshake_rejected",
                format!("world:{world_id}"),
                serde_json::json!({ "code": e.code.code(), "message": e.message, "transport": format!("{:?}", link.kind) }),
            )
            .await;
            refuse(&link.sender, &e).await;
            return;
        }
    };
    let conn = accepted.conn_id;
    // 发送端只由宿主持有：宿主丢弃连接（终局 / 违例 / 被取代）时传输随之关闭
    let (sender, mut receiver) = link.split();
    drop(sender);
    loop {
        match receiver.recv().await {
            Some(Ok(frame)) => {
                if !handle.frame(conn, frame).await {
                    break;
                }
            }
            Some(Err(e)) => {
                handle.violation(conn, e).await;
                break;
            }
            None => break,
        }
    }
    handle.leave(conn).await;
}

/// 解析路径中的 `world_id`
pub fn parse_world_id(text: &str) -> Result<WireUuid, ApiError> {
    text.parse()
        .map_err(|_| ApiError::new(RynvekErrorCode::EntityNotFoundOrTombstoned, "world_id 不是合法 UUID"))
}
