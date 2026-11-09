// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 平衡表与设计圣经逐格比对：直接解析 `docs/Rynvek_Project_Documentation_v1.0.md` 第五章的
//! Markdown 表格，断言资源、建筑、单位、科技与种子基因组的每个数值与 Rust 常量表完全一致。

use rynvek_neural::{NeuronModelKind, TopologyStorageClass};
use rynvek_rts::buildings::{BUILDING_SPECS, building_spec};
use rynvek_rts::genome::{Genome, SEED_GENOMES};
use rynvek_rts::ids::{BuildingKind, DamageType, ResourceKind, TechId, UnitKind};
use rynvek_rts::resources::resource_color_hex;
use rynvek_rts::tech::{TECH_SPECS, tech_spec};
use rynvek_rts::units::{UNIT_SPECS, unit_spec};

fn bible() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/Rynvek_Project_Documentation_v1.0.md"))
        .expect("缺少项目文档")
}

/// 取某个标题之后第一张表格的数据行（去掉表头与分隔行），每行拆为去反引号的单元格
fn table_after(doc: &str, heading: &str) -> Vec<Vec<String>> {
    let start = doc.find(heading).unwrap_or_else(|| panic!("找不到标题 {heading}"));
    let rows: Vec<Vec<String>> = doc[start..]
        .lines()
        .skip_while(|l| !l.starts_with('|'))
        .take_while(|l| l.starts_with('|'))
        .map(|l| l.trim().trim_matches('|').split('|').map(|c| c.trim().replace('`', "")).collect())
        .collect();
    rows.into_iter().skip(2).collect()
}

