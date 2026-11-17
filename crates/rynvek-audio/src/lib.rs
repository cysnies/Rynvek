// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-audio
//!
//! 程序化生物声学与可听化遥测内核（规范 07 §7.4 / 审计报告 §12.1）。
//!
//! 将 `ArenaWorld` 与神经生态群体的宏观/微观动态指标确定性映射为 `@rynvek/audio`（WebAudio 合成器）
//! 消费的控制帧 [`AudioTelemetryFrame`]：
//! - 群体发射输出一致性（非相位测量）与 Weaver 态势 → 主谐波基频（Hz 毫单位）与泛音调制深度；
//! - 危害场浓度与突然死亡状态 → 低频次声脉冲与滤波器截止频率；
//! - 战斗交火密度 → 颗粒合成器（Granular Synthesizer）触发率。

#![deny(unsafe_code)]

use rynvek_rts::genome::action;
use rynvek_rts::{ArenaWorld, FieldId, UnitKind};
use serde::{Deserialize, Serialize};

/// 生物声学态势档位
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SonificationMood {
    /// 静息稳态（开局采集与低密度巡逻）
    Homeostasis,
    /// 神经趋化与群体同步（Weaver 光环或高信号场引导）
    ChemotaxisSync,
    /// 激烈交火（高频武器冷却触发）
    SynapticCombat,
    /// 突然死亡或主脑危急告警
    CriticalAlert,
}

/// 单帧程序化声学合成控制参数（全部采用毫单位整数以保证跨平台确定性）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioTelemetryFrame {
    pub tick: u64,
    pub mood: SonificationMood,
    /// 基频（毫 Hz，范围 `55_000..=880_000` 即 `55 Hz..=880 Hz`）
    pub carrier_freq_mhz: u32,
    /// 发射输出一致性的谐波增益（不是 Kuramoto 相位序参量）（千分比 `0..=1_000`）
    pub sync_coherence_permille: u16,
    /// 脉冲颗粒触发密度（千分比 `0..=1_000`）
    pub combat_grain_density_permille: u16,
    /// 腐蚀/危害低通滤波压暗强度（千分比 `0..=1_000`）
    pub hazard_damping_permille: u16,
    /// 主混音增益（千分比 `0..=1_000`）
    pub master_gain_permille: u16,
}

