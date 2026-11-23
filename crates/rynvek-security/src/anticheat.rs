// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 反作弊五防线（D645–D657）中与规则无关的部分。
//!
//! | 防线 | 位置 | 作用 |
//! | --- | --- | --- |
//! | 1 身份 | [`crate::token`] + [`CommandGate`] | 令牌验签 / 吊销；命令信封的 `principal_id` 必须等于会话身份，且该身份已在本局入座 |
//! | 2 协议 | `rynvek-network` | 16B 帧头、通道、长度与负载 Schema 校验，违例即断开 |
//! | 3 时序 | [`CommandGate`] | `target_tick` 时间窗（`0x8003`）与 UUIDv7 幂等键去重（`0x8002`） |
//! | 4 速率 | [`CommandGate`] + `rynvek-agent` | 帧级突发令牌桶（`0x8004`）；游戏动作再受阵营 600 APM 约束 |
//! | 5 权威与审计 | `rynvek-server` + [`BehaviorMonitor`] | 服务端权威校验、迷雾物理裁剪、`.rreplay` 重算验证与可疑行为审计 |
//!
//! 第 5 防线的 [`BehaviorMonitor`] 只产出审计线索（写入 `audit_logs`），从不直接改变对局结果：
//! 启发式误报不应惩罚玩家，处罚由人工复核回放后决定。

use std::collections::{BTreeMap, VecDeque};

use rynvek_core::error::RynvekErrorCode;
use rynvek_protocol::envelope::{CommandTickWindow, CommandWindowError};
use rynvek_protocol::{CommandEnvelope, IdempotencyKey};
use serde::{Deserialize, Serialize};

use crate::error::SecurityError;

/// 命令闸门参数
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandGateConfig {
    /// `target_tick` 可接受时间窗：落后至多 30 Tick（1 s 网络抖动），超前至多 90 Tick（3 s 输入缓冲）
    pub window: CommandTickWindow,
    /// 幂等键保留时长（Tick）；必须 ≥ 时间窗总宽，保证窗内任何重放都能被识别
    pub idempotency_retention_ticks: u64,
    /// 每 Principal 突发令牌桶容量（帧）
    pub burst_capacity: u32,
    /// 每 Tick 回填的令牌数
    pub refill_per_tick: u32,
}

impl Default for CommandGateConfig {
    fn default() -> Self {
        Self {
            window: CommandTickWindow {
                max_past_ticks: 30,
                max_future_ticks: 90,
            },
            idempotency_retention_ticks: 1_800,
            burst_capacity: 60,
            refill_per_tick: 1,
        }
    }
}

/// 闸门判定结果
pub type GateDecision = Result<(), SecurityError>;

#[derive(Clone, Copy, Debug)]
struct Bucket {
    tokens: u32,
    last_tick: u64,
}

/// 每局一个的命令闸门
#[derive(Clone, Debug)]
pub struct CommandGate {
    config: CommandGateConfig,
    /// 已入座 Principal → 阵营
    seats: BTreeMap<u64, u32>,
    seen: BTreeMap<IdempotencyKey, u64>,
    seen_order: VecDeque<(u64, IdempotencyKey)>,
    buckets: BTreeMap<u64, Bucket>,
}

impl CommandGate {
    pub fn new(config: CommandGateConfig) -> Self {
        assert!(
            config.idempotency_retention_ticks >= config.window.max_past_ticks + config.window.max_future_ticks,
            "幂等键保留时长必须覆盖整个时间窗"
        );
        Self {
            config,
            seats: BTreeMap::new(),
            seen: BTreeMap::new(),
            seen_order: VecDeque::new(),
            buckets: BTreeMap::new(),
        }
    }

    pub const fn config(&self) -> &CommandGateConfig {
        &self.config
    }

    /// 入座：只有入座的 Principal 才能为其阵营提交命令
    pub fn bind(&mut self, principal_id: u64, faction: u32) {
        self.seats.insert(principal_id, faction);
    }

