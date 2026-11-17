// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 私有Agent profile复用RYPK容器；签名覆盖整个容器，任何程序始终只在无I/O QuickJS中求值。
use crate::{
    app::AppState,
    error::{ApiError, ApiResult},
    ids::{hex, parse_hex},
    store::PrivateArtifactRow,
};
use rynvek_core::{Blake3Hash, RynvekErrorCode};
use rynvek_rules::sandbox::{QuickJsSandbox, QuickJsSandboxConfig};
use rynvek_world::package::{PackageKind, read_package};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const MAX_ARTIFACT: usize = 256 << 10;
pub const MAX_SOURCE: usize = 64 << 10;
pub const MAX_COMPOSED: usize = 256 << 10;
pub const SCAN_STEPS: u64 = 64;
pub const MAX_MEMORY: usize = 16 << 10;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentProfile {
    pub schema: String,
    pub runtime: String,
    pub role: String,
    pub entrypoint: String,
    pub tools: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentDependency {
    pub id: String,
    pub version: String,
    pub blake3_256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentLock {
    pub lockfile_version: u32,
    pub packages: Vec<AgentDependency>,
}
#[derive(Clone, Debug)]
pub struct AgentPackage {
    pub id: String,
    pub version: String,
    pub profile: AgentProfile,
    pub source: String,
    pub lock: AgentLock,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDecision {
    pub name: String,
    pub arguments: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOutput {
    pub call: Option<ToolDecision>,
    pub memory: Value,
}

pub fn reject(message: &str) -> ApiError {
    ApiError::new(RynvekErrorCode::PluginCapabilityDenied, message)
}
pub fn signing_message(owner: u64, id: &str, version: &str, artifact: &[u8]) -> Vec<u8> {
    let mut msg = format!("rynvek-private-agent/1\0{owner}\0{id}\0{version}\0").into_bytes();
    msg.extend_from_slice(blake3::hash(artifact).as_bytes());
    msg
}
/// 保守的凭据污染拒绝，不冒充能检测所有编码秘密；真正边界是无I/O、私有CAS与不注入凭据。
pub fn credential_free(value: &str) -> ApiResult<()> {
    let lower = value.to_ascii_lowercase();
    if [
        "bearer ",
        "authorization",
        "capability_token",
        "api_key",
        "apikey",
        "password",
        "client_secret",
        "private key",
        "-----begin",
    ]
    .iter()
    .any(|p| lower.contains(p))
    {
        return Err(reject("包或运行上下文包含被禁止的凭据标记；不保存或回显内容"));
    }
    Ok(())
}
pub fn parse(bytes: &[u8]) -> ApiResult<AgentPackage> {
    if bytes.is_empty() || bytes.len() > MAX_ARTIFACT {
        return Err(reject("Agent包大小必须1..=256KiB"));
    }
    let pkg = read_package(bytes, Some(PackageKind::Rpack)).map_err(|e| ApiError::new(e.code, e.detail))?;
    if pkg.manifest.id.len() > 128 || pkg.manifest.version.len() > 32 {
        return Err(reject("Agent ID/版本长度超限"));
    }
    if pkg.manifest.engine_compat != "rynvek-agent-tools/1" || pkg.manifest.entries.len() != 3 {
        return Err(reject("Agent包须为当前tools/1三文件RYPK profile"));
    }
    for (path, kind, media) in [
        ("agent.json", "data", "application/json"),
        ("dependencies.lock", "data", "application/json"),
        ("main.js", "rule_source", "text/javascript"),
    ] {
        if !pkg
            .manifest
            .entries
            .iter()
            .any(|e| e.path == path && e.kind == kind && e.media_type == media)
        {
            return Err(reject("Agent包只允许agent.json、dependencies.lock和main.js"));
        }
    }
    credential_free(&serde_json::to_string(&pkg.manifest).unwrap())?;
    let profile: AgentProfile =
        serde_json::from_slice(pkg.file("agent.json").unwrap()).map_err(|_| reject("agent.json结构非法"))?;
    if profile.schema != "rynvek.agent.v1"
        || profile.runtime != "quickjs-tools/1"
        || profile.entrypoint != "main.js"
        || !["agent", "library"].contains(&profile.role.as_str())
    {
        return Err(reject("Agent profile运行时/版本/入口非法"));
    }
    let registry = rynvek_agent::schema::ToolRegistry::v1();
    let mut tools = BTreeSet::new();
    for tool in &profile.tools {
        if registry.get(tool).is_none() || !tools.insert(tool) {
            return Err(reject("声明工具未知或重复"));
        }
    }
    if profile.tools.len() > 6 || (profile.role == "library" && !profile.tools.is_empty()) {
        return Err(reject("library不能声明控制工具"));
    }
    let source = std::str::from_utf8(pkg.file("main.js").unwrap())
        .map_err(|_| reject("main.js须为UTF-8"))?
        .to_string();
    if source.len() > MAX_SOURCE {
        return Err(reject("main.js超出64KiB"));
    }
    credential_free(&source)?;
    let lock: AgentLock =
        serde_json::from_slice(pkg.file("dependencies.lock").unwrap()).map_err(|_| reject("私有依赖锁非法"))?;
    if lock.lockfile_version != 1 || lock.packages.len() > 8 {
        return Err(reject("仅支持v1锁与至多8个直接依赖"));
    }
    let mut ids = BTreeSet::new();
    for d in &lock.packages {
        if d.id.len() > 128
            || d.version.len() > 32
            || !ids.insert(&d.id)
            || !rynvek_world::package::is_valid_package_id(&d.id)
            || !rynvek_world::package::is_valid_version(&d.version)
            || parse_hex::<32>(&d.blake3_256).is_none()
        {
            return Err(reject("依赖必须唯一精确ID/版本/小写BLAKE3"));
        }
    }
    credential_free(&serde_json::to_string(&profile).unwrap())?;
    Ok(AgentPackage {
        id: pkg.manifest.id,
        version: pkg.manifest.version,
        profile,
        source,
        lock,
    })
}
#[derive(Clone, Debug)]
pub struct ResolvedAgent {
    pub package: AgentPackage,
    pub source: String,
    pub pins: Vec<PrivateArtifactRow>,
}
fn key(id: &str, ver: &str) -> String {
    format!("{id}@{ver}")
}
fn compose(
    node: &AgentPackage,
    nodes: &BTreeMap<String, AgentPackage>,
    stack: &mut BTreeSet<String>,
    depth: usize,
    root: bool,
) -> ApiResult<String> {
    if depth > 4 || !stack.insert(key(&node.id, &node.version)) {
        return Err(reject("依赖环或超过4层"));
    }
    let mut deps = Vec::new();
    for dep in &node.lock.packages {
        let child = nodes
            .get(&key(&dep.id, &dep.version))
            .ok_or_else(|| reject("缺少精确私有依赖"))?;
        deps.push(format!(
            "{}:{}",
            serde_json::to_string(&dep.id).unwrap(),
            compose(child, nodes, stack, depth + 1, false)?
        ));
        if deps.iter().map(String::len).sum::<usize>() > MAX_COMPOSED {
            return Err(reject("组合源码超限"));
        }
    }
    stack.remove(&key(&node.id, &node.version));
    let function = if node.profile.role == "agent" {
        "tick"
    } else {
        "transform"
    };
    let source = format!(
        "(function(){{'use strict';const deps=Object.freeze({{{}}});\n{}\n;return {};}})()",
        deps.join(","),
        node.source,
        function
    );
    if source.len() > MAX_COMPOSED {
        return Err(reject("组合源码超过256KiB"));
    }
    Ok(if root {
        format!("globalThis.tick={source};")
    } else {
        source
    })
}
pub async fn resolve(app: &AppState, owner: u64, root: AgentPackage) -> ApiResult<ResolvedAgent> {
    let mut nodes = BTreeMap::new();
    let mut pins = Vec::new();
    let mut todo: VecDeque<_> = root.lock.packages.iter().cloned().collect();
    while let Some(dep) = todo.pop_front() {
        if let Some(existing) = pins
            .iter()
            .find(|p: &&PrivateArtifactRow| p.package_id == dep.id && p.version == dep.version)
        {
            if hex(&existing.cid) != dep.blake3_256 {
                return Err(reject("图内依赖摘要冲突"));
            }
            continue;
        }
        if pins.len() >= 16 {
            return Err(reject("依赖图超过16个库"));
        }
        let row = app
            .store()
            .private_agent(owner, dep.id.clone(), dep.version.clone())
            .await?
            .ok_or_else(|| reject("私有依赖不存在或不属于本人"))?;
        if row.revoked || hex(&row.cid) != dep.blake3_256 {
            return Err(reject("依赖被撤销或精确摘要不符"));
        }
        let bytes = load_artifact(app, &row)?;
        let parsed = parse(&bytes)?;
        if parsed.id != row.package_id || parsed.version != row.version || parsed.profile.role != "library" {
            return Err(reject("依赖必须为签名身份一致的纯函数library"));
        }
        todo.extend(parsed.lock.packages.iter().cloned());
        nodes.insert(key(&dep.id, &dep.version), parsed);
        pins.push(row);
    }
    let source = compose(&root, &nodes, &mut BTreeSet::new(), 0, true)?;
    Ok(ResolvedAgent {
        package: root,
        source,
        pins,
    })
}
pub fn load_artifact(app: &AppState, row: &PrivateArtifactRow) -> ApiResult<Vec<u8>> {
    if row.revoked {
        return Err(reject("私有Agent版本已撤销"));
    }
    let bytes = app.inner.world_snapshots.get(&Blake3Hash(row.cid))?;
    if Blake3Hash::of(&bytes).0 != row.cid {
        return Err(reject("私有Agent CAS摘要不符"));
    }
    let sig: [u8; 64] = row
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| reject("签名结构损坏"))?;
    rynvek_security::verify_ed25519(
        &row.publisher_key,
        &signing_message(row.owner_id, &row.package_id, &row.version, &bytes),
        &sig,
    )
    .map_err(ApiError::from)?;
    Ok(bytes)
}
pub fn sandbox(source: &str) -> ApiResult<QuickJsSandbox> {
    QuickJsSandbox::new(
        source,
        QuickJsSandboxConfig {
            gas_budget: 100_000,
            heap_limit_bytes: 16 << 20,
            stack_limit_bytes: 256 << 10,
        },
    )
    .map_err(|_| reject("Agent沙箱加载失败（源码/预算/内存）；未执行任何工具"))
}
pub fn output(sandbox: &QuickJsSandbox, profile: &AgentProfile, input: &Value) -> ApiResult<(AgentOutput, u64)> {
    let result = sandbox
        .run_tick(&input.to_string())
        .map_err(|_| reject("Agent沙箱求值失败（异常/预算/内存）；未执行任何工具"))?;
    if result.output_json.len() > MAX_MEMORY + 8192 {
        return Err(reject("Agent输出超限"));
    }
    credential_free(&result.output_json)?;
    let shape: Value = serde_json::from_str(&result.output_json).map_err(|_| reject("Agent输出不是JSON"))?;
    if !shape
        .as_object()
        .is_some_and(|o| o.contains_key("call") && o.contains_key("memory"))
    {
        return Err(reject("Agent输出缺少call/memory"));
    }
    let output: AgentOutput =
        serde_json::from_str(&result.output_json).map_err(|_| reject("Agent输出须为严格call/memory结构"))?;
    if !output.memory.is_object() || output.memory.to_string().len() > MAX_MEMORY {
        return Err(reject("Agent memory须为至多16KiB对象"));
    }
    if let Some(ref call) = output.call {
        if !profile.tools.contains(&call.name) {
            return Err(reject("Agent调用了未声明工具"));
        }
        rynvek_agent::schema::ToolRegistry::v1()
            .validate(&call.name, &call.arguments)
            .map_err(|_| reject("Agent工具参数不符合原ToolBroker schema"))?;
    }
    Ok((output, result.gas_used))
}
pub fn scan(resolved: &ResolvedAgent) -> ApiResult<Value> {
    let js = sandbox(&resolved.source)?;
    let mut memory = json!({});
    let mut max_gas = 0;
    for step in 0..SCAN_STEPS {
        let input = json!({"observation":{"faction":0,"tick":step*30,"units":[],"buildings":[],"supply":[0,100],"ledger":{"balance":{"biomass":0,"flux":0,"neurogel":0}}},"memory":memory,"last_result":null});
        if resolved.package.profile.role == "agent" {
            let (out, gas) = output(&js, &resolved.package.profile, &input)?;
            memory = out.memory;
            max_gas = max_gas.max(gas);
        } else {
            let out = js.run_tick(&input.to_string()).map_err(|_| reject("library沙箱失败"))?;
            if out.output_json.len() > MAX_MEMORY {
                return Err(reject("library输出超限"));
            }
            credential_free(&out.output_json)?;
            max_gas = max_gas.max(out.gas_used);
        }
    }
    Ok(
        json!({"profile_validation":"passed","credential_marker_scan":"passed","sandbox":{"status":"passed","steps":SCAN_STEPS,"max_gas":max_gas,"gas_budget":100000,"heap_bytes":16<<20,"io":false},"trust_level":3}),
    )
}
