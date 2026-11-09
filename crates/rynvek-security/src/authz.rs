// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! RBAC + ABAC + Capability 三段式授权（D350–D369）。
//!
//! 一次请求依次通过：
//!
//! 1. **Capability**：令牌权限位必须包含路由所需的 [`CapabilityScope`]（规范 11.3 路由表第 4 列）；
//! 2. **主体状态**：`principals.status` 必须为 `Active`（`Suspended / Deleted` 一律拒绝）；
//! 3. **RBAC + ABAC**：按组织角色（[`OrgRole`]）与资源属性（所有者、可见性、赛道、命名空间）判定。
//!
//! 规范只冻结了权限名称；角色阶梯与各权限的属性规则（见 [`authorize`] 内的表）为本实现的最佳实践补全。
//! 所有拒绝统一为 `0x8001 UnauthorizedCapabilityToken`，不区分 "不存在" 与 "无权"，避免枚举探测。

use std::collections::BTreeMap;

use rynvek_protocol::{CapabilityScope, CapabilityScopeSet};
use serde::{Deserialize, Serialize};

use crate::error::SecurityError;

/// `principals.principal_kind`（DDL：`0 Human / 1 Bot·Agent / 2 Service`）
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PrincipalKindCode {
    Human = 0,
    Agent = 1,
    Service = 2,
}

impl PrincipalKindCode {
    pub fn from_i16(v: i16) -> Option<Self> {
        match v {
            0 => Some(Self::Human),
            1 => Some(Self::Agent),
            2 => Some(Self::Service),
            _ => None,
        }
    }

    /// 登录时签发的缺省权限集：Agent 只拿到对战与只读权限，账户、发布、创作类权限留给人类与服务
    pub fn default_scopes(self) -> CapabilityScopeSet {
        use CapabilityScope::*;
        match self {
            Self::Human | Self::Service => CapabilityScope::ALL.into_iter().collect(),
            Self::Agent => [WorldList, WorldRead, WorldConnect, MatchPlay, LabRead]
                .into_iter()
                .collect(),
        }
    }
}

/// `principals.status`（DDL：`0 Active / 1 Suspended / 2 Deleted`）
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrincipalStatus {
    Active = 0,
    Suspended = 1,
    Deleted = 2,
}

impl PrincipalStatus {
    pub fn from_i16(v: i16) -> Option<Self> {
        match v {
            0 => Some(Self::Active),
            1 => Some(Self::Suspended),
            2 => Some(Self::Deleted),
            _ => None,
        }
    }
}

/// 组织内角色（自低到高）
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum OrgRole {
    Viewer,
    Member,
    Admin,
    Owner,
}

/// `worlds.visibility`（DDL：`0 Private / 1 Unlisted / 2 Public`）
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Visibility {
    Private = 0,
    Unlisted = 1,
    Public = 2,
}

impl Visibility {
    pub fn from_i16(v: i16) -> Option<Self> {
        match v {
            0 => Some(Self::Private),
            1 => Some(Self::Unlisted),
            2 => Some(Self::Public),
            _ => None,
        }
    }
}

/// 天梯赛道（DDL：`'human' | 'script' | 'agent_hybrid'`）
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum League {
    Human,
    Script,
    AgentHybrid,
}

impl League {
    pub const ALL: [Self; 3] = [Self::Human, Self::Script, Self::AgentHybrid];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Script => "script",
            Self::AgentHybrid => "agent_hybrid",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.as_str() == s)
    }

    /// 某类主体能否参加该赛道：人类赛道只收人类，脚本赛道只收 Bot，混合赛道两者皆可
    pub const fn admits(self, kind: PrincipalKindCode) -> bool {
        matches!(
            (self, kind),
            (Self::Human, PrincipalKindCode::Human)
                | (Self::Script, PrincipalKindCode::Agent)
                | (Self::AgentHybrid, PrincipalKindCode::Human | PrincipalKindCode::Agent)
        )
    }
}

/// 请求主体（已验签令牌 + 数据库中的身份属性）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subject {
    pub principal_id: u64,
    pub kind: PrincipalKindCode,
    pub status: PrincipalStatus,
    pub scopes: CapabilityScopeSet,
    /// 组织 ID → 角色
    pub memberships: BTreeMap<u64, OrgRole>,
}

