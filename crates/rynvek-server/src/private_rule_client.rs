// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! CLI共用的私有规则客户端；可信公钥由调用者提供，错误不回显令牌/签名秘密。
use crate::{
    client,
    rule_package::{self as rp, ExactRuleRef, RuleInstallRequest, RuleReleaseManifest, SignedRuleRelease},
};
use base64::Engine as _;
use rynvek_security::ServerSigningKey;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub struct PrivateRuleClient {
    base: String,
    owner: u64,
    key: [u8; 32],
    token: String,
}
impl std::fmt::Debug for PrivateRuleClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivateRuleClient")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}
impl PrivateRuleClient {
    /// 原极简HTTP传输无TLS，新命令因此只接受明确回环origin，不能静默明文带凭据出站。
    pub fn new(base: &str, owner: u64, key: [u8; 32], token: String) -> Result<Self, String> {
        let base = base.trim_end_matches('/');
        let authority = base.strip_prefix("http://").ok_or("当前CLI规则传输仅支持回环http")?;
        let (host, port) = authority.rsplit_once(':').ok_or("须明确回环端口")?;
        if !matches!(host, "127.0.0.1" | "[::1]")
            || port.parse::<u16>().is_err()
            || token.is_empty()
            || token.contains(['\r', '\n'])
            || owner == 0
        {
            return Err("规则客户端origin/身份/令牌格式不符".into());
        }
        Ok(Self {
            base: base.into(),
            owner,
            key,
            token,
        })
    }
    async fn req(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        max: usize,
    ) -> Result<client::HttpResponse, String> {
        let auth = format!("Bearer {}", self.token);
        let body = body.map(|v| serde_json::to_vec(v).expect("定型请求"));
        let r = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            client::request_limited(
                method,
                &format!("{}{path}", self.base),
                &[("Authorization", &auth), ("Content-Type", "application/json")],
                body.as_deref(),
                max,
            ),
        )
        .await
        .map_err(|_| "私有规则请求超时")?
        .map_err(|_| "私有规则请求传输失败".to_string())?;
        if !(200..300).contains(&r.status) {
            return Err(format!("私有规则请求拒绝 HTTP {}，未确认执行", r.status));
        }
        Ok(r)
    }
    fn path(r: &ExactRuleRef) -> Result<String, String> {
        rp::validate_ref(r).map_err(|_| "精确规则引用非法")?;
        Ok(format!("/api/v1/private-hub/rules/{}/{}", r.id, r.version))
    }
    fn metadata(&self, v: &Value) -> Result<SignedRuleRelease, String> {
        if v["visibility"] != "Private" || v["revoked"] != false {
            return Err("规则私有元数据不可消费".into());
        }
        let r: SignedRuleRelease = serde_json::from_value(v["release"].clone()).map_err(|_| "规则签名元数据非法")?;
        if r.manifest.owner_id != self.owner.to_string() || r.publisher_key != crate::ids::hex(&self.key) {
            return Err("规则登记身份与可信公钥不符".into());
        }
        Ok(r)
    }
    pub async fn list(&self) -> Result<Value, String> {
        self.req("GET", "/api/v1/private-hub/rules", None, 512 * 1024)
            .await?
            .json()
            .map_err(|_| "发现响应非法".into())
    }
    pub async fn publish(
        &self,
        bytes: &[u8],
        mut deps: Vec<ExactRuleRef>,
        signer: &ServerSigningKey,
    ) -> Result<SignedRuleRelease, String> {
        if signer.public_key() != self.key {
            return Err("签名钥匙不等于可信登记钥".into());
        }
        let p = rynvek_rules::package::Tier0RulePackage::parse(bytes).map_err(|_| "本地RYPK/IR复验失败")?;
        deps.sort_by(|a, b| a.id.cmp(&b.id));
        let m = RuleReleaseManifest {
            format: "rynvek-private-rule-release/1".into(),
            owner_id: self.owner.to_string(),
            package_id: p.id,
            version: p.version,
            artifact_cid: rynvek_core::Blake3Hash::of(bytes).to_hex(),
            dependencies: deps,
        };
        let msg = rp::signing_message(&m, bytes).map_err(|_| "签名精确锁非法")?;
        let body = json!({"manifest":m,"artifact_b64":base64::engine::general_purpose::STANDARD.encode(bytes),"signature":crate::ids::hex(&signer.sign(&msg))});
        let v = self
            .req("PUT", &Self::path(&m.reference())?, Some(&body), 64 * 1024)
            .await?
            .json()
            .map_err(|_| "发布响应非法")?;
        let release = self.metadata(&v)?;
        if release.manifest != m {
            return Err("发布回执清单不一致".into());
        }
        rp::verify_download(self.owner, &self.key, &release, bytes).map_err(|_| "发布回执签名不符")?;
        Ok(release)
    }
    /// 不信resolved自报闭包：按每层已验签锁遍历，下载后共用唯一Rust包/编译复验器。
    pub async fn download(&self, root: &ExactRuleRef) -> Result<Vec<(SignedRuleRelease, Vec<u8>)>, String> {
        let mut stack = vec![(root.clone(), 0, false)];
        let mut seen = BTreeMap::new();
        let mut gray = BTreeSet::new();
        let mut done = BTreeSet::new();
        let mut out = BTreeMap::new();
        let mut size = 0;
        while let Some((pin, depth, finish)) = stack.pop() {
            if finish {
                gray.remove(&pin.id);
                done.insert(pin.id);
                continue;
            }
            if depth > 8 || seen.get(&pin.id).is_some_and(|p| p != &pin) {
                return Err("下载锁冲突/深度超限".into());
            }
            if done.contains(&pin.id) {
                continue;
            }
            if !gray.insert(pin.id.clone()) {
                return Err("下载依赖环".into());
            }
            seen.insert(pin.id.clone(), pin.clone());
            if seen.len() > 16 {
                return Err("下载包数超限".into());
            }
            let path = Self::path(&pin)?;
            let v = self
                .req("GET", &path, None, 64 * 1024)
                .await?
                .json()
                .map_err(|_| "元数据JSON非法")?;
            let release = self.metadata(&v)?;
            if release.manifest.reference() != pin {
                return Err("精确下载身份/CID不符".into());
            }
            let bytes = self
                .req("GET", &format!("{path}/artifact"), None, rp::MAX_CLOSURE_BYTES)
                .await?
                .body;
            size += bytes.len();
            if size > rp::MAX_CLOSURE_BYTES {
                return Err("下载闭包超过1MiB".into());
            }
            rp::verify_download(self.owner, &self.key, &release, &bytes).map_err(|_| "下载登记签名/CID/图IR不符")?;
            stack.push((pin.clone(), depth, true));
            for d in release.manifest.dependencies.iter().rev() {
                stack.push((d.clone(), depth + 1, false));
            }
            out.insert(pin.id, (release, bytes));
        }
        Ok(out.into_values().collect())
    }
    pub async fn observe(&self, world: &str) -> Result<Value, String> {
        let _: rynvek_protocol::WireUuid = world.parse().map_err(|_| "世界UUID非法")?;
        self.req("GET", &format!("/api/v1/worlds/{world}/general"), None, 256 * 1024)
            .await?
            .json()
            .map_err(|_| "观察响应非法".into())
    }
    pub async fn control(&self, world: &str, body: &Value) -> Result<Value, String> {
        let _: rynvek_protocol::WireUuid = world.parse().map_err(|_| "世界UUID非法")?;
        self.req(
            "POST",
            &format!("/api/v1/worlds/{world}/general"),
            Some(body),
            256 * 1024,
        )
        .await?
        .json()
        .map_err(|_| "控制回执非法".into())
    }
    /// 不获取lease、不自动推进Tick；调用者必须显式给原fence/时序/幂等键。
    pub async fn install(
        &self,
        world: &str,
        host: u64,
        control: u64,
        tick: u64,
        key: rynvek_protocol::IdempotencyKey,
        request: RuleInstallRequest,
        recover: bool,
    ) -> Result<Value, String> {
        let packages = self.download(&request.root).await?;
        if packages.len() != request.modules.len()
            || packages
                .iter()
                .any(|(r, _)| !request.modules.contains_key(&r.manifest.package_id))
            || request.modules.values().collect::<BTreeSet<_>>().len() != request.modules.len()
        {
            return Err("安装映射未精确覆盖可信下载闭包".into());
        }
        let mut input = crate::general_host::GeneralInput {
            host_fence: host,
            control_fence: control,
            envelope: rynvek_protocol::CommandEnvelope {
                idempotency_key: key,
                principal_id: self.owner,
                issued_at_tick: tick,
                target_tick: tick.checked_add(1).ok_or("Tick溢出")?,
                payload: vec![],
            },
            install: None,
            recovery: None,
        };
        if recover {
            input.recovery = Some(request);
        } else {
            input.install = Some(request);
        }
        self.control(
            world,
            &json!({"action":if recover{"recover"}else{"install"},"input":input}),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_http_requires_literal_loopback_and_debug_redacts_token() {
        for origin in [
            "http://example.com:80",
            "http://127.0.0.1:80/elsewhere",
            "http://127.0.0.1:80@evil:80",
            "https://127.0.0.1:80",
        ] {
            assert!(PrivateRuleClient::new(origin, 1, [0; 32], "test-token".into()).is_err());
        }
        let client =
            PrivateRuleClient::new("http://127.0.0.1:80", 1, [0; 32], "never-print-test-token".into()).unwrap();
        assert!(!format!("{client:?}").contains("never-print"));
        assert!(PrivateRuleClient::new("http://127.0.0.1:80", 1, [0; 32], "a\r\nx:b".into()).is_err());
    }
}
