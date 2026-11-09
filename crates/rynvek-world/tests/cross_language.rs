// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 跨语言容器夹具：`tests/packages/sample.rview` 由 `@rynvek/assets` 写出；
//! Rust 读取后用同一输入重写，必须逐字节一致。

use rynvek_world::package::{PackageInput, PackageInputEntry, PackageKind, read_package, write_package};

#[test]
fn rust_writer_reproduces_typescript_rview_byte_for_byte() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/packages/sample.rview");
    let ts = std::fs::read(path).expect("夹具存在（由 @rynvek/assets 测试生成）");
    let p = read_package(&ts, Some(PackageKind::Rview)).unwrap();
    assert_eq!(p.manifest.id, "rynvek.visual.industrial-default");
    let input = PackageInput {
        kind: PackageKind::Rview,
        id: p.manifest.id.clone(),
        version: p.manifest.version.clone(),
        name: p.manifest.name.clone(),
        license: p.manifest.license.clone(),
        engine_compat: p.manifest.engine_compat.clone(),
        // 故意打乱顺序：写出器负责排序
        entries: p
            .manifest
            .entries
            .iter()
            .rev()
            .map(|e| PackageInputEntry {
                path: e.path.clone(),
                kind: e.kind.clone(),
                media_type: e.media_type.clone(),
                data: p.files[&e.path].clone(),
            })
            .collect(),
    };
    assert_eq!(write_package(&input).unwrap(), ts);
}

/// `protocols/schemas/{rworld,rpack,rview}.schema.json` 与 Rust 校验规则一致
#[test]
fn json_schemas_match_container_rules() {
    for kind in PackageKind::ALL {
        let path = format!(
            "{}/../../protocols/schemas/{}.schema.json",
            env!("CARGO_MANIFEST_DIR"),
            kind.as_str()
        );
        let s: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(s["properties"]["schema"]["const"], kind.schema());
        let licenses: Vec<&str> = s["properties"]["license"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(licenses, kind.licenses(), "{path}");
        for k in s["$defs"]["entry"]["properties"]["kind"]["enum"].as_array().unwrap() {
            assert!(kind.media_types(k.as_str().unwrap()).is_some(), "{path}: {k}");
        }
    }
}
