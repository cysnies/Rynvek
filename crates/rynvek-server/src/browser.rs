// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 浏览器 JSON 适配：仅翻译传输；认证、席位、迷雾、闸门与事务仍由 serve_link/MatchHost 掌握。
use crate::{ApiError, ApiResult, AppState};
use axum::extract::ws::{Message, WebSocket};
use futures_util::SinkExt;
use rynvek_network::FrameEncoder;
use rynvek_network::control::{ClientHello, ControlBody, ControlMessage, WIRE_PROTOCOL};
use rynvek_network::link::Link;
use rynvek_protocol::{CommandEnvelope, IdempotencyKey, WireChannelId, WireUuid};
use serde_json::{Value, json};

async fn text(socket: &mut WebSocket, value: Value) -> ApiResult<()> {
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        socket.send(Message::Text(value.to_string().into())),
    )
    .await
    .map_err(|_| ApiError::bad_request("浏览器消费过慢"))?
    .map_err(|e| ApiError::bad_request(e.to_string()))
}

pub async fn serve(app: AppState, world: WireUuid, mut socket: WebSocket) {
    if let Err(e) = bridge(app, world, &mut socket).await {
        let _ = text(
            &mut socket,
            json!({"type":"error","error":{"code":e.code.code(),"name":format!("{:?}",e.code),"message":e.message}}),
        )
        .await;
    }
    let _ = socket.close().await;
}

async fn bridge(app: AppState, world: WireUuid, socket: &mut WebSocket) -> ApiResult<()> {
    let first = tokio::time::timeout(crate::stream::HELLO_TIMEOUT, socket.recv())
        .await
        .map_err(|_| ApiError::forbidden("JSON 握手超时"))?
        .ok_or_else(|| ApiError::forbidden("缺少 JSON hello"))?
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let Message::Text(raw) = first else {
        return Err(ApiError::bad_request("JSON 模式首帧必须是 hello"));
    };
    let hello: Value = serde_json::from_str(&raw).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if hello["type"] != "hello" || hello["protocol"] != rynvek_engine::arena::ARENA_PROTOCOL {
        return Err(ApiError::forbidden("不支持的 Arena hello"));
    }
    let token = crate::app::decode_token_text(hello["capability_token"].as_str().unwrap_or_default())?;
    let faction = hello["faction"]
        .as_u64()
        .and_then(|f| u32::try_from(f).ok())
        .ok_or_else(|| ApiError::bad_request("缺少阵营"))?;
    let (mut link, server) = Link::memory_pair();
    let task = tokio::spawn(crate::stream::serve_link(app, world, server));
    // principal 只取既有认证管道返回的 ServerHello，绝不信任前端身份字段。
    let result = async {
        let mut encoder = FrameEncoder::new(false);
        let wire_hello = ControlMessage::new(ControlBody::Hello(ClientHello {
            protocol:WIRE_PROTOCOL.into(),capability_token:token,world_id:world.0.to_vec(),faction,
            resume_token:Vec::new(),client_version:"rynvek-web/1".into(),
        }));
        link.send(encoder.encode(WireChannelId::Control,0,&wire_hello.to_bytes(),false).map_err(|e|ApiError::bad_request(e.to_string()))?).await.map_err(|e|ApiError::bad_request(e.to_string()))?;
        let mut principal = None;
        let mut tick = 0;
        let mut last_observe = None;
        let mut events: Vec<Value> = Vec::new();
        loop {
            tokio::select! {
                incoming = socket.recv() => {
                    let Some(Ok(message)) = incoming else { break };
                    let raw = match message {Message::Text(s)=>s,Message::Close(_)=>break,Message::Ping(_) | Message::Pong(_)=>continue,_=>return Err(ApiError::bad_request("JSON 模式不接受二进制帧"))};
                    let payload: Value = serde_json::from_str(&raw).map_err(|e|ApiError::bad_request(e.to_string()))?;
                    // 由 Actor 校验原 ArenaClientMessage、租约、APM 和权限，保持 SDK 的命令契约。
                    let env = CommandEnvelope {
                        idempotency_key:IdempotencyKey::new(crate::ids::new_uuid_v7()).expect("合法 v7"),
                        principal_id:principal.ok_or_else(||ApiError::forbidden("尚未认证"))?,
                        issued_at_tick:tick,target_tick:tick,payload,
                    };
                    let bytes = serde_json::to_vec(&env).expect("可序列化");
                    let frame = encoder.encode(WireChannelId::Command,tick,&bytes,false).map_err(|e|ApiError::bad_request(e.to_string()))?;
                    link.send(frame).await.map_err(|e|ApiError::bad_request(e.to_string()))?;
                }
                incoming = link.recv() => {
                    let Some(Ok(frame)) = incoming else {break};
                    tick = u64::from(frame.header.tick_low32);
                    let bytes = frame.decoded_payload().map_err(|e|ApiError::bad_request(e.to_string()))?;
                    match frame.channel() {
                        WireChannelId::Control => match ControlMessage::from_bytes(&bytes).map_err(|e|ApiError::bad_request(e.to_string()))? {
                            ControlBody::Welcome(w)=>principal=Some(w.principal_id),
                            ControlBody::Disconnect(d)=>{
                                if d.error_code != 0 {return Err(ApiError::forbidden(d.reason))}
                                break;
                            }
                            _=>{}
                        },
                        WireChannelId::Command => {
                            let reply:Value = serde_json::from_slice(&bytes).map_err(|e|ApiError::bad_request(e.to_string()))?;
                            let mut message = reply["message"].clone();
                            if message["type"] == "observation" {message["events"] = json!(std::mem::take(&mut events));}
                            text(socket,message).await?;
                        }
                        WireChannelId::StateStream => {
                            // 仅因已提交状态帧触发同一 Actor 的只读阵营查询；10Hz 显示不是另一份仿真。
                            if last_observe.is_none_or(|last|tick >= last + 3) {
                                last_observe=Some(tick);
                                let query=json!({"type":"observe","seq":tick});
                                let frame=encoder.encode(WireChannelId::Query,tick,&serde_json::to_vec(&query).expect("JSON"),false).map_err(|e|ApiError::bad_request(e.to_string()))?;
                                link.send(frame).await.map_err(|e|ApiError::bad_request(e.to_string()))?;
                            }
                        }
                        WireChannelId::Query => {
                            let reply:Value=serde_json::from_slice(&bytes).map_err(|e|ApiError::bad_request(e.to_string()))?;
                            if !reply["error"].is_null() {text(socket,json!({"type":"error","error":reply["error"]})).await?;}
                            else if !reply["result"].is_null() {text(socket,json!({"type":"observation","observation":reply["result"],"events":std::mem::take(&mut events)})).await?;}
                        }
                        WireChannelId::Event => {
                            let batch:Vec<Value>=serde_json::from_slice(&bytes).map_err(|e|ApiError::bad_request(e.to_string()))?;
                            events.extend(batch);
                            if events.len()>1024 {return Err(ApiError::bad_request("观察事件积压超过上限"));}
                        }
                        _=>{}
                    }
                }
            }
        }
        Ok(())
    }.await;
    drop(link);
    let _ = task.await;
    result
}
