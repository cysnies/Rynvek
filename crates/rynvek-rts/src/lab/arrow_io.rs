// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Lab 结果与实时观测的 Arrow RecordBatch / Parquet 编码（规范 1.1：Arrow 53+，Parquet 谱系文件）。
//!
//! 当前四张表的列结构在此统一定义，`@rynvek/lab`、Python 包与服务端 `GET .../parquet?table=` 共用：
//! - `runs`：每个 run 一行的汇总；
//! - `metrics`：每个采样 Tick × 玩家阵营一行的经济 / 规模 / 得分指标与状态根；
//! - `lineage`：真实单位出生节点与自然亲代链接；`energy`：精确生命账。
//!
//! Parquet 写出使用固定的写入属性（ZSTD 默认级别、单行组、不写统计时间戳），
//! 同一实验结果总是编码为逐字节相同的文件，可直接作为 CAS 内容寻址。

use std::sync::Arc;

use arrow::array::{
    ArrayRef, BooleanArray, Decimal128Array, FixedSizeBinaryArray, FixedSizeBinaryBuilder, Int64Array, RecordBatch,
    StringArray, UInt8Array, UInt32Array, UInt64Array,
};
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use arrow::error::ArrowError;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;

use super::{EnergyRow, ExperimentResult, FEATURE_NAMES, LineageRow, MetricRow, RunRecord};
use crate::observation::FactionObservation;

/// 结果表名
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LabTable {
    Runs,
    Metrics,
    Lineage,
    Energy,
}

impl LabTable {
    pub const ALL: [Self; 4] = [Self::Runs, Self::Metrics, Self::Lineage, Self::Energy];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Runs => "runs",
            Self::Metrics => "metrics",
            Self::Lineage => "lineage",
            Self::Energy => "energy",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == s)
    }
}

fn field(name: &str, dt: DataType, nullable: bool) -> Field {
    Field::new(name, dt, nullable)
}

/// `runs` 表结构
pub fn runs_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("run", DataType::UInt32, false),
        field("variant", DataType::Utf8, false),
        field("seed", DataType::UInt64, false),
        field("mode", DataType::Utf8, false),
        field("players", DataType::UInt8, false),
        field("policy", DataType::Utf8, false),
        field("final_tick", DataType::UInt64, false),
        field("winner", DataType::UInt8, true),
        field("end_reason", DataType::Utf8, false),
        field("final_state_root", DataType::FixedSizeBinary(32), false),
        field("commands_accepted", DataType::UInt64, false),
        field("commands_rejected", DataType::UInt64, false),
        field("genomes_bred", DataType::UInt32, false),
        field("waves_spawned", DataType::UInt32, false),
        field("natural_births", DataType::UInt32, false),
        field("max_generation", DataType::UInt32, false),
        field("model_digest", DataType::Utf8, true),
        field("parameter_fault", DataType::Utf8, true),
    ]))
}

/// `metrics` 表结构
pub fn metrics_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("run", DataType::UInt32, false),
        field("tick", DataType::UInt64, false),
        field("faction", DataType::UInt8, true),
        field("biomass_milli", DataType::Int64, true),
        field("flux_milli", DataType::Int64, true),
        field("neurogel_milli", DataType::Int64, true),
        field("biomass_harvested_total_milli", DataType::Int64, true),
        field("supply_used", DataType::UInt32, true),
        field("supply_cap", DataType::UInt32, true),
        field("units", DataType::UInt32, true),
        field("buildings", DataType::UInt32, true),
        field("score_milli", DataType::Int64, true),
        field("wild_units", DataType::UInt32, true),
        field("pve_wave", DataType::UInt32, true),
        field("state_root", DataType::FixedSizeBinary(32), false),
        field("parameter_entity", DataType::Utf8, true),
        field("parameter_component", DataType::Utf8, true),
        field("parameter_type", DataType::Utf8, true),
        field("parameter_value", DataType::Int32, true),
    ]))
}

/// `lineage` 表结构
pub fn lineage_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("run", DataType::UInt32, false),
        field("faction", DataType::UInt8, false),
        field("slug", DataType::Utf8, false),
        field("unit", DataType::Utf8, false),
        field("generation", DataType::UInt32, false),
        field("entity", DataType::UInt32, false),
        field("parent", DataType::UInt32, true),
        field("origin", DataType::Utf8, false),
        field("born_tick", DataType::UInt64, false),
        field("genome", DataType::FixedSizeBinary(32), false),
        field("parent_genome", DataType::FixedSizeBinary(32), true),
        field("allocated_q", DataType::UInt64, false),
    ]))
}

