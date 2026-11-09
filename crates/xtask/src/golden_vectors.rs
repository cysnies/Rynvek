// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 跨语言确定性 Golden Test Vectors 校验器（规范 12 §12.3）。
//!
//! 逐条读取基线文件中的向量，用 `rynvek-core` 的权威实现重新计算，并与期望值逐位比对。
//! 任何一比特不匹配都视为实现缺陷。64 位整数在 JSON 中以十进制字符串存储，以避免 JS 双精度截断。

use std::fmt;
use std::path::Path;

use rynvek_core::coord::WorldPosition2D;
use rynvek_core::fixed::Q32_32;
use rynvek_core::rng::philox4x32_10;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// 基线文件相对工作区根目录的路径
pub const BASELINE_RELATIVE_PATH: &str = "tests/golden_vectors/v1_baseline.json";

/// 基线文件要求的套件名
const EXPECTED_SUITE: &str = "rynvek.golden_vectors.v1";

/// 基线文件结构
#[derive(Debug, Deserialize)]
struct GoldenSuite {
    suite: String,
    vectors: Vec<RawVector>,
}

/// 未按类型解析的单条向量
#[derive(Debug, Deserialize)]
struct RawVector {
    id: String,
    inputs: Value,
    expected: Value,
}

/// 单条向量的校验结果
#[derive(Debug)]
pub struct VectorOutcome {
    pub id: String,
    /// `Ok(())` 表示通过；`Err` 携带不匹配说明
    pub result: Result<(), String>,
}

/// 整个套件的校验报告
#[derive(Debug)]
pub struct Report {
    pub suite: String,
    pub outcomes: Vec<VectorOutcome>,
}

impl Report {
    /// 是否全部通过
    pub fn all_passed(&self) -> bool {
        self.outcomes.iter().all(|outcome| outcome.result.is_ok())
    }

    /// 打印人类可读报告
    pub fn print(&self) {
        println!("Golden Vectors 套件：{}", self.suite);
        for outcome in &self.outcomes {
            match &outcome.result {
                Ok(()) => println!("  [通过] {}", outcome.id),
                Err(reason) => println!("  [失败] {}：{reason}", outcome.id),
            }
        }
        let passed = self.outcomes.iter().filter(|outcome| outcome.result.is_ok()).count();
        println!("结果：{passed}/{} 通过", self.outcomes.len());
    }
}

/// 加载错误
#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Json(serde_json::Error),
    WrongSuite(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "读取失败：{err}"),
            Self::Json(err) => write!(f, "JSON 解析失败：{err}"),
            Self::WrongSuite(suite) => write!(f, "套件名应为 {EXPECTED_SUITE}，实际为 {suite}"),
        }
    }
}

/// 读取并校验基线文件
pub fn verify_file(path: &Path) -> Result<Report, LoadError> {
    let text = std::fs::read_to_string(path).map_err(LoadError::Io)?;
    verify_str(&text)
}

/// 校验基线 JSON 文本
pub fn verify_str(text: &str) -> Result<Report, LoadError> {
    let suite: GoldenSuite = serde_json::from_str(text).map_err(LoadError::Json)?;
    if suite.suite != EXPECTED_SUITE {
        return Err(LoadError::WrongSuite(suite.suite));
    }
    let outcomes = suite
        .vectors
        .iter()
        .map(|vector| VectorOutcome {
            id: vector.id.clone(),
            result: verify_vector(vector),
        })
        .collect();
    Ok(Report {
        suite: suite.suite,
        outcomes,
    })
}

fn verify_vector(vector: &RawVector) -> Result<(), String> {
    match vector.id.as_str() {
        "GV-001-Q32_32_ARITHMETIC" => run_typed(vector, verify_q32_arithmetic),
        "GV-002-WORLD_POS_CANONICALIZE" => run_typed(vector, verify_world_pos_canonicalize),
        "GV-003-PHILOX4X32_10_ORIGIN" => run_typed(vector, verify_philox),
        "GV-004-Q8_8_LIF_STEP" => run_typed(vector, verify_lif_step),
        unknown => Err(format!("未知的向量 ID：{unknown}（校验器必须覆盖基线中的每一条向量）")),
    }
}

