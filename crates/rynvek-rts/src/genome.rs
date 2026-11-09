// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 6 大预置种子基因组（项目文档 §5.1.2「赛季认证初始种子基因池」）与单位基因组表示。
//!
//! 设计圣经冻结了每个种子基因组的神经元模型、拓扑 `N_in − N_hid − N_out`、初始非零突触数 `E_0` 与
//! 图存储布局，并用文字描述了「通过初始权重矩阵 W_0 固化」的先天行为偏置。规范没有给出 W_0 的具体数值，
//! 这里按以下确定性规则构造（ERRATA E-025）：
//!
//! 1. **先天通路**：每条行为偏置是一条「感知 → 隐藏 → 动作」通路，占用专属隐藏神经元，
//!    输入权重 `±2.0`、输出权重 `±2.0`；有符号感知量（梯度、方向）使用正负一对神经元，保证脉冲模型
//!    （只对正电流发放）也能表达双向偏置。
//! 2. **随机补齐**：其余 `E_0 − 通路突触数` 个突触由 Philox（密钥 = BLAKE3(genome_slug) 前 8 字节）
//!    在剩余空槽中抽取位置，幅值 `[0.03, 0.125]`、随机符号；`PagedDynamicAdjacency` 每神经元不超过 32 个突触。
//!
//! 16 维感知与 6 维动作读出的通道语义见 [`sensor`] 与 [`action`]。

use rynvek_core::{Blake3Hash, RandomKey};
use rynvek_neural::storage::SYNAPSE_SLOTS_PER_NEURON;
use rynvek_neural::{NeuronModelKind, TopologyStorageClass};
use serde::{Deserialize, Serialize};

use crate::ids::UnitKind;

/// 感知输入维度 `N_in`
pub const GENOME_INPUTS: usize = 16;
/// 动作读出维度 `N_out`
pub const GENOME_OUTPUTS: usize = 6;

/// 16 维感知输入通道（Q8.8，`[-1, 1]`）
pub mod sensor {
    /// `∇F_nutrient` x 分量
    pub const NUTRIENT_GRAD_X: usize = 0;
    /// `∇F_nutrient` y 分量
    pub const NUTRIENT_GRAD_Y: usize = 1;
    /// 所在 Tile 营养浓度（`F / 100`）
    pub const NUTRIENT: usize = 2;
    /// 所在 Tile 危害浓度（`F / 100`）
    pub const HAZARD: usize = 3;
    /// `∇S_A` x 分量（趋化引导场）
    pub const SIGNAL_A_GRAD_X: usize = 4;
    /// `∇S_A` y 分量
    pub const SIGNAL_A_GRAD_Y: usize = 5;
    /// 所在 Tile 战斗激发场 `S_B / 50`
    pub const SIGNAL_B: usize = 6;
    /// 所在 Tile 能量场（`F / 100`）
    pub const ENERGY: usize = 7;
    /// 最近敌方相对方向 x × 接近度（`1 − d / 视野`）
    pub const ENEMY_DIR_X: usize = 8;
    /// 最近敌方相对方向 y × 接近度
    pub const ENEMY_DIR_Y: usize = 9;
    /// 最近敌方是否在武器射程内（0 / 1）
    pub const ENEMY_IN_RANGE: usize = 10;
    /// 视野内友方群落质心方向 x（受损友方加权）
    pub const ALLY_DIR_X: usize = 11;
    /// 视野内友方群落质心方向 y
    pub const ALLY_DIR_Y: usize = 12;
    /// 自身生命值比例
    pub const HP_FRACTION: usize = 13;
    /// 调质 DA（多巴胺）广播值
    pub const MOD_DA: usize = 14;
    /// 调质 NE（去甲肾上腺素）广播值
    pub const MOD_NE: usize = 15;
}

/// 6 维动作读出通道（Q8.8，`[-1, 1]`）
pub mod action {
    /// 移动偏置 x（`f_move`）
    pub const MOVE_X: usize = 0;
    /// 移动偏置 y
    pub const MOVE_Y: usize = 1;
    /// 采集倾向 `a_harvest`
    pub const HARVEST: usize = 2;
    /// 攻击倾向 `a_attack`
    pub const ATTACK: usize = 3;
    /// 防御 / 护盾再生倾向 `a_defense`
    pub const DEFENSE: usize = 4;
    /// 信号发射 / 相位同步 `a_emit`
    pub const EMIT: usize = 5;
}

