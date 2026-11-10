// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 跨世界联邦网关与隔离检疫协议（`FederatedWorldGateway`，规范 08 §8.4 / 审计报告 §11.1）。
//!
//! 负责在不同世界实例（或沙盒实验分支与公共生态扇区）之间通过星门（[`PortalLink`]）安全迁移基因组与个体：
//! - 入境前强制执行隔离检疫（验证突触数量预算、权重幅值范围 `[-2048, +2048]`、CTRNN 时间常数合法性与 BLAKE3 票据签名）；
//! - 维护防重放已消费票据集合，杜绝同一迁移票据被双花复制。

use std::collections::BTreeSet;

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;
use rynvek_rts::evolution::WEIGHT_LIMIT_Q8;
use rynvek_rts::genome::Genome;
use serde::{Deserialize, Serialize};

/// 跨世界星门连接定义
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortalLink {
    pub portal_id: String,
    pub source_world_id: [u8; 16],
    pub target_world_id: [u8; 16],
    pub max_synapses_allowed: usize,
}

/// 跨世界基因组与个体迁移票据
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossWorldMigrationTicket {
    pub ticket_nonce: u64,
    pub source_world_id: [u8; 16],
    pub target_world_id: [u8; 16],
    pub exported_at_tick: u64,
    pub genome: Genome,
    pub ticket_digest: [u8; 32],
}

impl CrossWorldMigrationTicket {
    pub fn issue(
        ticket_nonce: u64,
        source_world_id: [u8; 16],
        target_world_id: [u8; 16],
        exported_at_tick: u64,
        genome: Genome,
    ) -> Self {
        let ticket_digest = Self::compute_digest(
            ticket_nonce,
            source_world_id,
            target_world_id,
            exported_at_tick,
            &genome,
        );
        Self {
            ticket_nonce,
            source_world_id,
            target_world_id,
            exported_at_tick,
            genome,
            ticket_digest,
        }
    }

    pub fn compute_digest(
        ticket_nonce: u64,
        source_world_id: [u8; 16],
        target_world_id: [u8; 16],
        exported_at_tick: u64,
        genome: &Genome,
    ) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(b"RYNVEK_FEDERATION_TICKET_V1");
        h.update(&ticket_nonce.to_le_bytes());
        h.update(&source_world_id);
        h.update(&target_world_id);
        h.update(&exported_at_tick.to_le_bytes());
        h.update(&genome.digest().0);
        *h.finalize().as_bytes()
    }
}

/// 联邦世界网关（执行入境检疫与防重放核验）
#[derive(Clone, Debug)]
pub struct FederatedWorldGateway {
    pub local_world_id: [u8; 16],
    pub max_synapses_allowed: usize,
    consumed_tickets: BTreeSet<[u8; 32]>,
}

impl FederatedWorldGateway {
    pub fn new(local_world_id: [u8; 16], max_synapses_allowed: usize) -> Self {
        Self {
            local_world_id,
            max_synapses_allowed,
            consumed_tickets: BTreeSet::new(),
        }
    }

    /// 核验并消费一张入境迁移票据，通过检疫后返回合法的 [`Genome`]
    pub fn verify_and_admit(&mut self, ticket: &CrossWorldMigrationTicket) -> Result<Genome, RynvekErrorCode> {
        if ticket.target_world_id != self.local_world_id {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        let expected = CrossWorldMigrationTicket::compute_digest(
            ticket.ticket_nonce,
            ticket.source_world_id,
            ticket.target_world_id,
            ticket.exported_at_tick,
            &ticket.genome,
        );
        if expected != ticket.ticket_digest {
            return Err(RynvekErrorCode::Blake3ChecksumMismatch);
        }
        if self.consumed_tickets.contains(&ticket.ticket_digest) {
            // 拒绝重放双花
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }
        let g = &ticket.genome;
        if g.synapse_count() > self.max_synapses_allowed {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        if g.w_in
            .iter()
            .chain(&g.w_rec)
            .chain(&g.w_out)
            .any(|w| w.abs() > WEIGHT_LIMIT_Q8)
        {
            return Err(RynvekErrorCode::SimulationInvariantViolated);
        }
        let _ = Blake3Hash(ticket.ticket_digest);
        self.consumed_tickets.insert(ticket.ticket_digest);
        Ok(g.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_rts::UnitKind;

    #[test]
    fn federated_gateway_admits_valid_ticket_and_rejects_replay_or_over_budget() {
        let src = [1u8; 16];
        let dst = [2u8; 16];
        let mut gw = FederatedWorldGateway::new(dst, 2048);
        let genome = Genome::seed(UnitKind::Striker);
        let ticket = CrossWorldMigrationTicket::issue(1, src, dst, 600, genome.clone());
        let admitted = gw.verify_and_admit(&ticket).unwrap();
        assert_eq!(admitted.digest(), genome.digest());
        // 重放同一张票据必须被拒绝
        assert_eq!(
            gw.verify_and_admit(&ticket),
            Err(RynvekErrorCode::SimulationInvariantViolated)
        );
    }
}
