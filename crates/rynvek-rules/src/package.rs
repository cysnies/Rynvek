// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! Tier0受限构建剖面，复用唯一RYPK容器、规则图编译和RYIR验证。
//! 包只承载权威图/IR，不接受作者自附TS作为编译等价证明，也不声称已签名/安装。
use crate::{CompileOptions, RuleModule, compile_rule_graph_json, encode_rir};
use rynvek_core::{
    RynvekErrorCode,
    package::{PackageError, PackageInput, PackageInputEntry, PackageKind, read_package, write_package},
};
pub const TIER0_BUILD_PROFILE: &str = "rynvek.tier0-build.v1";
pub const MAX_TIER0_PACKAGE_BYTES: usize = 1024 * 1024;
pub const MAX_GRAPH_BYTES: usize = 256 * 1024;
const COMPAT: &str = ">=0.1.0 <0.2.0";
const BUILD: &[u8] = b"{\"compiler\":\"rynvek-rules/0.1.0\",\"profile\":\"rynvek.tier0-build.v1\"}";
fn invalid(detail: impl Into<String>) -> PackageError {
    PackageError {
        code: RynvekErrorCode::InvalidFileMagicOrVersion,
        detail: detail.into(),
    }
}
fn compiled(graph: &str) -> Result<RuleModule, PackageError> {
    if graph.len() > MAX_GRAPH_BYTES {
        return Err(invalid("Tier0图超出256KiB"));
    }
    compile_rule_graph_json(graph, &CompileOptions::default()).map_err(|e| PackageError {
        code: e.code,
        detail: e.to_string(),
    })
}
#[derive(Clone, Debug)]
pub struct Tier0RulePackage {
    pub id: String,
    pub version: String,
    pub graph_json: String,
    pub module: RuleModule,
}
impl Tier0RulePackage {
    pub fn parse(bytes: &[u8]) -> Result<Self, PackageError> {
        if bytes.len() > MAX_TIER0_PACKAGE_BYTES {
            return Err(invalid("Tier0包超过1MiB"));
        }
        let p = read_package(bytes, Some(PackageKind::Rpack))?;
        if p.manifest.engine_compat != COMPAT || p.manifest.entries.len() != 3 {
            return Err(invalid("Tier0构建剖面仅支持固定引擎范围和三个条目"));
        }
        for e in &p.manifest.entries {
            if !matches!(
                (e.path.as_str(), e.kind.as_str(), e.media_type.as_str()),
                ("rule.json", "data", "application/json")
                    | ("graph.json", "data", "application/json")
                    | ("rule.rir", "rule_module", "application/vnd.rynvek.rir")
            ) {
                return Err(invalid("Tier0包含未声明的可执行/来源/依赖条目"));
            }
        }
        if p.file("rule.json") != Some(BUILD) {
            return Err(invalid("Tier0编译剖面/工具链不符"));
        }
        let graph_json = std::str::from_utf8(p.file("graph.json").ok_or_else(|| invalid("缺规则图"))?)
            .map_err(|_| invalid("规则图不是UTF8"))?
            .to_string();
        let module = compiled(&graph_json)?;
        let rir = p.file("rule.rir").ok_or_else(|| invalid("缺IR"))?;
        crate::decode_rir(rir).map_err(|e| invalid(e.to_string()))?;
        if encode_rir(&module) != rir {
            return Err(invalid("图重新编译与实际IR不逐字节等价"));
        }
        Ok(Self {
            id: p.manifest.id.clone(),
            version: p.manifest.version.clone(),
            graph_json,
            module,
        })
    }
}
/// 无时钟/随机/作者附件的确定构建；同工具链/输入/身份/许可证输出字节一致。
pub fn build_tier0_package(id: &str, version: &str, license: &str, graph_json: &str) -> Result<Vec<u8>, PackageError> {
    let module = compiled(graph_json)?;
    // 原图解析器消除空白而不省略SSA/声明，不把作者附加代码纳入可信编译链。
    let graph = crate::graph::UntypedRuleGraph::from_json(graph_json).map_err(|e| invalid(e.to_string()))?;
    let entry = |path: &str, kind: &str, media_type: &str, data: Vec<u8>| PackageInputEntry {
        path: path.into(),
        kind: kind.into(),
        media_type: media_type.into(),
        data,
    };
    let bytes = write_package(&PackageInput {
        kind: PackageKind::Rpack,
        id: id.into(),
        version: version.into(),
        name: module.name.clone(),
        license: license.into(),
        engine_compat: COMPAT.into(),
        entries: vec![
            entry("rule.json", "data", "application/json", BUILD.to_vec()),
            entry(
                "graph.json",
                "data",
                "application/json",
                serde_json::to_vec(&graph).map_err(|e| invalid(e.to_string()))?,
            ),
            entry(
                "rule.rir",
                "rule_module",
                "application/vnd.rynvek.rir",
                encode_rir(&module),
            ),
        ],
    })?;
    Tier0RulePackage::parse(&bytes)?;
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    const GRAPH: &str = r#"{"format":"rynvek.rule-graph.v1","name":"t","reads":[],"writes":[{"component":"state.a","ty":"I32"}],"nodes":[{"op":"ConstI32","imm":"1"},{"op":"EmitComponentWrite","component":"state.a","args":[0]}]}"#;
    #[test]
    fn deterministic_original_rpack_recompiles_every_ir_byte() {
        let a = build_tier0_package("rules.test.ast", "1.0.0", "CC0-1.0", GRAPH).unwrap();
        assert_eq!(
            a,
            build_tier0_package("rules.test.ast", "1.0.0", "CC0-1.0", GRAPH).unwrap()
        );
        let p = Tier0RulePackage::parse(&a).unwrap();
        assert_eq!(p.id, "rules.test.ast");
        assert_eq!(p.version, "1.0.0");
        assert_eq!(encode_rir(&p.module), encode_rir(&compiled(&p.graph_json).unwrap()));
    }
    #[test]
    fn repacked_valid_container_cannot_swap_graph_or_add_self_attested_ts() {
        let bytes = build_tier0_package("rules.test.ast", "1.0.0", "CC0-1.0", GRAPH).unwrap();
        let p = read_package(&bytes, Some(PackageKind::Rpack)).unwrap();
        for mode in 0..4 {
            let mut entries: Vec<_> = p
                .manifest
                .entries
                .iter()
                .map(|e| PackageInputEntry {
                    path: e.path.clone(),
                    kind: e.kind.clone(),
                    media_type: e.media_type.clone(),
                    data: p.file(&e.path).unwrap().to_vec(),
                })
                .collect();
            match mode {
                0 => {
                    entries.iter_mut().find(|e| e.path == "graph.json").unwrap().data =
                        GRAPH.replace("\"imm\":\"1\"", "\"imm\":\"2\"").into_bytes()
                }
                1 => entries.push(PackageInputEntry {
                    path: "rule.ts".into(),
                    kind: "rule_source".into(),
                    media_type: "text/typescript".into(),
                    data: b"export default 0".to_vec(),
                }),
                2 => entries.iter_mut().find(|e| e.path == "rule.json").unwrap().data = b"{}".to_vec(),
                _ => entries.iter_mut().find(|e| e.path == "rule.rir").unwrap().data[0] ^= 1,
            }
            let bad = write_package(&PackageInput {
                kind: PackageKind::Rpack,
                id: p.manifest.id.clone(),
                version: p.manifest.version.clone(),
                name: "t".into(),
                license: "CC0-1.0".into(),
                engine_compat: COMPAT.into(),
                entries,
            })
            .unwrap();
            assert!(Tier0RulePackage::parse(&bad).is_err());
        }
    }
}
