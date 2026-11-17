// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

use rynvek_neural::PopulationBrainSlot;
use rynvek_rts::{ArenaWorld, FactionId, MatchConfig};

#[test]
fn own_neural_projection_uses_real_rings_and_never_enemy_state() {
    let mut w = ArenaWorld::new(MatchConfig::duel(42));
    w.run(331); // 与跨语言真实Arena夹具相同的正常脑核/世界推进，不在投影内合成脉冲。
    let root = w.state_hash();
    let view = w.observe(FactionId(0)).unwrap().neural;
    let (mut neurons, mut spikes, mut continuous, mut abs) = (0, 0, 0, 0);
    for u in w.units.values().filter(|u| u.faction == FactionId(0)) {
        match &u.brain.0 {
            PopulationBrainSlot::Lif(b) => {
                neurons += b.spike_ring.len() as u64;
                spikes += b.spike_ring.iter().map(|r| r.count_ones() as u64).sum::<u64>();
            }
            PopulationBrainSlot::Izhikevich(b) => {
                neurons += b.spike_ring.len() as u64;
                spikes += b.spike_ring.iter().map(|r| r.count_ones() as u64).sum::<u64>();
            }
            PopulationBrainSlot::GatedMlp(b) => {
                continuous += b.hidden_q8.len() as u64;
                abs += b.hidden_q8.iter().map(|v| v.unsigned_abs() as u64).sum::<u64>();
            }
            PopulationBrainSlot::Ctrnn(b) => {
                continuous += b.y_q8.len() as u64;
                abs += b.y_q8.iter().map(|v| v.unsigned_abs() as u64).sum::<u64>();
            }
        }
    }
    assert_eq!(
        (view.spiking_neurons, view.spikes_in_window, view.continuous_neurons),
        (neurons, spikes, continuous)
    );
    assert_eq!(
        view.continuous_abs_mean_q8 as u64,
        if continuous == 0 { 0 } else { abs / continuous }
    );
    assert_eq!(w.state_hash(), root, "声音观测不得修改权威根/脑");
    for u in w.units.values_mut().filter(|u| u.faction != FactionId(0)) {
        match &mut u.brain.0 {
            PopulationBrainSlot::Lif(b) => b.spike_ring.fill(u64::MAX),
            PopulationBrainSlot::Izhikevich(b) => b.spike_ring.fill(u64::MAX),
            PopulationBrainSlot::GatedMlp(b) => b.hidden_q8.fill(i16::MAX),
            PopulationBrainSlot::Ctrnn(b) => b.y_q8.fill(i16::MIN),
        }
        u.weapon_cooldown = u32::MAX;
        u.actions = [256; 6];
    }
    assert_eq!(
        w.observe(FactionId(0)).unwrap().neural,
        view,
        "敌方脑/武器/输出不进入本人声化"
    );
    w.units.retain(|_, u| u.faction != FactionId(0));
    let empty = w.observe(FactionId(0)).unwrap().neural;
    assert_eq!(
        (
            empty.spiking_neurons,
            empty.continuous_neurons,
            empty.spikes_in_window,
            empty.continuous_abs_mean_q8
        ),
        (0, 0, 0, 0)
    );
}
