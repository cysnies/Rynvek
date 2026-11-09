# Rynvek

> Rules. Life. Emergence.

Rynvek 是一个以确定性神经生态仿真（Neural Soup）为内核的原生策略游戏与创作/科研平台。

- 开发规范：[`docs/Rynvek_Development_Specification_v1.0.md`](docs/Rynvek_Development_Specification_v1.0.md)
- 项目文档：[`docs/Rynvek_Project_Documentation_v1.0.md`](docs/Rynvek_Project_Documentation_v1.0.md)
- 规范勘误：[`docs/ERRATA.md`](docs/ERRATA.md)

## 工具链

| 工具 | 版本 |
| :--- | :--- |
| Rust | 1.87.0（由 `rust-toolchain.toml` 锁定，edition 2024；为编译 wgpu 30.0.1 自 1.85.0 上调，见 ERRATA E-013） |
| Node.js | 22 LTS（≥ 22.14） |
| pnpm | 10.x |
| TypeScript | 5.8.x |

## 快速开始

```bash
pnpm install
cargo test -p rynvek-core -p rynvek-schema -p rynvek-protocol
pnpm --filter @rynvek/core test
pnpm --filter @rynvek/protocol test
cargo xtask verify-golden-vectors
```

## 许可证

- 核心引擎与应用：`AGPL-3.0-only`
- 公开协议（`protocols/*`、`crates/rynvek-protocol`）与客户端 SDK（`@rynvek/core`、`@rynvek/protocol` 等）：`Apache-2.0 WITH LLVM-exception`
- 文档：`CC-BY-4.0`

全部许可证文本位于 [`LICENSES/`](LICENSES/)，仓库遵循 [REUSE 3.3](https://reuse.software/) 规范。
