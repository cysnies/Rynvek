// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 跨语言平衡表夹具：`tests/arena/balance.json` 必须与 Rust 冻结表逐项一致。
//! `@rynvek/client` 的 `balance.test.ts` 对同一文件做对称断言，保证前端预览数值与权威层 100% 相同。
//!
//! 修改平衡表后以 `RYNVEK_BLESS=1 cargo test -p rynvek-rts --test balance_fixture` 重新生成。

use rynvek_rts::buildings::{BUILDING_SPECS, VAT_QUEUE_LIMIT, VAT_SUPPLY_BONUS};
use rynvek_rts::resources::{NEXUS_STORAGE_CAP, STARTER_RESOURCES};
use rynvek_rts::tech::TECH_SPECS;
use rynvek_rts::terrain::allows_building;
use rynvek_rts::units::UNIT_SPECS;
use rynvek_rts::{ResourceAmounts, TerrainTileKind};
use serde_json::{Value, json};

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/arena/balance.json");

fn res(r: &ResourceAmounts) -> Value {
    json!({ "biomass": r.biomass, "flux": r.flux, "neurogel": r.neurogel })
}

fn generate() -> Value {
    let terrains = [
        TerrainTileKind::OpenSubstrate,
        TerrainTileKind::ConductiveBasalt,
        TerrainTileKind::NutrientMarsh,
        TerrainTileKind::AcidicFissure,
        TerrainTileKind::ImperviousWall,
    ];
    json!({
        "starter_resources": res(&STARTER_RESOURCES),
        "nexus_storage_cap": res(&NEXUS_STORAGE_CAP),
        "vat_supply_bonus": VAT_SUPPLY_BONUS,
        "vat_queue_limit": VAT_QUEUE_LIMIT,
        "buildings": BUILDING_SPECS.iter().map(|b| json!({
            "kind": b.kind.as_str(),
            "name_zh": b.name_zh,
            "footprint": b.footprint,
            "cost": res(&b.cost),
            "build_ticks": b.build_ticks,
            "hp_milli": b.hp_milli,
            "armor": b.armor,
            "flux_per_s_milli": b.flux_per_s_milli,
            "vision_tiles": b.vision_tiles,
            "allowed_terrain": terrains.iter().map(|t| allows_building(*t, b.kind)).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "units": UNIT_SPECS.iter().map(|u| json!({
            "kind": u.kind.as_str(),
            "name_zh": u.name_zh,
            "supply": u.supply,
            "cost": res(&u.cost),
            "hatch_ticks": u.hatch_ticks,
            "hp_milli": u.hp_milli,
            "shield_milli": u.shield_milli,
            "armor": u.armor,
            "speed_milli_per_s": u.speed_milli_per_s,
            "vision_tiles": u.vision_tiles,
            "unlocked_by": u.unlocked_by.map(|t| t.as_str()),
        })).collect::<Vec<_>>(),
        "techs": TECH_SPECS.iter().map(|t| json!({
            "id": t.id.as_str(),
            "name_zh": t.name_zh,
            "tier": t.tier,
            "research_site": t.research_site().as_str(),
            "requires_buildings": t.requires_buildings.iter().map(|b| b.as_str()).collect::<Vec<_>>(),
            "requires_techs": t.requires_techs.iter().map(|x| x.as_str()).collect::<Vec<_>>(),
            "cost": res(&t.cost),
            "research_ticks": t.research_ticks,
        })).collect::<Vec<_>>(),
    })
}

#[test]
fn balance_fixture_matches_frozen_tables() {
    let expected = generate();
    if std::env::var_os("RYNVEK_BLESS").is_some() {
        std::fs::create_dir_all(std::path::Path::new(PATH).parent().unwrap()).unwrap();
        std::fs::write(PATH, serde_json::to_string_pretty(&expected).unwrap() + "\n").unwrap();
    }
    let text = std::fs::read_to_string(PATH).expect("缺少 tests/arena/balance.json（以 RYNVEK_BLESS=1 生成）");
    let actual: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(actual, expected);
}
