// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! RTS 权威命令载荷与拒绝原因。
//!
//! 人类、脚本与 LLM Agent 发出的一切操作最终都归一为 [`RtsCommand`]，由 [`crate::world::ArenaWorld`]
//! 在 Tick 边界按提交顺序校验并执行；非法命令只拒绝该条（`ErrorRecoveryAction::RejectCommandOnly`），
//! 世界照常推进。
//!
//! 规范 11.5 只冻结了错误码数值，RTS 拒绝原因到错误码的映射为（ERRATA E-030）：
//!
//! | 拒绝原因 | 错误码 |
//! | --- | --- |
//! | 实体不存在 / 类型不符 | `0x2002 EntityNotFoundOrTombstoned` |
//! | 实体不属于本阵营、科技 / 前置未满足、站点忙、对局已结束 | `0x8001 UnauthorizedCapabilityToken` |
//! | 生产队列已满 | `0x8004 CommandRateLimitExceeded` |
//! | 资源或神经人口不足 | `0x8005 InsufficientFactionResources` |
//! | 建筑落点非法（越界、地形、重叠、不可见） | `0x8006 InvalidBuildingPlacementGrid` |
//! | 参数越界（数量、突变率等，与工具 Schema 校验同码） | `0x5001 RuleStaticTypeCheckFailed` |

use rynvek_core::RynvekErrorCode;
use serde::{Deserialize, Serialize};

use crate::ids::{BuildingKind, EntitySerial, FieldId, OrderKind, TechId, UnitKind};
use crate::math::Vec2;

/// 单位选择器
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum UnitSelector {
    /// 明确的实体列表
    Ids(Vec<EntitySerial>),
    /// 本阵营某原型的全部单位；`None` = 全部（工具参数 `"all"`）
    Archetype(Option<UnitKind>),
}

/// 坩埚动作
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrucibleAction {
    ResearchTech(TechId),
    /// 提取该原型的高适应度个体繁育；`mutation_rate_permille ∈ [10, 250]`
    BreedEliteGenome {
        unit: UnitKind,
        mutation_rate_permille: u32,
    },
}

/// 权威命令
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RtsCommand {
    /// 放置建筑（`origin` 为占地左下角 Tile）
    PlaceBuilding { kind: BuildingKind, origin: (u32, u32) },
    /// 拆除建筑（返还 50% 造价）
    Demolish { building: EntitySerial },
    /// 在孵化槽排队生产
    QueueUnit {
        vat: EntitySerial,
        unit: UnitKind,
        genome_slug: String,
        count: u32,
    },
    /// 取消孵化槽队尾一项（全额返还）
    CancelQueue { vat: EntitySerial },
    /// 下达移动 / 进攻 / 采集 / 驻守 / 巡逻指令
    IssueOrder {
        units: UnitSelector,
        order: OrderKind,
        target: Vec2,
    },
    /// 配置信标注入通道、强度（milli，`[-50000, 50000]`）与调质向量（milli，`[-1000, 1000]^4`）
    ConfigureBeacon {
        beacon: EntitySerial,
        channel: FieldId,
        strength_milli: i64,
        modulators_milli: [i64; 4],
    },
    /// 在精炼厂（Tier 1）研发科技
    ResearchTech { site: EntitySerial, tech: TechId },
    /// 演化坩埚动作
    Crucible {
        crucible: EntitySerial,
        action: CrucibleAction,
    },
    /// 投降
    Surrender,
}

/// 命令拒绝原因
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum RtsError {
    UnknownEntity { id: EntitySerial },
    WrongEntityKind { id: EntitySerial },
    NotOwner { id: EntitySerial },
    NotComplete { id: EntitySerial },
    TechLocked { unit: UnitKind },
    PrerequisitesUnmet { tech: TechId },
    AlreadyResearched { tech: TechId },
    SiteBusy { id: EntitySerial },
    UnknownGenome { slug: String },
    GenomeArchetypeMismatch { slug: String },
    QueueFull { id: EntitySerial },
    InsufficientResources,
    SupplyCapReached,
    InvalidPlacement { detail: String },
    NotBuildable { kind: BuildingKind },
    NoBreedingCandidates { unit: UnitKind },
    InvalidArgument { detail: String },
    MatchOver,
}

