# SPDX-FileCopyrightText: 2026 The Rynvek Contributors
# SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

"""PettingZoo ParallelEnv：官方 API 测试、Canonical Ordering 与可复现性。"""

from __future__ import annotations

import numpy as np
import pytest
from pettingzoo.test import parallel_api_test

import rynvek


def test_passes_pettingzoo_parallel_api_test() -> None:
    parallel_api_test(rynvek.parallel_env(mode="duel", max_ticks=1_200), num_cycles=50)


def test_agents_spaces_and_state() -> None:
    env = rynvek.parallel_env(mode="pve", players=2, max_ticks=120)
    assert env.possible_agents == ["player_0", "player_1"]
    obs, infos = env.reset(seed=9)
    assert set(obs) == set(env.possible_agents) and infos["player_1"]["faction"] == 1
    assert env.observation_space("player_0") is env.observation_space("player_0")
    assert env.state().shape == (32,) and env.state().dtype == np.uint8
    for _ in range(4):
        _, rewards, terms, truncs, _ = env.step({a: 1 for a in env.agents})
    assert env.agents == [] and all(truncs.values()) and not any(terms.values())
    with pytest.raises(RuntimeError):
        env.step({})


def test_joint_actions_are_order_independent_and_reproducible() -> None:
    def roots(order: list[str]) -> list[bytes]:
        env = rynvek.parallel_env(mode="duel", max_ticks=900)
        env.reset(seed=21)
        out = []
        for step in range(10):
            actions = {a: (step + i) % 9 for i, a in enumerate(env.possible_agents)}
            env.step({a: actions[a] for a in order})
            out.append(env.state().tobytes())
        return out

    assert roots(["player_0", "player_1"]) == roots(["player_1", "player_0"])


def test_unknown_agent_is_rejected() -> None:
    env = rynvek.parallel_env()
    env.reset(seed=0)
    with pytest.raises(ValueError):
        env.step({"player_7": 0})
