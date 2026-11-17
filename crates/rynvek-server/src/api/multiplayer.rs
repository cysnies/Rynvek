// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 私有自创双人PvP：正式主体定向邀请、受邀确认、冻结完整席位和显式整局关闭。
use crate::{
    app::{AppState, league_resource},
    error::{ApiError, ApiResult},
    host::{MatchSpec, SeatSpec},
    ids::new_uuid_v7,
    store::{CustomRoom, MatchRow, MatchState, ParticipantRow, WorldRow},
    transactions::conflict,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use rynvek_protocol::{CapabilityScope, WireUuid};
use rynvek_security::{League, PrincipalKindCode, PrincipalStatus, Subject, Visibility, authorize};
use serde::Deserialize;
use serde_json::{Value, json};
fn missing() -> ApiError {
    ApiError::not_found("私有自创房间不存在")
}
fn response(v: Value) -> Response {
    (
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::VARY, "Authorization"),
        ],
        Json(v),
    )
        .into_response()
}
async fn subject(app: &AppState, h: &HeaderMap) -> ApiResult<Subject> {
    let s = app.require(h, CapabilityScope::WorldConnect).await?;
    authorize(&s, CapabilityScope::MatchPlay, &league_resource(League::Human)).map_err(ApiError::from)?;
    if s.kind != PrincipalKindCode::Human || s.status != PrincipalStatus::Active {
        return Err(ApiError::forbidden("当前自创PvP仅接入正式Active人类身份"));
    }
    Ok(s)
}
async fn world(app: &AppState, id: &str) -> ApiResult<WorldRow> {
    let w = app
        .store()
        .get_world(id.parse().map_err(|_| ApiError::bad_request("世界UUID非法"))?)
        .await?
        .ok_or_else(missing)?;
    if w.visibility != Visibility::Private || w.game_mode != "PvP_Custom" {
        return Err(missing());
    }
    Ok(w)
}
async fn load(app: &AppState, s: &Subject, w: &WorldRow) -> ApiResult<Option<CustomRoom>> {
    let room = app.store().custom_room(w.world_id).await?;
    if s.principal_id != w.owner_principal_id && !room.as_ref().is_some_and(|r| r.guest_id == s.principal_id) {
        return Err(missing());
    }
    Ok(room)
}
/// 仅替代明确受房间治理的Private自创席位准入，不授予WorldRead/Edit或任意阵营。
pub(crate) async fn can_connect(app: &AppState, s: &Subject, w: &WorldRow) -> ApiResult<bool> {
    if w.visibility != Visibility::Private || w.game_mode != "PvP_Custom" {
        return Ok(false);
    }
    if s.kind != PrincipalKindCode::Human || s.status != PrincipalStatus::Active {
        return Ok(false);
    }
    if authorize(s, CapabilityScope::MatchPlay, &league_resource(League::Human)).is_err() {
        return Ok(false);
    }
    Ok(app.store().custom_room(w.world_id).await?.is_some_and(|r| {
        r.active && !r.closed && r.accepted && (s.principal_id == w.owner_principal_id || s.principal_id == r.guest_id)
    }))
}
async fn view(
    app: &AppState,
    s: &Subject,
    w: &WorldRow,
    room: Option<&CustomRoom>,
    recovered: bool,
) -> ApiResult<Response> {
    let Some(r) = room else {
        return Ok(response(
            json!({"world_id":w.world_id.to_string(),"principal_id":s.principal_id.to_string(),"role":"owner","status":"unconfigured","custom":true}),
        ));
    };
    let faction = if s.principal_id == w.owner_principal_id { 0 } else { 1 };
    let stored = app.store().get_match(r.match_id).await?;
    let host = app
        .registry()
        .by_world(&w.world_id)
        .filter(|h| h.match_id == r.match_id);
    let state = host
        .as_ref()
        .map(|h| h.status().state)
        .or(stored.as_ref().map(|(m, _)| m.match_state));
    let tick = host.as_ref().map(|h| h.status().tick).unwrap_or(w.latest_tick);
    let status = if r.closed {
        "closed"
    } else if state.is_some_and(|s| s.is_terminal()) {
        "ended"
    } else if r.active {
        "active"
    } else if r.accepted {
        "ready"
    } else {
        "invited"
    };
    let seat = if r.active && !r.closed {
        Some(
            json!({"world_id":w.world_id.to_string(),"match_id":r.match_id.to_string(),"principal_id":s.principal_id.to_string(),"faction":faction,"mode":"PvP_Ranked","match_state":state.unwrap_or(MatchState::Created),"tick":tick,"recovered":recovered,"custom":true}),
        )
    } else {
        None
    };
    Ok(response(
        json!({"world_id":w.world_id.to_string(),"principal_id":s.principal_id.to_string(),"owner_id":w.owner_principal_id.to_string(),"guest_id":r.guest_id.to_string(),"faction":faction,"role":if faction==0{"owner"}else{"player"},"version":r.version.to_string(),"status":status,"accepted":r.accepted,"custom":true,"seat":seat}),
    ))
}
pub async fn get(State(app): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Response> {
    let s = subject(&app, &headers).await?;
    let w = world(&app, &id).await?;
    let r = load(&app, &s, &w).await?;
    view(&app, &s, &w, r.as_ref(), false).await
}
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Invite { guest_id: String },
    Accept { version: String },
    Connect { version: String },
    Close { version: String },
}
fn version(v: &str) -> ApiResult<WireUuid> {
    v.parse().map_err(|_| ApiError::bad_request("房间版本UUID非法"))
}
pub async fn change(
    State(app): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(action): Json<Action>,
) -> ApiResult<Response> {
    let _guard = app.inner.authored_arena_start.lock().await;
    let s = subject(&app, &headers).await?;
    let w = world(&app, &id).await?;
    let existing = load(&app, &s, &w).await?;
    if let Action::Invite { guest_id } = action {
        if s.principal_id != w.owner_principal_id {
            return Err(missing());
        }
        app.require(&headers, CapabilityScope::WorldEdit).await?;
        if existing.is_some() {
            return Err(conflict("世界已有房间生命周期，请刷新；关闭后需显式fork或新建"));
        }
        let guest_id = guest_id
            .parse::<u64>()
            .ok()
            .filter(|p| *p > 0 && p.to_string() == guest_id)
            .ok_or_else(|| ApiError::bad_request("受邀主体ID非法"))?;
        let guest = app
            .store()
            .get_principal(guest_id)
            .await?
            .filter(|p| {
                p.kind == PrincipalKindCode::Human
                    && p.status == PrincipalStatus::Active
                    && p.principal_id != s.principal_id
            })
            .ok_or_else(|| ApiError::bad_request("须指定不同的正式Active人类主体"))?;
        let runtime = super::worlds::restore_runtime(&app, &w).await?;
        if runtime.config.players != 2
            || runtime.config.mode != rynvek_rts::MatchMode::PvpRanked
            || runtime.outcome.is_some()
        {
            return Err(conflict("须为未终局的真实双人自创PvP世界"));
        }
        let room = CustomRoom {
            world_id: w.world_id,
            version: new_uuid_v7(),
            guest_id: guest.principal_id,
            match_id: new_uuid_v7(),
            accepted: false,
            active: false,
            closed: false,
        };
        app.store().save_custom_room(room.clone(), None, None).await?;
        app.audit(
            Some(s.principal_id),
            "custom.invite",
            format!("world:{}", w.world_id),
            json!({"guest_id":guest_id.to_string(),"version":room.version.to_string()}),
        )
        .await;
        return view(&app, &s, &w, Some(&room), false).await;
    }
    let mut room = existing.ok_or_else(missing)?;
    let expected = match &action {
        Action::Accept { version: v } | Action::Connect { version: v } | Action::Close { version: v } => version(v)?,
        _ => unreachable!(),
    };
    if room.version != expected {
        return Err(conflict("房间版本已改变，须刷新确认，不覆盖新状态"));
    }
    if room.closed {
        return view(&app, &s, &w, Some(&room), false).await;
    }
    match action {
        Action::Accept { .. } => {
            if s.principal_id != room.guest_id {
                return Err(ApiError::forbidden("仅受邀主体可确认自己的席位"));
            }
            if !room.accepted {
                room.accepted = true;
                room.version = new_uuid_v7();
                app.store().save_custom_room(room.clone(), Some(expected), None).await?;
            }
        }
        Action::Close { .. } => {
            room.closed = true;
            room.active = false;
            room.version = new_uuid_v7();
            app.store().save_custom_room(room.clone(), Some(expected), None).await?; // 与世界fence轮换、Match Aborted原子提交
            let active = { app.registry().by_world(&w.world_id) };
            if let Some(h) = active {
                if h.match_id == room.match_id {
                    h.close_custom().await?;
                }
            }
            app.audit(Some(s.principal_id),"custom.close",format!("world:{}",w.world_id),json!({"version":room.version.to_string(),"authorization_closed":true,"policy":"abort_nonterminal_match"})).await;
        }
        Action::Connect { .. } => {
            let recovery = room.active;
            if !room.accepted {
                return Err(conflict("必须由受邀者正式确认，不自动代填玩家"));
            }
            for pid in [w.owner_principal_id, room.guest_id] {
                if !app
                    .store()
                    .get_principal(pid)
                    .await?
                    .is_some_and(|p| p.kind == PrincipalKindCode::Human && p.status == PrincipalStatus::Active)
                {
                    return Err(conflict("冻结席位身份不可用"));
                }
                if app.registry().active_for(pid).is_some_and(|h| h.world_id != w.world_id) {
                    return Err(conflict("参赛者仍在其他活动对局"));
                }
            }
            if !room.active {
                if s.principal_id != w.owner_principal_id {
                    return Err(ApiError::forbidden("完整席位确认后由所有者显式冻结并开局"));
                }
                let runtime = super::worlds::restore_runtime(&app, &w).await?;
                let hash = *blake3::hash(&[b"rynvek-custom/1\0".as_slice(), &runtime.state_root()].concat()).as_bytes();
                let row = MatchRow {
                    match_id: room.match_id,
                    world_id: w.world_id,
                    game_mode: "PvP_Custom".into(),
                    ruleset_blake3: hash,
                    map_seed: runtime.config.map_seed,
                    match_state: MatchState::Created,
                    winner_faction_id: None,
                    win_reason: None,
                    total_ticks: runtime.tick,
                    replay_cid_blake3: None,
                    started_at_unix_s: None,
                    completed_at_unix_s: None,
                };
                let parts = [w.owner_principal_id, room.guest_id]
                    .iter()
                    .enumerate()
                    .map(|(f, p)| ParticipantRow {
                        match_id: room.match_id,
                        faction_id: f as i16,
                        principal_id: *p,
                        controller_league: League::Human,
                        final_score: 0,
                        rating_delta: None,
                    })
                    .collect();
                room.active = true;
                room.version = new_uuid_v7();
                app.store()
                    .save_custom_room(room.clone(), Some(expected), Some((row, parts)))
                    .await?;
            }
            if app
                .registry()
                .by_world(&w.world_id)
                .is_some_and(|h| h.match_id == room.match_id && !h.status().state.is_terminal())
            {
                return view(&app, &s, &w, Some(&room), false).await;
            }
            let row = app.store().get_match(room.match_id).await?.ok_or_else(missing)?.0;
            if row.match_state.is_terminal() {
                return view(&app, &s, &w, Some(&room), false).await;
            }
            let runtime = super::worlds::restore_runtime(&app, &w).await?;
            app.attach_world_host(MatchSpec {
                custom: true,
                match_id: room.match_id,
                world_id: w.world_id,
                league: League::Human,
                ruleset_hash: row.ruleset_blake3,
                config: runtime.config.clone(),
                seats: [w.owner_principal_id, room.guest_id]
                    .iter()
                    .enumerate()
                    .map(|(f, p)| SeatSpec {
                        faction: f as u8,
                        principal_id: *p,
                        kind: PrincipalKindCode::Human,
                    })
                    .collect(),
            })
            .await?;
            return view(&app, &s, &w, Some(&room), recovery).await;
        }
        _ => unreachable!(),
    }
    view(&app, &s, &w, Some(&room), false).await
}
