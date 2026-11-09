// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 规范 9.1 的 6 个标准工具 JSON Schema 与 Draft 2020-12 子集校验器。
//!
//! 工具清单原文位于 `protocols/schemas/agent-tools.v1.json`（`Apache-2.0 WITH LLVM-exception`），
//! 编译期内嵌，集成测试逐字节比对规范代码块。
//!
//! 校验器只实现清单用到的关键字：`type`、`enum`、`minimum`、`maximum`、`minItems`、`maxItems`、
//! `items`、`properties`、`required`、`additionalProperties`。对象 Schema 未声明 `additionalProperties`
//! 时按**严格模式**拒绝未知字段（Draft 2020-12 缺省允许；LLM 幻觉字段应尽早暴露，ERRATA E-030）。

use serde::Serialize;
use serde_json::Value;

use crate::error::AgentError;

/// 规范 9.1 工具清单原文
pub const AGENT_TOOLS_V1_JSON: &str = include_str!("../../../protocols/schemas/agent-tools.v1.json");

/// 单个工具声明
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// 工具注册表（保持清单原始顺序）
#[derive(Clone, Debug, PartialEq)]
pub struct ToolRegistry {
    tools: Vec<ToolSchema>,
}

impl ToolRegistry {
    /// 加载内嵌的 v1 清单
    pub fn v1() -> Self {
        Self::from_json(AGENT_TOOLS_V1_JSON).expect("内嵌 agent-tools.v1.json 必须合法")
    }

    /// 从 JSON 文本解析工具清单
    pub fn from_json(text: &str) -> Result<Self, AgentError> {
        let value: Value = serde_json::from_str(text).map_err(|e| AgentError::invalid_arguments(e.to_string()))?;
        let list = value
            .as_array()
            .ok_or_else(|| AgentError::invalid_arguments("工具清单必须是数组"))?;
        let mut tools = Vec::with_capacity(list.len());
        for item in list {
            let field = |key: &str| item.get(key).and_then(Value::as_str).map(str::to_owned);
            let (Some(name), Some(description), Some(parameters)) =
                (field("name"), field("description"), item.get("parameters").cloned())
            else {
                return Err(AgentError::invalid_arguments(
                    "工具声明缺少 name / description / parameters",
                ));
            };
            tools.push(ToolSchema {
                name,
                description,
                parameters,
            });
        }
        Ok(Self { tools })
    }

    pub fn tools(&self) -> &[ToolSchema] {
        &self.tools
    }

    pub fn get(&self, name: &str) -> Option<&ToolSchema> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// 按 Schema 校验调用参数
    pub fn validate(&self, name: &str, arguments: &Value) -> Result<(), AgentError> {
        let tool = self
            .get(name)
            .ok_or_else(|| AgentError::invalid_arguments(format!("未知工具 `{name}`")))?;
        validate(&tool.parameters, arguments, "$")
    }
}

/// 递归校验 `value` 是否满足 `schema`；`path` 为 JSONPath 风格定位，写入错误信息
pub fn validate(schema: &Value, value: &Value, path: &str) -> Result<(), AgentError> {
    let fail = |reason: String| Err(AgentError::invalid_arguments(format!("{path}: {reason}")));

    if let Some(ty) = schema.get("type") {
        let ok = match ty {
            Value::String(t) => type_matches(t, value),
            Value::Array(ts) => ts.iter().filter_map(Value::as_str).any(|t| type_matches(t, value)),
            _ => false,
        };
        if !ok {
            return fail(format!("类型应为 {ty}"));
        }
    }
    if let Some(Value::Array(options)) = schema.get("enum") {
        if !options.iter().any(|o| json_equal(o, value)) {
            return fail(format!("取值必须属于枚举 {}", Value::Array(options.clone())));
        }
    }
    if let Some(x) = value.as_f64() {
        if let Some(min) = schema.get("minimum").and_then(Value::as_f64).filter(|&min| x < min) {
            return fail(format!("{x} 小于下限 {min}"));
        }
        if let Some(max) = schema.get("maximum").and_then(Value::as_f64).filter(|&max| x > max) {
            return fail(format!("{x} 大于上限 {max}"));
        }
    }
    if let Value::Array(items) = value {
        if let Some(min) = schema
            .get("minItems")
            .and_then(Value::as_u64)
            .filter(|&min| (items.len() as u64) < min)
        {
            return fail(format!("元素个数 {} 少于 {min}", items.len()));
        }
        if let Some(max) = schema
            .get("maxItems")
            .and_then(Value::as_u64)
            .filter(|&max| (items.len() as u64) > max)
        {
            return fail(format!("元素个数 {} 多于 {max}", items.len()));
        }
        if let Some(item_schema) = schema.get("items") {
            for (i, item) in items.iter().enumerate() {
                validate(item_schema, item, &format!("{path}[{i}]"))?;
            }
        }
    }
    if let Value::Object(map) = value {
        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(Value::Array(required)) = schema.get("required") {
            for key in required.iter().filter_map(Value::as_str) {
                if !map.contains_key(key) {
                    return fail(format!("缺少必填字段 `{key}`"));
                }
            }
        }
        let allow_extra = schema
            .get("additionalProperties")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        for (key, v) in map {
            match properties.and_then(|p| p.get(key)) {
                Some(sub) => validate(sub, v, &format!("{path}.{key}"))?,
                None if !allow_extra => return fail(format!("未声明的字段 `{key}`")),
                None => {}
            }
        }
    }
    Ok(())
}

