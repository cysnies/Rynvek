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
      ,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true}}}}]
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
        serde_json::json!({"label":"x","mode":"duel","ecology":{"roles":{"unit.forager":{"basal_cost_micro_per_tick":1000,"reserve_ticks":4,"birth_energy_milli":1000,"movement_cost_micro_per_tile":0,"synapse_maintenance_micro_per_tick":0,"natural_reproduction":true}}}}),
    ] {
        let spec: ExperimentSpec =
            serde_json::from_value(serde_json::json!({"name":"bad","seeds":[1],"ticks":1,"variants":[variant]}))
                .unwrap();
        assert!(spec.validate().is_err());
    }
}
