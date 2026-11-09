# SPDX-FileCopyrightText: 2026 The Rynvek Contributors
# SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

"""观测、奖励与终局判定的共享逻辑（Gymnasium 与 PettingZoo 环境共用）。"""

from __future__ import annotations

import numpy as np
import numpy.typing as npt
from gymnasium import spaces

from rynvek._native import FEATURE_NAMES, MACRO_ACTIONS, ArenaSim

#: 特征列下标
FEATURE_INDEX: dict[str, int] = {name: i for i, name in enumerate(FEATURE_NAMES)}

#: ``outcome`` 特征取值：0 进行中、1 本方胜、2 本方负、3 平局
OUTCOME_RUNNING, OUTCOME_WIN, OUTCOME_LOSS, OUTCOME_DRAW = 0, 1, 2, 3

#: 终局奖励（得分差之外的一次性奖惩）
TERMINAL_REWARD = {OUTCOME_WIN: 10.0, OUTCOME_LOSS: -10.0, OUTCOME_DRAW: 0.0}

_I64 = np.iinfo(np.int64)


def observation_space() -> spaces.Box:
    """24 维定点整数特征（列名见 ``FEATURE_NAMES``）。"""
    return spaces.Box(low=_I64.min, high=_I64.max, shape=(len(FEATURE_NAMES),), dtype=np.int64)


def action_space() -> spaces.Discrete:
    """离散宏动作（编号顺序冻结，见 ``MACRO_ACTIONS``）。"""
    return spaces.Discrete(len(MACRO_ACTIONS))


def observe(sim: ArenaSim, faction: int) -> npt.NDArray[np.int64]:
    """阵营特征向量。

    原生侧返回 ``pyarrow.Int64Array``（Arrow C Data Interface 移交，未复制）；
    ``to_numpy(zero_copy_only=True)`` 再以只读视图暴露同一块缓冲区。
    """
    return sim.features(faction).to_numpy(zero_copy_only=True)


def reward(prev: npt.NDArray[np.int64] | None, now: npt.NDArray[np.int64]) -> float:
    """得分增量（score_milli / 1000）加终局奖惩。"""
    score = FEATURE_INDEX["score_milli"]
    delta = 0.0 if prev is None else float(now[score] - prev[score]) / 1000.0
    return delta + TERMINAL_REWARD.get(int(now[FEATURE_INDEX["outcome"]]), 0.0)


def outcome_of(obs: npt.NDArray[np.int64]) -> int:
    return int(obs[FEATURE_INDEX["outcome"]])