    pub fn unbind(&mut self, principal_id: u64) {
        self.seats.remove(&principal_id);
    }

    pub fn faction_of(&self, principal_id: u64) -> Option<u32> {
        self.seats.get(&principal_id).copied()
    }

    /// 帧级速率检查（每个上行业务帧调用一次，包括查询与工具调用）
    pub fn admit_frame(&mut self, principal_id: u64, current_tick: u64) -> GateDecision {
        let cfg = self.config;
        let bucket = self.buckets.entry(principal_id).or_insert(Bucket {
            tokens: cfg.burst_capacity,
            last_tick: current_tick,
        });
        let elapsed = current_tick.saturating_sub(bucket.last_tick);
        let refill = elapsed
            .saturating_mul(cfg.refill_per_tick as u64)
            .min(cfg.burst_capacity as u64) as u32;
        bucket.tokens = (bucket.tokens + refill).min(cfg.burst_capacity);
        bucket.last_tick = current_tick;
        if bucket.tokens == 0 {
            return Err(SecurityError::new(
                RynvekErrorCode::CommandRateLimitExceeded,
                "上行帧超过突发速率上限",
            ));
        }
        bucket.tokens -= 1;
        Ok(())
    }

    /// 命令信封检查：身份绑定 → 时间窗 → 幂等键。通过后幂等键被记住。
    pub fn check_command<P>(
        &mut self,
        session_principal_id: u64,
        envelope: &CommandEnvelope<P>,
        current_tick: u64,
    ) -> Result<u32, SecurityError> {
        if envelope.principal_id != session_principal_id {
            return Err(SecurityError::unauthorized("命令信封的 principal_id 与会话身份不一致"));
        }
        let faction = self
            .faction_of(session_principal_id)
            .ok_or_else(|| SecurityError::unauthorized("该 Principal 未在本局入座"))?;
        envelope
            .check_tick_window(current_tick, self.config.window)
            .map_err(|e| match e {
                CommandWindowError::Expired { .. } | CommandWindowError::TooFarInFuture { .. } => {
                    SecurityError::new(RynvekErrorCode::CommandExpiredOrFutureTooFar, e.to_string())
                }
            })?;
        self.expire_keys(current_tick);
        if self.seen.contains_key(&envelope.idempotency_key) {
            return Err(SecurityError::new(
                RynvekErrorCode::DuplicateCommandIdempotencyKey,
                "重复的命令幂等键",
            ));
        }
        self.seen.insert(envelope.idempotency_key, current_tick);
        self.seen_order.push_back((current_tick, envelope.idempotency_key));
        Ok(faction)
    }

    fn expire_keys(&mut self, current_tick: u64) {
        let horizon = current_tick.saturating_sub(self.config.idempotency_retention_ticks);
        while let Some((tick, key)) = self.seen_order.front().copied() {
            if tick >= horizon {
                break;
            }
            self.seen_order.pop_front();
            self.seen.remove(&key);
        }
    }

    /// 当前记住的幂等键数量
    pub fn remembered_keys(&self) -> usize {
        self.seen.len()
    }
}

/// 可疑行为种类
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuspicionKind {
    /// 人类赛道中连续多次完全等间隔的输入（疑似宏 / 脚本代打）
    MachineRegularInput,
    /// 反复对迷雾中不可见的实体或位置下达指令（疑似地图透视）
    FogProbe,
    /// 大量被权威层拒绝的命令（疑似篡改客户端 / 协议模糊测试）
    RejectedCommandFlood,
}

/// 一条审计线索
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suspicion {
    pub principal_id: u64,
    pub kind: SuspicionKind,
    pub tick: u64,
    pub detail: String,
}

/// 连续等间隔输入的判定阈值
pub const REGULAR_INPUT_STREAK: usize = 40;
/// 一分钟（1,800 Tick）内迷雾探测次数阈值
pub const FOG_PROBE_THRESHOLD: usize = 3;
/// 一分钟内被拒命令数阈值
pub const REJECTED_FLOOD_THRESHOLD: usize = 120;
/// 统计窗口
pub const MONITOR_WINDOW_TICKS: u64 = 1_800;

