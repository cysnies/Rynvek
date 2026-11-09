// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-security
//!
//! Rynvek 的 Tier 3 安全层（D350–D369 RBAC + ABAC、D645–D657 反作弊五防线、D130–D140 Ed25519）：
//!
//! - [`keys`]：Ed25519 服务端签名密钥、按 `signer_key_id` 轮转的公钥环与验签；
//! - [`token`]：96 字节 [`CapabilityToken`](rynvek_protocol::CapabilityToken) 的签发、验签与吊销；
//! - [`challenge`]：`/api/v1/auth/session` 的 Ed25519 质询-应答登录；
//! - [`authz`]：Capability（令牌权限位）→ RBAC（组织角色）→ ABAC（资源属性）三段式授权决策；
//! - [`anticheat`]：命令闸门（身份绑定、Tick 时间窗、幂等键、突发速率）与行为审计。
//!
//! 本 crate 只依赖 Tier 0（`rynvek-core`、`rynvek-protocol`），不感知任何游戏规则；
//! 所有拒绝都映射到规范 11.4 的 [`RynvekErrorCode`](rynvek_core::error::RynvekErrorCode) 稳定码。

#![deny(unsafe_code)]

pub mod authz;
pub mod challenge;
pub mod error;
pub mod keys;
pub mod token;

pub use authz::{League, OrgRole, PrincipalKindCode, PrincipalStatus, Resource, Subject, Visibility, authorize};
pub use challenge::{AUTH_CHALLENGE_TTL_S, AuthChallenges, challenge_message};
pub use error::SecurityError;
pub use keys::{Keyring, ServerSigningKey, from_hex, to_hex, verify_ed25519};
pub use token::{TokenAuthority, VerifiedToken};
