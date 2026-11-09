# SPDX-FileCopyrightText: 2026 The Rynvek Contributors
# SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

"""原生仿真句柄：确定性、状态根、Arrow 零拷贝观测、错误码与 `.rworld` 加载。"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
import pyarrow as pa
import pytest

import rynvek
from rynvek import ArenaSim, RynvekError


def run(sim: ArenaSim, ticks: int) -> bytes:
    for _ in range(ticks):
        for f in sim.players:
            sim.run_policy(f, "economy")
        sim.step(1)
    return sim.state_root()


def test_constants_are_frozen() -> None:
    assert len(rynvek.FEATURE_NAMES) == 24
    assert rynvek.FEATURE_NAMES[0] == "tick" and rynvek.FEATURE_NAMES[-1] == "genomes_known"
    assert rynvek.MACRO_ACTIONS == [
        "noop",
        "harvest",
        "build_harvester",
        "build_vat",
        "train_forager",
        "train_striker",
        "attack_move",
        "defend",
        "build_turret",
    ]
    assert rynvek.POLICY_INTERVAL_TICKS == 30


def test_same_seed_is_bit_exact_and_seeds_diverge() -> None:
    a, b, c = ArenaSim("duel", 7), ArenaSim("duel", 7), ArenaSim("duel", 8)
    assert a.players == [0, 1]
    assert a.state_root() == b.state_root() != c.state_root()
    ra, rb = run(a, 600), run(b, 600)
    assert len(ra) == 32 and ra == rb
    assert a.tick == 600
    a.reset()
    assert a.tick == 0 and a.state_root() == ArenaSim("duel", 7).state_root()
    a.reset(8)
    assert a.state_root() == c.state_root() and a.seed == 8


def test_features_cross_the_ffi_without_copying() -> None:
    sim = ArenaSim("pve", 3)
    before = pa.total_allocated_bytes()
    feats = sim.features(0)
    units = sim.units(0)
    # 缓冲区由 Rust 分配并通过 C Data Interface 移交：pyarrow 内存池没有新增分配
    assert pa.total_allocated_bytes() == before
    assert isinstance(feats, pa.Int64Array) and len(feats) == 24 and feats.null_count == 0
    view = feats.to_numpy(zero_copy_only=True)
    assert view.dtype == np.int64 and not view.flags.writeable
    assert view.ctypes.data == feats.buffers()[1].address
    assert view[rynvek.FEATURE_NAMES.index("biomass_milli")] == 500_000
    assert units.schema.names == ["id", "faction", "kind", "x_raw", "y_raw", "hp_milli", "max_hp_milli", "own"]
    assert units.num_rows > 0 and all(units.column("own").to_pylist())


def test_macro_actions_and_raw_commands() -> None:
    sim = ArenaSim("duel", 11)
    accepted, rejected = sim.apply_macro(0, rynvek.MACRO_ACTIONS.index("harvest"))
    assert accepted >= 1 and rejected == 0
    with pytest.raises(ValueError):
        sim.apply_macro(0, 9)
    with pytest.raises(ValueError):
        sim.apply_macro(5, 0)
    with pytest.raises(ValueError):
        sim.run_policy(0, "berserk")
    cmd = {
        "type": "issue_order",
        "units": {"kind": "archetype", "value": "unit.forager"},
        "order": "HoldPosition",
        "target": [0, 0],
    }
    sim.apply_command(0, json.dumps(cmd))
    with pytest.raises(RynvekError) as e:
        sim.apply_command(0, "{not json")
    code, name, _ = e.value.args
    assert (code, name) == (0x5001, "RuleStaticTypeCheckFailed")


def test_invalid_configs_are_rejected() -> None:
    for args in (("chess", 0, None), ("duel", 0, 3), ("pve", 0, 0), ("pve", 0, 3)):
        with pytest.raises(ValueError):
            ArenaSim(*args)


def test_match_runs_to_an_outcome_deterministically() -> None:
    sim = ArenaSim("pve", 5)
    assert sim.outcome() is None and not sim.is_over()
    advanced = sim.step(200)
    assert advanced == 200 and sim.tick == 200


def test_rworld_fixture_loads_the_same_world(repo_root: Path) -> None:
    data = (repo_root / "tests/packages/sample.rworld").read_bytes()
    sim = ArenaSim.from_rworld(data)
    assert sim.mode == "duel" and sim.seed == 42
    assert sim.state_root() == ArenaSim("duel", 42).state_root()
    corrupted = bytearray(data)
    corrupted[-1] ^= 0xFF
    with pytest.raises(RynvekError) as e:
        ArenaSim.from_rworld(bytes(corrupted))
    assert e.value.args[0] in (0x7001, 0x7002)
    with pytest.raises(RynvekError) as e:
        ArenaSim.from_rworld(b"RYVW" + bytes(60))
    assert e.value.args[0] == 0x7001