#[derive(Debug, Default)]
struct Track {
    human: bool,
    last_tick: Option<u64>,
    last_interval: Option<u64>,
    streak: usize,
    probes: VecDeque<u64>,
    rejects: VecDeque<u64>,
    flagged: BTreeMap<SuspicionKind, u64>,
}

/// 行为审计器（第 5 防线）
#[derive(Debug, Default)]
pub struct BehaviorMonitor {
    tracks: BTreeMap<u64, Track>,
    findings: Vec<Suspicion>,
}

impl BehaviorMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记参赛者；只有人类赛道参赛者才做 "机器般规律输入" 检测
    pub fn register(&mut self, principal_id: u64, human_league: bool) {
        self.tracks.entry(principal_id).or_default().human = human_league;
    }

    /// 记录一次被接受的命令
    pub fn on_command(&mut self, principal_id: u64, tick: u64) {
        let track = self.tracks.entry(principal_id).or_default();
        if let Some(prev) = track.last_tick {
            let interval = tick.saturating_sub(prev);
            if interval > 0 && track.last_interval == Some(interval) {
                track.streak += 1;
            } else {
                track.streak = 1;
            }
            track.last_interval = Some(interval);
        }
        track.last_tick = Some(tick);
        if track.human && track.streak >= REGULAR_INPUT_STREAK {
            let detail = format!(
                "连续 {} 次输入间隔恒为 {} Tick",
                track.streak,
                track.last_interval.unwrap_or(0)
            );
            self.flag(principal_id, SuspicionKind::MachineRegularInput, tick, detail);
        }
    }

    /// 记录一次针对不可见目标的指令
    pub fn on_fog_probe(&mut self, principal_id: u64, tick: u64) {
        let n = Self::push_window(&mut self.tracks.entry(principal_id).or_default().probes, tick);
        if n >= FOG_PROBE_THRESHOLD {
            self.flag(
                principal_id,
                SuspicionKind::FogProbe,
                tick,
                format!("一分钟内 {n} 次指向迷雾中的目标"),
            );
        }
    }

    /// 记录一次被权威层拒绝的命令
    pub fn on_rejected(&mut self, principal_id: u64, tick: u64) {
        let n = Self::push_window(&mut self.tracks.entry(principal_id).or_default().rejects, tick);
        if n >= REJECTED_FLOOD_THRESHOLD {
            self.flag(
                principal_id,
                SuspicionKind::RejectedCommandFlood,
                tick,
                format!("一分钟内 {n} 条命令被拒绝"),
            );
        }
    }

    fn push_window(q: &mut VecDeque<u64>, tick: u64) -> usize {
        q.push_back(tick);
        while q.front().is_some_and(|t| t + MONITOR_WINDOW_TICKS <= tick) {
            q.pop_front();
        }
        q.len()
    }

    /// 同一种线索每个窗口最多报告一次
    fn flag(&mut self, principal_id: u64, kind: SuspicionKind, tick: u64, detail: String) {
        let track = self.tracks.entry(principal_id).or_default();
        if track
            .flagged
            .get(&kind)
            .is_some_and(|t| t + MONITOR_WINDOW_TICKS > tick)
        {
            return;
        }
        track.flagged.insert(kind, tick);
        self.findings.push(Suspicion {
            principal_id,
            kind,
            tick,
            detail,
        });
    }

    /// 取走累计的线索
    pub fn drain(&mut self) -> Vec<Suspicion> {
        std::mem::take(&mut self.findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_protocol::WireUuid;

    fn key(n: u16) -> IdempotencyKey {
        let mut b = [0u8; 16];
        b[0..2].copy_from_slice(&n.to_be_bytes());
        b[6] = 0x70; // version 7
        b[8] = 0x80; // RFC 4122 variant
        IdempotencyKey::new(WireUuid(b)).unwrap()
    }

    fn env(n: u16, principal: u64, target: u64) -> CommandEnvelope<()> {
        CommandEnvelope {
            idempotency_key: key(n),
            principal_id: principal,
            issued_at_tick: target,
            target_tick: target,
            payload: (),
        }
    }

    #[test]
    fn identity_binding_is_enforced() {
        let mut g = CommandGate::new(CommandGateConfig::default());
        assert_eq!(
            g.check_command(1, &env(1, 1, 100), 100).unwrap_err().code.code(),
            0x8001
        );
        g.bind(1, 0);
        assert_eq!(g.check_command(1, &env(1, 1, 100), 100), Ok(0));
        assert_eq!(
            g.check_command(1, &env(2, 2, 100), 100).unwrap_err().code.code(),
            0x8001,
            "冒用他人身份"
        );
    }

    #[test]
    fn tick_window_and_idempotency() {
        let mut g = CommandGate::new(CommandGateConfig::default());
        g.bind(1, 1);
        assert_eq!(g.check_command(1, &env(1, 1, 69), 100).unwrap_err().code.code(), 0x8003);
        assert_eq!(
            g.check_command(1, &env(2, 1, 191), 100).unwrap_err().code.code(),
            0x8003
        );
        assert!(g.check_command(1, &env(3, 1, 70), 100).is_ok());
        assert!(g.check_command(1, &env(4, 1, 190), 100).is_ok());
        assert_eq!(
            g.check_command(1, &env(4, 1, 190), 101).unwrap_err().code.code(),
            0x8002
        );
        // 保留期之后旧键被清理，但旧命令早已落在时间窗外
        assert!(g.check_command(1, &env(5, 1, 2_000), 2_000).is_ok());
        assert_eq!(g.remembered_keys(), 1);
        assert_eq!(
            g.check_command(1, &env(4, 1, 190), 2_000).unwrap_err().code.code(),
            0x8003
        );
    }

    #[test]
    fn burst_bucket_refills_per_tick() {
        let cfg = CommandGateConfig {
            burst_capacity: 3,
            ..CommandGateConfig::default()
        };
        let mut g = CommandGate::new(cfg);
        for _ in 0..3 {
            g.admit_frame(9, 10).unwrap();
        }
        assert_eq!(g.admit_frame(9, 10).unwrap_err().code.code(), 0x8004);
        g.admit_frame(9, 11).unwrap();
        assert!(g.admit_frame(9, 11).is_err());
        assert!(g.admit_frame(8, 11).is_ok(), "令牌桶按 Principal 隔离");
    }

    #[test]
    fn monitor_flags_regular_human_input_only() {
        let mut m = BehaviorMonitor::new();
        m.register(1, true);
        m.register(2, false);
        for i in 0..=REGULAR_INPUT_STREAK as u64 {
            m.on_command(1, i * 6);
            m.on_command(2, i * 6);
        }
        let f = m.drain();
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].principal_id, f[0].kind), (1, SuspicionKind::MachineRegularInput));
    }

    #[test]
    fn monitor_fog_probes_and_rejection_floods_once_per_window() {
        let mut m = BehaviorMonitor::new();
        m.on_fog_probe(3, 10);
        m.on_fog_probe(3, 20);
        assert!(m.drain().is_empty());
        m.on_fog_probe(3, 30);
        m.on_fog_probe(3, 40);
        assert_eq!(m.drain().len(), 1, "同一窗口只报告一次");
        for t in 0..REJECTED_FLOOD_THRESHOLD as u64 {
            m.on_rejected(4, t);
        }
        assert_eq!(m.drain()[0].kind, SuspicionKind::RejectedCommandFlood);
        // 稀疏探测不会累积成线索
        for t in 0..10 {
            m.on_fog_probe(5, t * MONITOR_WINDOW_TICKS);
        }
        assert!(m.drain().is_empty());
    }
}