/// 把 `inputs` / `expected` 解析为具体类型后交给校验函数
fn run_typed<I, E>(vector: &RawVector, check: fn(I, E) -> Result<(), String>) -> Result<(), String>
where
    I: DeserializeOwned,
    E: DeserializeOwned,
{
    let inputs = I::deserialize(&vector.inputs).map_err(|err| format!("inputs 结构错误：{err}"))?;
    let expected = E::deserialize(&vector.expected).map_err(|err| format!("expected 结构错误：{err}"))?;
    check(inputs, expected)
}

/// 比较单个字段，不匹配时给出字段名与两侧数值
fn expect_eq<T: PartialEq + fmt::Debug>(field: &str, actual: T, expected: T) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{field} 不匹配：实际 {actual:?}，期望 {expected:?}"))
    }
}

/// 解析以十进制字符串存储的 i64
fn parse_i64(field: &str, text: &str) -> Result<i64, String> {
    text.parse()
        .map_err(|err| format!("{field} 不是合法的 i64 十进制字符串（{text}）：{err}"))
}

// ---------------------------------------------------------------------------
// GV-001：Q32_32 定点乘法、安全除法与确定性平方根
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Q32Inputs {
    a_raw_i64: String,
    b_raw_i64: String,
    sqrt_target_raw_i64: String,
}

#[derive(Deserialize)]
struct Q32Expected {
    mul_raw_i64: String,
    div_raw_i64: String,
    sqrt_raw_i64: String,
}

fn verify_q32_arithmetic(inputs: Q32Inputs, expected: Q32Expected) -> Result<(), String> {
    let a = Q32_32(parse_i64("a_raw_i64", &inputs.a_raw_i64)?);
    let b = Q32_32(parse_i64("b_raw_i64", &inputs.b_raw_i64)?);
    let sqrt_target = Q32_32(parse_i64("sqrt_target_raw_i64", &inputs.sqrt_target_raw_i64)?);

    expect_eq(
        "mul_raw_i64",
        a.mul(b).0,
        parse_i64("mul_raw_i64", &expected.mul_raw_i64)?,
    )?;
    // 除数非零，fallback 不会被使用；取 ZERO 便于在失败时识别
    expect_eq(
        "div_raw_i64",
        a.div_safe(b, Q32_32::ZERO).0,
        parse_i64("div_raw_i64", &expected.div_raw_i64)?,
    )?;
    expect_eq(
        "sqrt_raw_i64",
        sqrt_target.sqrt_deterministic().0,
        parse_i64("sqrt_raw_i64", &expected.sqrt_raw_i64)?,
    )
}

// ---------------------------------------------------------------------------
// GV-002：WorldPosition2D 跨 Chunk 规范化
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct WorldPosVector {
    chunk_x: i32,
    chunk_y: i32,
    local_x_raw: String,
    local_y_raw: String,
}

impl WorldPosVector {
    fn to_position(&self, prefix: &str) -> Result<WorldPosition2D, String> {
        Ok(WorldPosition2D::new(
            self.chunk_x,
            self.chunk_y,
            Q32_32(parse_i64(&format!("{prefix}.local_x_raw"), &self.local_x_raw)?),
            Q32_32(parse_i64(&format!("{prefix}.local_y_raw"), &self.local_y_raw)?),
        ))
    }
}

fn verify_world_pos_canonicalize(inputs: WorldPosVector, expected: WorldPosVector) -> Result<(), String> {
    let actual = inputs
        .to_position("inputs")?
        .canonicalize()
        .map_err(|code| format!("规范化失败：{code}"))?;
    expect_eq("canonical_position", actual, expected.to_position("expected")?)
}

// ---------------------------------------------------------------------------
// GV-003：Philox4x32-10 原点向量
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PhiloxInputs {
    ctr_u32x4: [u32; 4],
    key_u32x2: [u32; 2],
}

#[derive(Deserialize)]
struct PhiloxExpected {
    out_u32x4: [u32; 4],
}

fn verify_philox(inputs: PhiloxInputs, expected: PhiloxExpected) -> Result<(), String> {
    expect_eq(
        "out_u32x4",
        philox4x32_10(inputs.ctr_u32x4, inputs.key_u32x2),
        expected.out_u32x4,
    )
}