/// `energy` 表使用Decimal128保存全世界q，禁止转成浮点导致伪守恒。
pub fn energy_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("enabled", DataType::Boolean, false),
        field("run", DataType::UInt32, false),
        field("tick", DataType::UInt64, false),
        field("live_bodies", DataType::UInt32, false),
        field("remains", DataType::UInt32, false),
        field("natural_births", DataType::UInt32, false),
        field("max_generation", DataType::UInt32, false),
        field("received_q", DataType::Decimal128(38, 0), false),
        field("stored_q", DataType::Decimal128(38, 0), false),
        field("dissipated_q", DataType::Decimal128(38, 0), false),
        field("exported_q", DataType::Decimal128(38, 0), false),
        field("returned_q", DataType::Decimal128(38, 0), false),
        field("transferred_q", DataType::Decimal128(38, 0), false),
        field("external_received_q", DataType::Decimal128(38, 0), false),
        field("state_root", DataType::FixedSizeBinary(32), false),
        field("attack_q", DataType::Decimal128(38, 0), false),
        field("repair_q", DataType::Decimal128(38, 0), false),
        field("secretion_q", DataType::Decimal128(38, 0), false),
        field("synthesis_q", DataType::Decimal128(38, 0), false),
        field("continuous_q", DataType::Decimal128(38, 0), false),
        field("rule_q", DataType::Decimal128(38, 0), false),
        field("material_received_u", DataType::Decimal128(38, 0), false),
        field("material_external_u", DataType::Decimal128(38, 0), false),
        field("material_converted_u", DataType::Decimal128(38, 0), false),
        field("material_stored_u", DataType::Decimal128(38, 0), false),
        field("material_transferred_u", DataType::Decimal128(38, 0), false),
        field("material_returned_u", DataType::Decimal128(38, 0), false),
    ]))
}

fn arc<A: arrow::array::Array + 'static>(a: A) -> ArrayRef {
    Arc::new(a)
}

fn roots<'a>(it: impl Iterator<Item = &'a [u8; 32]>) -> Result<ArrayRef, ArrowError> {
    Ok(arc(FixedSizeBinaryArray::try_from_iter(it)?))
}

pub fn runs_batch(rows: &[RunRecord]) -> Result<RecordBatch, ArrowError> {
    RecordBatch::try_new(
        runs_schema(),
        vec![
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.run))),
            arc(StringArray::from_iter_values(rows.iter().map(|r| r.variant.as_str()))),
            arc(UInt64Array::from_iter_values(rows.iter().map(|r| r.seed))),
            arc(StringArray::from_iter_values(rows.iter().map(|r| r.mode.as_str()))),
            arc(UInt8Array::from_iter_values(rows.iter().map(|r| r.players))),
            arc(StringArray::from_iter_values(rows.iter().map(|r| r.policy.as_str()))),
            arc(UInt64Array::from_iter_values(rows.iter().map(|r| r.final_tick))),
            arc(UInt8Array::from_iter(rows.iter().map(|r| r.winner))),
            arc(StringArray::from_iter_values(
                rows.iter().map(|r| r.end_reason.as_str()),
            )),
            if rows.is_empty() {
                arc(FixedSizeBinaryArray::new_null(32, 0))
            } else {
                roots(rows.iter().map(|r| &r.final_state_root))?
            },
            arc(UInt64Array::from_iter_values(rows.iter().map(|r| r.commands_accepted))),
            arc(UInt64Array::from_iter_values(rows.iter().map(|r| r.commands_rejected))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.genomes_bred))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.waves_spawned))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.natural_births))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.max_generation))),
            arc(StringArray::from_iter(rows.iter().map(|r| r.model_digest.as_deref()))),
            arc(StringArray::from_iter(
                rows.iter().map(|r| r.parameter_fault.as_deref()),
            )),
        ],
    )
}

