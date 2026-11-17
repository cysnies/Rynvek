// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 排位队列与已提交 Created 对局（持久配对预留）。CAS 在事务前准备；事务内同时建世界、首提交、两席和对局。
//! 单个协调锁刻意串行化入队/建局与其它模式的席位冻结；不是跨机灾备或自动转发凭据的实现。
use crate::{
    ids::unix_now_s,
    matchmaking::{Matchmaker, Pairing, QueueEntry},
    store::*,
};
use rynvek_protocol::WireUuid;
use rynvek_security::League;
use serde::{Deserialize, Serialize};

pub const HOST_TTL_S: u64 = 30;
/// 从完整配对提交算起的固定入座期限；宿主重建不能无限重置大厅期限。
pub const READY_TIMEOUT_S: u64 = 120;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueueTicket {
    pub league: League,
    pub ruleset: [u8; 32],
    pub entry: QueueEntry,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueAction {
    Read,
    Join,
    Poll,
    Cancel,
}
#[derive(Clone, Debug)]
pub struct QueueCommand {
    pub ticket: QueueTicket,
    pub action: QueueAction,
}
#[derive(Clone, Debug)]
pub enum QueueDecision {
    Idle,
    Cancelled,
    Queued { position: usize, rating: f64 },
    Prepare,
    Matched(WireUuid),
}
#[derive(Clone, Debug)]
pub struct RankedGenesis {
    pub world: WorldRow,
    pub commit: CommitRow,
    pub row: MatchRow,
}
impl RankedGenesis {
    pub fn bind(&mut self, pair: &Pairing) {
        let owner = pair.players[0].principal_id;
        self.world.org_id = owner;
        self.world.owner_principal_id = owner;
        self.row.ruleset_blake3 = pair.ruleset_hash;
        self.world.title = format!("Ranked {} {}", pair.league.as_str(), self.row.match_id);
    }
    pub fn participants(&self, pair: &Pairing) -> Vec<ParticipantRow> {
        pair.players
            .iter()
            .enumerate()
            .map(|(f, p)| ParticipantRow {
                match_id: self.row.match_id,
                principal_id: p.principal_id,
                faction_id: f as i16,
                controller_league: pair.league,
                final_score: 0,
                rating_delta: None,
            })
            .collect()
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostLease {
    pub world_id: WireUuid,
    pub node: String,
    pub origin: String,
    pub fence: u64,
    pub expires_at_s: u64,
}
#[derive(Clone, Debug)]
pub struct HostClaim {
    pub world: WireUuid,
    pub revision: u64,
    pub fence: u64,
    pub node: String,
    pub origin: String,
}

/// 纯匹配决策，双方后端都在各自同一事务/互斥锁内调用；无进程内队列作为事实源。
pub(crate) fn decide(
    tickets: &mut Vec<QueueTicket>,
    req: &QueueCommand,
    has_genesis: bool,
) -> (QueueDecision, Option<Pairing>) {
    let pid = req.ticket.entry.principal_id;
    let current = tickets.iter().position(|t| t.entry.principal_id == pid);
    if req.action == QueueAction::Cancel {
        if let Some(i) = current {
            tickets.remove(i);
            return (QueueDecision::Cancelled, None);
        }
        return (QueueDecision::Idle, None);
    }
    if current.is_none() && req.action != QueueAction::Join {
        return (QueueDecision::Idle, None);
    }
    if req.action == QueueAction::Join {
        if let Some(i) = current {
            if tickets[i].league == req.ticket.league && tickets[i].ruleset == req.ticket.ruleset {
                tickets[i].entry.rating = req.ticket.entry.rating;
                tickets[i].entry.deviation = req.ticket.entry.deviation;
            } else {
                tickets[i] = req.ticket.clone();
            }
        } else {
            tickets.push(req.ticket.clone());
        }
    }
    tickets.sort_by_key(|t| (t.entry.enqueued_at_s, t.entry.principal_id));
    let mut mm = Matchmaker::new();
    for t in tickets.iter() {
        mm.enqueue((t.league, t.ruleset), t.entry);
    }
    let mut pair = None;
    if req.action != QueueAction::Read {
        // 仅推进请求者所在候选对；其它桶/用户由其已认证轮询推进，不创建无关宿主。
        pair = mm
            .pair(unix_now_s())
            .into_iter()
            .find(|p| p.players.iter().any(|p| p.principal_id == pid));
        if pair.is_some() && !has_genesis {
            return (QueueDecision::Prepare, None);
        }
    }
    if let Some(ref p) = pair {
        tickets.retain(|t| !p.players.iter().any(|p| p.principal_id == t.entry.principal_id));
        return (QueueDecision::Prepare, pair);
    }
    let current = tickets.iter().find(|t| t.entry.principal_id == pid).expect("已入队");
    let position = tickets
        .iter()
        .filter(|t| t.league == current.league && t.ruleset == current.ruleset)
        .position(|t| t.entry.principal_id == pid)
        .unwrap()
        + 1;
    (
        QueueDecision::Queued {
            position,
            rating: current.entry.rating,
        },
        None,
    )
}

pub(crate) fn occupied_error() -> crate::error::ApiError {
    crate::transactions::conflict("主体已有活动对局或已在排位队列中，请先完成/离开原入口")
}

/// 在持久事务持锁后计算，不能先读评分再分批写入。
pub(crate) fn ratings_for(row: &MatchRow, parts: &mut [ParticipantRow], old: &[RatingRow]) -> Vec<RatingRow> {
    let score = match row.winner_faction_id {
        Some(f) if f == parts[0].faction_id => 1.0,
        Some(_) => 0.0,
        None => 0.5,
    };
    let (a, b) = crate::glicko2::update_pair(old[0].glicko(), old[1].glicko(), score, crate::glicko2::DEFAULT_TAU);
    parts
        .iter_mut()
        .zip(old)
        .zip([a, b])
        .map(|((p, o), n)| {
            p.rating_delta = Some(n.rating - o.rating);
            RatingRow {
                principal_id: p.principal_id,
                league: p.controller_league,
                rating: n.rating,
                deviation: n.deviation,
                volatility: n.volatility,
                matches_played: o.matches_played + 1,
            }
        })
        .collect()
}