// ---------------------------------------------------------------------------
// GV-004：单步 Q8.8 LIF 神经元
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LifInputs {
    v_prev_q8: i32,
    leak_beta_q8: i32,
    syn_in_q8: i32,
    m_ne_q8: i32,
}

#[derive(Deserialize, Debug, PartialEq)]
struct LifExpected {
    theta_eff_q8: i32,
    v_candidate_q8: i32,
    spiked: bool,
    v_after_reset_q8: i32,
    refractory_ticks_set: u8,
}

/// 脉冲后进入的不应期 Tick 数（规范 05 §5.1）
const LIF_REFRACTORY_TICKS: u8 = 2;
/// 去甲肾上腺素阈值调制系数：0.30 × 256 ≈ 77（规范 05 §5.1）
const LIF_NE_THRESHOLD_GAIN_Q8: i32 = 77;
/// 基础发放阈值 1.0（Q8.8）
const LIF_BASE_THRESHOLD_Q8: i32 = 256;
/// 未发放时膜电位的钳制范围 [-2.0, +2.0]（Q8.8）
const LIF_MEMBRANE_CLAMP_Q8: i32 = 512;

/// 单个 LIF 神经元单步积分的参照计算。
///
/// 逐式对应规范 05 §5.1 `step_lif_brain_deterministic` 中单个隐藏神经元的整数运算；
/// M0 尚无 `rynvek-neural`，待其在 M3 落地后，此处改为直接调用其权威实现。
fn lif_reference_step(inputs: &LifInputs) -> LifExpected {
    let theta_eff = LIF_BASE_THRESHOLD_Q8 - ((LIF_NE_THRESHOLD_GAIN_Q8 * inputs.m_ne_q8) >> 8);
    let v_candidate = ((inputs.v_prev_q8 * inputs.leak_beta_q8) >> 8) + inputs.syn_in_q8;
    let spiked = v_candidate >= theta_eff;
    let (v_after, refractory) = if spiked {
        (0, LIF_REFRACTORY_TICKS)
    } else {
        (v_candidate.clamp(-LIF_MEMBRANE_CLAMP_Q8, LIF_MEMBRANE_CLAMP_Q8), 0)
    };
    LifExpected {
        theta_eff_q8: theta_eff,
        v_candidate_q8: v_candidate,
        spiked,
        v_after_reset_q8: v_after,
        refractory_ticks_set: refractory,
    }
}

fn verify_lif_step(inputs: LifInputs, expected: LifExpected) -> Result<(), String> {
    expect_eq("lif_step", lif_reference_step(&inputs), expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline_text() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(BASELINE_RELATIVE_PATH);
        std::fs::read_to_string(path).expect("基线文件必须存在")
    }

    #[test]
    fn baseline_passes() {
        let report = verify_str(&baseline_text()).unwrap();
        assert_eq!(report.outcomes.len(), 4);
        for outcome in &report.outcomes {
            assert_eq!(outcome.result, Ok(()), "{}", outcome.id);
        }
    }

    #[test]
    fn single_bit_flip_is_detected() {
        let tampered = baseline_text().replace("\"16106127360\"", "\"16106127361\"");
        let report = verify_str(&tampered).unwrap();
        assert!(!report.all_passed());
        assert!(report.outcomes[0].result.as_ref().unwrap_err().contains("mul_raw_i64"));
    }

    #[test]
    fn unknown_vectors_and_suites_are_rejected() {
        let unknown = r#"{"suite":"rynvek.golden_vectors.v1","vectors":[{"id":"GV-999","inputs":{},"expected":{}}]}"#;
        assert!(!verify_str(unknown).unwrap().all_passed());
        let wrong_suite = r#"{"suite":"other","vectors":[]}"#;
        assert!(matches!(verify_str(wrong_suite), Err(LoadError::WrongSuite(_))));
    }

    #[test]
    fn lif_reference_handles_subthreshold_input() {
        let inputs = LifInputs {
            v_prev_q8: 0,
            leak_beta_q8: 230,
            syn_in_q8: 10,
            m_ne_q8: 0,
        };
        let outcome = lif_reference_step(&inputs);
        assert!(!outcome.spiked);
        assert_eq!(outcome.v_after_reset_q8, 10);
        assert_eq!(outcome.refractory_ticks_set, 0);
    }
}