/// 种子基因组规格（§5.1.2 表格）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeedGenomeSpec {
    pub slug: &'static str,
    pub unit: UnitKind,
    pub model: NeuronModelKind,
    pub n_hid: u32,
    /// 初始非零突触数 `E_0`
    pub synapses: u32,
    pub storage: TopologyStorageClass,
    /// CTRNN 时间常数区间 `[τ_min, τ_max]`（Ticks）
    pub tau_range: (u16, u16),
    pub pathways: &'static [Pathway],
}

impl SeedGenomeSpec {
    /// 总神经元数 `N = N_in + N_hid + N_out`
    pub const fn neuron_count(&self) -> u32 {
        GENOME_INPUTS as u32 + self.n_hid + GENOME_OUTPUTS as u32
    }
}

/// 一条先天行为通路：感知通道 → 专属隐藏神经元 → 动作通道
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pathway {
    pub input: usize,
    pub output: usize,
    /// 输出符号（+1 / −1）
    pub sign: i16,
    /// 感知量是否有符号（有符号则使用正负一对隐藏神经元）
    pub signed: bool,
}

const fn path(input: usize, output: usize, sign: i16, signed: bool) -> Pathway {
    Pathway {
        input,
        output,
        sign,
        signed,
    }
}

use action as a;
use sensor as s;

/// 6 大种子基因组（顺序与设计圣经一致）
pub const SEED_GENOMES: [SeedGenomeSpec; 6] = [
    // 强正权重 ∇F_nutrient → f_move 及 a_harvest；强负权重规避 F_hazard 与敌方近邻
    SeedGenomeSpec {
        slug: "seed.forager.v1",
        unit: UnitKind::Forager,
        model: NeuronModelKind::GatedMLP,
        n_hid: 24,
        synapses: 216,
        storage: TopologyStorageClass::DenseTiledMatrix,
        tau_range: (1, 1),
        pathways: &[
            path(s::NUTRIENT_GRAD_X, a::MOVE_X, 1, true),
            path(s::NUTRIENT_GRAD_Y, a::MOVE_Y, 1, true),
            path(s::NUTRIENT, a::HARVEST, 1, false),
            path(s::HAZARD, a::HARVEST, -1, false),
            path(s::ENEMY_DIR_X, a::MOVE_X, -1, true),
            path(s::ENEMY_DIR_Y, a::MOVE_Y, -1, true),
        ],
    },
    // 对最近敌方相对向量与 F_signal_b 高增益响应，敌方进入射程时高频触发 a_attack
    SeedGenomeSpec {
        slug: "seed.striker.v1",
        unit: UnitKind::Striker,
        model: NeuronModelKind::LIF,
        n_hid: 32,
        synapses: 288,
        storage: TopologyStorageClass::BlockSparseCSR,
        tau_range: (1, 1),
        pathways: &[
            path(s::ENEMY_DIR_X, a::MOVE_X, 1, true),
            path(s::ENEMY_DIR_Y, a::MOVE_Y, 1, true),
            path(s::ENEMY_IN_RANGE, a::ATTACK, 1, false),
            path(s::SIGNAL_B, a::ATTACK, 1, true),
            path(s::SIGNAL_A_GRAD_X, a::MOVE_X, 1, true),
            path(s::SIGNAL_A_GRAD_Y, a::MOVE_Y, 1, true),
        ],
    },
    // 慢时间常数 τ ∈ [4, 12]；强趋向友方群落质心前方阻挡敌火，高 a_defense 护盾再生偏置
    SeedGenomeSpec {
        slug: "seed.carapace.v1",
        unit: UnitKind::Carapace,
        model: NeuronModelKind::CTRNN,
        n_hid: 32,
        synapses: 320,
        storage: TopologyStorageClass::DenseTiledMatrix,
        tau_range: (4, 12),
        pathways: &[
            path(s::ALLY_DIR_X, a::MOVE_X, 1, true),
            path(s::ALLY_DIR_Y, a::MOVE_Y, 1, true),
            path(s::ENEMY_DIR_X, a::MOVE_X, 1, true),
            path(s::ENEMY_DIR_Y, a::MOVE_Y, 1, true),
            path(s::ENEMY_IN_RANGE, a::ATTACK, 1, false),
            path(s::ENEMY_IN_RANGE, a::DEFENSE, 1, false),
        ],
    },
    // 保持与敌方距离 [7.5, 11.0] 的风筝环路：敌方越近越后撤；优先攻击射程内目标
    SeedGenomeSpec {
        slug: "seed.spitter.v1",
        unit: UnitKind::Spitter,
        model: NeuronModelKind::LIF,
        n_hid: 40,
        synapses: 360,
        storage: TopologyStorageClass::BlockSparseCSR,
        tau_range: (1, 1),
        pathways: &[
            path(s::ENEMY_DIR_X, a::MOVE_X, -1, true),
            path(s::ENEMY_DIR_Y, a::MOVE_Y, -1, true),
            path(s::ENEMY_IN_RANGE, a::ATTACK, 1, false),
            path(s::SIGNAL_A_GRAD_X, a::MOVE_X, 1, true),
            path(s::SIGNAL_A_GRAD_Y, a::MOVE_Y, 1, true),
        ],
    },
    // 自动跟随受损友方簇并激活修复与相位同步输出 a_emit（双耦合振荡环路见 `weaver_oscillator`）
    SeedGenomeSpec {
        slug: "seed.weaver.v1",
        unit: UnitKind::Weaver,
        model: NeuronModelKind::CTRNN,
        n_hid: 48,
        synapses: 448,
        storage: TopologyStorageClass::BlockSparseCSR,
        tau_range: (2, 8),
        pathways: &[
            path(s::ALLY_DIR_X, a::MOVE_X, 1, true),
            path(s::ALLY_DIR_Y, a::MOVE_Y, 1, true),
            path(s::ENEMY_DIR_X, a::MOVE_X, -1, true),
            path(s::ENEMY_DIR_Y, a::MOVE_Y, -1, true),
        ],
    },
    // 多簇模块化脑：A 主炮集火，B 护盾与局部调质广播，C 富营养区触发增殖（a_emit）
    SeedGenomeSpec {
        slug: "seed.leviathan.v1",
        unit: UnitKind::Leviathan,
        model: NeuronModelKind::Izhikevich,
        n_hid: 106,
        synapses: 1024,
        storage: TopologyStorageClass::PagedDynamicAdjacency,
        tau_range: (1, 1),
        pathways: &[
            path(s::ENEMY_DIR_X, a::MOVE_X, 1, true),
            path(s::ENEMY_DIR_Y, a::MOVE_Y, 1, true),
            path(s::ENEMY_IN_RANGE, a::ATTACK, 1, false),
            path(s::SIGNAL_B, a::ATTACK, 1, true),
            path(s::ENEMY_IN_RANGE, a::DEFENSE, 1, false),
            path(s::NUTRIENT, a::EMIT, 1, false),
        ],
    },
];