impl Subject {
    pub fn role_in(&self, org_id: u64) -> Option<OrgRole> {
        self.memberships.get(&org_id).copied()
    }

    fn has_role(&self, org_id: u64, at_least: OrgRole) -> bool {
        self.role_in(org_id).is_some_and(|r| r >= at_least)
    }
}

/// 被访问的资源及其属性（ABAC 输入）
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resource {
    /// 组织级集合（创建世界、列出世界）
    Org { org_id: u64 },
    /// 一个世界实例
    World {
        owner_principal_id: u64,
        org_id: u64,
        visibility: Visibility,
    },
    /// 天梯赛道
    League(League),
    /// Hub 包命名空间
    Package {
        package_id: String,
        org_id: u64,
        /// 组织是否为官方组织（`rynvek.*` 命名空间的唯一合法发布者，TrustLevel 1）
        org_is_official: bool,
    },
    /// 实验任务
    Experiment { owner_principal_id: u64 },
    /// 账户本身
    Account { principal_id: u64 },
}

/// 授权决策
pub fn authorize(subject: &Subject, scope: CapabilityScope, resource: &Resource) -> Result<(), SecurityError> {
    if !subject.scopes.contains(scope) {
        return Err(SecurityError::unauthorized(format!("令牌缺少权限 `{scope}`")));
    }
    if subject.status != PrincipalStatus::Active {
        return Err(SecurityError::unauthorized("Principal 已被停用或注销"));
    }
    let readable = |owner: u64, org: u64, vis: Visibility| {
        owner == subject.principal_id || vis >= Visibility::Unlisted || subject.has_role(org, OrgRole::Viewer)
    };
    let allowed = match (scope, resource) {
        (CapabilityScope::WorldList, Resource::Org { .. }) => true,
        (CapabilityScope::WorldCreate, Resource::Org { org_id }) => subject.has_role(*org_id, OrgRole::Member),
        (
            CapabilityScope::WorldRead | CapabilityScope::WorldConnect | CapabilityScope::WorldFork,
            Resource::World {
                owner_principal_id,
                org_id,
                visibility,
            },
        ) => readable(*owner_principal_id, *org_id, *visibility),
        (
            CapabilityScope::WorldEdit,
            Resource::World {
                owner_principal_id,
                org_id,
                ..
            },
        ) => *owner_principal_id == subject.principal_id || subject.has_role(*org_id, OrgRole::Member),
        (CapabilityScope::MatchPlay, Resource::League(league)) => league.admits(subject.kind),
        (
            CapabilityScope::HubPublish,
            Resource::Package {
                package_id,
                org_id,
                org_is_official,
            },
        ) => subject.has_role(*org_id, OrgRole::Admin) && (!package_id.starts_with("rynvek.") || *org_is_official),
        (CapabilityScope::LabSubmit, Resource::Org { org_id }) => subject.has_role(*org_id, OrgRole::Member),
        (CapabilityScope::LabRead, Resource::Experiment { owner_principal_id }) => {
            *owner_principal_id == subject.principal_id
        }
        (CapabilityScope::AccountOwner, Resource::Account { principal_id }) => *principal_id == subject.principal_id,
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(SecurityError::unauthorized(format!("无权对该资源执行 `{scope}`")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(kind: PrincipalKindCode, orgs: &[(u64, OrgRole)]) -> Subject {
        Subject {
            principal_id: 10,
            kind,
            status: PrincipalStatus::Active,
            scopes: kind.default_scopes(),
            memberships: orgs.iter().copied().collect(),
        }
    }

    fn world(owner: u64, org: u64, visibility: Visibility) -> Resource {
        Resource::World {
            owner_principal_id: owner,
            org_id: org,
            visibility,
        }
    }

    #[test]
    fn capability_bit_is_checked_first() {
        let mut s = subject(PrincipalKindCode::Human, &[(1, OrgRole::Owner)]);
        s.scopes = CapabilityScopeSet::EMPTY;
        let e = authorize(&s, CapabilityScope::WorldCreate, &Resource::Org { org_id: 1 }).unwrap_err();
        assert_eq!(e.code.code(), 0x8001);
    }

    #[test]
    fn world_visibility_and_roles() {
        let outsider = subject(PrincipalKindCode::Human, &[]);
        let viewer = subject(PrincipalKindCode::Human, &[(1, OrgRole::Viewer)]);
        let member = subject(PrincipalKindCode::Human, &[(1, OrgRole::Member)]);
        let private = world(99, 1, Visibility::Private);
        assert!(authorize(&outsider, CapabilityScope::WorldRead, &private).is_err());
        assert!(
            authorize(
                &outsider,
                CapabilityScope::WorldRead,
                &world(99, 1, Visibility::Unlisted)
            )
            .is_ok()
        );
        assert!(authorize(&viewer, CapabilityScope::WorldRead, &private).is_ok());
        assert!(authorize(&viewer, CapabilityScope::WorldEdit, &private).is_err());
        assert!(authorize(&member, CapabilityScope::WorldEdit, &private).is_ok());
        assert!(
            authorize(
                &outsider,
                CapabilityScope::WorldEdit,
                &world(10, 5, Visibility::Private)
            )
            .is_ok(),
            "所有者可编辑"
        );
        assert!(authorize(&viewer, CapabilityScope::WorldCreate, &Resource::Org { org_id: 1 }).is_err());
        assert!(authorize(&member, CapabilityScope::WorldCreate, &Resource::Org { org_id: 1 }).is_ok());
    }

    #[test]
    fn league_admission_by_principal_kind() {
        let human = subject(PrincipalKindCode::Human, &[]);
        let bot = subject(PrincipalKindCode::Agent, &[]);
        let play = |s: &Subject, l| authorize(s, CapabilityScope::MatchPlay, &Resource::League(l)).is_ok();
        assert!(play(&human, League::Human));
        assert!(!play(&human, League::Script));
        assert!(play(&human, League::AgentHybrid));
        assert!(!play(&bot, League::Human), "Bot 不得混入人类赛道");
        assert!(play(&bot, League::Script));
        assert!(play(&bot, League::AgentHybrid));
    }

    #[test]
    fn agents_never_get_account_or_publish_scopes() {
        let bot = subject(PrincipalKindCode::Agent, &[(1, OrgRole::Owner)]);
        let pkg = Resource::Package {
            package_id: "community.x".into(),
            org_id: 1,
            org_is_official: false,
        };
        assert!(authorize(&bot, CapabilityScope::HubPublish, &pkg).is_err());
        assert!(
            authorize(
                &bot,
                CapabilityScope::AccountOwner,
                &Resource::Account { principal_id: 10 }
            )
            .is_err()
        );
    }

    #[test]
    fn official_namespace_and_account_ownership() {
        let admin = subject(PrincipalKindCode::Human, &[(1, OrgRole::Admin)]);
        let official = |id: &str, off| Resource::Package {
            package_id: id.into(),
            org_id: 1,
            org_is_official: off,
        };
        assert!(authorize(&admin, CapabilityScope::HubPublish, &official("community.pack", false)).is_ok());
        assert!(authorize(&admin, CapabilityScope::HubPublish, &official("rynvek.visual.x", false)).is_err());
        assert!(authorize(&admin, CapabilityScope::HubPublish, &official("rynvek.visual.x", true)).is_ok());
        assert!(
            authorize(
                &admin,
                CapabilityScope::AccountOwner,
                &Resource::Account { principal_id: 10 }
            )
            .is_ok()
        );
        assert!(
            authorize(
                &admin,
                CapabilityScope::AccountOwner,
                &Resource::Account { principal_id: 11 }
            )
            .is_err()
        );
    }

    #[test]
    fn suspended_principals_are_denied_everything() {
        let mut s = subject(PrincipalKindCode::Human, &[(1, OrgRole::Owner)]);
        s.status = PrincipalStatus::Suspended;
        assert!(authorize(&s, CapabilityScope::WorldList, &Resource::Org { org_id: 1 }).is_err());
    }

    #[test]
    fn mismatched_scope_and_resource_is_denied() {
        let s = subject(PrincipalKindCode::Human, &[(1, OrgRole::Owner)]);
        assert!(authorize(&s, CapabilityScope::WorldRead, &Resource::Org { org_id: 1 }).is_err());
    }
}
