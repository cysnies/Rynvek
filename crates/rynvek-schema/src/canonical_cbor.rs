// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Canonical CBOR 编码：RFC 8949 §4.2.1「核心确定性编码要求」（Core Deterministic Encoding）。
//!
//! 以 JSON 数据模型为输入，保证同一逻辑文档在任何实现中编码为完全相同的字节：
//! - 整数与长度一律使用最短头部编码；
//! - 浮点数使用能无损表示该值的最短格式（half → single → double）；
//! - 映射的键按其编码后字节序列的字典序升序排列；
//! - 不使用不定长编码。

use serde_json::{Map, Number, Value};

/// CBOR 主类型（major type，位于首字节高 3 位）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum MajorType {
    UnsignedInt = 0,
    NegativeInt = 1,
    TextString = 3,
    Array = 4,
    Map = 5,
}

/// 主类型 7（简单值与浮点数）的完整首字节
const FALSE: u8 = 0xF4;
const TRUE: u8 = 0xF5;
const NULL: u8 = 0xF6;
const FLOAT16: u8 = 0xF9;
const FLOAT32: u8 = 0xFA;
const FLOAT64: u8 = 0xFB;

/// 将 JSON 值编码为确定性 CBOR 字节
pub fn to_canonical_cbor(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    encode_value(&mut out, value);
    out
}

fn encode_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => out.push(NULL),
        Value::Bool(false) => out.push(FALSE),
        Value::Bool(true) => out.push(TRUE),
        Value::Number(n) => encode_number(out, n),
        Value::String(s) => encode_text(out, s),
        Value::Array(items) => {
            encode_head(out, MajorType::Array, items.len() as u64);
            items.iter().for_each(|item| encode_value(out, item));
        }
        Value::Object(map) => encode_map(out, map),
    }
}

/// 写入「主类型 + 参数」头部，参数使用最短编码（RFC 8949 §4.2.1 第 1 条）
fn encode_head(out: &mut Vec<u8>, major: MajorType, argument: u64) {
    let major_bits = (major as u8) << 5;
    match argument {
        0..=23 => out.push(major_bits | argument as u8),
        24..=0xFF => out.extend_from_slice(&[major_bits | 24, argument as u8]),
        0x100..=0xFFFF => {
            out.push(major_bits | 25);
            out.extend_from_slice(&(argument as u16).to_be_bytes());
        }
        0x1_0000..=0xFFFF_FFFF => {
            out.push(major_bits | 26);
            out.extend_from_slice(&(argument as u32).to_be_bytes());
        }
        _ => {
            out.push(major_bits | 27);
            out.extend_from_slice(&argument.to_be_bytes());
        }
    }
}

fn encode_text(out: &mut Vec<u8>, s: &str) {
    encode_head(out, MajorType::TextString, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

fn encode_number(out: &mut Vec<u8>, n: &Number) {
    if let Some(u) = n.as_u64() {
        encode_head(out, MajorType::UnsignedInt, u);
    } else if let Some(i) = n.as_i64() {
        // 负整数 n 编码为参数 -1 - n（对 i64::MIN 同样不会溢出）
        encode_head(out, MajorType::NegativeInt, !(i as u64));
    } else if let Some(f) = n.as_f64() {
        encode_float(out, f);
    }
}

/// 以能无损表示该值的最短浮点格式编码（RFC 8949 §4.2.1 第 2 条「preferred serialization」）
fn encode_float(out: &mut Vec<u8>, value: f64) {
    let half = half::f16::from_f64(value);
    if half.to_f64().to_bits() == value.to_bits() {
        out.push(FLOAT16);
        out.extend_from_slice(&half.to_bits().to_be_bytes());
        return;
    }
    let single = value as f32;
    if f64::from(single).to_bits() == value.to_bits() {
        out.push(FLOAT32);
        out.extend_from_slice(&single.to_bits().to_be_bytes());
        return;
    }
    out.push(FLOAT64);
    out.extend_from_slice(&value.to_bits().to_be_bytes());
}

/// 映射的键按编码后字节的字典序排序（RFC 8949 §4.2.1 第 3 条）
fn encode_map(out: &mut Vec<u8>, map: &Map<String, Value>) {
    let mut entries: Vec<(Vec<u8>, &Value)> = map
        .iter()
        .map(|(key, value)| {
            let mut encoded_key = Vec::with_capacity(key.len() + 1);
            encode_text(&mut encoded_key, key);
            (encoded_key, value)
        })
        .collect();
    entries.sort_by(|(a, _), (b, _)| a.cmp(b));

    encode_head(out, MajorType::Map, entries.len() as u64);
    for (encoded_key, value) in entries {
        out.extend_from_slice(&encoded_key);
        encode_value(out, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        })
    }

    fn encode_json(json: &str) -> String {
        hex(&to_canonical_cbor(
            &serde_json::from_str(json).expect("测试输入必须是合法 JSON"),
        ))
    }

    /// RFC 8949 附录 A 的官方编码示例（JSON 可表达的子集）
    #[test]
    fn rfc8949_appendix_a_vectors() {
        let cases = [
            ("0", "00"),
            ("1", "01"),
            ("10", "0a"),
            ("23", "17"),
            ("24", "1818"),
            ("100", "1864"),
            ("1000", "1903e8"),
            ("1000000", "1a000f4240"),
            ("1000000000000", "1b000000e8d4a51000"),
            ("18446744073709551615", "1bffffffffffffffff"),
            ("-1", "20"),
            ("-10", "29"),
            ("-100", "3863"),
            ("-1000", "3903e7"),
            ("0.0", "f90000"),
            ("-0.0", "f98000"),
            ("1.0", "f93c00"),
            ("1.1", "fb3ff199999999999a"),
            ("1.5", "f93e00"),
            ("65504.0", "f97bff"),
            ("100000.0", "fa47c35000"),
            ("3.4028234663852886e+38", "fa7f7fffff"),
            ("1.0e+300", "fb7e37e43c8800759c"),
            ("5.960464477539063e-8", "f90001"),
            ("0.00006103515625", "f90400"),
            ("-4.0", "f9c400"),
            ("-4.1", "fbc010666666666666"),
            ("false", "f4"),
            ("true", "f5"),
            ("null", "f6"),
            (r#""""#, "60"),
            (r#""a""#, "6161"),
            (r#""IETF""#, "6449455446"),
            (r#""\u00fc""#, "62c3bc"),
            ("[]", "80"),
            ("[1, 2, 3]", "83010203"),
            ("{}", "a0"),
            (r#"{"a": 1, "b": [2, 3]}"#, "a26161016162820203"),
        ];
        for (json, expected) in cases {
            assert_eq!(encode_json(json), expected, "input = {json}");
        }
    }

    #[test]
    fn i64_min_is_encoded_without_overflow() {
        assert_eq!(
            hex(&to_canonical_cbor(&serde_json::json!(i64::MIN))),
            "3b7fffffffffffffff"
        );
    }

    #[test]
    fn map_keys_sort_by_encoded_bytes_shorter_first() {
        // 编码后的键：短字符串的长度前缀更小，因此 "b" 排在 "aa" 之前
        assert_eq!(encode_json(r#"{"aa": 1, "b": 2}"#), "a261620262616101");
    }
}
