// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 端到端：质询登录 → 签发 96 字节令牌 → 线上解码验签 → RBAC/ABAC 授权 → 注销吊销。

use std::collections::BTreeMap;

use rynvek_protocol::CapabilityScope;
use rynvek_security::{
    AuthChallenges, OrgRole, PrincipalKindCode, PrincipalStatus, Resource, ServerSigningKey, Subject, TokenAuthority,
    Visibility, authorize, challenge_message,
};

#[test]
fn login_issue_verify_authorize_and_revoke() {
    let player = ServerSigningKey::from_seed(0, [0x11; 32]);
    let mut challenges = AuthChallenges::with_secret([0x22; 32]);
    let mut authority = TokenAuthority::new(ServerSigningKey::from_seed(1, [0x33; 32]));

    let now = 1_800_000_000;
    let challenge = challenges.issue(501, now);
    let signature = player.sign(&challenge_message(501, &challenge));
    challenges
        .redeem(501, &challenge, &player.public_key(), &signature, now + 5)
        .unwrap();
    let scopes = PrincipalKindCode::Human.default_scopes();
    let wire = authority.issue(501, scopes, now, 3_600).encode();

    let verified = authority.verify_bytes(&wire, now + 10).unwrap();
    let subject = Subject {
        principal_id: verified.principal_id(),
        kind: PrincipalKindCode::Human,
        status: PrincipalStatus::Active,
        scopes: verified.scopes(),
        memberships: BTreeMap::from([(7, OrgRole::Member)]),
    };
    let world = Resource::World {
        owner_principal_id: 900,
        org_id: 7,
        visibility: Visibility::Private,
    };
    authorize(&subject, CapabilityScope::WorldConnect, &world).unwrap();
    authorize(&subject, CapabilityScope::WorldEdit, &world).unwrap();

    authority.revoke_principal(501);
    assert_eq!(authority.verify_bytes(&wire, now + 11).unwrap_err().code.code(), 0x8001);
}
