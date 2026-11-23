// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Tier-1 规则：QuickJS-NG 确定性沙箱宿主（GAP-06，规范 01 §1.1：单 Tick `500,000 Gas`、堆 `64 MiB`）。
//!
//! 通过 `rquickjs` 内嵌 QuickJS-NG（C 源码仅随原生权威宿主编译；浏览器 unknown-unknown 不支持本地 Tier1）。
//!
//! 确定性措施：
//!
//! 1. **每 Tick 新建 Context**：QuickJS-NG 的中断计数器挂在 Context 上，新建即复位，
//!    因此同一脚本 + 同一输入的 Gas 计量逐次完全相同；同时杜绝跨 Tick 的隐藏状态。
//! 2. **最小内建集**：不注册 `Date` / `Performance` / `Promise` / `WeakRef`（时间源、异步与 GC 可观测性均不确定）；
//!    `Math.random` 被替换为抛错函数，确定性随机数改由宿主注入的 `Rynvek.philox4x32_10` 提供。
//! 3. **严格模式 + 纯函数接口**：脚本以严格模式求值（rquickjs `EvalOptions::default()` 即 `strict: true`），脚本须定义全局 `tick(input)`，输入/输出均经 JSON 序列化，宿主不暴露任何 I/O。
//!
//! Gas 计量（见 ERRATA E-011）：QuickJS-NG 每执行 `10,000` 次中断计数递减（函数调用与循环回边）调用一次
//! 中断回调；本宿主以「回调次数 × 10,000」计 Gas，超过预算即抛出不可捕获异常并报
//! `SandboxGasBudgetExhausted (0x5004)`。内存上限由 `JS_SetMemoryLimit` 强制，超限报
//! `SandboxMemoryLimitExceeded (0x5005)`。

use std::cell::Cell;
use std::rc::Rc;

use rquickjs::context::intrinsic::{BigInt, Eval, Json, MapSet, RegExp, RegExpCompiler, TypedArrays};
use rquickjs::{Context, Ctx, Exception, Function, Runtime, Value};
use rynvek_core::error::RynvekErrorCode;
use rynvek_core::rng::philox4x32_10;

use crate::error::{RuleError, RuleResult};

/// Tier-1 单 Tick Gas 预算
pub const TIER1_TICK_GAS_BUDGET: u64 = 500_000;
/// Tier-1 堆上限 64 MiB
pub const TIER1_HEAP_LIMIT_BYTES: usize = 64 * 1024 * 1024;
/// 每次中断回调对应的 Gas（QuickJS-NG `JS_INTERRUPT_COUNTER_INIT`）
pub const GAS_PER_INTERRUPT_POLL: u64 = 10_000;
/// 原生栈上限
pub const TIER1_STACK_LIMIT_BYTES: usize = 512 * 1024;

/// 确定性加固前导：在用户脚本之前执行
const HARDEN_PRELUDE: &str = r#"
"use strict";
Object.defineProperty(Math, "random", {
    value: function () { throw new Error("Math.random 在确定性沙箱中被禁用，请使用 Rynvek.philox4x32_10"); },
    writable: false,
    configurable: false,
});
"#;

/// 只允许确定性的内建对象
type DeterministicIntrinsics = (Eval, Json, MapSet, TypedArrays, BigInt, RegExpCompiler, RegExp);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuickJsSandboxConfig {
    pub gas_budget: u64,
    pub heap_limit_bytes: usize,
    pub stack_limit_bytes: usize,
}

impl Default for QuickJsSandboxConfig {
    fn default() -> Self {
        Self {
            gas_budget: TIER1_TICK_GAS_BUDGET,
            heap_limit_bytes: TIER1_HEAP_LIMIT_BYTES,
            stack_limit_bytes: TIER1_STACK_LIMIT_BYTES,
        }
    }
}

/// 单 Tick 执行结果
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxTickOutput {
    /// `tick(input)` 返回值的 JSON
    pub output_json: String,
    pub gas_used: u64,
}

/// QuickJS-NG Tier-1 规则沙箱
pub struct QuickJsSandbox {
    config: QuickJsSandboxConfig,
    source: String,
    runtime: Runtime,
    polls: Rc<Cell<u64>>,
}

impl std::fmt::Debug for QuickJsSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuickJsSandbox")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

fn host_error(msg: impl Into<String>) -> RuleError {
    RuleError::new(RynvekErrorCode::RuleStaticTypeCheckFailed, msg)
}