/// 按单位类型查询种子基因组
pub const fn seed_genome_spec(unit: UnitKind) -> &'static SeedGenomeSpec {
    &SEED_GENOMES[unit as usize]
}

/// 按 slug 查询种子基因组
pub fn seed_genome_by_slug(slug: &str) -> Option<&'static SeedGenomeSpec> {
    SEED_GENOMES.iter().find(|g| g.slug == slug)
}

/// 先天通路权重（`±2.0`，Q8.8）
pub const PATHWAY_WEIGHT_Q8: i16 = 512;
/// 随机补齐突触的幅值区间（Q8.8 原始值）
pub const FILL_WEIGHT_RANGE_Q8: (i16, i16) = (8, 32);
/// 基因组构造使用的 Philox 随机流 ID
pub const GENOME_RNG_STREAM: u16 = 0x0601;

/// 单位基因组（模型无关的稠密权重表示）
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Genome {
    /// 基因组 slug：种子为 `seed.*.v1`，繁育后代为 `bred.<阵营>.<序号>` 或 `wild.w<波次>.<序号>`
    pub slug: String,
    pub unit: UnitKind,
    pub model: NeuronModelKind,
    pub storage: TopologyStorageClass,
    pub n_hid: u32,
    /// 输入 → 隐藏，行主序 `[h][i]`，长度 `n_hid × 16`
    pub w_in: Vec<i16>,
    /// 隐藏 → 隐藏循环权重 `[post][pre]`，长度 `n_hid²`（前馈模型为空）
    pub w_rec: Vec<i16>,
    /// 隐藏 → 动作，`[k][h]`，长度 `6 × n_hid`
    pub w_out: Vec<i16>,
    /// CTRNN 每神经元时间常数（Ticks）；其他模型为空
    pub tau_ticks: Vec<u16>,
    /// 繁育代数（种子为 0）
    pub generation: u32,
}

