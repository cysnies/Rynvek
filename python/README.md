<!--
SPDX-FileCopyrightText: 2026 The Rynvek Contributors
SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
-->

# rynvek（Python）

Rynvek 科研接口包：

- `rynvek.ArenaSim`：权威竞技场仿真句柄（定点、确定性，可读状态根）；
- `rynvek.RynvekArenaEnv`：Gymnasium 单智能体环境（`Discrete(9)` 宏动作，24 维定点特征观测）；
- `rynvek.parallel_env()`：PettingZoo `ParallelEnv` 双人对局环境；
- `rynvek.lab.run_experiment()`：Lab 批量多种子实验，返回 `pyarrow` 表或 Parquet 字节。

观测与实验表通过 Arrow C Data Interface 由 Rust 直接移交给 `pyarrow`，不复制列缓冲区。

## 本地开发

```bash
cargo xtask python-env     # 在 python/.venv 中按 uv.lock 安装依赖并 maturin develop
cargo xtask pytest         # 运行 python/tests（会先增量重建原生扩展）
```
