// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Canonical JSON 序列化：RFC 8785 JSON Canonicalization Scheme（JCS，GAP-16）。
//!
//! 用于构建签名清单与内容哈希，保证同一逻辑文档在任何实现中产生完全相同的字节序列：
//! - 对象成员按键的 UTF-16 码元序升序排列；
//! - 不输出任何空白；
//! - 字符串仅转义 `"`、`\` 与 U+0000..U+001F 控制字符；
//! - 数字按 ECMAScript `Number.prototype.toString` 规则输出。

use core::fmt;
use std::fmt::Write as _;

use serde_json::{Map, Number, Value};

/// IEEE-754 双精度可精确表示的最大整数（`2^53 - 1`，I-JSON 安全整数上界）
pub const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// Canonical JSON 序列化错误
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalJsonError {
    /// 整数超出 I-JSON 安全范围 `±(2^53 - 1)`，无法在双精度下无损表示
    UnsafeInteger(String),
}

impl fmt::Display for CanonicalJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeInteger(n) => write!(f, "integer {n} is outside the I-JSON safe range ±(2^53-1)"),
        }
    }
}

impl std::error::Error for CanonicalJsonError {}

/// 将 JSON 值序列化为 RFC 8785 规范字符串
pub fn to_canonical_string(value: &Value) -> Result<String, CanonicalJsonError> {
    let mut out = String::new();
    write_value(&mut out, value)?;
    Ok(out)
}

/// 将 JSON 值序列化为 RFC 8785 规范 UTF-8 字节
pub fn to_canonical_vec(value: &Value) -> Result<Vec<u8>, CanonicalJsonError> {
    to_canonical_string(value).map(String::into_bytes)
}

fn write_value(out: &mut String, value: &Value) -> Result<(), CanonicalJsonError> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => write_number(out, n)?,
        Value::String(s) => write_string(out, s),
        Value::Array(items) => write_array(out, items)?,
        Value::Object(map) => write_object(out, map)?,
    }
    Ok(())
}

fn write_array(out: &mut String, items: &[Value]) -> Result<(), CanonicalJsonError> {
    out.push('[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_value(out, item)?;
    }
    out.push(']');
    Ok(())
}

fn write_object(out: &mut String, map: &Map<String, Value>) -> Result<(), CanonicalJsonError> {
    let mut entries: Vec<(&String, &Value)> = map.iter().collect();
    // RFC 8785 §3.2.3：按 UTF-16 码元序比较键（与 UTF-8 字节序在增补平面字符上不同）
    entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
    out.push('{');
    for (i, (key, item)) in entries.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_string(out, key);
        out.push(':');
        write_value(out, item)?;
    }
    out.push('}');
    Ok(())
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{09}' => out.push_str("\\t"),
            '\u{0A}' => out.push_str("\\n"),
            '\u{0C}' => out.push_str("\\f"),
            '\u{0D}' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                // 其余控制字符一律使用小写十六进制 \u00xx
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_number(out: &mut String, n: &Number) -> Result<(), CanonicalJsonError> {
    if let Some(u) = n.as_u64() {
        if u > MAX_SAFE_INTEGER {
            return Err(CanonicalJsonError::UnsafeInteger(u.to_string()));
        }
        let _ = write!(out, "{u}");
    } else if let Some(i) = n.as_i64() {
        if i.unsigned_abs() > MAX_SAFE_INTEGER {
            return Err(CanonicalJsonError::UnsafeInteger(i.to_string()));
        }
        let _ = write!(out, "{i}");
    } else if let Some(f) = n.as_f64() {
        out.push_str(&format_ecmascript_f64(f));
    }
    Ok(())
}

