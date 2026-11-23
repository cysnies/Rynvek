// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

//! Command / Observation / Event 三类协议信封（D149「Command/Event/StateMutation 三元分离」）。
//!
//! 信封只承载路由、因果与幂等元数据；具体业务负载由泛型参数 `P` 决定，
//! 由上层领域 crate 定义，保证协议底座不依赖任何游戏规则。

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::{Digest256, IdempotencyKey, WireUuid};

/// 客户端 / Agent 提交给权威世界的命令信封
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEnvelope<P> {
    /// UUIDv7 幂等键：服务端据此拒绝重复提交（`0x8002 DuplicateCommandIdempotencyKey`）
    pub idempotency_key: IdempotencyKey,
    /// 发起命令的 `Principal`
    #[serde(deserialize_with = "deserialize_principal_id")]
    pub principal_id: u64,
    /// 客户端发出命令时观察到的最新权威 Tick
    pub issued_at_tick: u64,
    /// 期望命令生效的 Tick（在该 Tick 的 Phase 01 入队校验）
    pub target_tick: u64,
    /// 业务负载
    pub payload: P,
}

// 原JSON数值输入兼容；JS客户端可发规范十进制字符串，禁止浮点截断u64身份。
fn deserialize_principal_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Number(u64),
        Text(String),
    }
    match Id::deserialize(d)? {
        Id::Number(n) => Ok(n),
        Id::Text(s) => {
            let n = s.parse::<u64>().map_err(serde::de::Error::custom)?;
            if s != n.to_string() {
                return Err(serde::de::Error::custom("主体须规范u64十进制"));
            }
            Ok(n)
        }
    }
}
/// 命令可接受的 Tick 时间窗
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandTickWindow {
    /// 允许 `target_tick` 落后当前 Tick 的最大 Tick 数
    pub max_past_ticks: u64,
    /// 允许 `target_tick` 超前当前 Tick 的最大 Tick 数
    pub max_future_ticks: u64,
}

/// 命令时间窗校验失败（对应错误码 `0x8003 CommandExpiredOrFutureTooFar`）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandWindowError {
    /// 目标 Tick 已过期
    Expired { target_tick: u64, current_tick: u64 },
    /// 目标 Tick 超前过多
    TooFarInFuture { target_tick: u64, current_tick: u64 },
}

impl fmt::Display for CommandWindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expired {
                target_tick,
                current_tick,
            } => {
                write!(
                    f,
                    "command target tick {target_tick} expired (current tick {current_tick})"
                )
            }
            Self::TooFarInFuture {
                target_tick,
                current_tick,
            } => {
                write!(
                    f,
                    "command target tick {target_tick} is too far ahead of current tick {current_tick}"
                )
            }
        }
    }
}

impl std::error::Error for CommandWindowError {}

impl<P> CommandEnvelope<P> {
    /// 校验 `target_tick` 是否落在以 `current_tick` 为中心的可接受时间窗内
    pub fn check_tick_window(&self, current_tick: u64, window: CommandTickWindow) -> Result<(), CommandWindowError> {
        let target_tick = self.target_tick;
        if target_tick.saturating_add(window.max_past_ticks) < current_tick {
            return Err(CommandWindowError::Expired {
                target_tick,
                current_tick,
            });
        }
        if target_tick > current_tick.saturating_add(window.max_future_ticks) {
            return Err(CommandWindowError::TooFarInFuture {
                target_tick,
                current_tick,
            });
        }
        Ok(())
    }
}

/// 权威世界在 `Phase 15` 发布给观察者的只读观测信封
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationEnvelope<P> {
    /// 世界实例 ID
    pub world_id: WireUuid,
    /// 观测对应的已提交 Tick
    pub tick: u64,
    /// 世界修订号
    pub world_revision: u64,
    /// 该 Tick 的 BLAKE3 状态根（`TickCommitManifest::state_root_hash`）
    pub state_root_hash: Digest256,
    /// 观测所属的 `Principal`（服务端按其视野执行战争迷雾裁剪）
    pub observer_principal_id: u64,
    /// 业务负载
    pub payload: P,
}

/// 事件因果全序键（D287「EventKey 因果全序」）。
///
/// 字段声明顺序即比较顺序：先 Tick，再阶段，再发射者，最后发射者内序号。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EventKey {
    /// 事件产生的 Tick
    pub tick: u64,
    /// 产生事件的阶段（`CanonicalPhaseId` 数值）
    pub phase_id: u16,
    /// 发射者实体序号（`EntityId` 低 64 位；系统事件为 0）
    pub emitter_serial: u64,
    /// 同一发射者在同一阶段内的单调序号
    pub sequence: u32,
}

