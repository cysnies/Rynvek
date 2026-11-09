# SPDX-FileCopyrightText: 2026 The Rynvek Contributors
# SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

"""Gymnasium 单智能体环境：智能体控制一个玩家阵营，其余玩家阵营由内置脚本策略控制。"""

from __future__ import annotations

from typing import Any

import gymnasium as gym
import numpy as np
import numpy.typing as npt

from rynvek import _features as F
from rynvek._native import MAX_TICKS, POLICY_INTERVAL_TICKS, ArenaSim


class RynvekArenaEnv(gym.Env[npt.NDArray[np.int64], np.int64]):
    """Rynvek 竞技场 Gymnasium 环境。

    - 动作：``Discrete(9)`` 宏动作（``rynvek.MACRO_ACTIONS``）；
    - 观测：24 维 ``int64`` 定点特征（``rynvek.FEATURE_NAMES``，遵守战争迷雾）；
    - 每步推进 ``decision_ticks`` 个权威 Tick（默认 30，即 30 Hz 下 1 秒）；
    - 奖励：本方得分增量（score_milli / 1000），终局胜 +10、负 -10；
    - ``terminated``：对局分出结果；``truncated``：达到 ``max_ticks``。
    """

    metadata = {"render_modes": []}

    def __init__(
        self,
        mode: str = "duel",
        players: int | None = None,
        decision_ticks: int = POLICY_INTERVAL_TICKS,
        max_ticks: int = 18_000,
        opponent_policy: str = "economy",
        render_mode: str | None = None,
    ) -> None:
        super().__init__()
        if render_mode is not None:
            raise ValueError("RynvekArenaEnv 不提供渲染（render_mode 必须为 None）")
        if not 1 <= decision_ticks <= 3_600:
            raise ValueError("decision_ticks 必须在 1..=3600")
        if not 1 <= max_ticks <= MAX_TICKS:
            raise ValueError(f"max_ticks 必须在 1..={MAX_TICKS}")
        if opponent_policy not in ("idle", "economy"):
            raise ValueError("opponent_policy 必须为 idle 或 economy")
        self.mode = mode
        self.players = players
        self.decision_ticks = decision_ticks
        self.max_ticks = max_ticks
        self.opponent_policy = opponent_policy
        self.render_mode = render_mode
        self.observation_space = F.observation_space()
        self.action_space = F.action_space()
        self.sim = ArenaSim(mode, 0, players)
        self.agent_faction = self.sim.players[0]
        self._last: npt.NDArray[np.int64] | None = None

    def reset(
        self, *, seed: int | None = None, options: dict[str, Any] | None = None
    ) -> tuple[npt.NDArray[np.int64], dict[str, Any]]:
        super().reset(seed=seed)
        # 地图种子：显式 seed 直接使用；否则从环境 RNG 派生（保证 reset(seed=s) 序列可复现）
        map_seed = seed if seed is not None else int(self.np_random.integers(0, 2**63 - 1))
        self.sim.reset(map_seed)
        self._last = F.observe(self.sim, self.agent_faction)
        return self._last, self._info(0, 0)

    def step(
        self, action: np.int64 | int
    ) -> tuple[npt.NDArray[np.int64], float, bool, bool, dict[str, Any]]:
        if self._last is None:
            raise RuntimeError("必须先调用 reset()")
        if not self.action_space.contains(np.int64(action)):
            raise ValueError(f"非法动作 {action!r}")
        accepted, rejected = self.sim.apply_macro(self.agent_faction, int(action))
        opponents = [f for f in self.sim.players if f != self.agent_faction]
        for _ in range(self.decision_ticks):
            if self.sim.is_over() or self.sim.tick >= self.max_ticks:
                break
            for f in opponents:
                self.sim.run_policy(f, self.opponent_policy)
            self.sim.step(1)
        obs = F.observe(self.sim, self.agent_faction)
        r = F.reward(self._last, obs)
        self._last = obs
        terminated = F.outcome_of(obs) != F.OUTCOME_RUNNING
        truncated = not terminated and self.sim.tick >= self.max_ticks
        return obs, r, terminated, truncated, self._info(accepted, rejected)

    def _info(self, accepted: int, rejected: int) -> dict[str, Any]:
        return {
            "tick": self.sim.tick,
            "faction": self.agent_faction,
            "state_root": self.sim.state_root().hex(),
            "commands_accepted": accepted,
            "commands_rejected": rejected,
        }

    def render(self) -> None:
        return None

    def close(self) -> None:
        self._last = None
