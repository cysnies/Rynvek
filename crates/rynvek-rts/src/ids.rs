// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! RTS 领域的稳定字符串标识（项目文档第五章）。
//!
//! 资源 `res.*`、建筑 `bld.*`、单位 `unit.*`、科技 `tech.*` 与种子基因组 `seed.*.v1` 的文本 ID
//! 是跨语言（Rust / TypeScript / LLM ToolBroker）与回放归档的公开契约，必须与设计圣经逐字一致。

use serde::{Deserialize, Serialize};

/// 为「Rust 枚举 ↔ 规范文本 ID」生成 `ALL` / `as_str` / `parse` 与 serde 文本形态
macro_rules! text_id_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $( $(#[$vmeta:meta])* $variant:ident = $text:literal ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u8)]
        pub enum $name { $( $(#[$vmeta])* $variant ),+ }

        impl $name {
            /// 按规范表格顺序列出全部取值
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// 规范文本 ID
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            /// 由规范文本 ID 解析
            pub fn parse(text: &str) -> Option<Self> {
                match text { $($text => Some(Self::$variant),)+ _ => None }
            }

            /// 在 [`Self::ALL`] 中的序号
            pub const fn index(self) -> usize {
                self as usize
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                Self::parse(&text).ok_or_else(|| {
                    serde::de::Error::custom(format!(concat!("未知的 ", stringify!($name), "：{}"), text))
                })
            }
        }
    };
}

text_id_enum! {
    /// 三大战略资源（§5.2.1）
    pub enum ResourceKind {
        Biomass = "res.biomass",
        Flux = "res.flux",
        Neurogel = "res.neurogel",
    }
}

text_id_enum! {
    /// 八种工业网格建筑（§5.3.1）
    pub enum BuildingKind {
        NexusCore = "bld.nexus_core",
        BiomassHarvester = "bld.biomass_harvester",
        FluxReactor = "bld.flux_reactor",
        SynapseRefinery = "bld.synapse_refinery",
        SpawningVat = "bld.spawning_vat",
        SignalBeacon = "bld.signal_beacon",
        PulseTurret = "bld.pulse_turret",
        MutationCrucible = "bld.mutation_crucible",
    }
}

text_id_enum! {
    /// 六种神经原生单位（§5.3.2）
    pub enum UnitKind {
        Forager = "unit.forager",
        Striker = "unit.striker",
        Carapace = "unit.carapace",
        Spitter = "unit.spitter",
        Weaver = "unit.weaver",
        Leviathan = "unit.leviathan",
    }
}

text_id_enum! {
    /// 九项三阶科技 / 演化节点（§5.4）
    pub enum TechId {
        ChemotaxisAmp = "tech.chemotaxis_amp",
        FluxCapacitance = "tech.flux_capacitance",
        KineticResonance = "tech.kinetic_resonance",
        MulticellularChitin = "tech.multicellular_chitin",
        HebbianPlasticity = "tech.hebbian_plasticity",
        GapJunctionSync = "tech.gap_junction_sync",
        MorphogenesisApex = "tech.morphogenesis_apex",
        MetabolicSuperconduct = "tech.metabolic_superconduct",
        DirectedSpeciation = "tech.directed_speciation",
    }
}

text_id_enum! {
    /// 武器伤害类型（§5.3.2）
    pub enum DamageType {
        Kinetic = "Kinetic",
        BioElectric = "BioElectric",
        Corrosive = "Corrosive",
    }
}

text_id_enum! {
    /// 高层战略意图的命令种类（规范 09 §9.1 `issue_strategic_intent.order_kind`）
    pub enum OrderKind {
        Move = "Move",
        AttackMove = "AttackMove",
        Harvest = "Harvest",
        HoldPosition = "HoldPosition",
        Patrol = "Patrol",
    }
}

text_id_enum! {
    /// 五层标准连续环境场（§5.2.2），顺序即通道号 `Ch 0..4`
    pub enum FieldId {
        Nutrient = "field.nutrient",
        Energy = "field.energy",
        SignalA = "field.signal_a",
        SignalB = "field.signal_b",
        Hazard = "field.hazard",
    }
}

impl FieldId {
    /// 对应 `rynvek-field` 的通道
    pub const fn channel(self) -> rynvek_field::FieldChannelId {
        match self {
            Self::Nutrient => rynvek_field::FieldChannelId::Nutrient,
            Self::Energy => rynvek_field::FieldChannelId::Energy,
            Self::SignalA => rynvek_field::FieldChannelId::SignalA,
            Self::SignalB => rynvek_field::FieldChannelId::SignalB,
            Self::Hazard => rynvek_field::FieldChannelId::Hazard,
        }
    }
}

/// RTS 世界内的实体序号（单调递增、永不复用；建筑与单位共用同一序号空间）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntitySerial(pub u32);

impl EntitySerial {
    /// ToolBroker 与 UI 使用的文本形态（十进制序号）
    pub fn to_text(self) -> String {
        self.0.to_string()
    }

    /// 由十进制文本解析（拒绝前导 `+`、空串与前导零，保证文本形态唯一）
    pub fn parse(text: &str) -> Option<Self> {
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) || (text.len() > 1 && text.starts_with('0')) {
            return None;
        }
        text.parse().ok().map(Self)
    }
}

impl core::fmt::Display for EntitySerial {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 阵营序号：`0..` 为玩家阵营；[`FactionId::WILD`] 为 PvE 野生虫潮
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FactionId(pub u8);

impl FactionId {
    /// PvE 野生神经生物潮阵营
    pub const WILD: Self = Self(u8::MAX);

    pub const fn is_wild(self) -> bool {
        self.0 == u8::MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_ids_round_trip() {
        for k in BuildingKind::ALL {
            assert_eq!(BuildingKind::parse(k.as_str()), Some(*k));
        }
        for k in UnitKind::ALL {
            assert_eq!(UnitKind::parse(k.as_str()), Some(*k));
        }
        for k in TechId::ALL {
            assert_eq!(TechId::parse(k.as_str()), Some(*k));
        }
        assert_eq!(BuildingKind::ALL.len(), 8);
        assert_eq!(UnitKind::ALL.len(), 6);
        assert_eq!(TechId::ALL.len(), 9);
        assert_eq!(ResourceKind::ALL.len(), 3);
        assert_eq!(FieldId::ALL.len(), 5);
        assert_eq!(UnitKind::parse("unit.unknown"), None);
    }

    #[test]
    fn entity_serial_text_is_canonical() {
        assert_eq!(EntitySerial::parse("17"), Some(EntitySerial(17)));
        assert_eq!(EntitySerial::parse("0"), Some(EntitySerial(0)));
        assert_eq!(EntitySerial::parse("017"), None);
        assert_eq!(EntitySerial::parse("+1"), None);
        assert_eq!(EntitySerial::parse(""), None);
        assert_eq!(EntitySerial::parse("4294967296"), None);
    }
}
