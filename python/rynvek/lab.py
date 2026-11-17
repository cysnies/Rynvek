# SPDX-FileCopyrightText: 2026 The Rynvek Contributors
# SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

"""Rynvek Lab：本地批量多随机种子实验与 Parquet 谱系文件。

实验规格与 ``POST /api/v1/lab/experiments`` 的实验部分相同；四张结果表
（``runs`` / ``metrics`` / ``lineage`` / ``energy``）的列结构与服务端 ``GET .../parquet?table=`` 完全一致，
同一规格在本地与服务端产出逐字节相同的 Parquet 文件。
"""

from __future__ import annotations

import json
from collections.abc import Iterable, Mapping
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Literal

import pyarrow as pa
import pyarrow.parquet as pq

from rynvek import _native

TableName = Literal["runs", "metrics", "lineage", "energy"]
TABLES: tuple[TableName, ...] = ("runs", "metrics", "lineage", "energy")


@dataclass(frozen=True)
class RoleEnergyContract:
    """显式角色生命成本；零值明确表示未启用该项，不冒充规范平衡。"""

    basal_cost_micro_per_tick: int
    reserve_ticks: int
    birth_energy_milli: int
    movement_cost_micro_per_tile: int
    synapse_maintenance_micro_per_tick: int
    natural_reproduction: bool
    replication_mutation_rate_permille: int


@dataclass(frozen=True)
class EcologyContract:
    roles: dict[str, RoleEnergyContract]


@dataclass(frozen=True)
class Variant:
    """实验变体（一组对照条件）。"""

    label: str
    mode: Literal["pve", "duel", "parameters"] = "pve"
    players: int | None = None
    policy: Literal["idle", "economy", "ecology"] = "economy"
    ecology: EcologyContract | None = None
    # Immutable bounded World/Rules JSON model, validated/compiled by the original native Engine.
    model: dict[str, Any] | None = None


@dataclass(frozen=True)
class ExperimentSpec:
    """实验规格。``variants`` 为空时使用服务端基线变体（单人 PvE + 经济策略）。"""

    name: str
    seeds: list[int]
    ticks: int
    sample_every: int = _native.POLICY_INTERVAL_TICKS
    variants: list[Variant] = field(default_factory=list)

    def to_json(self) -> str:
        d = asdict(self)
        d["variants"] = [{k: v for k, v in var.items() if v is not None} for var in d["variants"]]
        return json.dumps(d, ensure_ascii=False, separators=(",", ":"))


def sweep(name: str, seeds: Iterable[int], ticks: int, variants: Iterable[Variant], **kw: Any) -> ExperimentSpec:
    """多变体 × 多种子扫描。"""
    return ExperimentSpec(name=name, seeds=list(seeds), ticks=ticks, variants=list(variants), **kw)


class ExperimentResult:
    """实验结果（四张 Arrow 表）。"""

    def __init__(self, native: _native.Experiment) -> None:
        self._native = native

    def batch(self, table: TableName) -> pa.RecordBatch:
        return self._native.table(table)

    def table(self, table: TableName) -> pa.Table:
        return pa.Table.from_batches([self.batch(table)])

    @property
    def runs(self) -> pa.Table:
        return self.table("runs")

    @property
    def metrics(self) -> pa.Table:
        return self.table("metrics")

    @property
    def lineage(self) -> pa.Table:
        return self.table("lineage")

    @property
    def energy(self) -> pa.Table:
        return self.table("energy")

    def parquet(self, table: TableName) -> bytes:
        """Parquet 字节（ZSTD；可直接作为 CAS 内容寻址）。"""
        return self._native.parquet(table)

    def write_parquet(self, directory: str | Path) -> dict[TableName, Path]:
        """写出 ``<directory>/<table>.parquet``，返回各表路径。"""
        out = Path(directory)
        out.mkdir(parents=True, exist_ok=True)
        paths: dict[TableName, Path] = {}
        for t in TABLES:
            p = out / f"{t}.parquet"
            p.write_bytes(self.parquet(t))
            paths[t] = p
        return paths

    def __repr__(self) -> str:
        return repr(self._native)


def run_experiment(spec: ExperimentSpec | Mapping[str, Any] | str) -> ExperimentResult:
    """执行实验（释放 GIL，原生侧多线程并行）。规格非法时抛出 ``ValueError``。"""
    if isinstance(spec, ExperimentSpec):
        text = spec.to_json()
    elif isinstance(spec, str):
        text = spec
    else:
        text = json.dumps(dict(spec), ensure_ascii=False)
    return ExperimentResult(_native.run_experiment(text))


def read_parquet(data: bytes) -> pa.Table:
    """读取实验 Parquet 字节（例如服务端路由 18 的响应体）。"""
    return pq.read_table(pa.BufferReader(data))


__all__ = [
    "TABLES",
    "ExperimentResult",
    "EcologyContract",
    "RoleEnergyContract",
    "ExperimentSpec",
    "Variant",
    "read_parquet",
    "run_experiment",
    "sweep",
]
