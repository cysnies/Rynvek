// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::ranked::*;

pub(super) async fn lock_seats(tx: &tokio_postgres::Transaction<'_>) -> StoreResult<()> {
    tx.query_one("SELECT id FROM matchmaking_coordination WHERE id=1 FOR UPDATE", &[])
        .await
        .map_err(db_err)?;
    Ok(())
}
pub(super) async fn ensure_free(tx: &tokio_postgres::Transaction<'_>, pid: u64) -> StoreResult<()> {
    let busy:bool=tx.query_one("SELECT EXISTS(SELECT 1 FROM match_participants p JOIN matches m USING(match_id) WHERE p.principal_id=$1 AND m.match_state NOT IN ('Completed','Aborted')) OR EXISTS(SELECT 1 FROM ranked_queue_tickets WHERE principal_id=$1 AND active)",&[&i(pid)]).await.map_err(db_err)?.get(0);
    if busy {
        return Err(occupied_error());
    }
    Ok(())
}
const HOST_COLS: &str = "world_id::text,node_id,public_origin,fencing_token,expires_at_s";
fn host_of(r: &Row) -> StoreResult<HostLease> {
    Ok(HostLease {
        world_id: parse_uuid(r.get(0))?,
        node: r.get(1),
        origin: r.get(2),
        fence: u(r.get(3)),
        expires_at_s: u(r.get(4)),
    })
}
pub(super) async fn check_lease(tx: &tokio_postgres::Transaction<'_>, world: WireUuid, fence: u64) -> StoreResult<()> {
    let expired:bool=tx.query_one("SELECT EXISTS(SELECT 1 FROM ranked_host_leases WHERE world_id=$1::text::uuid AND (fencing_token<>$2 OR expires_at_s<=floor(extract(epoch FROM clock_timestamp()))::bigint))",&[&uuid_text(&world),&i(fence)]).await.map_err(db_err)?.get(0);
    if expired {
        return Err(crate::transactions::conflict("宿主租约已过期"));
    }
    Ok(())
}
impl PostgresStore {
    pub async fn settle_ranked(&self, row: MatchRow, fence: u64, mut parts: Vec<ParticipantRow>) -> StoreResult<()> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        lock_seats(&tx).await?;
        let w = tx
            .query_one(
                "SELECT fencing_token FROM worlds WHERE world_id=$1::text::uuid FOR UPDATE",
                &[&uuid_text(&row.world_id)],
            )
            .await
            .map_err(db_err)?;
        if u(w.get(0)) != fence {
            return Err(crate::transactions::conflict("旧宿主不得结算"));
        }
        check_lease(&tx, row.world_id, fence).await?;
        let current = match_of(
            &tx.query_one(
                &format!("SELECT {MATCH_COLS} FROM matches WHERE match_id=$1::text::uuid FOR UPDATE"),
                &[&uuid_text(&row.match_id)],
            )
            .await
            .map_err(db_err)?,
        )?;
        if current.match_state == MatchState::Completed {
            return if current.replay_cid_blake3 == row.replay_cid_blake3 {
                Ok(())
            } else {
                Err(crate::transactions::conflict("不同终局已结算"))
            };
        }
        if current.match_state.is_terminal() || row.game_mode != "PvP_Ranked" || parts.len() != 2 {
            return Err(crate::transactions::conflict("终局无效"));
        }
        let mut old = Vec::new();
        for p in &parts {
            let valid:bool=tx.query_one("SELECT EXISTS(SELECT 1 FROM match_participants WHERE match_id=$1::text::uuid AND faction_id=$2 AND principal_id=$3)",&[&uuid_text(&row.match_id),&p.faction_id,&i(p.principal_id)]).await.map_err(db_err)?.get(0);
            if !valid || p.match_id != row.match_id {
                return Err(crate::transactions::conflict("终局席位无效"));
            }
            let r=tx.query_opt("SELECT principal_id,league,rating,deviation,volatility,matches_played FROM glicko2_ratings WHERE principal_id=$1 AND league=$2 FOR UPDATE",&[&i(p.principal_id),&p.controller_league.as_str()]).await.map_err(db_err)?;
            old.push(
                r.as_ref()
                    .map(rating_of)
                    .transpose()?
                    .unwrap_or_else(|| RatingRow::fresh(p.principal_id, p.controller_league)),
            );
        }
        for r in ratings_for(&row, &mut parts, &old) {
            tx.execute("INSERT INTO glicko2_ratings(principal_id,league,rating,deviation,volatility,matches_played) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(principal_id,league) DO UPDATE SET rating=EXCLUDED.rating,deviation=EXCLUDED.deviation,volatility=EXCLUDED.volatility,matches_played=EXCLUDED.matches_played,updated_at=NOW()",&[&i(r.principal_id),&r.league.as_str(),&r.rating,&r.deviation,&r.volatility,&r.matches_played]).await.map_err(db_err)?;
        }
        for p in parts {
            tx.execute("UPDATE match_participants SET final_score=$3,rating_delta=$4 WHERE match_id=$1::text::uuid AND faction_id=$2",&[&uuid_text(&row.match_id),&p.faction_id,&p.final_score,&p.rating_delta]).await.map_err(db_err)?;
        }
        tx.execute("UPDATE matches SET match_state='Completed',winner_faction_id=$2,win_reason=$3,total_ticks=$4,replay_cid_blake3=$5,started_at=COALESCE(started_at,to_timestamp($6::bigint)),completed_at=to_timestamp($6::bigint) WHERE match_id=$1::text::uuid",&[&uuid_text(&row.match_id),&row.winner_faction_id,&row.win_reason,&i(row.total_ticks),&row.replay_cid_blake3.as_ref().map(|c|c.as_slice()),&row.completed_at_unix_s.map(i)]).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    pub async fn ranked_queue(&self, req: QueueCommand, genesis: Option<RankedGenesis>) -> StoreResult<QueueDecision> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        lock_seats(&tx).await?;
        let pid = req.ticket.entry.principal_id;
        expire_reservations(&tx).await?;
        // 与注销更新锁同一主体行；等待中的旧队列记录不再获得新席位。
        let status = tx
            .query_opt(
                "SELECT status FROM principals WHERE principal_id=$1 FOR SHARE",
                &[&i(pid)],
            )
            .await
            .map_err(db_err)?;
        if !status.is_some_and(|r| r.get::<_, i16>(0) == 0) {
            return Err(ApiError::forbidden("主体已失效"));
        }
        if let Some(r)=tx.query_opt("SELECT m.match_id::text,m.game_mode FROM matches m JOIN match_participants p USING(match_id) WHERE p.principal_id=$1 AND m.match_state NOT IN ('Completed','Aborted') ORDER BY m.match_id LIMIT 1",&[&i(pid)]).await.map_err(db_err)? {
            if r.get::<_,String>(1)!="PvP_Ranked" {return Err(occupied_error())}
            let id=parse_uuid(r.get(0))?;tx.commit().await.map_err(db_err)?;return Ok(QueueDecision::Matched(id))
        }
        let rows=tx.query("SELECT q.ticket_json::text,p.status,EXISTS(SELECT 1 FROM match_participants mp JOIN matches m USING(match_id) WHERE mp.principal_id=q.principal_id AND m.match_state NOT IN ('Completed','Aborted')) FROM ranked_queue_tickets q JOIN principals p USING(principal_id) WHERE q.active ORDER BY q.principal_id FOR SHARE OF p",&[]).await.map_err(db_err)?;
        let mut previous = Vec::new();
        let mut tickets = Vec::new();
        for r in rows {
            let ticket: QueueTicket = serde_json::from_str(&r.get::<_, String>(0)).map_err(|_| corrupt("排位队列"))?;
            previous.push(ticket.clone());
            if r.get::<_, i16>(1) == 0 && !r.get::<_, bool>(2) {
                tickets.push(ticket);
            }
        }
        let (mut decision, pair) = decide(&mut tickets, &req, genesis.is_some());
        if let Some(pair) = pair {
            let mut g = genesis.expect("首提交已准备");
            g.bind(&pair);
            insert_world_tx(&tx, &g.world, &g.commit).await?;
            insert_match_tx(&tx, &g.row, &g.participants(&pair)).await?;
            tx.execute("INSERT INTO ranked_match_allocations(match_id,join_deadline_s) VALUES($1::text::uuid,floor(extract(epoch FROM clock_timestamp()))::bigint+$2)",&[&uuid_text(&g.row.match_id),&i(READY_TIMEOUT_S)]).await.map_err(db_err)?;
            decision = QueueDecision::Matched(g.row.match_id);
        }
        if req.action == QueueAction::Read {
            tx.commit().await.map_err(db_err)?;
            return Ok(decision);
        }
        for old in &previous {
            if !tickets.iter().any(|t| t.entry.principal_id == old.entry.principal_id) {
                tx.execute(
                    "UPDATE ranked_queue_tickets SET active=FALSE WHERE principal_id=$1",
                    &[&i(old.entry.principal_id)],
                )
                .await
                .map_err(db_err)?;
            }
        }
        for t in tickets {
            if !previous.contains(&t) {
                tx.execute("INSERT INTO ranked_queue_tickets(principal_id,ticket_json,active) VALUES($1,$2::text::jsonb,TRUE) ON CONFLICT(principal_id) DO UPDATE SET ticket_json=EXCLUDED.ticket_json,active=TRUE",&[&i(t.entry.principal_id),&serde_json::to_string(&t).expect("队列")]).await.map_err(db_err)?;
            }
        }
        tx.commit().await.map_err(db_err)?;
        Ok(decision)
    }
    pub async fn ranked_host(&self, world: WireUuid) -> StoreResult<Option<HostLease>> {
        self.client
            .lock()
            .await
            .query_opt(
                &format!("SELECT {HOST_COLS} FROM ranked_host_leases WHERE world_id=$1::text::uuid"),
                &[&uuid_text(&world)],
            )
            .await
            .map_err(db_err)?
            .as_ref()
            .map(host_of)
            .transpose()
    }
    pub async fn claim_ranked_host(&self, c: HostClaim) -> StoreResult<HostLease> {
        let mut client = self.client.lock().await;
        let tx = client.transaction().await.map_err(db_err)?;
        let row = tx
            .query_one(
                "SELECT latest_revision,fencing_token,game_mode FROM worlds WHERE world_id=$1::text::uuid FOR UPDATE",
                &[&uuid_text(&c.world)],
            )
            .await
            .map_err(db_err)?;
        let now: u64 = u(tx
            .query_one("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint", &[])
            .await
            .map_err(db_err)?
            .get(0));
        let active:bool=tx.query_one("SELECT EXISTS(SELECT 1 FROM matches WHERE world_id=$1::text::uuid AND match_state NOT IN ('Completed','Aborted'))",&[&uuid_text(&c.world)]).await.map_err(db_err)?.get(0);
        if !active || row.get::<_, String>(2) != "PvP_Ranked" {
            return Err(crate::transactions::conflict("排位已终局"));
        }
        if let Some(r) = tx
            .query_opt(
                &format!("SELECT {HOST_COLS} FROM ranked_host_leases WHERE world_id=$1::text::uuid"),
                &[&uuid_text(&c.world)],
            )
            .await
            .map_err(db_err)?
        {
            let h = host_of(&r)?;
            if h.expires_at_s > now && h.fence == u(row.get(1)) {
                return Ok(h);
            }
        }
        if u(row.get(0)) != c.revision || u(row.get(1)) != c.fence || c.fence >= i64::MAX as u64 {
            return Err(crate::transactions::conflict("恢复基线过期，请刷新"));
        }
        let h = HostLease {
            world_id: c.world,
            node: c.node,
            origin: c.origin,
            fence: c.fence + 1,
            expires_at_s: now + HOST_TTL_S,
        };
        tx.execute(
            "UPDATE worlds SET fencing_token=$2,lease_holder_node=$3 WHERE world_id=$1::text::uuid",
            &[&uuid_text(&c.world), &i(h.fence), &h.node],
        )
        .await
        .map_err(db_err)?;
        tx.execute("INSERT INTO ranked_host_leases(world_id,node_id,public_origin,fencing_token,expires_at_s) VALUES($1::text::uuid,$2,$3,$4,$5) ON CONFLICT(world_id) DO UPDATE SET node_id=EXCLUDED.node_id,public_origin=EXCLUDED.public_origin,fencing_token=EXCLUDED.fencing_token,expires_at_s=EXCLUDED.expires_at_s",&[&uuid_text(&c.world),&h.node,&h.origin,&i(h.fence),&i(h.expires_at_s)]).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(h)
    }
    pub async fn renew_ranked_host(&self, world: WireUuid, fence: u64) -> StoreResult<bool> {
        // 和认领/提交同样先锁世界行，防止旧宿主在新fence发布后续租成功。
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        tx.query_one(
            "SELECT world_id::text FROM worlds WHERE world_id=$1::text::uuid FOR UPDATE",
            &[&uuid_text(&world)],
        )
        .await
        .map_err(db_err)?;
        let n=tx.execute("UPDATE ranked_host_leases h SET expires_at_s=floor(extract(epoch FROM clock_timestamp()))::bigint+$3 FROM worlds w WHERE h.world_id=$1::text::uuid AND w.world_id=h.world_id AND w.fencing_token=$2 AND h.fencing_token=$2 AND h.expires_at_s>floor(extract(epoch FROM clock_timestamp()))::bigint AND EXISTS(SELECT 1 FROM matches m WHERE m.world_id=w.world_id AND m.match_state NOT IN ('Completed','Aborted'))",&[&uuid_text(&world),&i(fence),&i(HOST_TTL_S)]).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(n == 1)
    }
    pub async fn release_ranked_hosts(&self, node: String) -> StoreResult<()> {
        let mut c = self.client.lock().await;
        let tx = c.transaction().await.map_err(db_err)?;
        // 锁序保持 world -> lease，与 renew/claim 相同。
        tx.query("SELECT w.world_id::text FROM worlds w JOIN ranked_host_leases h USING(world_id) WHERE h.node_id=$1 ORDER BY w.world_id FOR UPDATE OF w",&[&node]).await.map_err(db_err)?;
        tx.execute("UPDATE worlds w SET fencing_token=w.fencing_token+1,lease_holder_node=NULL FROM ranked_host_leases h WHERE w.world_id=h.world_id AND h.node_id=$1 AND w.fencing_token=h.fencing_token",&[&node]).await.map_err(db_err)?;
        tx.execute(
            "UPDATE ranked_host_leases SET expires_at_s=0 WHERE node_id=$1",
            &[&node],
        )
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }
}

