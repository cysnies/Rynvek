// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 6 个标准受控工具的强类型调用。
//!
//! 参数先经 [`crate::schema`] 校验，再在此转换为定点整数：LLM 给出的浮点数只在边界处按
//! "四舍五入、远离零" 一次性量化为 milli 单位，之后宿主与权威层全程整数（D0 确定性）。
//! 实体 ID、原型与科技 ID 保持文本，由 Tier 4 宿主解析（本 crate 不依赖 `rynvek-rts`）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::control::ControlLevel;
use crate::error::AgentError;

/// 工具名
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolName {
    QueryVisibleClusters,
    SampleRegionalFields,
    IssueStrategicIntent,
    ConfigureSignalBeacon,
    QueueVatProduction,
    TriggerCrucibleBreeding,
}

impl ToolName {
    /// 规范 9.1 清单顺序
    pub const ALL: [Self; 6] = [
        Self::QueryVisibleClusters,
        Self::SampleRegionalFields,
        Self::IssueStrategicIntent,
        Self::ConfigureSignalBeacon,
        Self::QueueVatProduction,
        Self::TriggerCrucibleBreeding,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QueryVisibleClusters => "query_visible_clusters",
            Self::SampleRegionalFields => "sample_regional_fields",
            Self::IssueStrategicIntent => "issue_strategic_intent",
            Self::ConfigureSignalBeacon => "configure_signal_beacon",
            Self::QueueVatProduction => "queue_vat_production",
            Self::TriggerCrucibleBreeding => "trigger_crucible_breeding",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == text)
    }

    /// 调用该工具所需的最低控制等级（ERRATA E-030）
    pub const fn required_level(self) -> ControlLevel {
        match self {
            Self::QueryVisibleClusters | Self::SampleRegionalFields => ControlLevel::Perceive,
            Self::IssueStrategicIntent | Self::ConfigureSignalBeacon => ControlLevel::Modulate,
            Self::QueueVatProduction | Self::TriggerCrucibleBreeding => ControlLevel::Produce,
        }
    }

    /// 是否产生权威命令（计入 600 APM）；感知类查询不计
    pub const fn is_actuating(self) -> bool {
        !matches!(self, Self::QueryVisibleClusters | Self::SampleRegionalFields)
    }
}

/// 坩埚动作
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrucibleToolAction {
    ResearchTech,
    BreedEliteGenome,
}

/// 强类型工具调用
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    QueryVisibleClusters {
        region_chunk: Option<(i64, i64)>,
        include_enemy: bool,
    },
    SampleRegionalFields {
        chunk_x: i64,
        chunk_y: i64,
    },
    IssueStrategicIntent {
        /// `"all"` 或 `unit.*`
        unit_archetype: String,
        order_kind: String,
        /// 目标点（milli 世界单位，1 世界单位 = 1 Tile）
        target_world_milli: (i64, i64),
    },
    ConfigureSignalBeacon {
        beacon_entity_id: String,
        signal_channel: String,
        /// `[-50_000, 50_000]`
        signal_strength_milli: i64,
        /// `[DA, 5-HT, NE, ACh]`，各 `[-1000, 1000]`
        neuromodulators_milli: [i64; 4],
    },
    QueueVatProduction {
        vat_entity_id: String,
        unit_archetype: String,
        genome_slug: String,
        count: u32,
    },
    TriggerCrucibleBreeding {
        crucible_entity_id: String,
        action: CrucibleToolAction,
        tech_or_archetype_id: String,
        /// 缺省时由宿主取默认突变率
        mutation_rate_permille: Option<u32>,
    },
}

/// 世界坐标量化上限（milli）：超出 i32 Tile 范围的坐标视为非法
const MAX_WORLD_MILLI: f64 = 2_147_483_647_000.0;

/// 浮点 → milli 整数（四舍五入、远离零）
fn to_milli(x: f64) -> i64 {
    (x * 1000.0).round() as i64
}

