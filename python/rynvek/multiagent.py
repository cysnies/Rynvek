# SPDX-FileCopyrightText: 2026 The Rynvek Contributors
# SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

"""PettingZoo ``ParallelEnv``：每个玩家阵营一个智能体，同一决策步内同时行动。"""

from __future__ import annotations

from typing import Any

import numpy as np
import numpy.typing as npt
from gymnasium import spaces
from pettingzoo import ParallelEnv

from rynvek import _features as F
from rynvek._native import MAX_TICKS, POLICY_INTERVAL_TICKS, ArenaSim

AgentId = str
Obs = npt.NDArray[np.int64]


class RynvekParallelEnv(ParallelEnv[AgentId, Obs, np.int64]):
    """Rynvek 竞技场多智能体环境（智能体名 ``player_<阵营编号>``）。

    同一步内各智能体的宏动作按阵营编号升序依次结算（权威 Canonical Ordering），
    然后统一推进 ``decision_ticks`` 个 Tick。奖励与终局判定同 :class:`rynvek.RynvekArenaEnv`。
    """

    metadata = {"name": "rynvek_arena_v0", "render_modes": [], "is_parallelizable": True}

    def __init__(
        self,
        mode: str = "duel",
        players: int | None = None,
        decision_ticks: int = POLICY_INTERVAL_TICKS,
        max_ticks: int = 18_000,
        render_mode: str | None = None,
    ) -> None:
        if render_mode is not None:
            raise ValueError("RynvekParallelEnv 不提供渲染（render_mode 必须为 None）")
        if not 1 <= decision_ticks <= 3_600:
            raise ValueError("decision_ticks 必须在 1..=3600")
        if not 1 <= max_ticks <= MAX_TICKS:
            raise ValueError(f"max_ticks 必须在 1..={MAX_TICKS}")
        self.render_mode = render_mode
        self.decision_ticks = decision_ticks
        self.max_ticks = max_ticks
        self.sim = ArenaSim(mode, 0, players)
        self._faction = {f"player_{f}": f for f in self.sim.players}
        self.possible_agents = list(self._faction)
        self.agents: list[AgentId] = []
        self._obs_space = {a: F.observation_space() for a in self.possible_agents}
        self._act_space = {a: F.action_space() for a in self.possible_agents}
        self._last: dict[AgentId, Obs] = {}

    def observation_space(self, agent: AgentId) -> spaces.Box:
        return self._obs_space[agent]

    def action_space(self, agent: AgentId) -> spaces.Discrete:
        return self._act_space[agent]

    def reset(
        self, seed: int | None = None, options: dict[str, Any] | None = None
    ) -> tuple[dict[AgentId, Obs], dict[AgentId, dict[str, Any]]]:
        map_seed = seed if seed is not None else int(np.random.default_rng().integers(0, 2**63 - 1))
        self.sim.reset(map_seed)
        self.agents = list(self.possible_agents)
        self._last = {a: F.observe(self.sim, self._faction[a]) for a in self.agents}
        return dict(self._last), {a: self._info(a) for a in self.agents}

    def step(self, actions: dict[AgentId, Any]) -> tuple[
        dict[AgentId, Obs],
        dict[AgentId, float],
        dict[AgentId, bool],
        dict[AgentId, bool],
        dict[AgentId, dict[str, Any]],
    ]:
        if not self.agents:
            raise RuntimeError("所有智能体均已结束，请先 reset()")
        unknown = set(actions) - set(self.agents)
        if unknown:
            raise ValueError(f"未知或已结束的智能体：{sorted(unknown)}")
        for agent in sorted(self.agents, key=self._faction.__getitem__):
            if agent in actions:
                self.sim.apply_macro(self._faction[agent], int(actions[agent]))
        remaining = min(self.decision_ticks, max(0, self.max_ticks - self.sim.tick))
        self.sim.step(remaining)
        obs = {a: F.observe(self.sim, self._faction[a]) for a in self.agents}
        rewards = {a: F.reward(self._last.get(a), obs[a]) for a in self.agents}
        over = self.sim.is_over()
        terminations = {a: over for a in self.agents}
        truncations = {a: (not over) and self.sim.tick >= self.max_ticks for a in self.agents}
        infos = {a: self._info(a) for a in self.agents}
        self._last = obs
        self.agents = [a for a in self.agents if not (terminations[a] or truncations[a])]
        return obs, rewards, terminations, truncations, infos

    def _info(self, agent: AgentId) -> dict[str, Any]:
        return {"tick": self.sim.tick, "faction": self._faction[agent]}

    def state(self) -> npt.NDArray[np.uint8]:
        """全局状态：当前权威状态根（32 字节）。"""
        return np.frombuffer(self.sim.state_root(), dtype=np.uint8)

    def render(self) -> None:
        return None

    def close(self) -> None:
        self.agents = []


def parallel_env(**kwargs: Any) -> RynvekParallelEnv:
    """PettingZoo 约定的环境构造函数。"""
    return RynvekParallelEnv(**kwargs)
