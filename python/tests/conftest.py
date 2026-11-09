# SPDX-FileCopyrightText: 2026 The Rynvek Contributors
# SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

"""pytest 公共夹具。原生扩展由 `cargo xtask pytest`（或 `cargo xtask python-env`）预先 `maturin develop`。"""

from __future__ import annotations

from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]


def pytest_report_header() -> list[str]:
    import pyarrow

    import rynvek

    return [f"rynvek {rynvek.__version__} · pyarrow {pyarrow.__version__}"]


@pytest.fixture(scope="session")
def repo_root() -> Path:
    return REPO_ROOT
