// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

#![cfg(feature = "arrow")]

use rynvek_rts::lab::arrow_io::{self, LabTable};
use rynvek_rts::lab::{ExperimentSpec, run_experiment};

#[test]
fn real_multigeneration_births_feed_lab_genealogy_and_exact_energy_tables() {
    let spec: ExperimentSpec = serde_json::from_value(serde_json::json!({
      "name":"真实自然谱系", "seeds":[57],"ticks":600,"sample_every":300,
      "variants":[{"label":"自然复制", "policy":"ecology", "ecology":{"roles":{"unit.forager":{
        "basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000
      ,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"attack_cost_micro_per_shot":0,"repair_cost_micro_per_milli":0,"rule_cost_micro_per_gas":0,"material_capacity_milli":8,"material_growth_milli_per_tick":1,"material_synthesis_micro_per_milli":1,"continuous_cost_micro_per_neuron_step":1,"secretion_cost_micro_per_field_raw":0,"birth_synthesis_milli":10,"activity_reserve_milli":0,"replication_mutation_rate_permille":50}}}}]
    }))
    .unwrap();
    spec.validate().unwrap();
    let result = run_experiment(&spec, &|_, _| {});
    let births: Vec<_> = result.lineage.iter().filter(|r| r.origin == "mitosis").collect();
    assert!(
        !births.is_empty(),
        "真实生成地图/合法采集与建槽后须有自然出生，不注入个体能量"
    );
    assert!(
        births
            .iter()
            .any(|child| births.iter().any(|p| Some(p.entity) == child.parent)),
        "必须观察到至少连续两次自然代际，不拿虫潮代数冒充"
    );
    assert_eq!(result.runs[0].natural_births, births.len() as u32);
    assert_eq!(
        result.runs[0].max_generation,
        births.iter().map(|b| b.generation).max().unwrap()
    );
    for child in births {
        let parent = result.lineage.iter().find(|p| Some(p.entity) == child.parent).unwrap();
        assert_eq!(child.generation, parent.generation + 1);
        assert_eq!(child.parent_genome, Some(parent.genome));
        assert!(child.entity > parent.entity);
        assert_eq!(child.allocated_q, 4_000_000);
    }
    for row in &result.energy {
        assert_eq!(row.received_q, row.external_received_q + row.transferred_q);
        assert_eq!(
            row.external_received_q,
            row.stored_q + row.dissipated_q + row.exported_q + row.returned_q
        );
        assert_eq!(row.transferred_q, u128::from(row.natural_births) * 4_000_000);
        assert_eq!(
            row.state_root,
            result.metrics.iter().find(|m| m.tick == row.tick).unwrap().state_root
        );
    }
    assert_eq!(result.energy.len(), 2);
    assert!(result.energy.last().unwrap().synthesis_q > 0);
    assert!(result.energy.last().unwrap().continuous_q > 0);
    assert!(result.energy.last().unwrap().material_external_u > 0);
    for row in &result.energy {
        assert_eq!(
            row.material_received_u - row.material_transferred_u,
            row.material_external_u
        );
        assert_eq!(row.material_external_u, row.material_stored_u + row.material_returned_u);
    }
    for row in &result.energy {
        assert!(row.synthesis_q >= u128::from(row.natural_births) * 40_000);
        assert!(
            row.attack_q + row.repair_q + row.secretion_q + row.synthesis_q + row.continuous_q + row.rule_q
                <= row.dissipated_q
        );
    }
    for name in [
        "attack_q",
        "repair_q",
        "secretion_q",
        "synthesis_q",
        "continuous_q",
        "rule_q",
        "material_received_u",
        "material_external_u",
        "material_stored_u",
        "material_transferred_u",
        "material_returned_u",
    ] {
        let schema = arrow_io::energy_schema();
        assert_eq!(
            schema.field_with_name(name).unwrap().data_type(),
            &arrow::datatypes::DataType::Decimal128(38, 0)
        );
    }
    for table in LabTable::ALL {
        let batch = arrow_io::table_batch(&result, table).unwrap();
        let a = arrow_io::to_parquet(&batch).unwrap();
        assert_eq!(&a[..4], b"PAR1");
        assert_eq!(a, arrow_io::to_parquet(&batch).unwrap());
    }
}

#[test]
fn ecology_experiment_cannot_silently_enable_ranked_or_incomplete_contract() {
    for variant in [
        serde_json::json!({"label":"x","policy":"ecology"}),
        serde_json::json!({"label":"x","mode":"duel","ecology":{"roles":{"unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true,"attack_cost_micro_per_shot":0,"repair_cost_micro_per_milli":0,"rule_cost_micro_per_gas":0,"material_capacity_milli":8,"material_growth_milli_per_tick":1,"material_synthesis_micro_per_milli":1,"continuous_cost_micro_per_neuron_step":1,"secretion_cost_micro_per_field_raw":0,"birth_synthesis_milli":0,"activity_reserve_milli":0,"replication_mutation_rate_permille":50}}}}),
    ] {
        let spec: ExperimentSpec =
            serde_json::from_value(serde_json::json!({"name":"bad","seeds":[1],"ticks":1,"variants":[variant]}))
                .unwrap();
        assert!(spec.validate().is_err());
    }
}

#[test]
fn workbench_page_preserves_decimal128_and_64bit_values_without_floating_point() {
    use rynvek_rts::lab::{EnergyRow, ExperimentResult};
    let exact = 1u128 << 96;
    let result = ExperimentResult {
        energy: vec![EnergyRow {
            enabled: true,
            tick: u64::MAX,
            received_q: exact,
            ..Default::default()
        }],
        ..Default::default()
    };
    let bytes = arrow_io::to_parquet(&arrow_io::table_batch(&result, LabTable::Energy).unwrap()).unwrap();
    let page = arrow_io::parquet_page(bytes.clone(), 0, 1).unwrap();
    assert_eq!(page.rows[0]["received_q"], exact.to_string());
    assert_eq!(page.rows[0]["tick"], u64::MAX.to_string());
    let empty = arrow_io::parquet_page(bytes, 1, 1).unwrap();
    assert!(empty.rows.is_empty());
    assert!(empty.next_offset.is_none());
}