impl Genome {
    /// 是否带循环连接（CTRNN / Izhikevich）
    pub const fn is_recurrent(model: NeuronModelKind) -> bool {
        matches!(model, NeuronModelKind::CTRNN | NeuronModelKind::Izhikevich)
    }

    /// 由种子规格确定性构造 W_0
    pub fn seed(unit: UnitKind) -> Self {
        let spec = seed_genome_spec(unit);
        let n = spec.n_hid as usize;
        let recurrent = Self::is_recurrent(spec.model);
        let mut g = Self {
            slug: spec.slug.to_owned(),
            unit,
            model: spec.model,
            storage: spec.storage,
            n_hid: spec.n_hid,
            w_in: vec![0; n * GENOME_INPUTS],
            w_rec: if recurrent { vec![0; n * n] } else { Vec::new() },
            w_out: vec![0; GENOME_OUTPUTS * n],
            tau_ticks: Vec::new(),
            generation: 0,
        };
        // 1) 先天通路
        let mut h = 0usize;
        for p in spec.pathways {
            let halves: &[i16] = if p.signed { &[1, -1] } else { &[1] };
            for &half in halves {
                g.w_in[h * GENOME_INPUTS + p.input] = half * PATHWAY_WEIGHT_Q8;
                g.w_out[p.output * n + h] = half * p.sign * PATHWAY_WEIGHT_Q8;
                h += 1;
            }
        }
        // Weaver 双耦合振荡环路：h_a → h_b 兴奋、h_b → h_a 抑制，h_a 驱动 a_emit（Kuramoto 同步种子）
        if unit == UnitKind::Weaver {
            let (ha, hb) = (h, h + 1);
            g.w_rec[hb * n + ha] = PATHWAY_WEIGHT_Q8;
            g.w_rec[ha * n + hb] = -PATHWAY_WEIGHT_Q8;
            g.w_in[ha * GENOME_INPUTS + sensor::HP_FRACTION] = PATHWAY_WEIGHT_Q8;
            g.w_out[action::EMIT * n + ha] = PATHWAY_WEIGHT_Q8;
        }
        // CTRNN 时间常数：在 [τ_min, τ_max] 内按神经元序号确定性铺开
        if spec.model == NeuronModelKind::CTRNN {
            let (lo, hi) = spec.tau_range;
            let span = (hi - lo) as usize + 1;
            g.tau_ticks = (0..n).map(|j| lo + (j % span) as u16).collect();
        }
        // 2) 随机补齐到 E_0
        let target = spec.synapses as usize;
        let key = slug_key(spec.slug);
        let mut counter = 0u32;
        while g.synapse_count() < target {
            let r = RandomKey {
                world_seed: key,
                tick: 0,
                entity_serial: counter as u64,
                stream_and_phase: RandomKey::pack_stream_and_phase(GENOME_RNG_STREAM, 0),
                sub_counter: 0,
            }
            .generate_u32x4();
            counter += 1;
            let slot = r[0] as usize % g.slot_count();
            if g.slot_weight(slot) != 0 || !g.slot_has_room(slot) {
                continue;
            }
            let (lo, hi) = FILL_WEIGHT_RANGE_Q8;
            let mag = lo + (r[1] % (hi - lo + 1) as u32) as i16;
            let sign = if r[2] & 1 == 0 { 1 } else { -1 };
            g.set_slot_weight(slot, sign * mag);
        }
        g
    }

    /// 总神经元数 `N`
    pub fn neuron_count(&self) -> u32 {
        GENOME_INPUTS as u32 + self.n_hid + GENOME_OUTPUTS as u32
    }

    /// 非零突触数（`w_in ∪ w_rec ∪ w_out`）
    pub fn synapse_count(&self) -> usize {
        self.w_in.iter().chain(&self.w_rec).chain(&self.w_out).filter(|w| **w != 0).count()
    }

    /// 全部突触槽位数
    pub fn slot_count(&self) -> usize {
        self.w_in.len() + self.w_rec.len() + self.w_out.len()
    }

