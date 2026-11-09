// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! RTS ⇄ WASM 插件适配层：把 [`ArenaWorld`] 的阵营观测转换为 WIT `faction-view`，
//! 把插件返回的命令 JSON 严格解析为 [`RtsCommand`] 并经与玩家相同的校验路径提交。
//!
//! - 视图来自 [`ArenaWorld::observe`]，已按战争迷雾裁剪，插件看不到对手的隐藏信息；
//! - 命令 JSON 无法解析为 `RtsCommand` 视为插件违约（隔离，0x5006）；
//!   命令被规则拒绝（资源不足、落点非法…）只计数，不视为故障；
//! - 插件故障只隔离该插件，世界继续推进（规范 11.4 `IsolateFaultyExtension`）。

use rynvek_core::hash::Blake3Hash;
use rynvek_rts::commands::RtsCommand;
use rynvek_rts::{ArenaWorld, FactionId};
use rynvek_rules::plugin::{
    BuildingView, FactionView, PluginFault, PluginInstance, PluginModule, PluginRuntime, UnitView,
};

/// 由世界种子与阵营派生插件随机数密钥（域分隔 `rynvek-plugin-rng/1`）
pub fn plugin_random_seed(world_seed: u64, faction: FactionId) -> u64 {
    let mut buf = Vec::with_capacity(32);
    buf.extend_from_slice(b"rynvek-plugin-rng/1\0");
    buf.extend_from_slice(&world_seed.to_le_bytes());
    buf.push(faction.0);
    let h = Blake3Hash::of(&buf).0;
    u64::from_le_bytes([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]])
}

/// 生成阵营视图（未知阵营返回 None）
pub fn faction_view(world: &ArenaWorld, faction: FactionId) -> Option<FactionView> {
    let obs = world.observe(faction)?;
    Some(FactionView {
        tick: obs.tick,
        faction: faction.0,
        biomass_milli: obs.ledger.balance.biomass,
        flux_milli: obs.ledger.balance.flux,
        neurogel_milli: obs.ledger.balance.neurogel,
        supply_used: obs.supply.0,
        supply_cap: obs.supply.1,
        score_milli: obs.score_milli,
        units: obs
            .units
            .iter()
            .map(|u| UnitView {
                id: u.id.0,
                faction: u.faction.0,
                kind: u.kind.index() as u8,
                x_raw: u.position.x.0,
                y_raw: u.position.y.0,
                hp_milli: u.hp_milli,
            })
            .collect(),
        buildings: obs
            .buildings
            .iter()
            .map(|b| BuildingView {
                id: b.id.0,
                faction: b.faction.0,
                kind: b.kind.index() as u8,
                origin_x: b.origin.0,
                origin_y: b.origin.1,
                hp_milli: b.hp_milli,
                complete: b.complete,
            })
            .collect(),
    })
}

/// 一次驱动的结果
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PluginTickReport {
    pub emitted: u32,
    pub accepted: u32,
    pub rejected: u32,
}

/// 累计统计
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PluginDriverStats {
    pub ticks: u64,
    pub accepted: u64,
    pub rejected: u64,
}

/// 控制一个阵营的插件
#[derive(Debug)]
pub struct ArenaPluginDriver {
    instance: PluginInstance,
    faction: FactionId,
    stats: PluginDriverStats,
}

impl ArenaPluginDriver {
    /// 实例化插件并绑定到阵营
    pub fn new(
        runtime: &PluginRuntime,
        module: &PluginModule,
        config_json: &str,
        world_seed: u64,
        faction: FactionId,
    ) -> Result<Self, PluginFault> {
        let instance =
            PluginInstance::instantiate(runtime, module, config_json, plugin_random_seed(world_seed, faction))?;
        Ok(Self {
            instance,
            faction,
            stats: PluginDriverStats::default(),
        })
    }

    pub fn instance(&self) -> &PluginInstance {
        &self.instance
    }

    pub fn instance_mut(&mut self) -> &mut PluginInstance {
        &mut self.instance
    }

    pub fn stats(&self) -> PluginDriverStats {
        self.stats
    }

    /// 在当前 Tick 调用插件并提交其命令（应在 `world.step()` 之前调用）
    pub fn tick(&mut self, world: &mut ArenaWorld) -> Result<PluginTickReport, PluginFault> {
        let Some(view) = faction_view(world, self.faction) else {
            return Ok(PluginTickReport::default());
        };
        self.stats.ticks += 1;
        let payloads = self.instance.on_tick(view)?;
        let mut commands = Vec::with_capacity(payloads.len());
        for p in &payloads {
            match serde_json::from_str::<RtsCommand>(p) {
                Ok(c) => commands.push(c),
                Err(e) => {
                    return Err(self
                        .instance
                        .reject_output(format!("命令 JSON 不是合法的 RtsCommand：{e}")));
                }
            }
        }
        let mut report = PluginTickReport {
            emitted: commands.len() as u32,
            ..PluginTickReport::default()
        };
        for c in &commands {
            match world.apply_command(self.faction, c) {
                Ok(_) => report.accepted += 1,
                Err(_) => report.rejected += 1,
            }
        }
        self.stats.accepted += u64::from(report.accepted);
        self.stats.rejected += u64::from(report.rejected);
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_core::error::RynvekErrorCode;
    use rynvek_rts::MatchConfig;

    const PROBE: &[u8] = include_bytes!("../../rynvek-rules/tests/fixtures/plugin/probe.component.wasm");

    #[test]
    fn probe_plugin_drives_a_faction_for_1000_ticks_deterministically() {
        let run = || {
            let rt = PluginRuntime::new().unwrap();
            let m = rt.compile(PROBE).unwrap();
            let mut w = ArenaWorld::new(MatchConfig::pve(5, 1));
            let mut d = ArenaPluginDriver::new(&rt, &m, "{}", 5, FactionId(0)).unwrap();
            for _ in 0..1_000 {
                d.tick(&mut w).unwrap();
                w.step();
            }
            (w.state_hash(), d.stats())
        };
        let (a, sa) = run();
        let (b, sb) = run();
        assert_eq!(a, b);
        assert_eq!(sa, sb);
        assert!(sa.accepted > 0, "探针插件的驻守命令应被接受");
    }

    #[test]
    fn faulty_plugin_is_isolated_while_world_continues() {
        let rt = PluginRuntime::new().unwrap();
        let m = rt.compile(PROBE).unwrap();
        let mut w = ArenaWorld::new(MatchConfig::pve(5, 1));
        let mut d = ArenaPluginDriver::new(&rt, &m, r#"{"mode":"spin"}"#, 5, FactionId(0)).unwrap();
        let f = d.tick(&mut w).unwrap_err();
        assert_eq!(f.code(), RynvekErrorCode::SandboxGasBudgetExhausted);
        w.run(10);
        assert!(d.tick(&mut w).is_err());
        assert_eq!(w.tick, 10);
    }

    #[test]
    fn seeds_are_domain_separated() {
        assert_ne!(plugin_random_seed(1, FactionId(0)), plugin_random_seed(1, FactionId(1)));
        assert_eq!(plugin_random_seed(1, FactionId(0)), plugin_random_seed(1, FactionId(0)));
    }
}
