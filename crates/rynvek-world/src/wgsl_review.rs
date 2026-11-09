// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `.rview` WGSL 片段着色器静态安全审查（拒绝时 `0x9004 UnsafeRviewShaderRejected`）。
//!
//! 本模块是 `packages/assets/src/wgsl_review.ts`（ERRATA E-019）的逐条移植：词法切分、指令估算、
//! 禁用清单与各信任等级预算完全一致，Hub 服务端与浏览器端对同一着色器给出同一结论
//! （共享向量见 `tests/packages/wgsl_review_vectors.json`）。规则摘要：
//! - 指令数 = 运算符 + 函数 / 构造器调用 + 赋值 + 下标访问 + 控制流语句，自定义函数按调用点展开；
//! - 只允许单个 `@fragment` 入口；TrustLevel ≥ 2 禁止计算 / 顶点阶段、存储缓冲、原子、工作组同步、
//!   存储纹理写入与 `enable`；TrustLevel 3 额外禁止一切循环；
//! - 预算：TrustLevel 3 = 512（项目文档 7.2 冻结），TrustLevel 2 = 2048，TrustLevel 0/1 不限。
//!
//! 服务端在此之上再用 naga 做完整的 WGSL 解析与校验（[`validate_with_naga`]），浏览器端无此步骤。

use std::collections::{BTreeMap, BTreeSet};

/// 信任等级（项目文档 7.2）
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrustLevel {
    CoreBuiltin = 0,
    OfficialSigned = 1,
    VerifiedCreator = 2,
    UntrustedEphemeral = 3,
}

impl TrustLevel {
    pub fn from_u8(v: u8) -> Option<Self> {
        [
            Self::CoreBuiltin,
            Self::OfficialSigned,
            Self::VerifiedCreator,
            Self::UntrustedEphemeral,
        ]
        .get(usize::from(v))
        .copied()
    }

    /// 片段着色器指令预算（None = 不限）
    pub const fn fragment_budget(self) -> Option<u32> {
        match self {
            Self::CoreBuiltin | Self::OfficialSigned => None,
            Self::VerifiedCreator => Some(2048),
            Self::UntrustedEphemeral => Some(512),
        }
    }
}

/// 着色器源码长度上限（字节）
pub const WGSL_MAX_SOURCE_BYTES: usize = 64 * 1024;

/// TrustLevel ≥ 2 禁用的标识符
pub const WGSL_FORBIDDEN_IDENTIFIERS: [&str; 24] = [
    "enable",
    "storage",
    "atomic",
    "atomicAdd",
    "atomicSub",
    "atomicMax",
    "atomicMin",
    "atomicAnd",
    "atomicOr",
    "atomicXor",
    "atomicExchange",
    "atomicCompareExchangeWeak",
    "atomicLoad",
    "atomicStore",
    "workgroup",
    "workgroupBarrier",
    "storageBarrier",
    "textureBarrier",
    "workgroupUniformLoad",
    "textureStore",
    "texture_storage_1d",
    "texture_storage_2d",
    "texture_storage_2d_array",
    "texture_storage_3d",
];

/// 禁止的入口属性
pub const WGSL_FORBIDDEN_ATTRIBUTES: [&str; 3] = ["compute", "vertex", "workgroup_size"];
/// TrustLevel 3 额外禁止的循环关键字
pub const WGSL_LOOP_KEYWORDS: [&str; 3] = ["loop", "for", "while"];

const CONTROL_KEYWORDS: [&str; 10] = [
    "if",
    "switch",
    "return",
    "discard",
    "break",
    "continue",
    "loop",
    "for",
    "while",
    "continuing",
];

const TEMPLATE_TYPES: [&str; 28] = [
    "vec2",
    "vec3",
    "vec4",
    "mat2x2",
    "mat2x3",
    "mat2x4",
    "mat3x2",
    "mat3x3",
    "mat3x4",
    "mat4x2",
    "mat4x3",
    "mat4x4",
    "array",
    "ptr",
    "atomic",
    "var",
    "bitcast",
    "texture_1d",
    "texture_2d",
    "texture_2d_array",
    "texture_3d",
    "texture_cube",
    "texture_cube_array",
    "texture_multisampled_2d",
    "texture_storage_1d",
    "texture_storage_2d",
    "texture_storage_2d_array",
    "texture_storage_3d",
];

