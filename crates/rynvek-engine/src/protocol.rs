// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! React 主线程 ⇄ Dedicated Engine Worker 消息协议（规范 07 §7.1 第 1 条）。
//!
//! 线上格式为 `postMessage` 的结构化克隆对象：`{ type: "ENGINE_INIT", ... }`。
//! `type` 取 SCREAMING_SNAKE_CASE，字段取 camelCase；`ENGINE_INIT` 中的 `canvas: OffscreenCanvas`
//! 由 `rynvek-wasm` 单独提取（不可序列化），其余字段经 JSON 映射到这里的强类型枚举。
//! `@rynvek/engine` 的 `protocol.ts` 与本文件逐字段一一对应（ERRATA E-016）。

use serde::{Deserialize, Serialize};

/// GPU 使用策略
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuMode {
    /// 有设备则启用 GPU 镜像与渲染，无设备则以 CPU Reference 运行
    #[default]
    Auto,
    /// 仅 CPU Reference（无头 / 研究批处理）
    Disabled,
    /// 必须获得 GPU，否则 `ENGINE_INIT` 失败（0x6001）
    Required,
}

/// 计算后端
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComputeBackendKind {
    /// 权威 CPU Reference + GPU 驻留镜像
    Webgpu,
    /// 仅 CPU Reference（未启用 GPU 或 DeviceLost 后降级）
    CpuReference,
}

/// 主线程 → Worker 命令
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
pub enum EngineCommand {
    /// 画布已随消息转移（`[offscreen]`）；宽高为设备像素
    EngineInit {
        width: u32,
        height: u32,
        #[serde(default = "default_dpr")]
        device_pixel_ratio: f32,
        /// 32 位十六进制世界 ID（缺省全零）
        #[serde(default)]
        world_id: Option<String>,
        #[serde(default)]
        gpu_mode: GpuMode,
        /// 远程竞技场只读显示：绑定阵营，禁止推进本地演示世界。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arena_faction: Option<u8>,
    },
    Resize {
        width: u32,
        height: u32,
        #[serde(default = "default_dpr")]
        device_pixel_ratio: f32,
    },
    /// 推进若干 Tick（暂停状态下单步）
    Step {
        #[serde(default = "default_ticks")]
        ticks: u32,
    },
    /// 以固定 Tick 率连续运行（缺省 30 TPS）
    Start {
        #[serde(default = "default_tick_rate")]
        tick_rate: u32,
    },
    Pause,
    SetCamera {
        center_x: f32,
        center_y: f32,
        px_per_cell: f32,
    },
    /// 画布像素坐标拾取
    PickRequest {
        request_id: u32,
        x: u32,
        y: u32,
    },
    /// L0–L7 开关位集
    SetLayerMask {
        mask: u8,
    },
    /// 热力图通道（`nutrient` / `energy` / `signal_a` / `signal_b` / `hazard`）
    SetHeatmapChannel {
        channel: String,
    },
    /// 故障注入：主动销毁设备以演练 DeviceLost 恢复
    SimulateDeviceLost,
    /// 服务器已经按阵营裁剪的观察；不是权威世界或本地仿真输入。
    ArenaObservation {
        world_id: String,
        observation: Box<rynvek_rts::observation::FactionObservation>,
    },
    Shutdown,
}

fn default_dpr() -> f32 {
    1.0
}

fn default_ticks() -> u32 {
    1
}

fn default_tick_rate() -> u32 {
    30
}

/// Worker → 主线程事件
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
pub enum EngineEvent {
    EngineReady {
        backend: ComputeBackendKind,
        /// `TierA` … `TierD`；无 GPU 时为 `None`
        profile: Option<String>,
        adapter: Option<String>,
        generation: u32,
    },
    TickCommitted {
        tick: u64,
        /// 64 位十六进制状态根
        state_root: String,
    },
    FrameRendered {
        tick: u64,
    },
    PickResult {
        request_id: u32,
        /// 被拾取实体的稠密索引；未命中为 `None`
        dense_index: Option<u32>,
    },
    DeviceLost {
        tick: u64,
        reason: String,
    },
    /// 恢复进度：`step` 为刚完成的步骤（1–5）
    RecoveryProgress {
        step: u8,
        phase: String,
    },
    Recovered {
        generation: u32,
        resume_tick: u64,
    },
    BackendDegraded {
        reason: String,
    },
    /// GPU 镜像与 CPU 权威结果不一致（已自动从 CPU 重新同步）
    GpuDivergence {
        tick: u64,
        channel: String,
    },
    EngineError {
        code: u16,
        message: String,
    },
}

impl EngineCommand {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

impl EngineEvent {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("事件序列化不会失败")
    }
}

/// 字节 → 小写十六进制
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 32 位十六进制 → 16 字节世界 ID
pub fn parse_world_id(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_init_uses_spec_wire_names_and_ignores_canvas() {
        let cmd = EngineCommand::from_json(
            r#"{"type":"ENGINE_INIT","canvas":{},"width":800,"height":600,"devicePixelRatio":2,"gpuMode":"required"}"#,
        )
        .unwrap();
        assert_eq!(
            cmd,
            EngineCommand::EngineInit {
                width: 800,
                height: 600,
                device_pixel_ratio: 2.0,
                world_id: None,
                gpu_mode: GpuMode::Required,
                arena_faction: None,
            }
        );
        let pick = EngineCommand::from_json(r#"{"type":"PICK_REQUEST","requestId":7,"x":1,"y":2}"#).unwrap();
        assert_eq!(
            pick,
            EngineCommand::PickRequest {
                request_id: 7,
                x: 1,
                y: 2
            }
        );
        assert_eq!(
            EngineCommand::from_json(r#"{"type":"STEP"}"#).unwrap(),
            EngineCommand::Step { ticks: 1 }
        );
        assert!(EngineCommand::from_json(r#"{"type":"NOPE"}"#).is_err());
    }

    #[test]
    fn events_serialize_with_camel_case_fields() {
        let ev = EngineEvent::PickResult {
            request_id: 3,
            dense_index: Some(9),
        };
        assert_eq!(ev.to_json(), r#"{"type":"PICK_RESULT","requestId":3,"denseIndex":9}"#);
        let ready = EngineEvent::EngineReady {
            backend: ComputeBackendKind::CpuReference,
            profile: None,
            adapter: None,
            generation: 0,
        };
        assert_eq!(
            ready.to_json(),
            r#"{"type":"ENGINE_READY","backend":"cpu-reference","profile":null,"adapter":null,"generation":0}"#
        );
        let err = EngineEvent::EngineError {
            code: 0x6002,
            message: "x".into(),
        };
        assert_eq!(err.to_json(), r#"{"type":"ENGINE_ERROR","code":24578,"message":"x"}"#);
    }

    #[test]
    fn world_id_hex_roundtrip() {
        let id = [0xAB; 16];
        assert_eq!(parse_world_id(&to_hex(&id)), Some(id));
        assert_eq!(parse_world_id("zz"), None);
    }
}
