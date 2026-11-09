// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 验收：LIF 脑在可塑性开启下运行，经 NEUR 分块快照/恢复后续跑与不中断运行位级一致。

use rynvek_core::fixed::Q8_8;
use rynvek_core::rng::RandomKey;
use rynvek_neural::{LIF_INPUTS, LIF_OUTPUTS, LifBrain};
use rynvek_snapshot::NeurChunk;

const N_HID: usize = 24; // seed.forager.v1：16-24-6
const DENSE_INDEX: u32 = 42;

fn rand4(tick: u64, serial: u64) -> [u32; 4] {
    RandomKey {
        world_seed: 0x00C0_FFEE,
        tick,
        entity_serial: serial,
        stream_and_phase: RandomKey::pack_stream_and_phase(1, 6),
        sub_counter: 0,
    }
    .generate_u32x4()
}

fn genome_weights() -> Vec<i16> {
    (0..N_HID * (LIF_INPUTS + LIF_OUTPUTS))
        .map(|i| (rand4(0, i as u64)[0] % 257) as i16 - 64)
        .collect()
}

fn sensory_at(tick: u64) -> [Q8_8; LIF_INPUTS] {
    std::array::from_fn(|i| Q8_8((rand4(tick, 10_000 + i as u64)[1] % 513) as i16 - 256))
}

fn modulators_at(tick: u64) -> [i16; 4] {
    let r = rand4(tick, 99);
    std::array::from_fn(|k| (r[k] % 513) as i16 - 256)
}

fn run(brain: &mut LifBrain, ticks: std::ops::Range<u64>) -> Vec<[Q8_8; LIF_OUTPUTS]> {
    ticks
        .map(|t| brain.step(&sensory_at(t), modulators_at(t), true))
        .collect()
}

#[test]
fn neur_snapshot_resume_is_bit_identical() {
    let mut reference = LifBrain::new(N_HID, genome_weights()).unwrap();
    let mut first_half = run(&mut reference, 0..500);
    first_half.extend(run(&mut reference, 500..1000));

    let mut brain = LifBrain::new(N_HID, genome_weights()).unwrap();
    let mut outputs = run(&mut brain, 0..500);
    assert!(brain.weights_delta_q8.iter().any(|&d| d != 0), "可塑性应已产生 δw");

    // 经完整 NEUR 二进制往返恢复到全新实例
    let bytes = brain.to_neur_chunk(DENSE_INDEX).encode();
    let decoded = NeurChunk::decode(&bytes).unwrap();
    let mut restored = LifBrain::new(N_HID, genome_weights()).unwrap();
    restored.restore_from_neur(&decoded, DENSE_INDEX).unwrap();
    assert_eq!(restored, brain);

    outputs.extend(run(&mut restored, 500..1000));
    assert_eq!(outputs, first_half);
    assert_eq!(restored.state_root(), reference.state_root());
}

#[test]
fn restore_rejects_missing_neurons() {
    let brain = LifBrain::new(N_HID, genome_weights()).unwrap();
    let mut chunk = brain.to_neur_chunk(DENSE_INDEX);
    chunk.rows.pop();
    let mut target = LifBrain::new(N_HID, genome_weights()).unwrap();
    assert!(target.restore_from_neur(&chunk, DENSE_INDEX).is_err());
}
