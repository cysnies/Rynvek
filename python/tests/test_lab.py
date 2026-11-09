# SPDX-FileCopyrightText: 2026 The Rynvek Contributors
# SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

"""Lab 批量实验：冻结列结构、Parquet 逐字节可复现、与逐 Tick 仿真的状态根一致。"""

from __future__ import annotations

from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
import pytest

from rynvek import ArenaSim
from rynvek.lab import ExperimentSpec, Variant, read_parquet, run_experiment, sweep

SPEC = sweep(
    "m8-python",
    seeds=[1, 2],
    ticks=300,
    sample_every=60,
    variants=[Variant("pve-economy"), Variant("duel-idle", mode="duel", policy="idle")],
)


@pytest.fixture(scope="module")
def result():
    return run_experiment(SPEC)


def test_tables_have_frozen_schemas(result) -> None:
    assert result.runs.num_rows == 4
    assert result.runs.column_names[:3] == ["run", "variant", "seed"]
    assert result.runs.schema.field("final_state_root").type == pa.binary(32)
    assert set(result.runs.column("variant").to_pylist()) == {"pve-economy", "duel-idle"}
    metrics = result.metrics
    assert metrics.schema.field("state_root").type == pa.binary(32)
    # 每 60 Tick 采样一次（含第 300 Tick）：PvE 1 名玩家、双人局 2 名玩家
    assert metrics.num_rows == 2 * 5 * 1 + 2 * 5 * 2
    assert result.lineage.num_rows > 0
    assert result.lineage.column_names == ["run", "faction", "slug", "unit", "generation", "first_seen_tick"]


def test_final_state_roots_match_tick_by_tick_simulation(result) -> None:
    for row in result.runs.to_pylist():
        mode = row["mode"]
        sim = ArenaSim(mode, row["seed"], row["players"])
        while sim.tick < SPEC.ticks and not sim.is_over():
            for f in sim.players:
                sim.run_policy(f, row["policy"])
            sim.step(1)
        assert sim.state_root() == row["final_state_root"], row["variant"]
        assert sim.tick == row["final_tick"]


def test_parquet_is_byte_reproducible(result, tmp_path: Path) -> None:
    again = run_experiment(SPEC.to_json())
    for table in ("runs", "metrics", "lineage"):
        data = result.parquet(table)
        assert data[:4] == b"PAR1" and data == again.parquet(table)
        assert read_parquet(data).equals(result.table(table))
    paths = result.write_parquet(tmp_path / "out")
    meta = pq.ParquetFile(paths["metrics"]).metadata
    assert meta.created_by.startswith("rynvek-lab")
    assert meta.row_group(0).column(0).compression == "ZSTD"


def test_tables_cross_the_ffi_without_copying(result) -> None:
    before = pa.total_allocated_bytes()
    batch = result.batch("metrics")
    assert batch.num_rows > 0 and pa.total_allocated_bytes() == before


def test_invalid_specs_are_rejected() -> None:
    with pytest.raises(ValueError):
        run_experiment(ExperimentSpec(name="", seeds=[1], ticks=10))
    with pytest.raises(ValueError):
        run_experiment({"name": "x", "seeds": [], "ticks": 10})
    with pytest.raises(ValueError):
        run_experiment("{broken")
    with pytest.raises(ValueError):
        result_of_bad_table = run_experiment({"name": "x", "seeds": [1], "ticks": 30})
        result_of_bad_table.parquet("nope")  # type: ignore[arg-type]
