// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! 原 RYPK/QuickJS 组件沙箱，供两个领域共同调用；无宿主I/O或跨Tick隐态。
#[cfg(not(target_arch = "wasm32"))]
use super::{ExecutionMeter, ParameterState, value_valid};
use super::{Registry, component_name, valid_authoring_key};
#[cfg(not(target_arch = "wasm32"))]
use crate::sandbox::{GAS_PER_INTERRUPT_POLL, QuickJsSandbox, QuickJsSandboxConfig};
use rynvek_core::{
    RynvekErrorCode,
    package::{PackageInput, PackageInputEntry, PackageKind, read_package, write_package},
};
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::BTreeMap;
use std::collections::BTreeSet;
type Result<T> = std::result::Result<T, RynvekErrorCode>;
const INVALID: RynvekErrorCode = RynvekErrorCode::DimensionMismatch;
pub const MAX_PACKAGE_BYTES: usize = 12 * 1024;
/// 当前组件profile的冻结引擎范围，不把任意兼容字符串当执行许可。
pub const ENGINE_COMPAT: &str = ">=0.1.0 <0.2.0";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tier1Install {
    pub template_id: String,
    pub package_hex: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentAccess {
    pub profile: String,
    pub reads: Vec<String>,
    pub writes: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tier1Draft {
    pub package_id: String,
    pub version: String,
    pub template_id: String,
    pub reads: Vec<String>,
    pub writes: Vec<String>,
    pub source_js: String,
    pub source_ts: Option<String>,
}
#[derive(Debug)]
pub struct CheckedTier1 {
    pub access: ComponentAccess,
    pub source: String,
    pub artifact_hash: [u8; 32],
}
impl ComponentAccess {
    fn validate(&self) -> Result<()> {
        if self.profile != "rynvek.tier1-components/1"
            || self.reads.len() > 16
            || self.writes.is_empty()
            || self.writes.len() > 16
            || self.reads.iter().any(|n| !component_name(n))
            || self
                .writes
                .iter()
                .any(|n| !n.starts_with("state.") || !component_name(n))
            || self.reads.iter().collect::<BTreeSet<_>>().len() != self.reads.len()
            || self.writes.iter().collect::<BTreeSet<_>>().len() != self.writes.len()
        {
            return Err(INVALID);
        }
        Ok(())
    }
}
impl Tier1Install {
    pub fn checked(&self) -> Result<CheckedTier1> {
        if !valid_authoring_key(&self.template_id)
            || self.package_hex.len() > MAX_PACKAGE_BYTES * 2
            || self.package_hex.len() % 2 != 0
        {
            return Err(INVALID);
        }
        let bytes = self
            .package_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|v| {
                let n = |c: u8| match c {
                    b'0'..=b'9' => Ok(c - b'0'),
                    b'a'..=b'f' => Ok(c - b'a' + 10),
                    _ => Err(INVALID),
                };
                Ok(n(v[0])? * 16 + n(v[1])?)
            })
            .collect::<Result<Vec<_>>>()?;
        let p = read_package(&bytes, Some(PackageKind::Rpack)).map_err(|e| e.code)?;
        if p.manifest.engine_compat != ENGINE_COMPAT {
            return Err(RynvekErrorCode::PluginCapabilityDenied);
        }
        if !(2..=3).contains(&p.files.len()) {
            return Err(INVALID);
        }
        for e in &p.manifest.entries {
            if !matches!(
                (e.path.as_str(), e.kind.as_str(), e.media_type.as_str()),
                ("rule.json", "data", "application/json")
                    | ("rule.js", "rule_source", "text/javascript")
                    | ("rule.ts", "rule_source", "text/typescript")
            ) {
                return Err(RynvekErrorCode::PluginCapabilityDenied);
            }
        }
        let access: ComponentAccess =
            serde_json::from_slice(p.file("rule.json").ok_or(INVALID)?).map_err(|_| INVALID)?;
        access.validate()?;
        let source = std::str::from_utf8(p.file("rule.js").ok_or(INVALID)?)
            .map_err(|_| INVALID)?
            .to_owned();
        if source.is_empty()
            || source.len() > 8192
            || p.file("rule.ts")
                .is_some_and(|b| b.len() > 8192 || std::str::from_utf8(b).is_err())
        {
            return Err(INVALID);
        }
        Ok(CheckedTier1 {
            access,
            source,
            artifact_hash: *blake3::hash(&bytes).as_bytes(),
        })
    }
}
impl Tier1Draft {
    /// 只构建原容器与访问声明，不在创作HTTP/补丁检查时执行不可信顶层脚本。
    pub fn build(&self) -> Result<Tier1Install> {
        let access = ComponentAccess {
            profile: "rynvek.tier1-components/1".into(),
            reads: self.reads.clone(),
            writes: self.writes.clone(),
        };
        access.validate()?;
        let mut entries = vec![
            PackageInputEntry {
                path: "rule.json".into(),
                kind: "data".into(),
                media_type: "application/json".into(),
                data: serde_json::to_vec(&access).map_err(|_| INVALID)?,
            },
            PackageInputEntry {
                path: "rule.js".into(),
                kind: "rule_source".into(),
                media_type: "text/javascript".into(),
                data: self.source_js.as_bytes().to_vec(),
            },
        ];
        if let Some(ts) = &self.source_ts {
            entries.push(PackageInputEntry {
                path: "rule.ts".into(),
                kind: "rule_source".into(),
                media_type: "text/typescript".into(),
                data: ts.as_bytes().to_vec(),
            });
        }
        if self.source_js.len() > 8192 || self.source_ts.as_ref().is_some_and(|s| s.len() > 8192) {
            return Err(INVALID);
        }
        let bytes = write_package(&PackageInput {
            kind: PackageKind::Rpack,
            id: self.package_id.clone(),
            version: self.version.clone(),
            name: self.package_id.clone(),
            license: "AGPL-3.0-only".into(),
            engine_compat: ENGINE_COMPAT.into(),
            entries,
        })
        .map_err(|e| e.code)?;
        let install = Tier1Install {
            template_id: self.template_id.clone(),
            package_hex: bytes.iter().map(|b| format!("{b:02x}")).collect(),
        };
        install.checked()?;
        Ok(install)
    }
}
impl Registry<'_> {
    pub fn validate_tier1(&self, writes: &mut BTreeSet<(String, String)>) -> Result<()> {
        // 不为编译通过而静默跳过规则；当前浏览器仅消费服务器权威结果。
        if cfg!(target_arch = "wasm32") && !self.tier1_rules.is_empty() {
            return Err(RynvekErrorCode::PluginCapabilityDenied);
        }
        if self.tier1_rules.len() > 4 {
            return Err(INVALID);
        }
        for (id, i) in self.tier1_rules {
            if !valid_authoring_key(id) {
                return Err(INVALID);
            }
            let c = i.checked()?;
            let t = self.parameter_templates.get(&i.template_id).ok_or(INVALID)?;
            if c.access
                .reads
                .iter()
                .chain(c.access.writes.iter())
                .any(|n| !t.components.contains_key(n))
            {
                return Err(INVALID);
            }
            for n in c.access.writes {
                if !writes.insert((i.template_id.clone(), n)) {
                    return Err(RynvekErrorCode::RuleUndeclaredWriteAccess);
                }
            }
        }
        Ok(())
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(Serialize)]
struct InputEntity<'a> {
    id: &'a str,
    serial: u32,
    values: BTreeMap<&'a str, i32>,
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    writes: Vec<OutputEntity>,
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputEntity {
    id: String,
    values: BTreeMap<String, i64>,
}
#[cfg(not(target_arch = "wasm32"))]
impl Registry<'_> {
    /// Tier0/Tier1读同一旧状态；全部成功才发布。失败保留上一状态并记故障与实际配额扣记。
    pub(super) fn step_tier1_rules(
        &self,
        state: &mut ParameterState,
        original: &ParameterState,
        tick: u64,
        world_seed: u64,
        remaining: &mut u64,
        meter: &mut ExecutionMeter,
    ) {
        let mut staged = (*state).clone();
        staged.tier1_gas_charged = 0;
        for (id, i) in self.tier1_rules {
            let mut charged = 0;
            let result = (|| {
                let c = i.checked()?;
                let t = &self.parameter_templates[&i.template_id];
                let ids: Vec<_> = self
                    .parameter_instances
                    .iter()
                    .filter(|(_, p)| p.template_id == i.template_id)
                    .map(|(id, _)| id)
                    .collect();
                if ids.is_empty() {
                    return Ok(());
                }
                let entities: Vec<_> = ids
                    .iter()
                    .map(|id| InputEntity {
                        id,
                        serial: original.entities[*id].serial,
                        values: c
                            .access
                            .reads
                            .iter()
                            .map(|n| {
                                let v = if n.starts_with("state.") {
                                    original.entities[*id].values[n]
                                } else {
                                    *self.parameter_instances[*id]
                                        .parameters
                                        .get(n)
                                        .unwrap_or(&t.components[n].default)
                                };
                                (n.as_str(), v)
                            })
                            .collect(),
                    })
                    .collect();
                let input=serde_json::to_string(&serde_json::json!({"tick":tick.to_string(),"world_seed":world_seed.to_string(),"entities":entities})).map_err(|_|INVALID)?;
                if *remaining < GAS_PER_INTERRUPT_POLL {
                    return Err(RynvekErrorCode::SandboxGasBudgetExhausted);
                }
                // 粗粒度QuickJS轮询配额：每次调用至少扣1万，失败烧掉本次剩余额度，不冒称逐指令Gas或物理能量。
                let out = QuickJsSandbox::run_once(
                    &c.source,
                    &input,
                    QuickJsSandboxConfig {
                        gas_budget: *remaining - GAS_PER_INTERRUPT_POLL,
                        ..Default::default()
                    },
                );
                charged = out.as_ref().map_or(*remaining, |o| {
                    o.gas_used.saturating_add(GAS_PER_INTERRUPT_POLL).min(*remaining)
                });
                *remaining -= charged;
                let out = out.map_err(|e| e.code)?;
                if out.output_json.len() > 64 * 1024 {
                    return Err(INVALID);
                }
                let out: Output = serde_json::from_str(&out.output_json).map_err(|_| INVALID)?;
                if out.writes.len() != ids.len() {
                    return Err(INVALID);
                }
                for (id, w) in ids.iter().zip(out.writes) {
                    if w.id.as_str() != id.as_str()
                        || w.values.len() != c.access.writes.len()
                        || w.values.keys().any(|n| !c.access.writes.contains(n))
                    {
                        return Err(RynvekErrorCode::RuleUndeclaredWriteAccess);
                    }
                    for (n, v) in w.values {
                        if !value_valid(t.components[&n].ty, v) {
                            return Err(INVALID);
                        }
                        staged.entities.get_mut(*id).ok_or(INVALID)?.values.insert(n, v as i32);
                    }
                }
                Ok::<(), RynvekErrorCode>(())
            })();
            staged.tier1_gas_charged += charged;
            meter.tier1_quota += charged;
            let ids: Vec<_> = self
                .parameter_instances
                .iter()
                .filter(|(_, p)| p.template_id == i.template_id)
                .map(|(id, _)| id)
                .collect();
            if !ids.is_empty() {
                let n = ids.len() as u64;
                for (index, entity) in ids.iter().enumerate() {
                    // 共享调用固定开销按稳定ID均分，余数逐一分配，不冒称逐指令实测。
                    meter.add(entity, charged / n + u64::from((index as u64) < charged % n));
                }
            }
            if let Err(code) = result {
                (*state) = original.clone();
                (*state).tier1_gas_charged = staged.tier1_gas_charged;
                (*state).fault = Some((format!("tier1:{id}"), code as u16));
                return;
            }
        }
        staged.fault = None;
        (*state) = staged;
    }
}