fn str_field(args: &Value, key: &str) -> Result<String, AgentError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| AgentError::invalid_arguments(format!("缺少字符串字段 `{key}`")))
}

fn num_field(args: &Value, key: &str) -> Result<f64, AgentError> {
    args.get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| AgentError::invalid_arguments(format!("缺少数值字段 `{key}`")))
}

fn int_field(args: &Value, key: &str) -> Result<i64, AgentError> {
    let v = num_field(args, key)?;
    if v.fract() != 0.0 || v.abs() > i64::MAX as f64 / 2.0 {
        return Err(AgentError::invalid_arguments(format!("`{key}` 必须是整数")));
    }
    Ok(v as i64)
}

impl ToolCall {
    pub fn name(&self) -> ToolName {
        match self {
            Self::QueryVisibleClusters { .. } => ToolName::QueryVisibleClusters,
            Self::SampleRegionalFields { .. } => ToolName::SampleRegionalFields,
            Self::IssueStrategicIntent { .. } => ToolName::IssueStrategicIntent,
            Self::ConfigureSignalBeacon { .. } => ToolName::ConfigureSignalBeacon,
            Self::QueueVatProduction { .. } => ToolName::QueueVatProduction,
            Self::TriggerCrucibleBreeding { .. } => ToolName::TriggerCrucibleBreeding,
        }
    }