    /// 读取线性槽位（依次为 `w_in`、`w_rec`、`w_out`）
    pub fn slot_weight(&self, slot: usize) -> i16 {
        let (a, b) = (self.w_in.len(), self.w_rec.len());
        if slot < a {
            self.w_in[slot]
        } else if slot < a + b {
            self.w_rec[slot - a]
        } else {
            self.w_out[slot - a - b]
        }
    }

    /// 写入线性槽位
    pub fn set_slot_weight(&mut self, slot: usize, w: i16) {
        let (a, b) = (self.w_in.len(), self.w_rec.len());
        if slot < a {
            self.w_in[slot] = w;
        } else if slot < a + b {
            self.w_rec[slot - a] = w;
        } else {
            self.w_out[slot - a - b] = w;
        }
    }

    /// 该槽位所属循环行是否仍有空位（只约束 `PagedDynamicAdjacency` 的 32 槽位页）
    pub fn slot_has_room(&self, slot: usize) -> bool {
        if self.storage != TopologyStorageClass::PagedDynamicAdjacency {
            return true;
        }
        let a = self.w_in.len();
        if slot < a || slot >= a + self.w_rec.len() {
            return true;
        }
        let n = self.n_hid as usize;
        let post = (slot - a) / n;
        self.w_rec[post * n..(post + 1) * n].iter().filter(|w| **w != 0).count() < SYNAPSE_SLOTS_PER_NEURON
    }

    /// 规范字节序列（小端 i16 / u16 / u32 依次拼接）的 BLAKE3 摘要
    pub fn digest(&self) -> Blake3Hash {
        let mut bytes = Vec::with_capacity(self.slot_count() * 2 + 64);
        bytes.extend_from_slice(self.slug.as_bytes());
        bytes.push(0);
        bytes.push(self.unit as u8);
        bytes.push(self.model as u8);
        bytes.push(self.storage as u8);
        bytes.extend_from_slice(&self.n_hid.to_le_bytes());
        bytes.extend_from_slice(&self.generation.to_le_bytes());
        for w in self.w_in.iter().chain(&self.w_rec).chain(&self.w_out) {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        for t in &self.tau_ticks {
            bytes.extend_from_slice(&t.to_le_bytes());
        }
        Blake3Hash::of(&bytes)
    }
}

/// 由 slug 导出 64 位 Philox 世界种子：BLAKE3(slug) 前 8 字节（小端）
pub fn slug_key(slug: &str) -> u64 {
    let h = Blake3Hash::of(slug.as_bytes());
    u64::from_le_bytes(h.0[..8].try_into().expect("8 字节"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_genomes_match_frozen_topology_and_synapse_budget() {
        let expected = [
            ("seed.forager.v1", 46u32, 216usize),
            ("seed.striker.v1", 54, 288),
            ("seed.carapace.v1", 54, 320),
            ("seed.spitter.v1", 62, 360),
            ("seed.weaver.v1", 70, 448),
            ("seed.leviathan.v1", 128, 1024),
        ];
        for (unit, (slug, n, e0)) in UnitKind::ALL.iter().zip(expected) {
            let g = Genome::seed(*unit);
            assert_eq!(g.slug, slug);
            assert_eq!(g.neuron_count(), n);
            assert_eq!(seed_genome_spec(*unit).neuron_count(), n);
            assert_eq!(g.synapse_count(), e0, "{slug}");
            assert_eq!(Genome::seed(*unit), g, "{slug} 构造应确定");
        }
    }

    #[test]
    fn paged_dynamic_rows_respect_32_slots() {
        let g = Genome::seed(UnitKind::Leviathan);
        let n = g.n_hid as usize;
        for post in 0..n {
            assert!(g.w_rec[post * n..(post + 1) * n].iter().filter(|w| **w != 0).count() <= 32);
        }
    }

    #[test]
    fn carapace_time_constants_are_slow() {
        let g = Genome::seed(UnitKind::Carapace);
        assert!(g.tau_ticks.iter().all(|t| (4..=12).contains(t)));
        assert_eq!(g.tau_ticks.len(), 32);
    }

    #[test]
    fn digests_differ_between_seeds() {
        let mut ds: Vec<_> = UnitKind::ALL.iter().map(|u| Genome::seed(*u).digest()).collect();
        ds.sort();
        ds.dedup();
        assert_eq!(ds.len(), 6);
    }
}