/// 按 ECMAScript `Number::toString(10)` 规则格式化有限双精度浮点数（ECMA-262 §6.1.6.1.20）
pub fn format_ecmascript_f64(value: f64) -> String {
    debug_assert!(value.is_finite(), "JSON 中不存在非有限数");
    if value == 0.0 {
        // +0 与 -0 均输出 "0"
        return "0".to_owned();
    }
    let (negative, digits, exponent) = shortest_decimal(value);
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    let k = digits.len() as i32;
    // n：十进制小数点相对于有效数字串起点的位置（value = 0.d1d2...dk × 10^n）
    let n = exponent + 1;
    if k <= n && n <= 21 {
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        let (int_part, frac_part) = digits.split_at(n as usize);
        out.push_str(int_part);
        out.push('.');
        out.push_str(frac_part);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(&digits);
    } else {
        let (first, rest) = digits.split_at(1);
        out.push_str(first);
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        let e = n - 1;
        let _ = write!(out, "e{}{}", if e < 0 { '-' } else { '+' }, e.unsigned_abs());
    }
    out
}

/// 求最短往返十进制表示：返回 `(是否为负, 有效数字串, 科学计数法指数)`。
///
/// Rust 的 `{:e}` 格式化保证输出能精确往返的最短有效数字（与 ECMAScript 要求一致）。
fn shortest_decimal(value: f64) -> (bool, String, i32) {
    let formatted = format!("{:e}", value.abs());
    let (mantissa, exponent) = formatted.split_once('e').expect("LowerExp 输出必含指数");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().expect("LowerExp 指数必为整数");
    (value.is_sign_negative(), digits, exponent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn canon(input: &str) -> String {
        let value: Value = serde_json::from_str(input).expect("测试输入必须是合法 JSON");
        to_canonical_string(&value).expect("应可规范化")
    }

    #[test]
    fn rfc8785_section_3_2_2_example() {
        let input = r#"{
            "numbers": [333333333.33333329, 1E30, 4.50, 2e-3, 0.000000000000000000000000001],
            "string": "\u20ac$\u000F\u000aA'\u0042\u0022\u005c\\\"\/",
            "literals": [null, true, false]
        }"#;
        let expected = r#"{"literals":[null,true,false],"numbers":[333333333.3333333,1e+30,4.5,0.002,1e-27],"string":"€$\u000f\nA'B\"\\\\\"/"}"#;
        assert_eq!(canon(input), expected);
    }

    #[test]
    fn rfc8785_section_3_2_3_key_sorting_uses_utf16() {
        let input = r#"{
            "\u20ac": "Euro Sign",
            "\r": "Carriage Return",
            "\ufb33": "Hebrew Letter Dalet With Dagesh",
            "1": "One",
            "\ud83d\ude00": "Emoji: Grinning Face",
            "\u0080": "Control",
            "\u00f6": "Latin Small Letter O With Diaeresis"
        }"#;
        let expected = concat!(
            "{\"\\r\":\"Carriage Return\",\"1\":\"One\",\"\u{80}\":\"Control\",",
            "\"\u{f6}\":\"Latin Small Letter O With Diaeresis\",\"\u{20ac}\":\"Euro Sign\",",
            "\"\u{1f600}\":\"Emoji: Grinning Face\",\"\u{fb33}\":\"Hebrew Letter Dalet With Dagesh\"}"
        );
        assert_eq!(canon(input), expected);
    }

    #[test]
    fn ecmascript_number_formatting() {
        let cases: [(f64, &str); 12] = [
            (1.0, "1"),
            (-0.0, "0"),
            (0.5, "0.5"),
            (-1.5, "-1.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (123.456, "123.456"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (-2.5e-8, "-2.5e-8"),
        ];
        for (value, expected) in cases {
            assert_eq!(format_ecmascript_f64(value), expected, "value = {value:e}");
        }
    }

    #[test]
    fn integers_and_safe_range() {
        assert_eq!(
            canon("[0, -7, 9007199254740991, -9007199254740991]"),
            "[0,-7,9007199254740991,-9007199254740991]"
        );
        let too_big = json!(9_007_199_254_740_992u64);
        assert_eq!(
            to_canonical_string(&too_big),
            Err(CanonicalJsonError::UnsafeInteger("9007199254740992".to_owned()))
        );
        assert!(to_canonical_string(&json!(i64::MIN)).is_err());
    }

    #[test]
    fn nested_structures_have_no_whitespace() {
        assert_eq!(
            canon(r#"{ "b" : [ 1 , { "z":1, "a":2 } ], "a" : {} }"#),
            r#"{"a":{},"b":[1,{"a":2,"z":1}]}"#
        );
    }
}