pub fn metrics_batch(rows: &[MetricRow]) -> Result<RecordBatch, ArrowError> {
    RecordBatch::try_new(
        metrics_schema(),
        vec![
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.run))),
            arc(UInt64Array::from_iter_values(rows.iter().map(|r| r.tick))),
            arc(UInt8Array::from_iter(rows.iter().map(|r| r.faction))),
            arc(Int64Array::from_iter(rows.iter().map(|r| r.biomass_milli))),
            arc(Int64Array::from_iter(rows.iter().map(|r| r.flux_milli))),
            arc(Int64Array::from_iter(rows.iter().map(|r| r.neurogel_milli))),
            arc(Int64Array::from_iter(
                rows.iter().map(|r| r.biomass_harvested_total_milli),
            )),
            arc(UInt32Array::from_iter(rows.iter().map(|r| r.supply_used))),
            arc(UInt32Array::from_iter(rows.iter().map(|r| r.supply_cap))),
            arc(UInt32Array::from_iter(rows.iter().map(|r| r.units))),
            arc(UInt32Array::from_iter(rows.iter().map(|r| r.buildings))),
            arc(Int64Array::from_iter(rows.iter().map(|r| r.score_milli))),
            arc(UInt32Array::from_iter(rows.iter().map(|r| r.wild_units))),
            arc(UInt32Array::from_iter(rows.iter().map(|r| r.pve_wave))),
            if rows.is_empty() {
                arc(FixedSizeBinaryArray::new_null(32, 0))
            } else {
                roots(rows.iter().map(|r| &r.state_root))?
            },
            arc(StringArray::from_iter(
                rows.iter().map(|r| r.parameter_entity.as_deref()),
            )),
            arc(StringArray::from_iter(
                rows.iter().map(|r| r.parameter_component.as_deref()),
            )),
            arc(StringArray::from_iter(rows.iter().map(|r| r.parameter_type.as_deref()))),
            arc(arrow::array::Int32Array::from_iter(
                rows.iter().map(|r| r.parameter_value),
            )),
        ],
    )
}

pub fn lineage_batch(rows: &[LineageRow]) -> Result<RecordBatch, ArrowError> {
    let mut parents = FixedSizeBinaryBuilder::with_capacity(rows.len(), 32);
    for row in rows {
        if let Some(root) = &row.parent_genome {
            parents.append_value(root)?;
        } else {
            parents.append_null();
        }
    }
    RecordBatch::try_new(
        lineage_schema(),
        vec![
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.run))),
            arc(UInt8Array::from_iter_values(rows.iter().map(|r| r.faction))),
            arc(StringArray::from_iter_values(rows.iter().map(|r| r.slug.as_str()))),
            arc(StringArray::from_iter_values(rows.iter().map(|r| r.unit.as_str()))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.generation))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.entity))),
            arc(UInt32Array::from_iter(rows.iter().map(|r| r.parent))),
            arc(StringArray::from_iter_values(rows.iter().map(|r| r.origin.as_str()))),
            arc(UInt64Array::from_iter_values(rows.iter().map(|r| r.born_tick))),
            if rows.is_empty() {
                arc(FixedSizeBinaryArray::new_null(32, 0))
            } else {
                roots(rows.iter().map(|r| &r.genome))?
            },
            arc(parents.finish()),
            arc(UInt64Array::from_iter_values(rows.iter().map(|r| r.allocated_q))),
        ],
    )
}

pub fn energy_batch(rows: &[EnergyRow]) -> Result<RecordBatch, ArrowError> {
    RecordBatch::try_new(
        energy_schema(),
        vec![
            arc(BooleanArray::from_iter(rows.iter().map(|r| Some(r.enabled)))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.run))),
            arc(UInt64Array::from_iter_values(rows.iter().map(|r| r.tick))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.live_bodies))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.remains))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.natural_births))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.max_generation))),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.received_q).expect("最多u32个实体的u64账本之和可由i128精确表示")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.stored_q).expect("最多u32个实体的u64账本之和可由i128精确表示")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.dissipated_q).expect("最多u32个实体的u64账本之和可由i128精确表示")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.exported_q).expect("最多u32个实体的u64账本之和可由i128精确表示")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.returned_q).expect("最多u32个实体的u64账本之和可由i128精确表示")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.transferred_q).expect("最多u32个实体的u64账本之和可由i128精确表示")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter().map(|r| {
                    i128::try_from(r.external_received_q).expect("最多u32个实体的u64账本之和可由i128精确表示")
                }),
            )
            .with_precision_and_scale(38, 0)?),
            if rows.is_empty() {
                arc(FixedSizeBinaryArray::new_null(32, 0))
            } else {
                roots(rows.iter().map(|r| &r.state_root))?
            },
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.attack_q).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.repair_q).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.secretion_q).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.synthesis_q).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.continuous_q).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter().map(|r| i128::try_from(r.rule_q).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.material_received_u).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.material_external_u).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.material_converted_u).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.material_stored_u).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.material_transferred_u).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
            arc(Decimal128Array::from_iter_values(
                rows.iter()
                    .map(|r| i128::try_from(r.material_returned_u).expect("热耗子账精确汇总")),
            )
            .with_precision_and_scale(38, 0)?),
        ],
    )
}