/// 运算符表：顺序即匹配优先级（长运算符在前），与 TS 版一致
const OPERATORS: [&str; 45] = [
    "<<=", ">>=", "&&", "||", "==", "!=", "<=", ">=", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "++",
    "--", "->", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "<", ">", "=", "(", ")", "{", "}", "[", "]", ",",
    ";", ":", ".", "@",
];

const NON_COUNTING_PUNCT: [&str; 10] = ["(", ")", "{", "}", "]", ",", ";", ":", ".", "->"];

/// 审查结论
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WgslReviewReport {
    pub entry_point: String,
    pub instruction_count: u32,
    pub budget: Option<u32>,
    /// 各函数自身（不含被调函数）的指令数
    pub per_function: BTreeMap<String, u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Ident,
    Number,
    Punct,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    text: String,
}

type R<T> = Result<T, String>;

/// 去掉行注释与可嵌套块注释
pub fn strip_wgsl_comments(src: &str) -> R<String> {
    let c: Vec<char> = src.chars().collect();
    let starts = |i: usize, a: char, b: char| c.get(i) == Some(&a) && c.get(i + 1) == Some(&b);
    let mut out = String::with_capacity(src.len());
    let (mut i, mut depth) = (0usize, 0u32);
    while i < c.len() {
        if depth == 0 && starts(i, '/', '/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if starts(i, '/', '*') {
            depth += 1;
            i += 2;
        } else if depth > 0 && starts(i, '*', '/') {
            depth -= 1;
            i += 2;
            if depth == 0 {
                out.push(' ');
            }
        } else {
            if depth == 0 {
                out.push(c[i]);
            }
            i += 1;
        }
    }
    if depth != 0 {
        return Err("块注释未闭合".into());
    }
    Ok(out)
}

/// JS `/\s/` 的字符集：Unicode 空白 + BOM，但不含 NEL（U+0085）
fn is_js_space(ch: char) -> bool {
    (ch.is_whitespace() && ch != '\u{85}') || ch == '\u{FEFF}'
}

fn tokenize(src: &str) -> R<Vec<Token>> {
    let text: Vec<char> = strip_wgsl_comments(src)?.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let digits_at = |mut j: usize, end: usize| {
        let s = j;
        while j < end && text[j].is_ascii_digit() {
            j += 1;
        }
        j - s
    };
    while i < text.len() {
        let ch = text[i];
        if is_js_space(ch) {
            i += 1;
            continue;
        }
        // 标识符（TS 版在 128 字符窗口内匹配，超长标识符会被切成多段，这里保持一致）
        if ch.is_ascii_alphabetic() || ch == '_' {
            let end = (i + 128).min(text.len());
            let mut j = i + 1;
            while j < end && (text[j].is_ascii_alphanumeric() || text[j] == '_') {
                j += 1;
            }
            tokens.push(Token {
                kind: Kind::Ident,
                text: text[i..j].iter().collect(),
            });
            i = j;
            continue;
        }
        // 数字（64 字符窗口）：0x 十六进制 | [0-9]*\.?[0-9]+ 指数? 后缀?
        let end = (i + 64).min(text.len());
        let mut len = 0usize;
        if ch == '0' && matches!(text.get(i + 1), Some('x' | 'X')) {
            let mut j = i + 2;
            while j < end && text[j].is_ascii_hexdigit() {
                j += 1;
            }
            if j > i + 2 {
                len = j - i;
            }
        }
        if len == 0 {
            let d1 = digits_at(i, end);
            let after = i + d1;
            if after < end && text[after] == '.' && digits_at(after + 1, end) > 0 {
                len = d1 + 1 + digits_at(after + 1, end);
            } else if d1 > 0 {
                len = d1;
            }
            if len > 0 {
                // 指数
                let e = i + len;
                if e < end && matches!(text[e], 'e' | 'E') {
                    let mut k = e + 1;
                    if k < end && matches!(text[k], '+' | '-') {
                        k += 1;
                    }
                    let d = digits_at(k, end);
                    if d > 0 {
                        len = k + d - i;
                    }
                }
            }
        }
        if len > 0 {
            if i + len < end && matches!(text[i + len], 'i' | 'u' | 'h' | 'f') {
                len += 1;
            }
            tokens.push(Token {
                kind: Kind::Number,
                text: text[i..i + len].iter().collect(),
            });
            i += len;
            continue;
        }
        let op = OPERATORS.iter().find(|o| {
            let oc: Vec<char> = o.chars().collect();
            text.get(i..i + oc.len()) == Some(oc.as_slice())
        });
        let Some(op) = op else {
            return Err(format!(
                "无法识别的字符 {}",
                serde_json::to_string(&ch.to_string()).unwrap_or_default()
            ));
        };
        tokens.push(Token {
            kind: Kind::Punct,
            text: (*op).to_owned(),
        });
        i += op.chars().count();
    }
    Ok(tokens)
}