async fn expire_reservations(tx: &tokio_postgres::Transaction<'_>) -> StoreResult<()> {
    let rows=tx.query("SELECT a.match_id::text,m.world_id::text FROM ranked_match_allocations a JOIN matches m USING(match_id) WHERE a.open AND a.join_deadline_s<=floor(extract(epoch FROM clock_timestamp()))::bigint ORDER BY m.world_id",&[]).await.map_err(db_err)?;
    for r in rows {
        let id: String = r.get(0);
        let world: String = r.get(1);
        tx.query_one(
            "SELECT world_id::text FROM worlds WHERE world_id=$1::text::uuid FOR UPDATE",
            &[&world],
        )
        .await
        .map_err(db_err)?;
        let n=tx.execute("UPDATE matches m SET match_state='Aborted',completed_at=NOW() FROM worlds w WHERE m.match_id=$1::text::uuid AND w.world_id=m.world_id AND w.latest_revision=0 AND m.match_state IN ('Created','Lobby','Ready')",&[&id]).await.map_err(db_err)?;
        if n == 1 {
            tx.execute(
                "UPDATE worlds SET fencing_token=fencing_token+1 WHERE world_id=$1::text::uuid",
                &[&world],
            )
            .await
            .map_err(db_err)?;
        }
        tx.execute(
            "UPDATE ranked_match_allocations SET open=FALSE WHERE match_id=$1::text::uuid",
            &[&id],
        )
        .await
        .map_err(db_err)?;
    }
    Ok(())
}
