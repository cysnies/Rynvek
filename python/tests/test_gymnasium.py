# SPDX-FileCopyrightText: 2026 The Rynvek Contributors
# SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

"""Gymnasium 环境：官方 API 检查、注册 ID、可复现性与回合结束语义。"""

from __future__ import annotations

import gymnasium as gym
import numpy as np
import pytest
from gymnasium.utils.env_checker import check_env

import rynvek
from rynvek import RynvekArenaEnv


def test_passes_gymnasium_env_checker() -> None:
    env = RynvekArenaEnv(mode="duel", max_ticks=900)
    check_env(env, skip_render_check=True)
    env.close()


def test_registered_ids() -> None:
    for env_id, mode in (("rynvek/ArenaDuel-v0", "duel"), ("rynvek/ArenaPvE-v0", "pve")):
        env = gym.make(env_id, max_ticks=300)
        assert env.unwrapped.mode == mode
        obs, info = env.reset(seed=1)
        assert obs.shape == (24,) and obs.dtype == np.int64
        assert len(info["state_root"]) == 64
        env.close()


def test_seeded_rollouts_are_reproducible() -> None:
    def rollout(seed: int) -> list[tuple[str, float]]:
        env = RynvekArenaEnv(mode="duel", max_ticks=1_800)
        env.reset(seed=seed)
        env.action_space.seed(seed)
        trace = []
        for _ in range(20):
            _, r, term, trunc, info = env.step(env.action_space.sample())
            trace.append((info["state_root"], r))
            if term or trunc:
                break
        return trace

    assert rollout(3) == rollout(3)
    assert rollout(3) != rollout(4)


def test_episode_truncates_at_max_ticks() -> None:
    env = RynvekArenaEnv(mode="pve", max_ticks=95, decision_ticks=30)
    env.reset(seed=0)
    steps, done = 0, False
    while not done:
        _, _, term, trunc, info = env.step(rynvek.MACRO_ACTIONS.index("harvest"))
        steps += 1
        done = term or trunc
    assert trunc and not term
    assert info["tick"] == 95 and steps == 4


def test_invalid_usage() -> None:
    env = RynvekArenaEnv()
    with pytest.raises(RuntimeError):
        env.step(0)
    env.reset(seed=0)
    with pytest.raises(ValueError):
        env.step(42)
    for kwargs in ({"render_mode": "human"}, {"decision_ticks": 0}, {"max_ticks": 10**9}, {"opponent_policy": "x"}):
        with pytest.raises(ValueError):
            RynvekArenaEnv(**kwargs)