/// Draft 2020-12 类型判定：`integer` 接受小数部分为 0 的数（如 `3.0`）
fn type_matches(ty: &str, value: &Value) -> bool {
    match ty {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "integer" => {
            value.is_i64() || value.is_u64() || value.as_f64().is_some_and(|x| x.is_finite() && x.fract() == 0.0)
        }
        _ => false,
    }
}

/// 数值按数学值比较（`1` 与 `1.0` 相等），其余按结构比较
fn json_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn registry_lists_six_tools_in_spec_order() {
        let r = ToolRegistry::v1();
        let names: Vec<&str> = r.tools().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "query_visible_clusters",
                "sample_regional_fields",
                "issue_strategic_intent",
                "configure_signal_beacon",
                "queue_vat_production",
                "trigger_crucible_breeding"
            ]
        );
    }

    #[test]
    fn validator_enforces_keywords() {
        let r = ToolRegistry::v1();
        let ok = json!({"beacon_entity_id": "7", "signal_channel": "field.signal_a", "signal_strength": 12.5, "neuromodulators": [0.5, 0, -1, 1]});
        r.validate("configure_signal_beacon", &ok).unwrap();
        let cases = [
            json!({"beacon_entity_id": "7", "signal_channel": "field.hazard", "signal_strength": 1, "neuromodulators": [0, 0, 0, 0]}),
            json!({"beacon_entity_id": "7", "signal_channel": "field.signal_a", "signal_strength": 50.5, "neuromodulators": [0, 0, 0, 0]}),
            json!({"beacon_entity_id": "7", "signal_channel": "field.signal_a", "signal_strength": 1, "neuromodulators": [0, 0, 0]}),
            json!({"beacon_entity_id": "7", "signal_channel": "field.signal_a", "signal_strength": 1, "neuromodulators": [0, 0, 0, 1.5]}),
            json!({"beacon_entity_id": 7, "signal_channel": "field.signal_a", "signal_strength": 1, "neuromodulators": [0, 0, 0, 0]}),
            json!({"signal_channel": "field.signal_a", "signal_strength": 1, "neuromodulators": [0, 0, 0, 0]}),
            json!({"beacon_entity_id": "7", "signal_channel": "field.signal_a", "signal_strength": 1, "neuromodulators": [0, 0, 0, 0], "extra": true}),
        ];
        for c in cases {
            let e = r.validate("configure_signal_beacon", &c).unwrap_err();
            assert_eq!(e.code.code(), 0x5001, "{c}");
        }
        r.validate("sample_regional_fields", &json!({"chunk_x": 1.0, "chunk_y": 0}))
            .unwrap();
        assert!(
            r.validate("sample_regional_fields", &json!({"chunk_x": 1.5, "chunk_y": 0}))
                .is_err()
        );
        assert!(
            r.validate(
                "queue_vat_production",
                &json!({"vat_entity_id": "1", "unit_archetype": "unit.forager", "genome_slug": "g", "count": 6})
            )
            .is_err()
        );
        assert!(r.validate("no_such_tool", &json!({})).is_err());
        assert!(r.validate("sample_regional_fields", &json!([1, 2])).is_err());
    }
}