impl QuickJsSandbox {
    /// 加载规则脚本；脚本须定义全局函数 `tick(input)`
    pub fn new(source: &str, config: QuickJsSandboxConfig) -> RuleResult<Self> {
        let sandbox = Self::build(source, config)?;
        sandbox.with_fresh_context(|ctx| {
            let tick: Value = ctx.globals().get("tick").map_err(|e| host_error(e.to_string()))?;
            if tick.is_function() { Ok(()) } else { Err(host_error("规则脚本未定义全局函数 tick(input)")) }
        })?;
        Ok(sandbox)
    }
    /// 权威单次调用不先执行一遍顶层预检，整份源只在同一预算Context内执行一次。
    pub fn run_once(source: &str, input_json: &str, config: QuickJsSandboxConfig) -> RuleResult<SandboxTickOutput> {
        Self::build(source, config)?.run_tick(input_json)
    }
    fn build(source: &str, config: QuickJsSandboxConfig) -> RuleResult<Self> {
        let runtime = Runtime::new().map_err(|e| host_error(format!("QuickJS 运行时创建失败：{e}")))?;
        runtime.set_memory_limit(config.heap_limit_bytes);
        runtime.set_max_stack_size(config.stack_limit_bytes);
        let polls = Rc::new(Cell::new(0u64));
        let max_polls = config.gas_budget / GAS_PER_INTERRUPT_POLL;
        let counter = Rc::clone(&polls);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let n = counter.get() + 1;
            counter.set(n);
            n > max_polls
        })));
        let sandbox = Self {
            config,
            source: source.to_string(),
            runtime,
            polls,
        };
        Ok(sandbox)
    }

    pub fn config(&self) -> QuickJsSandboxConfig {
        self.config
    }

    /// 以 JSON 输入执行一次 `tick(input)`
    pub fn run_tick(&self, input_json: &str) -> RuleResult<SandboxTickOutput> {
        let output_json = self.with_fresh_context(|ctx| {
            let tick: Function = ctx.globals().get("tick").map_err(|e| host_error(e.to_string()))?;
            let input = ctx.json_parse(input_json).map_err(|e| self.classify(&ctx, e))?;
            let output: Value = tick.call((input,)).map_err(|e| self.classify(&ctx, e))?;
            let text = ctx.json_stringify(output).map_err(|e| self.classify(&ctx, e))?;
            match text {
                Some(s) => s.to_string().map_err(|e| host_error(e.to_string())),
                None => Ok("null".to_string()),
            }
        })?;
        Ok(SandboxTickOutput {
            output_json,
            gas_used: self.polls.get() * GAS_PER_INTERRUPT_POLL,
        })
    }

    /// 新建确定性 Context，执行加固前导与用户脚本后调用 `f`；结束后丢弃 Context 并回收内存
    fn with_fresh_context<T>(&self, f: impl for<'js> FnOnce(Ctx<'js>) -> RuleResult<T>) -> RuleResult<T> {
        self.polls.set(0);
        let context =
            Context::custom::<DeterministicIntrinsics>(&self.runtime).map_err(|e| self.memory_or(e.to_string()))?;
        let result = context.with(|ctx| {
            ctx.eval::<(), _>(HARDEN_PRELUDE).map_err(|e| self.classify(&ctx, e))?;
            install_host_api(&ctx).map_err(|e| self.classify(&ctx, e))?;
            ctx.eval::<(), _>(self.source.as_str())
                .map_err(|e| self.classify(&ctx, e))?;
            f(ctx.clone())
        });
        drop(context);
        self.runtime.run_gc();
        result
    }

    fn memory_or(&self, msg: String) -> RuleError {
        if msg.contains("out of memory") || msg.contains("Allocation") {
            RuleError::new(RynvekErrorCode::SandboxMemoryLimitExceeded, msg)
        } else {
            host_error(msg)
        }
    }

    /// 把 QuickJS 异常归类为规范错误码
    fn classify(&self, ctx: &Ctx<'_>, err: rquickjs::Error) -> RuleError {
        let message = if err.is_exception() {
            let value = ctx.catch();
            value
                .into_object()
                .and_then(Exception::from_object)
                .and_then(|e| e.message())
                .unwrap_or_else(|| "未知 JS 异常".to_string())
        } else {
            err.to_string()
        };
        if self.polls.get() * GAS_PER_INTERRUPT_POLL > self.config.gas_budget {
            RuleError::new(
                RynvekErrorCode::SandboxGasBudgetExhausted,
                format!("单 Tick Gas 预算 {} 已耗尽", self.config.gas_budget),
            )
        } else {
            self.memory_or(format!("规则脚本异常：{message}"))
        }
    }
}

/// 注入 `Rynvek` 宿主对象（仅确定性纯函数）
fn install_host_api(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let api = rquickjs::Object::new(ctx.clone())?;
    let philox = Function::new(ctx.clone(), |ctr: Vec<u32>, key: Vec<u32>| -> Vec<u32> {
        let c = [0, 1, 2, 3].map(|i| ctr.get(i).copied().unwrap_or(0));
        let k = [0, 1].map(|i| key.get(i).copied().unwrap_or(0));
        philox4x32_10(c, k).to_vec()
    })?;
    api.set("philox4x32_10", philox)?;
    ctx.globals().set("Rynvek", api)?;
    Ok(())
}