impl RtsError {
    /// 映射到跨语言稳定错误码
    pub const fn code(&self) -> RynvekErrorCode {
        match self {
            Self::UnknownEntity { .. } | Self::WrongEntityKind { .. } | Self::UnknownGenome { .. } => {
                RynvekErrorCode::EntityNotFoundOrTombstoned
            }
            Self::QueueFull { .. } => RynvekErrorCode::CommandRateLimitExceeded,
            Self::InsufficientResources | Self::SupplyCapReached => RynvekErrorCode::InsufficientFactionResources,
            Self::InvalidPlacement { .. } => RynvekErrorCode::InvalidBuildingPlacementGrid,
            Self::InvalidArgument { .. } => RynvekErrorCode::RuleStaticTypeCheckFailed,
            Self::NotOwner { .. }
            | Self::NotComplete { .. }
            | Self::TechLocked { .. }
            | Self::PrerequisitesUnmet { .. }
            | Self::AlreadyResearched { .. }
            | Self::SiteBusy { .. }
            | Self::GenomeArchetypeMismatch { .. }
            | Self::NotBuildable { .. }
            | Self::NoBreedingCandidates { .. }
            | Self::MatchOver => RynvekErrorCode::UnauthorizedCapabilityToken,
        }
    }
}

impl std::fmt::Display for RtsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} (0x{:04X})", self, self.code().code())
    }
}

impl std::error::Error for RtsError {}

/// 命令执行回执
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandAck {
    BuildingPlaced { id: EntitySerial },
    Demolished { id: EntitySerial },
    Queued { vat: EntitySerial, count: u32 },
    Cancelled { vat: EntitySerial },
    OrderIssued { units: u32 },
    BeaconConfigured { id: EntitySerial },
    ResearchStarted { site: EntitySerial, tech: TechId },
    BreedingStarted { crucible: EntitySerial, unit: UnitKind },
    Surrendered,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_follow_mapping_table() {
        assert_eq!(RtsError::InsufficientResources.code().code(), 0x8005);
        assert_eq!(
            RtsError::InvalidPlacement { detail: String::new() }.code().code(),
            0x8006
        );
        assert_eq!(RtsError::QueueFull { id: EntitySerial(1) }.code().code(), 0x8004);
        assert_eq!(RtsError::NotOwner { id: EntitySerial(1) }.code().code(), 0x8001);
        assert_eq!(RtsError::UnknownEntity { id: EntitySerial(1) }.code().code(), 0x2002);
        assert_eq!(RtsError::SupplyCapReached.code().code(), 0x8005);
    }

    #[test]
    fn commands_round_trip_through_json() {
        let cmds = vec![
            RtsCommand::PlaceBuilding {
                kind: BuildingKind::FluxReactor,
                origin: (40, 41),
            },
            RtsCommand::IssueOrder {
                units: UnitSelector::Archetype(None),
                order: OrderKind::AttackMove,
                target: Vec2::from_int(100, 100),
            },
            RtsCommand::Crucible {
                crucible: EntitySerial(9),
                action: CrucibleAction::BreedEliteGenome {
                    unit: UnitKind::Striker,
                    mutation_rate_permille: 80,
                },
            },
            RtsCommand::Surrender,
        ];
        for c in cmds {
            let json = serde_json::to_string(&c).unwrap();
            assert_eq!(serde_json::from_str::<RtsCommand>(&json).unwrap(), c, "{json}");
        }
        let json = serde_json::to_string(&RtsCommand::PlaceBuilding {
            kind: BuildingKind::NexusCore,
            origin: (1, 2),
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"type":"place_building","kind":"bld.nexus_core","origin":[1,2]}"#
        );
    }
}