/// 抽取单元格中的全部数字（含负号与小数）
fn numbers(cell: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<f64>| {
        if !cur.is_empty() && cur != "-" && cur != "." {
            out.push(cur.trim_end_matches('.').parse().unwrap());
        }
        cur.clear();
    };
    for ch in cell.chars() {
        if ch.is_ascii_digit() || ch == '.' || (ch == '-' && cur.is_empty()) {
            cur.push(ch);
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn milli(v: f64) -> i64 {
    (v * 1000.0).round() as i64
}

#[test]
fn resources_table_matches() {
    let rows = table_after(&bible(), "#### 5.2.1 三大战略资源");
    assert_eq!(rows.len(), 3);
    for (row, kind) in rows.iter().zip(ResourceKind::ALL) {
        assert_eq!(row[0], kind.as_str());
        assert_eq!(row[3], resource_color_hex(*kind));
        assert_eq!(row[2], "i64 milli-unit");
    }
}

#[test]
fn building_table_matches() {
    let rows = table_after(&bible(), "#### 5.3.1 八种工业建筑平衡表");
    assert_eq!(rows.len(), BUILDING_SPECS.len());
    for (row, kind) in rows.iter().zip(BuildingKind::ALL) {
        let s = building_spec(*kind);
        assert_eq!(row[0], kind.as_str());
        assert_eq!(row[1], s.name_zh);
        assert_eq!(row[2], format!("{0}×{0}", s.footprint), "{kind} 尺寸");
        if *kind != BuildingKind::NexusCore {
            assert_eq!(numbers(&row[3]), [s.cost.biomass as f64 / 1000.0, s.cost.neurogel as f64 / 1000.0], "{kind} 造价");
        } else {
            assert_eq!(row[3], "开局自带");
        }
        assert_eq!(numbers(&row[4])[0] as u32, s.build_ticks, "{kind} 建造耗时");
        assert_eq!(numbers(&row[5]), [s.hp_milli as f64 / 1000.0, s.armor as f64], "{kind} HP/Armor");
        assert_eq!(milli(numbers(&row[6])[0]), s.flux_per_s_milli, "{kind} Flux 收支");
    }
    // 战术参数列中的关键常量
    let tactical: Vec<f64> = numbers(&rows[0][7]);
    assert_eq!(&tactical[..3], &[2000.0, 1000.0, 500.0]);
    assert!(rows[6][7].contains("射程 12 Tiles") && rows[6][7].contains("单发消耗 4 Flux") && rows[6][7].contains("28"));
    assert!(rows[4][7].contains("生产队列上限 5") && rows[4][7].contains("+24 Supply"));
    assert!(rows[5][7].contains("作用半径 16 Tiles") && rows[5][7].contains("14 Tiles"));
}

#[test]
fn unit_table_matches() {
    let rows = table_after(&bible(), "#### 5.3.2 六种神经原生单位集群平衡表");
    assert_eq!(rows.len(), UNIT_SPECS.len());
    for (row, kind) in rows.iter().zip(UnitKind::ALL) {
        let s = unit_spec(*kind);
        assert_eq!(row[0], kind.as_str());
        assert_eq!(row[1], s.name_zh);
        assert_eq!(numbers(&row[2])[0] as u32, s.supply, "{kind} 人口");
        let c = numbers(&row[3]);
        assert_eq!(
            (milli(c[0]), milli(c[1]), milli(c[2])),
            (s.cost.biomass, s.cost.neurogel, s.cost.flux),
            "{kind} 造价 Bio/Gel/Flux"
        );
        assert_eq!(numbers(&row[4])[0] as u32, s.hatch_ticks, "{kind} 孵化");
        let d = numbers(&row[5]);
        assert_eq!((milli(d[0]), milli(d[1]), d[2] as i64), (s.hp_milli, s.shield_milli, s.armor), "{kind} 防御");
        let m = numbers(&row[6]);
        assert_eq!((milli(m[0]), milli(m[1])), (s.radius_milli, s.speed_milli_per_s), "{kind} 半径/移速");
        assert_eq!(numbers(&row[7])[0] as u32, s.vision_tiles, "{kind} 视野");
        match s.weapon {
            Some(w) => {
                let cell = &row[8];
                assert!(cell.starts_with(w.damage_type.as_str()), "{kind} 伤害类型");
                let n: Vec<f64> = cell.split('/').skip(1).flat_map(|p| numbers(p).into_iter().take(1)).collect();
                assert_eq!(
                    (milli(n[0]), milli(n[1]), n[2] as u32),
                    (w.damage_milli, w.range_milli, w.cooldown_ticks),
                    "{kind} 武器"
                );
            }
            None => assert!(row[8].starts_with("支援光环")),
        }
    }
    assert!(rows[0][8].contains("采集速率 4.0 Bio/s"));
    assert!(rows[2][8].contains("90^\\circ"));
    assert!(rows[3][8].contains("沉积 15.0 Hazard"));
    assert!(rows[4][8].contains("半径 5.0") && rows[4][8].contains("+12 HP/s") && rows[4][8].contains("+15%"));
    assert!(rows[5][8].contains("半径 8.0"));
    assert_eq!(unit_spec(UnitKind::Carapace).weapon.unwrap().damage_type, DamageType::BioElectric);
}

#[test]
fn tech_table_matches() {
    let rows = table_after(&bible(), "### 5.4 三阶工业-生物协同科技与演化树");
    assert_eq!(rows.len(), TECH_SPECS.len());
    for (row, id) in rows.iter().zip(TechId::ALL) {
        let s = tech_spec(*id);
        assert_eq!(numbers(&row[0])[0] as u8, s.tier, "{id} 层级");
        assert_eq!(row[1], id.as_str());
        assert_eq!(row[2].trim_matches('*'), s.name_zh);
        // 前置条件：逐个 ID 出现在单元格中
        for b in s.requires_buildings {
            let text = if *b == BuildingKind::MutationCrucible && !s.requires_techs.is_empty() {
                "Crucible"
            } else {
                b.as_str()
            };
            assert!(row[3].contains(text), "{id} 前置建筑 {b}");
        }
        for t in s.requires_techs {
            assert!(row[3].contains(t.as_str()), "{id} 前置科技 {t}");
        }
        let c = numbers(&row[4]);
        assert_eq!(
            (milli(c[0]), milli(c[1]), milli(c[2])),
            (s.cost.biomass, s.cost.neurogel, s.cost.flux),
            "{id} 研发成本 Biomass/Neurogel/Flux"
        );
        assert_eq!(numbers(&row[5])[0] as u32, s.research_ticks, "{id} 研发耗时");
    }
}

#[test]
fn seed_genome_table_matches() {
    let rows = table_after(&bible(), "#### 5.1.2 标准开局初始配额");
    assert_eq!(rows.len(), SEED_GENOMES.len());
    for (row, spec) in rows.iter().zip(&SEED_GENOMES) {
        assert_eq!(row[0], spec.slug);
        assert_eq!(row[1], spec.unit.as_str());
        let model = match spec.model {
            NeuronModelKind::GatedMLP => "GatedMLP",
            NeuronModelKind::LIF => "LIF (SNN)",
            NeuronModelKind::CTRNN => "CTRNN",
            NeuronModelKind::Izhikevich => "Izhikevich",
        };
        assert_eq!(row[2], model);
        let topo = numbers(&row[3]);
        assert_eq!(topo[..3], [16.0, spec.n_hid as f64, 6.0], "{} 拓扑", spec.slug);
        assert_eq!(topo[3] as u32, spec.neuron_count(), "{} N", spec.slug);
        assert_eq!(numbers(&row[4])[0] as u32, spec.synapses, "{} E_0", spec.slug);
        let storage = match spec.storage {
            TopologyStorageClass::DenseTiledMatrix => "DenseTiled",
            TopologyStorageClass::BlockSparseCSR => "BlockSparse",
            TopologyStorageClass::PagedDynamicAdjacency => "PagedDynamic",
        };
        assert!(row[4].contains(storage), "{} 存储布局", spec.slug);
        assert_eq!(Genome::seed(spec.unit).synapse_count() as u32, spec.synapses);
    }
}