fn text_at(t: &[Token], i: usize) -> Option<&str> {
    t.get(i).map(|x| x.text.as_str())
}

fn skip_template(t: &[Token], start: usize) -> R<usize> {
    let mut depth: i64 = 0;
    for (i, tok) in t.iter().enumerate().skip(start) {
        match tok.text.as_str() {
            "<" => depth += 1,
            ">" => depth -= 1,
            ">>" => depth -= 2,
            _ => {}
        }
        if depth <= 0 {
            return Ok(i + 1);
        }
    }
    Err("模板参数未闭合".into())
}

fn skip_balanced(t: &[Token], start: usize, open: &str, close: &str) -> R<usize> {
    let mut depth: i64 = 0;
    for (i, tok) in t.iter().enumerate().skip(start) {
        if tok.text == open {
            depth += 1;
        } else if tok.text == close {
            depth -= 1;
            if depth == 0 {
                return Ok(i + 1);
            }
        }
    }
    Err(format!("括号 {open}{close} 不配对"))
}

fn cost_of(t: &[Token], fn_names: &BTreeSet<String>) -> R<(u32, Vec<String>)> {
    let mut cost = 0u32;
    let mut calls = Vec::new();
    let mut i = 0usize;
    while i < t.len() {
        let tok = &t[i];
        let next = text_at(t, i + 1);
        match tok.kind {
            Kind::Ident => {
                if CONTROL_KEYWORDS.contains(&tok.text.as_str()) {
                    cost += 1;
                } else if next == Some("<") && TEMPLATE_TYPES.contains(&tok.text.as_str()) {
                    let after = skip_template(t, i + 1)?;
                    if text_at(t, after) == Some("(") {
                        cost += 1; // 带模板参数的构造器调用
                    }
                    i = after;
                    continue;
                } else if next == Some("(") {
                    cost += 1;
                    if fn_names.contains(&tok.text) {
                        calls.push(tok.text.clone());
                    }
                }
            }
            Kind::Punct => {
                if tok.text == "@" {
                    // 函数体内的属性（极少见）不计入
                    i += 1;
                    if text_at(t, i + 1) == Some("(") {
                        i = skip_balanced(t, i + 1, "(", ")")? - 1;
                    }
                } else if tok.text == "[" || !NON_COUNTING_PUNCT.contains(&tok.text.as_str()) {
                    cost += 1;
                }
            }
            Kind::Number => {}
        }
        i += 1;
    }
    Ok((cost, calls))
}

struct FnInfo {
    name: String,
    attributes: Vec<String>,
    own_cost: u32,
    calls: Vec<String>,
}

fn parse_functions(t: &[Token]) -> R<Vec<FnInfo>> {
    let mut names = BTreeSet::new();
    for i in 0..t.len().saturating_sub(1) {
        if t[i].text == "fn" && t[i + 1].kind == Kind::Ident {
            names.insert(t[i + 1].text.clone());
        }
    }
    let mut fns = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < t.len() {
        let tx = t[i].text.as_str();
        if tx == "@" {
            pending.push(text_at(t, i + 1).unwrap_or_default().to_owned());
            i += 2;
            if text_at(t, i) == Some("(") {
                i = skip_balanced(t, i, "(", ")")?;
            }
            continue;
        }
        if tx == "fn" {
            let name = text_at(t, i + 1).unwrap_or_default().to_owned();
            let mut j = i + 2;
            if text_at(t, j) != Some("(") {
                return Err(format!("函数 {name} 缺少参数列表"));
            }
            j = skip_balanced(t, j, "(", ")")?;
            while j < t.len() && t[j].text != "{" {
                j += 1;
            }
            let end = skip_balanced(t, j, "{", "}")?;
            let (own_cost, calls) = cost_of(&t[j + 1..end - 1], &names)?;
            fns.push(FnInfo {
                name,
                attributes: std::mem::take(&mut pending),
                own_cost,
                calls,
            });
            i = end;
            continue;
        }
        if tx == ";" || tx == "}" {
            pending.clear();
        }
        i += 1;
    }
    Ok(fns)
}

