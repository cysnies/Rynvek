// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Lab 结果与实时观测的 Arrow RecordBatch / Parquet 编码（规范 1.1：Arrow 53+，Parquet 谱系文件）。
//!
//! 三张表的列结构在此冻结，`@rynvek/lab`、Python 包与服务端 `GET .../parquet?table=` 共用：
//! - `runs`：每个 run 一行的汇总；
//! - `metrics`：每个采样 Tick × 玩家阵营一行的经济 / 规模 / 得分指标与状态根；
//! - `lineage`：基因组 slug 首次出现记录（演化谱系）。
//!
//! Parquet 写出使用固定的写入属性（ZSTD 默认级别、单行组、不写统计时间戳），
//! 同一实验结果总是编码为逐字节相同的文件，可直接作为 CAS 内容寻址。

use std::sync::Arc;

use arrow::array::{
    ArrayRef, BooleanArray, FixedSizeBinaryArray, Int64Array, RecordBatch, StringArray, UInt8Array, UInt32Array,
    UInt64Array,
};
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use arrow::error::ArrowError;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;

use super::{ExperimentResult, FEATURE_NAMES, LineageRow, MetricRow, RunRecord};
use crate::observation::FactionObservation;

/// 结果表名
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LabTable {
    Runs,
    Metrics,
    Lineage,
}

impl LabTable {
    pub const ALL: [Self; 3] = [Self::Runs, Self::Metrics, Self::Lineage];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Runs => "runs",
            Self::Metrics => "metrics",
            Self::Lineage => "lineage",
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
    ]))
}

/// `metrics` 表结构
pub fn metrics_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("run", DataType::UInt32, false),
        field("tick", DataType::UInt64, false),
        field("faction", DataType::UInt8, false),
        field("biomass_milli", DataType::Int64, false),
        field("flux_milli", DataType::Int64, false),
        field("neurogel_milli", DataType::Int64, false),
        field("biomass_harvested_total_milli", DataType::Int64, false),
        field("supply_used", DataType::UInt32, false),
        field("supply_cap", DataType::UInt32, false),
        field("units", DataType::UInt32, false),
        field("buildings", DataType::UInt32, false),
        field("score_milli", DataType::Int64, false),
        field("wild_units", DataType::UInt32, false),
        field("pve_wave", DataType::UInt32, true),
        field("state_root", DataType::FixedSizeBinary(32), false),
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
        field("first_seen_tick", DataType::UInt64, false),
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
        ],
    )
}

pub fn metrics_batch(rows: &[MetricRow]) -> Result<RecordBatch, ArrowError> {
    RecordBatch::try_new(
        metrics_schema(),
        vec![
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.run))),
            arc(UInt64Array::from_iter_values(rows.iter().map(|r| r.tick))),
            arc(UInt8Array::from_iter_values(rows.iter().map(|r| r.faction))),
            arc(Int64Array::from_iter_values(rows.iter().map(|r| r.biomass_milli))),
            arc(Int64Array::from_iter_values(rows.iter().map(|r| r.flux_milli))),
            arc(Int64Array::from_iter_values(rows.iter().map(|r| r.neurogel_milli))),
            arc(Int64Array::from_iter_values(
                rows.iter().map(|r| r.biomass_harvested_total_milli),
            )),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.supply_used))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.supply_cap))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.units))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.buildings))),
            arc(Int64Array::from_iter_values(rows.iter().map(|r| r.score_milli))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.wild_units))),
            arc(UInt32Array::from_iter(rows.iter().map(|r| r.pve_wave))),
            if rows.is_empty() {
                arc(FixedSizeBinaryArray::new_null(32, 0))
            } else {
                roots(rows.iter().map(|r| &r.state_root))?
            },
        ],
    )
}

pub fn lineage_batch(rows: &[LineageRow]) -> Result<RecordBatch, ArrowError> {
    RecordBatch::try_new(
        lineage_schema(),
        vec![
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.run))),
            arc(UInt8Array::from_iter_values(rows.iter().map(|r| r.faction))),
            arc(StringArray::from_iter_values(rows.iter().map(|r| r.slug.as_str()))),
            arc(StringArray::from_iter_values(rows.iter().map(|r| r.unit.as_str()))),
            arc(UInt32Array::from_iter_values(rows.iter().map(|r| r.generation))),
            arc(UInt64Array::from_iter_values(rows.iter().map(|r| r.first_seen_tick))),
        ],
    )
}

/// 取实验结果中的一张表
pub fn table_batch(result: &ExperimentResult, table: LabTable) -> Result<RecordBatch, ArrowError> {
    match table {
        LabTable::Runs => runs_batch(&result.runs),
        LabTable::Metrics => metrics_batch(&result.metrics),
        LabTable::Lineage => lineage_batch(&result.lineage),
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