/// 取实验结果中的一张表
pub fn table_batch(result: &ExperimentResult, table: LabTable) -> Result<RecordBatch, ArrowError> {
    match table {
        LabTable::Runs => runs_batch(&result.runs),
        LabTable::Metrics => metrics_batch(&result.metrics),
        LabTable::Lineage => lineage_batch(&result.lineage),
        LabTable::Energy => energy_batch(&result.energy),
    }
}

/// 固定的 Parquet 写入属性（确定性输出）
pub fn writer_properties() -> WriterProperties {
    WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .set_created_by("rynvek-lab".to_owned())
        .set_max_row_group_size(1 << 20)
        .build()
}

/// 把 RecordBatch 编码为单个 Parquet 文件
pub fn to_parquet(batch: &RecordBatch) -> Result<Vec<u8>, parquet::errors::ParquetError> {
    let mut buf = Vec::new();
    let mut w = ArrowWriter::try_new(&mut buf, batch.schema(), Some(writer_properties()))?;
    w.write(batch)?;
    w.close()?;
    Ok(buf)
}

/// 阵营特征向量（零拷贝：`Int64Array` 直接接管 `Vec<i64>` 的缓冲区）
pub fn features_array(features: [i64; 24]) -> Int64Array {
    Int64Array::from(features.to_vec())
}

/// 特征名称列（与 [`FEATURE_NAMES`] 一一对应）
pub fn feature_names_array() -> StringArray {
    StringArray::from_iter_values(FEATURE_NAMES)
}

