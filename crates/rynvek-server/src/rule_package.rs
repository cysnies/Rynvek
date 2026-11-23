// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 私有规则发布信封：复用原RYPK/图IR复验、Hub签名域及原私库/私有CAS。
//! 精确锁位于签名信封，不放宽Tier0三条目叶子构建剖面；仅表示显式共同安装的规则包组。
use crate::{
    agent_package::AgentDependency,
    app::AppState,
    error::{ApiError, ApiResult},
    ids::{hex, parse_hex},
    store::{PrivateArtifactRow, RulePermit},
    transactions::conflict,
};
use rynvek_core::{
    Blake3Hash,
    package::{is_valid_package_id, is_valid_version},
};
use rynvek_rules::package::Tier0RulePackage;
use rynvek_simulation::engine::SimulationEngine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_CLOSURE_BYTES: usize = 1024 * 1024;
pub const MAX_RELEASES: usize = 16;
pub type ExactRuleRef = AgentDependency;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleReleaseManifest {
    pub format: String,
    /// 使用十进制字符串，签名规范不依赖JS的浮点整数截断。
    pub owner_id: String,
    pub package_id: String,
    pub version: String,
    pub artifact_cid: String,
    pub dependencies: Vec<ExactRuleRef>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedRuleRelease {
    pub manifest: RuleReleaseManifest,
    pub publisher_key: String,
    pub signature: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleInstallRequest {
    pub root: ExactRuleRef,
    /// 包ID到已有绑定模块；必须精确覆盖整个依赖闭包且模块不能重复。
    pub modules: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleInstallEvidence {
    pub format: String,
    pub owner_id: String,
    pub request: RuleInstallRequest,
    pub releases: Vec<SignedRuleRelease>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleInstallReplay {
    pub evidence_cid: [u8; 32],
    pub graphs: BTreeMap<String, String>,
}
#[derive(Debug)]
pub struct ResolvedRules {
    pub releases: Vec<SignedRuleRelease>,
    pub packages: BTreeMap<String, Tier0RulePackage>,
    pub pins: Vec<ExactRuleRef>,
}
fn reject(s: &str) -> ApiError {
    ApiError::bad_request(s)
}
pub fn validate_ref(r: &ExactRuleRef) -> ApiResult<()> {
    if !is_valid_package_id(&r.id)
        || !is_valid_version(&r.version)
        || parse_hex::<32>(&r.blake3_256).is_none()
        || r.blake3_256 != r.blake3_256.to_ascii_lowercase()
    {
        return Err(reject("规则依赖必须为精确包ID/版本/小写BLAKE3，不允许范围或浮动引用"));
    }
    Ok(())
}
impl RuleReleaseManifest {
    pub fn reference(&self) -> ExactRuleRef {
        ExactRuleRef {
            id: self.package_id.clone(),
            version: self.version.clone(),
            blake3_256: self.artifact_cid.clone(),
        }
    }
    fn validate(&self, owner: u64) -> ApiResult<()> {
        validate_ref(&self.reference())?;
        if self.format != "rynvek-private-rule-release/1"
            || self.owner_id != owner.to_string()
            || self.dependencies.len() > 8
        {
            return Err(reject("私有规则签名域/主体/依赖数不符"));
        }
        let mut last: Option<&str> = None;
        for d in &self.dependencies {
            validate_ref(d)?;
            if d.id == self.package_id || last.is_some_and(|s| s >= d.id.as_str()) {
                return Err(reject("规则直接锁须按包ID严格排序且不能自依赖/重复"));
            }
            last = Some(&d.id);
        }
        Ok(())
    }
}
/// 固定剖面的JCS子集：键按ASCII排序，值仅含ASCII字符串/数组/对象，无浮点/整数。
/// 在原Hub消息后追加私有owner和完整精确锁域；不可重放成公共包或其他私有制品。
pub fn signing_message(manifest: &RuleReleaseManifest, artifact: &[u8]) -> ApiResult<Vec<u8>> {
    let owner = manifest.owner_id.parse::<u64>().map_err(|_| reject("主体非法"))?;
    manifest.validate(owner)?;
    if hex(blake3::hash(artifact).as_bytes()) != manifest.artifact_cid {
        return Err(reject("制品CID与签名清单不符"));
    }
    // 显式按ASCII键序编码，不受serde_json的preserve_order编译特性影响。
    #[derive(Serialize)]
    struct CanonicalRef<'a> {
        blake3_256: &'a str,
        id: &'a str,
        version: &'a str,
    }
    #[derive(Serialize)]
    struct CanonicalManifest<'a> {
        artifact_cid: &'a str,
        dependencies: Vec<CanonicalRef<'a>>,
        format: &'a str,
        owner_id: &'a str,
        package_id: &'a str,
        version: &'a str,
    }
    let canonical = serde_json::to_vec(&CanonicalManifest {
        artifact_cid: &manifest.artifact_cid,
        dependencies: manifest
            .dependencies
            .iter()
            .map(|r| CanonicalRef {
                blake3_256: &r.blake3_256,
                id: &r.id,
                version: &r.version,
            })
            .collect(),
        format: &manifest.format,
        owner_id: &manifest.owner_id,
        package_id: &manifest.package_id,
        version: &manifest.version,
    })
    .expect("定型ASCII签名清单");
    let mut msg = crate::api::hub::package_signing_message(&manifest.package_id, &manifest.version, artifact);
    msg.extend_from_slice(b"\0rynvek-private-rule-release/1\0");
    msg.extend_from_slice(&canonical);
    Ok(msg)
}
/// 下载端也必须传入登记/可信的主体公钥，不能把响应自带的任意公钥当作身份认证。
pub fn verify_download(
    owner: u64,
    expected_key: &[u8; 32],
    release: &SignedRuleRelease,
    bytes: &[u8],
) -> ApiResult<Tier0RulePackage> {
    release.manifest.validate(owner)?;
    if bytes.len() > MAX_CLOSURE_BYTES || parse_hex::<32>(&release.publisher_key).as_ref() != Some(expected_key) {
        return Err(reject("发布者公钥/制品大小不符"));
    }
    let signature = parse_hex::<64>(&release.signature).ok_or_else(|| reject("规则签名格式非法"))?;
    rynvek_security::verify_ed25519(expected_key, &signing_message(&release.manifest, bytes)?, &signature)
        .map_err(ApiError::from)?;
    let p = Tier0RulePackage::parse(bytes).map_err(|e| ApiError::new(e.code, e.detail))?;
    if p.id != release.manifest.package_id || p.version != release.manifest.version {
        return Err(reject("RYPK身份与签名发布身份不符"));
    }
    Ok(p)
}
pub fn release_of(row: &PrivateArtifactRow) -> ApiResult<SignedRuleRelease> {
    let manifest: RuleReleaseManifest =
        serde_json::from_value(row.profile.clone()).map_err(|_| conflict("规则发布清单损坏"))?;
    if manifest.owner_id != row.owner_id.to_string()
        || manifest.package_id != row.package_id
        || manifest.version != row.version
        || manifest.artifact_cid != hex(&row.cid)
    {
        return Err(conflict("私库行与签名身份不符"));
    }
    Ok(SignedRuleRelease {
        manifest,
        publisher_key: hex(&row.publisher_key),
        signature: hex(&row.signature),
    })
}
async fn trusted_key(app: &AppState, owner: u64, live: bool) -> ApiResult<[u8; 32]> {
    let p = app
        .store()
        .get_principal(owner)
        .await?
        .ok_or_else(|| ApiError::forbidden("规则主体不存在"))?;
    if live && p.status != rynvek_security::PrincipalStatus::Active {
        return Err(ApiError::forbidden("规则主体已失效"));
    }
    Ok(p.ed25519_pubkey)
}
fn artifact(app: &AppState, release: &SignedRuleRelease) -> ApiResult<Vec<u8>> {
    let cid = parse_hex::<32>(&release.manifest.artifact_cid).ok_or_else(|| reject("CID非法"))?;
    let bytes = app.inner.world_snapshots.get(&Blake3Hash(cid))?;
    if bytes.len() > MAX_CLOSURE_BYTES || Blake3Hash::of(&bytes).0 != cid {
        return Err(conflict("私有规则CAS摘要/大小不符"));
    }
    Ok(bytes)
}
async fn verify_async(
    owner: u64,
    key: [u8; 32],
    release: SignedRuleRelease,
    bytes: Vec<u8>,
) -> ApiResult<Tier0RulePackage> {
    tokio::task::spawn_blocking(move || verify_download(owner, &key, &release, &bytes))
        .await
        .map_err(|_| ApiError::internal("规则复验任务失败"))?
}
/// 有界DAG；同ID不能出现不同版本/CID；访问每个私有版本都复验原登记公钥及原RYPK编译等价。
pub async fn resolve(
    app: &AppState,
    owner: u64,
    root: &ExactRuleRef,
    seed: Option<PrivateArtifactRow>,
) -> ApiResult<ResolvedRules> {
    validate_ref(root)?;
    let key = trusted_key(app, owner, true).await?;
    let mut seen: BTreeMap<String, ExactRuleRef> = BTreeMap::new();
    let mut gray = BTreeSet::new();
    let mut done = BTreeSet::new();
    let mut releases = BTreeMap::new();
    let mut packages = BTreeMap::new();
    let mut size = 0usize;
    let mut stack = vec![(root.clone(), 0usize, false)];
    while let Some((r, depth, finish)) = stack.pop() {
        if finish {
            gray.remove(&r.id);
            done.insert(r.id);
            continue;
        }
        validate_ref(&r)?;
        if depth > 8 || seen.get(&r.id).is_some_and(|old| old != &r) {
            return Err(reject("规则依赖深度超限或同ID锁冲突"));
        }
        if done.contains(&r.id) {
            continue;
        }
        if !gray.insert(r.id.clone()) {
            return Err(reject("规则依赖成环"));
        }
        seen.insert(r.id.clone(), r.clone());
        if seen.len() > MAX_RELEASES {
            return Err(reject("规则依赖闭包超过16包"));
        }
        let row = if r == *root && seed.is_some() {
            seed.as_ref().unwrap().clone()
        } else {
            app.store()
                .private_rule(owner, r.id.clone(), r.version.clone())
                .await?
                .ok_or_else(|| ApiError::not_found("同主体私有规则依赖不存在"))?
        };
        if row.revoked || row.owner_id != owner || hex(&row.cid) != r.blake3_256 {
            return Err(conflict("私有规则依赖已撤销/摘要不符"));
        }
        let release = release_of(&row)?;
        let bytes = artifact(app, &release)?;
        size = size.checked_add(bytes.len()).ok_or_else(|| reject("包大小溢出"))?;
        if size > MAX_CLOSURE_BYTES {
            return Err(reject("规则依赖闭包总制品超过1MiB"));
        }
        let p = verify_async(owner, key, release.clone(), bytes).await?;
        stack.push((r.clone(), depth, true));
        for d in release.manifest.dependencies.iter().rev() {
            stack.push((d.clone(), depth + 1, false));
        }
        releases.insert(r.id.clone(), release);
        packages.insert(r.id, p);
    }
    Ok(ResolvedRules {
        releases: releases.into_values().collect(),
        packages,
        pins: seen.into_values().collect(),
    })
}
fn graphs(request: &RuleInstallRequest, resolved: &ResolvedRules) -> ApiResult<BTreeMap<String, String>> {
    if request.modules.len() != resolved.packages.len()
        || request.modules.values().collect::<BTreeSet<_>>().len() != request.modules.len()
    {
        return Err(reject("显式模块映射须完整且一对一覆盖精确闭包"));
    }
    request
        .modules
        .iter()
        .map(|(id, module)| {
            if module.is_empty() || module.len() > 128 {
                return Err(reject("目标模块名非法"));
            }
            let package = resolved.packages.get(id).ok_or_else(|| reject("模块映射含闭包外包"))?;
            Ok((module.clone(), package.graph_json.clone()))
        })
        .collect()
}
pub async fn prepare_install(app: &AppState, owner: u64, request: &RuleInstallRequest) -> ApiResult<RuleInstallReplay> {
    let resolved = resolve(app, owner, &request.root, None).await?;
    let graphs = graphs(request, &resolved)?;
    let evidence = RuleInstallEvidence {
        format: "rynvek-rule-install-evidence/1".into(),
        owner_id: owner.to_string(),
        request: request.clone(),
        releases: resolved.releases,
    };
    let bytes = serde_json::to_vec(&evidence).expect("定型安装证据");
    if bytes.len() > 64 * 1024 {
        return Err(reject("规则闭包签名证据超限"));
    }
    let cid = app.inner.world_snapshots.put(&bytes)?;
    Ok(RuleInstallReplay {
        evidence_cid: cid.0,
        graphs,
    })
}
/// 历史证据按原签名/CID恢复，不以当前撤销状态抹除已提交历史；live另由原Store发布事务检查。
async fn historical(app: &AppState, owner: u64, cid: [u8; 32]) -> ApiResult<(RuleInstallEvidence, ResolvedRules)> {
    let bytes = app.inner.world_snapshots.get(&Blake3Hash(cid))?;
    if bytes.len() > 64 * 1024 || Blake3Hash::of(&bytes).0 != cid {
        return Err(conflict("规则安装证据损坏/超限"));
    }
    let evidence: RuleInstallEvidence = serde_json::from_slice(&bytes).map_err(|_| conflict("规则安装证据格式不符"))?;
    if evidence.format != "rynvek-rule-install-evidence/1"
        || evidence.owner_id != owner.to_string()
        || evidence.releases.is_empty()
        || evidence.releases.len() > MAX_RELEASES
    {
        return Err(conflict("规则历史主体/闭包数不符"));
    }
    let key = trusted_key(app, owner, false).await?;
    let mut releases = BTreeMap::new();
    let mut packages = BTreeMap::new();
    let mut size = 0;
    for release in &evidence.releases {
        let bytes = artifact(app, release)?;
        size += bytes.len();
        if size > MAX_CLOSURE_BYTES {
            return Err(conflict("历史规则闭包大小超限"));
        }
        let p = verify_async(owner, key, release.clone(), bytes).await?;
        if releases.insert(p.id.clone(), release.clone()).is_some() {
            return Err(conflict("历史规则重复ID"));
        }
        packages.insert(p.id.clone(), p);
    }
    let mut gray = BTreeSet::new();
    let mut done = BTreeSet::new();
    let mut stack = vec![(evidence.request.root.clone(), 0, false)];
    while let Some((r, depth, finish)) = stack.pop() {
        if finish {
            gray.remove(&r.id);
            done.insert(r.id);
            continue;
        }
        validate_ref(&r)?;
        if depth > 8 {
            return Err(conflict("历史规则依赖过深"));
        }
        let release = releases.get(&r.id).ok_or_else(|| conflict("历史精确依赖缺失"))?;
        if release.manifest.reference() != r {
            return Err(conflict("历史精确依赖冲突"));
        }
        if done.contains(&r.id) {
            continue;
        }
        if !gray.insert(r.id.clone()) {
            return Err(conflict("历史规则成环"));
        }
        stack.push((r, depth, true));
        for d in release.manifest.dependencies.iter().rev() {
            stack.push((d.clone(), depth + 1, false));
        }
    }
    if done.len() != releases.len() {
        return Err(conflict("历史证据含闭包外包"));
    }
    let pins = releases.values().map(|r| r.manifest.reference()).collect();
    Ok((
        evidence,
        ResolvedRules {
            releases: releases.into_values().collect(),
            packages,
            pins,
        },
    ))
}
pub async fn validate_replay(
    app: &AppState,
    owner: u64,
    request: &RuleInstallRequest,
    plan: &RuleInstallReplay,
) -> ApiResult<()> {
    let (evidence, resolved) = historical(app, owner, plan.evidence_cid).await?;
    if evidence.request != *request || graphs(request, &resolved)? != plan.graphs {
        return Err(conflict("日志安装映射/图与签名证据不符"));
    }
    Ok(())
}
/// 对当前模块来源逐一校验，返回原提交事务必须复验的完整精确pins。
pub async fn installed_permits(app: &AppState, owner: u64, world: &SimulationEngine) -> ApiResult<Vec<RulePermit>> {
    let Some(rules) = world.component_rules() else {
        return Ok(vec![]);
    };
    let cids: BTreeSet<_> = rules.installed_releases.values().copied().collect();
    let mut pins = BTreeMap::new();
    for cid in cids {
        let (evidence, resolved) = historical(app, owner, cid).await?;
        let expected = graphs(&evidence.request, &resolved)?;
        let actual: BTreeSet<_> = rules
            .installed_releases
            .iter()
            .filter_map(|(m, c)| (*c == cid).then_some(m.clone()))
            .collect();
        if actual != expected.keys().cloned().collect() {
            return Err(conflict("已安装包组只剩部分模块"));
        }
        for (module, graph) in expected {
            if rules.config.rule_graphs.get(&module) != Some(&graph) {
                return Err(conflict("已安装规则图与签名包不符"));
            }
        }
        for pin in resolved.pins {
            if pins.get(&pin.id).is_some_and(|p| p != &pin) {
                return Err(conflict("世界已安装包锁冲突"));
            }
            pins.insert(pin.id.clone(), pin);
        }
    }
    Ok(if pins.is_empty() {
        vec![]
    } else {
        vec![RulePermit {
            owner_id: owner,
            pins: pins.into_values().collect(),
        }]
    })
}
