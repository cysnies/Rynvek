// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! # rynvek-rules
//!
//! TypeScript→WGSL 规则编译器与沙箱（规范 08）：
//!
//! ```text
//! [@rynvek/rules Kernel.define(TS)] ── Untyped Rule Graph JSON ──► graph
//!     Pass 1 类型推断与定点提升 / Pass 2 读写集与权限 / Pass 3 循环上界与 Gas  ──► compiler
//! [Typed SSA Rule IR (.rir, "RYIR", 36 Opcodes)] ──► ir / rir
//!     ├─► Backend A：emit_wgsl_compute_shader   ──► wgsl
//!     └─► Backend B：emit_cpu_simd_bytecode     ──► cpu
//! Tier-1：QuickJS-NG 确定性沙箱                 ──► sandbox（feature `quickjs`）
//! ```

#![deny(unsafe_code)]

pub mod compiler;
#[cfg(feature = "quickjs")]
pub mod components;
pub mod cpu;
pub mod error;
pub mod graph;
pub mod ir;
#[cfg(feature = "wasmtime")]
pub mod plugin;
pub mod rir;
#[cfg(all(feature = "quickjs", not(target_arch = "wasm32")))]
pub mod sandbox;
pub mod semantics;
pub mod wgsl;

pub use compiler::{CompileOptions, compile_rule_graph, compile_rule_graph_json};
pub use cpu::{
    AlignedBytecode, DEFAULT_TICK_GAS_BUDGET, RuleTickOutput, RuleWorldState, emit_cpu_simd_bytecode, execute_rule_tick,
};
pub use error::{RuleError, RuleResult};
pub use graph::{ComponentDecl, GraphNode, RULE_GRAPH_FORMAT, UntypedRuleGraph};
pub use ir::{ComponentSlot, Instr, Opcode, RuleModule, ValueType};
pub use rir::{RIR_MAGIC, decode_rir, encode_rir};
#[cfg(all(feature = "quickjs", not(target_arch = "wasm32")))]
pub use sandbox::{QuickJsSandbox, QuickJsSandboxConfig, SandboxTickOutput};
pub use wgsl::emit_wgsl_compute_shader;

pub mod package;