/// 审查视觉包着色器；通过时返回报告，违规时返回拒绝原因（对应 `0x9004`）
pub fn review_rview_shader(source: &str, trust: TrustLevel) -> R<WgslReviewReport> {
    if source.len() > WGSL_MAX_SOURCE_BYTES {
        return Err(format!("着色器源码超过 {WGSL_MAX_SOURCE_BYTES} 字节"));
    }
    let t = tokenize(source)?;
    if trust >= TrustLevel::VerifiedCreator {
        for (i, tok) in t.iter().enumerate() {
            if tok.kind != Kind::Ident {
                continue;
            }
            let is_attr = i > 0 && t[i - 1].text == "@";
            let s = tok.text.as_str();
            if is_attr && WGSL_FORBIDDEN_ATTRIBUTES.contains(&s) {
                return Err(format!("禁止的入口属性 @{s}"));
            }
            if !is_attr && WGSL_FORBIDDEN_IDENTIFIERS.contains(&s) {
                return Err(format!("禁止使用 {s}"));
            }
            if trust == TrustLevel::UntrustedEphemeral && WGSL_LOOP_KEYWORDS.contains(&s) {
                return Err(format!("TrustLevel 3 禁止循环（{s}）"));
            }
        }
    }
    let fns = parse_functions(&t)?;
    let entries: Vec<&FnInfo> = fns
        .iter()
        .filter(|f| f.attributes.iter().any(|a| a == "fragment"))
        .collect();
    if entries.len() != 1 {
        return Err(format!(
            "视觉包着色器必须恰好包含一个 @fragment 入口（实际 {} 个）",
            entries.len()
        ));
    }
    let entry = entries[0];
    let by_name: BTreeMap<&str, &FnInfo> = fns.iter().map(|f| (f.name.as_str(), f)).collect();
    if by_name.len() != fns.len() {
        return Err("函数重名".into());
    }
    fn total<'a>(
        name: &'a str,
        by_name: &BTreeMap<&'a str, &'a FnInfo>,
        memo: &mut BTreeMap<&'a str, u32>,
        visiting: &mut BTreeSet<&'a str>,
    ) -> R<u32> {
        if let Some(v) = memo.get(name) {
            return Ok(*v);
        }
        if !visiting.insert(name) {
            return Err(format!("检测到递归调用 {name}"));
        }
        let f = by_name.get(name);
        let mut sum = f.map_or(0, |f| f.own_cost);
        if let Some(f) = f {
            for callee in &f.calls {
                sum = sum.saturating_add(total(callee, by_name, memo, visiting)?);
            }
        }
        visiting.remove(name);
        memo.insert(name, sum);
        Ok(sum)
    }
    let count = total(&entry.name, &by_name, &mut BTreeMap::new(), &mut BTreeSet::new())?;
    let budget = trust.fragment_budget();
    if let Some(b) = budget {
        if count > b {
            return Err(format!(
                "片段着色器估算指令数 {count} 超过 TrustLevel {} 上限 {b}",
                trust as u8
            ));
        }
    }
    Ok(WgslReviewReport {
        entry_point: entry.name.clone(),
        instruction_count: count,
        budget,
        per_function: fns.iter().map(|f| (f.name.clone(), f.own_cost)).collect(),
    })
}

/// 服务端补充校验：naga 完整解析并验证（类型、绑定、入口签名）
pub fn validate_with_naga(source: &str) -> R<()> {
    let module = naga::front::wgsl::parse_str(source).map_err(|e| format!("WGSL 解析失败：{}", e.message()))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .map_err(|e| format!("WGSL 校验失败：{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_and_basic_counts() {
        assert_eq!(TrustLevel::UntrustedEphemeral.fragment_budget(), Some(512));
        assert_eq!(TrustLevel::VerifiedCreator.fragment_budget(), Some(2048));
        assert_eq!(TrustLevel::OfficialSigned.fragment_budget(), None);
        let src = "@fragment fn m() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }";
        let r = review_rview_shader(src, TrustLevel::UntrustedEphemeral).unwrap();
        assert_eq!(r.instruction_count, 2);
        assert_eq!(r.entry_point, "m");
        validate_with_naga(src).unwrap();
        assert!(validate_with_naga("@fragment fn m() -> @location(0) vec4<f32> { return 1; }").is_err());
    }

    #[test]
    fn comment_stripping_matches_ts() {
        assert_eq!(
            strip_wgsl_comments("a /* x /* y */ z */ b // c\nd").unwrap(),
            "a   b \nd"
        );
        assert!(strip_wgsl_comments("/* open").is_err());
    }
}