    /// 由**已通过 Schema 校验**的参数构造强类型调用
    pub fn from_arguments(name: ToolName, args: &Value) -> Result<Self, AgentError> {
        Ok(match name {
            ToolName::QueryVisibleClusters => {
                let region_chunk = match args.get("region_chunk").and_then(Value::as_array) {
                    Some(a) if a.len() == 2 => {
                        let c = |i: usize| {
                            a[i].as_f64()
                                .filter(|v| v.fract() == 0.0)
                                .map(|v| v as i64)
                                .ok_or_else(|| AgentError::invalid_arguments("region_chunk 必须是整数对"))
                        };
                        Some((c(0)?, c(1)?))
                    }
                    Some(_) => return Err(AgentError::invalid_arguments("region_chunk 必须是整数对")),
                    None => None,
                };
                let include_enemy = args
                    .get("include_enemy")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| AgentError::invalid_arguments("缺少布尔字段 `include_enemy`"))?;
                Self::QueryVisibleClusters {
                    region_chunk,
                    include_enemy,
                }
            }
            ToolName::SampleRegionalFields => Self::SampleRegionalFields {
                chunk_x: int_field(args, "chunk_x")?,
                chunk_y: int_field(args, "chunk_y")?,
            },
            ToolName::IssueStrategicIntent => {
                let (x, y) = (num_field(args, "target_world_x")?, num_field(args, "target_world_y")?);
                if !(x.abs() * 1000.0 <= MAX_WORLD_MILLI && y.abs() * 1000.0 <= MAX_WORLD_MILLI) {
                    return Err(AgentError::invalid_arguments("目标坐标超出世界范围"));
                }
                Self::IssueStrategicIntent {
                    unit_archetype: str_field(args, "unit_archetype")?,
                    order_kind: str_field(args, "order_kind")?,
                    target_world_milli: (to_milli(x), to_milli(y)),
                }
            }
            ToolName::ConfigureSignalBeacon => {
                let mods = args
                    .get("neuromodulators")
                    .and_then(Value::as_array)
                    .filter(|a| a.len() == 4)
                    .ok_or_else(|| AgentError::invalid_arguments("neuromodulators 必须是 4 维数组"))?;
                let mut neuromodulators_milli = [0i64; 4];
                for (slot, v) in neuromodulators_milli.iter_mut().zip(mods) {
                    let v = v
                        .as_f64()
                        .ok_or_else(|| AgentError::invalid_arguments("调质分量必须是数值"))?;
                    *slot = to_milli(v).clamp(-1000, 1000);
                }
                Self::ConfigureSignalBeacon {
                    beacon_entity_id: str_field(args, "beacon_entity_id")?,
                    signal_channel: str_field(args, "signal_channel")?,
                    signal_strength_milli: to_milli(num_field(args, "signal_strength")?).clamp(-50_000, 50_000),
                    neuromodulators_milli,
                }
            }
            ToolName::QueueVatProduction => Self::QueueVatProduction {
                vat_entity_id: str_field(args, "vat_entity_id")?,
                unit_archetype: str_field(args, "unit_archetype")?,
                genome_slug: str_field(args, "genome_slug")?,
                count: int_field(args, "count")?.clamp(1, 5) as u32,
            },
            ToolName::TriggerCrucibleBreeding => {
                let action = match str_field(args, "action")?.as_str() {
                    "research_tech" => CrucibleToolAction::ResearchTech,
                    "breed_elite_genome" => CrucibleToolAction::BreedEliteGenome,
                    other => return Err(AgentError::invalid_arguments(format!("未知坩埚动作 `{other}`"))),
                };
                let mutation_rate_permille = match args.get("mutation_rate") {
                    Some(_) => Some(to_milli(num_field(args, "mutation_rate")?).clamp(10, 250) as u32),
                    None => None,
                };
                Self::TriggerCrucibleBreeding {
                    crucible_entity_id: str_field(args, "crucible_entity_id")?,
                    action,
                    tech_or_archetype_id: str_field(args, "tech_or_archetype_id")?,
                    mutation_rate_permille,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn names_round_trip_and_levels() {
        for t in ToolName::ALL {
            assert_eq!(ToolName::parse(t.as_str()), Some(t));
        }
        assert!(!ToolName::QueryVisibleClusters.is_actuating());
        assert!(ToolName::QueueVatProduction.is_actuating());
        assert_eq!(
            ToolName::TriggerCrucibleBreeding.required_level(),
            ControlLevel::Produce
        );
    }

    #[test]
    fn floats_quantize_once_at_the_boundary() {
        let c = ToolCall::from_arguments(
            ToolName::ConfigureSignalBeacon,
            &json!({"beacon_entity_id": "9", "signal_channel": "field.signal_b", "signal_strength": -12.3456, "neuromodulators": [0.0005, -0.0005, 1.0, -0.25]}),
        )
        .unwrap();
        assert_eq!(
            c,
            ToolCall::ConfigureSignalBeacon {
                beacon_entity_id: "9".into(),
                signal_channel: "field.signal_b".into(),
                signal_strength_milli: -12_346,
                neuromodulators_milli: [1, -1, 1000, -250],
            }
        );
        let c = ToolCall::from_arguments(
            ToolName::TriggerCrucibleBreeding,
            &json!({"crucible_entity_id": "3", "action": "breed_elite_genome", "tech_or_archetype_id": "unit.striker", "mutation_rate": 0.05}),
        )
        .unwrap();
        assert!(matches!(
            c,
            ToolCall::TriggerCrucibleBreeding {
                mutation_rate_permille: Some(50),
                ..
            }
        ));
        let c = ToolCall::from_arguments(
            ToolName::IssueStrategicIntent,
            &json!({"unit_archetype": "all", "order_kind": "Move", "target_world_x": 10.5, "target_world_y": 3}),
        )
        .unwrap();
        assert!(matches!(
            c,
            ToolCall::IssueStrategicIntent {
                target_world_milli: (10_500, 3_000),
                ..
            }
        ));
        let c = ToolCall::from_arguments(
            ToolName::QueryVisibleClusters,
            &json!({"include_enemy": false, "region_chunk": [1, 2]}),
        )
        .unwrap();
        assert_eq!(c.name(), ToolName::QueryVisibleClusters);
        assert!(matches!(
            c,
            ToolCall::QueryVisibleClusters {
                region_chunk: Some((1, 2)),
                include_enemy: false
            }
        ));
    }
}