/// 观测中可见单位的列式视图（`units` RecordBatch：已按迷雾裁剪）
pub fn units_batch(obs: &FactionObservation) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(Schema::new(vec![
        field("id", DataType::UInt32, false),
        field("faction", DataType::UInt8, false),
        field("kind", DataType::Utf8, false),
        field("x_raw", DataType::Int64, false),
        field("y_raw", DataType::Int64, false),
        field("hp_milli", DataType::Int64, false),
        field("max_hp_milli", DataType::Int64, false),
        field("own", DataType::Boolean, false),
    ]));
    let u = &obs.units;
    RecordBatch::try_new(
        schema,
        vec![
            arc(UInt32Array::from_iter_values(u.iter().map(|v| v.id.0))),
            arc(UInt8Array::from_iter_values(u.iter().map(|v| v.faction.0))),
            arc(StringArray::from_iter_values(u.iter().map(|v| v.kind.as_str()))),
            arc(Int64Array::from_iter_values(u.iter().map(|v| v.position.x.0))),
            arc(Int64Array::from_iter_values(u.iter().map(|v| v.position.y.0))),
            arc(Int64Array::from_iter_values(u.iter().map(|v| v.hp_milli))),
            arc(Int64Array::from_iter_values(u.iter().map(|v| v.max_hp_milli))),
            arc(BooleanArray::from_iter(
                u.iter().map(|v| Some(v.faction == obs.faction)),
            )),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::{ExperimentSpec, run_experiment};
    use arrow::array::Array;

    #[test]
    fn parquet_output_is_deterministic_and_readable() {
        let spec: ExperimentSpec = serde_json::from_value(serde_json::json!({
            "name": "pq", "seeds": [5, 6], "ticks": 600, "sample_every": 150
        }))
        .unwrap();
        let r = run_experiment(&spec, &|_, _| {});
        for t in LabTable::ALL {
            let b = table_batch(&r, t).unwrap();
            let a = to_parquet(&b).unwrap();
            let c = to_parquet(&table_batch(&r, t).unwrap()).unwrap();
            assert_eq!(a, c, "{} 表 Parquet 编码不确定", t.name());
            assert_eq!(&a[..4], b"PAR1");
            let reader = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(bytes_of(a))
                .unwrap()
                .build()
                .unwrap();
            let rows: usize = reader.map(|b| b.unwrap().num_rows()).sum();
            assert_eq!(rows, b.num_rows());
        }
        assert_eq!(table_batch(&r, LabTable::Metrics).unwrap().num_rows(), 8);
        assert_eq!(LabTable::parse("lineage"), Some(LabTable::Lineage));
        assert_eq!(LabTable::parse("x"), None);
    }

    fn bytes_of(v: Vec<u8>) -> bytes::Bytes {
        bytes::Bytes::from(v)
    }

    #[test]
    fn empty_tables_encode() {
        let r = ExperimentResult::default();
        for t in LabTable::ALL {
            assert_eq!(table_batch(&r, t).unwrap().num_rows(), 0);
        }
    }

    #[test]
    fn units_batch_respects_fog() {
        let w = crate::world::ArenaWorld::new(crate::world::MatchConfig::duel(9));
        let obs = w.observe(crate::ids::FactionId(0)).unwrap();
        let b = units_batch(&obs).unwrap();
        assert_eq!(b.num_rows(), obs.units.len());
        assert_eq!(
            features_array(crate::lab::faction_features(&w, crate::ids::FactionId(0)).unwrap()).len(),
            24
        );
        assert_eq!(feature_names_array().len(), 24);
    }
}

/// 来自既有CAS Parquet的有界页面；64/128位整数作为十进制字符串，不经JS浮点。
#[derive(Debug, serde::Serialize)]
pub struct TablePage {
    pub total_rows: usize,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub columns: Vec<String>,
    pub rows: Vec<serde_json::Value>,
}
pub fn parquet_page(bytes: Vec<u8>, offset: usize, limit: usize) -> Result<TablePage, String> {
    use arrow::array::Array;
    use serde_json::{Value, json};
    if !(1..=500).contains(&limit) {
        return Err("分页limit必须在1..=500".into());
    }
    let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(bytes::Bytes::from(bytes))
        .map_err(|e| e.to_string())?;
    let total_rows = usize::try_from(builder.metadata().file_metadata().num_rows()).map_err(|e| e.to_string())?;
    let columns = builder.schema().fields().iter().map(|f| f.name().clone()).collect();
    let mut rows = Vec::new();
    if offset < total_rows {
        let reader = builder
            .with_offset(offset)
            .with_limit(limit)
            .with_batch_size(limit)
            .build()
            .map_err(|e| e.to_string())?;
        for batch in reader {
            let batch = batch.map_err(|e| e.to_string())?;
            for i in 0..batch.num_rows() {
                let mut row = serde_json::Map::new();
                for (field, col) in batch.schema().fields().iter().zip(batch.columns()) {
                    let v = if col.is_null(i) {
                        Value::Null
                    } else {
                        macro_rules! val {
                            ($ty:ty) => {
                                col.as_any()
                                    .downcast_ref::<$ty>()
                                    .ok_or("Parquet列类型不符")?
                                    .value(i)
                            };
                        }
                        match field.data_type() {
                            DataType::UInt8 => json!(val!(UInt8Array)),
                            DataType::UInt32 => json!(val!(UInt32Array)),
                            DataType::UInt64 => json!(val!(UInt64Array).to_string()),
                            DataType::Int32 => json!(val!(arrow::array::Int32Array)),
                            DataType::Int64 => json!(val!(Int64Array).to_string()),
                            DataType::Decimal128(_, 0) => json!(val!(Decimal128Array).to_string()),
                            DataType::Boolean => json!(val!(BooleanArray)),
                            DataType::Utf8 => json!(val!(StringArray)),
                            DataType::FixedSizeBinary(32) => json!(
                                val!(FixedSizeBinaryArray)
                                    .iter()
                                    .map(|b| format!("{b:02x}"))
                                    .collect::<String>()
                            ),
                            _ => return Err("结果表包含不支持的当前列类型".into()),
                        }
                    };
                    row.insert(field.name().clone(), v);
                }
                rows.push(Value::Object(row));
            }
        }
    }
    let end = offset.saturating_add(rows.len());
    Ok(TablePage {
        total_rows,
        offset,
        next_offset: (end < total_rows).then_some(end),
        columns,
        rows,
    })
}
