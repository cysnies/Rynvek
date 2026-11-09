// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! WGSL 审查跨语言向量：Rust 移植必须与 `@rynvek/assets` 的 TypeScript 审查器逐条一致。

use rynvek_world::wgsl_review::{TrustLevel, review_rview_shader};

#[test]
fn rust_reviewer_matches_typescript_vectors() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/packages/wgsl_review_vectors.json"
    );
    let vectors: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(vectors.len() >= 60);
    for v in &vectors {
        let name = v["name"].as_str().unwrap();
        let trust = TrustLevel::from_u8(v["trust_level"].as_u64().unwrap() as u8).unwrap();
        let got = review_rview_shader(v["source"].as_str().unwrap(), trust);
        assert_eq!(
            got.is_ok(),
            v["ok"].as_bool().unwrap(),
            "{name} @TL{}: {got:?}",
            trust as u8
        );
        if let Ok(r) = got {
            assert_eq!(
                u64::from(r.instruction_count),
                v["instruction_count"].as_u64().unwrap(),
                "{name} @TL{}",
                trust as u8
            );
        }
    }
}