/// 已提交的权威事件信封
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope<P> {
    /// 因果全序键
    pub key: EventKey,
    /// 点分事件类型，例如 `building.completed`
    pub event_kind: String,
    /// 业务负载
    pub payload: P,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const KEY: &str = "017f22e2-79b0-7cc3-98c4-dc0c0c07398f";

    fn command(target_tick: u64) -> CommandEnvelope<Value> {
        CommandEnvelope {
            idempotency_key: KEY.parse().unwrap(),
            principal_id: 42,
            issued_at_tick: 100,
            target_tick,
            payload: json!({"order_kind": "Move"}),
        }
    }

    const WINDOW: CommandTickWindow = CommandTickWindow {
        max_past_ticks: 2,
        max_future_ticks: 30,
    };

    #[test]
    fn command_json_shape_is_stable() {
        let value = serde_json::to_value(command(103)).unwrap();
        assert_eq!(
            value,
            json!({
                "idempotency_key": KEY,
                "principal_id": 42,
                "issued_at_tick": 100,
                "target_tick": 103,
                "payload": {"order_kind": "Move"}
            })
        );
        let back: CommandEnvelope<Value> = serde_json::from_value(value).unwrap();
        assert_eq!(back, command(103));
    }

    #[test]
    fn command_with_non_v7_key_is_rejected() {
        let mut value = serde_json::to_value(command(1)).unwrap();
        value["idempotency_key"] = json!("919108f7-52d1-4320-9bac-f847db4148a8");
        assert!(serde_json::from_value::<CommandEnvelope<Value>>(value).is_err());
    }

    #[test]
    fn tick_window_bounds_are_inclusive() {
        assert_eq!(command(98).check_tick_window(100, WINDOW), Ok(()));
        assert_eq!(command(130).check_tick_window(100, WINDOW), Ok(()));
        assert_eq!(
            command(97).check_tick_window(100, WINDOW),
            Err(CommandWindowError::Expired {
                target_tick: 97,
                current_tick: 100
            })
        );
        assert_eq!(
            command(131).check_tick_window(100, WINDOW),
            Err(CommandWindowError::TooFarInFuture {
                target_tick: 131,
                current_tick: 100
            })
        );
    }

    #[test]
    fn event_keys_order_causally() {
        let base = EventKey {
            tick: 10,
            phase_id: 9,
            emitter_serial: 5,
            sequence: 0,
        };
        let mut keys = vec![
            EventKey { sequence: 1, ..base },
            EventKey { tick: 9, ..base },
            EventKey { phase_id: 15, ..base },
            base,
            EventKey {
                emitter_serial: 1,
                ..base
            },
        ];
        keys.sort();
        assert_eq!(
            keys,
            vec![
                EventKey { tick: 9, ..base },
                EventKey {
                    emitter_serial: 1,
                    ..base
                },
                base,
                EventKey { sequence: 1, ..base },
                EventKey { phase_id: 15, ..base },
            ]
        );
    }

    #[test]
    fn observation_roundtrips_through_json() {
        let observation = ObservationEnvelope {
            world_id: KEY.parse().unwrap(),
            tick: 7,
            world_revision: 3,
            state_root_hash: Digest256([1; 32]),
            observer_principal_id: 42,
            payload: json!(null),
        };
        let text = serde_json::to_string(&observation).unwrap();
        assert!(text.contains(&"01".repeat(32)));
        assert_eq!(
            serde_json::from_str::<ObservationEnvelope<Value>>(&text).unwrap(),
            observation
        );
    }

    #[test]
    fn event_envelope_roundtrips_through_json() {
        let event = EventEnvelope {
            key: EventKey::default(),
            event_kind: "unit.spawned".to_owned(),
            payload: 1u8,
        };
        let text = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<EventEnvelope<u8>>(&text).unwrap(), event);
    }
}

#[cfg(test)]
mod decimal_identity_tests {
    use super::*;
    #[test]
    fn exact_u64_string_accepts_without_changing_original_numeric_encoding() {
        let base = serde_json::json!({"idempotency_key":"019aabbc-dddd-7eee-8fff-000000000001","principal_id":"18446744073709551615","issued_at_tick":0,"target_tick":1,"payload":[]});
        let value: CommandEnvelope<Vec<u8>> = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(value.principal_id, u64::MAX);
        assert_eq!(
            serde_json::to_value(value).unwrap()["principal_id"],
            serde_json::json!(u64::MAX)
        );
        for id in ["01", "-1", "18446744073709551616", "1.0"] {
            let mut invalid = base.clone();
            invalid["principal_id"] = serde_json::json!(id);
            assert!(serde_json::from_value::<CommandEnvelope<Vec<u8>>>(invalid).is_err());
        }
        let mut numeric = base;
        numeric["principal_id"] = serde_json::json!(42);
        assert_eq!(
            serde_json::from_value::<CommandEnvelope<Vec<u8>>>(numeric)
                .unwrap()
                .principal_id,
            42
        );
    }
}