/// 从 `ArenaWorld` 提取当前 Tick 的确定性程序化声学遥测帧
pub fn extract_arena_audio_telemetry(world: &ArenaWorld) -> AudioTelemetryFrame {
    let unit_count = world.units.len() as i64;
    let firing_units = world.units.values().filter(|u| u.weapon_cooldown > 0).count() as i64;
    let weaver_count = world.units.values().filter(|u| u.kind == UnitKind::Weaver).count() as i64;

    // 群体发射输出一致性（通道 5 a_emit）；全静默不是同步发射。
    let emitting = world.units.values().any(|u| u.actions[action::EMIT] != 0);
    let sync_permille = if unit_count == 0 || !emitting {
        0u16
    } else {
        let mean_emit: i64 = world
            .units
            .values()
            .map(|u| u.actions[action::EMIT] as i64)
            .sum::<i64>()
            / unit_count;
        let mad: i64 = world
            .units
            .values()
            .map(|u| (u.actions[action::EMIT] as i64 - mean_emit).abs())
            .sum::<i64>()
            / unit_count;
        ((256 - mad).clamp(0, 256) * 1_000 / 256) as u16
    };

    let combat_density = if unit_count == 0 {
        0u16
    } else {
        ((firing_units * 1_000) / unit_count.max(1)).clamp(0, 1_000) as u16
    };

    // 采样单位所在格的平均危害场强度
    let hazard_damping = if unit_count == 0 {
        0u16
    } else {
        let total_haz: i64 = world
            .units
            .values()
            .map(|u| {
                let (tx, ty) = u.tile();
                world.fields.get(FieldId::Hazard, tx as i64, ty as i64).max(0) as i64
            })
            .sum();
        ((total_haz / unit_count) * 1_000 / 25_600).clamp(0, 1_000) as u16
    };

    let nexus_critical = world.factions.iter().any(|f| {
        f.nexus
            .and_then(|id| world.buildings.get(&id))
            .is_some_and(|b| b.defense.hp_milli * 4 < b.max_hp_milli)
    });

    let mood = if world.sudden_death_until.is_some() || nexus_critical {
        SonificationMood::CriticalAlert
    } else if combat_density >= 250 {
        SonificationMood::SynapticCombat
    } else if weaver_count > 0 || sync_permille >= 850 {
        SonificationMood::ChemotaxisSync
    } else {
        SonificationMood::Homeostasis
    };

    let carrier_freq_mhz = 110_000 + (sync_permille as u32) * 110 + (combat_density as u32) * 220;

    AudioTelemetryFrame {
        tick: world.tick,
        mood,
        carrier_freq_mhz: carrier_freq_mhz.clamp(55_000, 880_000),
        sync_coherence_permille: sync_permille,
        combat_grain_density_permille: combat_density,
        hazard_damping_permille: hazard_damping,
        master_gain_permille: 850,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_rts::MatchConfig;

    #[test]
    fn extracts_deterministic_audio_telemetry_from_arena_world() {
        let mut world = ArenaWorld::new(MatchConfig::duel(42));
        let frame0 = extract_arena_audio_telemetry(&world);
        assert_eq!(frame0.tick, 0);
        assert_eq!(frame0.mood, SonificationMood::Homeostasis);
        assert_eq!(frame0.sync_coherence_permille, 0);
        assert!((55_000..=880_000).contains(&frame0.carrier_freq_mhz));

        world.sudden_death_until = Some(39_600);
        let frame_sd = extract_arena_audio_telemetry(&world);
        assert_eq!(frame_sd.mood, SonificationMood::CriticalAlert);
    }

    #[test]
    fn silent_identical_active_and_divergent_outputs_are_distinguished() {
        let mut world = ArenaWorld::new(MatchConfig::duel(42));
        for unit in world.units.values_mut() {
            unit.actions[action::EMIT] = 0;
        }
        assert_eq!(extract_arena_audio_telemetry(&world).sync_coherence_permille, 0);
        for unit in world.units.values_mut() {
            unit.actions[action::EMIT] = 256;
        }
        let active = extract_arena_audio_telemetry(&world);
        assert_eq!(active.sync_coherence_permille, 1000);
        assert_eq!(active.mood, SonificationMood::ChemotaxisSync);
        assert_eq!(active, extract_arena_audio_telemetry(&world));
        for (i, unit) in world.units.values_mut().enumerate() {
            unit.actions[action::EMIT] = if i % 2 == 0 { -256 } else { 256 };
        }
        let diverse = extract_arena_audio_telemetry(&world);
        assert_eq!(diverse.sync_coherence_permille, 0);
        assert_eq!(diverse.mood, SonificationMood::Homeostasis);
    }

    #[test]
    fn combat_alert_and_weaver_priorities_remain_intact() {
        let mut world = ArenaWorld::new(MatchConfig::duel(42));
        world.units.values_mut().next().unwrap().kind = UnitKind::Weaver;
        assert_eq!(
            extract_arena_audio_telemetry(&world).mood,
            SonificationMood::ChemotaxisSync
        );
        for unit in world.units.values_mut() {
            unit.weapon_cooldown = 1;
        }
        assert_eq!(
            extract_arena_audio_telemetry(&world).mood,
            SonificationMood::SynapticCombat
        );
        world.sudden_death_until = Some(39_600);
        assert_eq!(
            extract_arena_audio_telemetry(&world).mood,
            SonificationMood::CriticalAlert
        );
        world.sudden_death_until = None;
        let nexus = world.factions[0].nexus.unwrap();
        world.buildings.get_mut(&nexus).unwrap().defense.hp_milli = 1;
        assert_eq!(
            extract_arena_audio_telemetry(&world).mood,
            SonificationMood::CriticalAlert
        );
    }

    #[test]
    fn empty_world_has_no_coherence_or_combat_density() {
        let mut world = ArenaWorld::new(MatchConfig::duel(42));
        world.units.clear();
        let frame = extract_arena_audio_telemetry(&world);
        assert_eq!(frame.sync_coherence_permille, 0);
        assert_eq!(frame.combat_grain_density_permille, 0);
        assert_eq!(frame.mood, SonificationMood::Homeostasis);
    }
}
