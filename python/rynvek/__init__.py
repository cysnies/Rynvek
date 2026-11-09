# SPDX-FileCopyrightText: 2026 The Rynvek Contributors
# SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

"""Rynvek 科研接口包。

- :class:`ArenaSim`：权威竞技场仿真句柄（定点、确定性、可读状态根）；
- :class:`RynvekArenaEnv`：Gymnasium 单智能体环境；
- :func:`parallel_env`：PettingZoo ``ParallelEnv`` 双人对局环境；
- :mod:`rynvek.lab`：Lab 批量实验与 Parquet 谱系文件。
"""

from __future__ import annotations

from rynvek._native import (
    FEATURE_NAMES,
    MACRO_ACTIONS,
    MAX_TICKS,
    POLICY_INTERVAL_TICKS,
    ArenaSim,
    RynvekError,
    __version__,
)
from rynvek.env import RynvekArenaEnv
from rynvek.multiagent import RynvekParallelEnv, parallel_env

__all__ = [
    "FEATURE_NAMES",
    "MACRO_ACTIONS",
    "MAX_TICKS",
    "POLICY_INTERVAL_TICKS",
    "ArenaSim",
    "RynvekArenaEnv",
    "RynvekError",
    "RynvekParallelEnv",
    "__version__",
    "parallel_env",
]


def _register_gymnasium_envs() -> None:
    """注册 Gymnasium 环境 ID（重复导入时幂等）。"""
    from gymnasium.envs.registration import register, registry

    for env_id, kwargs in (
        ("rynvek/ArenaDuel-v0", {"mode": "duel"}),
        ("rynvek/ArenaPvE-v0", {"mode": "pve"}),
    ):
        if env_id not in registry:
            register(id=env_id, entry_point="rynvek.env:RynvekArenaEnv", kwargs=kwargs)


_register_gymnasium_envs()
