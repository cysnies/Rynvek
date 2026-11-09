// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Tier 2 WASM 插件宿主：Wasmtime 29 Component Model 加载 `rynvek:plugin@1.0.0` 插件
//! （契约见 `protocols/wit/rynvek-plugin.wit`）。
//!
//! 沙箱边界（规范 1.1 / 项目文档 7.2 TrustLevel 3）：
//! - 每次宿主调用（实例化、init、on-tick）重置燃料为 [`PLUGIN_GAS_PER_CALL`] = 500,000 Gas，
//!   燃料耗尽 → `0x5004 SandboxGasBudgetExhausted`；Wasmtime 燃料按 Wasm 指令计数，1 燃料 = 1 Gas；
//! - 单实例只允许 1 块线性内存且不超过 [`PLUGIN_MEMORY_LIMIT_BYTES`] = 64 MiB，
//!   增长被拒 → `0x5005 SandboxMemoryLimitExceeded`；
//! - 只链接 `host` 接口：组件导入任何其它接口（含全部 WASI）在加载期即被拒绝，
//!   未在清单中声明能力就调用对应宿主函数或提交命令 → `0x5006 PluginCapabilityDenied`；
//! - 确定性：NaN 规范化、禁用 relaxed SIMD 与线程，宿主随机数为 Philox4x32-10。
//!
//! 一旦发生上述故障，实例被永久隔离（规范 11.4 `IsolateFaultyExtension`），之后的调用直接返回同一故障。
//! 本模块只处理与领域无关的沙箱机制；把世界状态转换为 `faction-view`、把命令 JSON 应用到世界
//! 的适配层位于 Tier 4 `rynvek-engine::plugins`。

use std::collections::BTreeSet;

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::rng::philox4x32_10;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, ResourceLimiter, Store, Trap};

/// wit-bindgen 生成的绑定（生成代码不受工作区 lint 约束）
#[allow(missing_debug_implementations, clippy::all, unsafe_code)]
mod bindings {
    wasmtime::component::bindgen!({
        path: "../../protocols/wit/rynvek-plugin.wit",
        world: "rynvek-plugin",
    });
}

pub use bindings::rynvek::plugin::host::LogLevel;
pub use bindings::rynvek::plugin::types::{BuildingView, Capability, Command, FactionView, PluginError, UnitView};

/// WIT 包标识
pub const PLUGIN_WIT_PACKAGE: &str = "rynvek:plugin@1.0.0";
/// 插件允许导入的接口
pub const PLUGIN_ALLOWED_IMPORTS: [&str; 2] = ["rynvek:plugin/types@1.0.0", "rynvek:plugin/host@1.0.0"];
/// 插件必须导出的接口
pub const PLUGIN_REQUIRED_EXPORT: &str = "rynvek:plugin/plugin@1.0.0";
/// 每次宿主调用的 Gas 预算
pub const PLUGIN_GAS_PER_CALL: u64 = 500_000;
/// 线性内存上限
pub const PLUGIN_MEMORY_LIMIT_BYTES: usize = 64 << 20;
/// Component 二进制大小上限
pub const PLUGIN_MAX_COMPONENT_BYTES: usize = 16 << 20;
/// 单次 on-tick 最多返回的命令数
pub const PLUGIN_MAX_COMMANDS_PER_TICK: usize = 64;
/// 单条命令 JSON 的字节上限
pub const PLUGIN_MAX_COMMAND_BYTES: usize = 4 << 10;
/// 单次调用最多写入的日志条数
pub const PLUGIN_MAX_LOGS_PER_CALL: usize = 64;
/// 单条日志的字节上限（超出截断）
pub const PLUGIN_MAX_LOG_BYTES: usize = 1 << 10;
/// Wasm 调用栈上限
pub const PLUGIN_MAX_STACK_BYTES: usize = 512 << 10;

/// 故障类别
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginFaultKind {
    /// 燃料耗尽（0x5004）
    GasExhausted,
    /// 线性内存超限（0x5005）
    MemoryLimit,
    /// 未声明能力或导入了宿主不提供的接口（0x5006）
    CapabilityDenied,
    /// 其它 Wasm 陷阱（unreachable、越界访问、栈溢出…）
    Trap,
    /// 输出违反契约（清单非法、命令过多 / 过长 / 非法 JSON）
    InvalidOutput,
    /// 二进制不是合法的 `rynvek:plugin@1.0.0` Component
    InvalidComponent,
    /// 插件自身返回 `plugin-error`（不隔离，仅跳过本次调用）
    GuestError,
}

/// 插件故障
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginFault {
    pub kind: PluginFaultKind,
    pub message: String,
    /// 插件自报的错误码（仅 `GuestError`）
    pub guest_code: Option<u16>,
}

impl PluginFault {
    fn new(kind: PluginFaultKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            guest_code: None,
        }
    }

    /// 对应的统一错误码。
    ///
    /// 注册表没有通用的“插件陷阱”错误码；陷阱、非法输出与非法组件都属于插件越过宿主契约，
    /// 按最佳实践归入 0x5006（同属 `IsolateFaultyExtension` 恢复动作，ERRATA E-048）。
    pub fn code(&self) -> RynvekErrorCode {
        match self.kind {
            PluginFaultKind::GasExhausted => RynvekErrorCode::SandboxGasBudgetExhausted,
            PluginFaultKind::MemoryLimit => RynvekErrorCode::SandboxMemoryLimitExceeded,
            PluginFaultKind::GuestError => self
                .guest_code
                .and_then(RynvekErrorCode::from_code)
                .unwrap_or(RynvekErrorCode::PluginCapabilityDenied),
            _ => RynvekErrorCode::PluginCapabilityDenied,
        }
    }

    /// 是否需要永久隔离该实例
    pub fn isolates(&self) -> bool {
        self.kind != PluginFaultKind::GuestError
    }
}

impl std::fmt::Display for PluginFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {:?}: {}", self.code(), self.kind, self.message)
    }
}

impl std::error::Error for PluginFault {}

/// 资源限制器：记录是否因超限被拒，以便把随后的陷阱归类为 0x5005
#[derive(Debug)]
struct Limiter {
    memory_exceeded: bool,
}

impl ResourceLimiter for Limiter {
    fn memory_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        if desired > PLUGIN_MEMORY_LIMIT_BYTES {
            self.memory_exceeded = true;
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        Ok(desired <= 65_536)
    }

    fn instances(&self) -> usize {
        16
    }

    fn tables(&self) -> usize {
        16
    }

    fn memories(&self) -> usize {
        1
    }
}

/// 每个实例的宿主状态
#[derive(Debug)]
pub struct HostState {
    limiter: Limiter,
    capabilities: BTreeSet<u8>,
    random_key: [u32; 2],
    logs: Vec<(LogLevel, String)>,
    logs_this_call: usize,
    capability_violation: Option<String>,
}

fn cap_bit(c: Capability) -> u8 {
    match c {
        Capability::Observe => 0,
        Capability::Command => 1,
        Capability::Log => 2,
    }
}

impl bindings::rynvek::plugin::types::Host for HostState {}

impl bindings::rynvek::plugin::host::Host for HostState {
    fn log(&mut self, level: LogLevel, message: String) -> Result<(), PluginError> {
        if !self.capabilities.contains(&cap_bit(Capability::Log)) {
            self.capability_violation = Some("未声明 log 能力却调用 host.log".into());
            return Err(PluginError {
                code: RynvekErrorCode::PluginCapabilityDenied.code(),
                message: "log 能力未声明".into(),
            });
        }
        if self.logs_this_call >= PLUGIN_MAX_LOGS_PER_CALL {
            return Ok(()); // 超额日志静默丢弃
        }
        self.logs_this_call += 1;
        let mut msg = message;
        if msg.len() > PLUGIN_MAX_LOG_BYTES {
            let mut cut = PLUGIN_MAX_LOG_BYTES;
            while !msg.is_char_boundary(cut) {
                cut -= 1;
            }
            msg.truncate(cut);
        }
        self.logs.push((level, msg));
        Ok(())
    }

    fn random(&mut self, counter: u64) -> u32 {
        // 计数器第 2 字为 "RYPL" 域分隔常量，避免与模拟内核的随机流重叠
        philox4x32_10(
            [counter as u32, (counter >> 32) as u32, 0x5259_504C, 0],
            self.random_key,
        )[0]
    }
}

/// 共享的插件运行时（编译器与链接器；可跨线程克隆）
#[derive(Clone)]
pub struct PluginRuntime {
    engine: Engine,
    linker: std::sync::Arc<Linker<HostState>>,
}

impl std::fmt::Debug for PluginRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginRuntime").finish_non_exhaustive()
    }
}

/// 已编译并通过导入 / 导出检查的插件
#[derive(Clone)]
pub struct PluginModule {
    component: Component,
    /// 组件导入的接口名
    pub imports: Vec<String>,
    /// 组件导出的接口名
    pub exports: Vec<String>,
}

impl std::fmt::Debug for PluginModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginModule")
            .field("imports", &self.imports)
            .field("exports", &self.exports)
            .finish_non_exhaustive()
    }
}

impl PluginRuntime {
    /// 以确定性配置创建运行时
    pub fn new() -> Result<Self, PluginFault> {
        let mut c = Config::new();
        c.wasm_component_model(true)
            .consume_fuel(true)
            .cranelift_nan_canonicalization(true)
            .wasm_relaxed_simd(false)
            .relaxed_simd_deterministic(true)
            .max_wasm_stack(PLUGIN_MAX_STACK_BYTES);
        let engine = Engine::new(&c).map_err(|e| PluginFault::new(PluginFaultKind::InvalidComponent, e.to_string()))?;
        let mut linker = Linker::new(&engine);
        bindings::RynvekPlugin::add_to_linker(&mut linker, |s: &mut HostState| s)
            .map_err(|e| PluginFault::new(PluginFaultKind::InvalidComponent, e.to_string()))?;
        Ok(Self {
            engine,
            linker: std::sync::Arc::new(linker),
        })
    }

    /// 编译并静态检查组件：只允许导入 `types` / `host`，必须导出 `plugin`
    pub fn compile(&self, bytes: &[u8]) -> Result<PluginModule, PluginFault> {
        let invalid = |m: String| PluginFault::new(PluginFaultKind::InvalidComponent, m);
        if bytes.len() > PLUGIN_MAX_COMPONENT_BYTES {
            return Err(invalid(format!("组件超过 {PLUGIN_MAX_COMPONENT_BYTES} 字节")));
        }
        if !wasmparser::Parser::is_component(bytes) {
            return Err(invalid(
                "不是 WebAssembly Component（核心模块必须先用 wit-component 封装）".into(),
            ));
        }
        let component = Component::new(&self.engine, bytes).map_err(|e| invalid(format!("组件编译失败：{e:#}")))?;
        let ty = component.component_type();
        let imports: Vec<String> = ty.imports(&self.engine).map(|(n, _)| n.to_owned()).collect();
        let exports: Vec<String> = ty.exports(&self.engine).map(|(n, _)| n.to_owned()).collect();
        if let Some(bad) = imports.iter().find(|i| !PLUGIN_ALLOWED_IMPORTS.contains(&i.as_str())) {
            return Err(PluginFault::new(
                PluginFaultKind::CapabilityDenied,
                format!("组件导入了宿主不提供的接口 {bad}（插件零网络、零文件系统，禁止 WASI）"),
            ));
        }
        if !exports.iter().any(|e| e == PLUGIN_REQUIRED_EXPORT) {
            return Err(invalid(format!("组件未导出 {PLUGIN_REQUIRED_EXPORT}")));
        }
        Ok(PluginModule {
            component,
            imports,
            exports,
        })
    }
}

/// 插件清单（经宿主校验）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginManifestInfo {
    pub id: String,
    pub version: String,
    pub capabilities: Vec<Capability>,
}

impl PluginManifestInfo {
    pub fn has(&self, c: Capability) -> bool {
        self.capabilities.contains(&c)
    }
}

/// 反向域名标识（与资源包 id 规则一致）
pub fn is_valid_plugin_id(id: &str) -> bool {
    let seg_ok = |s: &str| {
        !s.is_empty()
            && s.split('-')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
    };
    id.len() <= 128 && id.split('.').count() >= 2 && id.split('.').all(seg_ok)
}

/// SemVer（`MAJOR.MINOR.PATCH[-pre]`，无前导零）
pub fn is_valid_semver(v: &str) -> bool {
    let (core, pre) = v.split_once('-').map_or((v, None), |(c, p)| (c, Some(p)));
    let nums: Vec<&str> = core.split('.').collect();
    let num_ok = |n: &str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && (n == "0" || !n.starts_with('0'));
    nums.len() == 3
        && nums.iter().all(|n| num_ok(n))
        && pre.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
}

/// 调用统计
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PluginStats {
    pub calls: u64,
    pub gas_last_call: u64,
    pub gas_max_call: u64,
    pub commands_emitted: u64,
    pub guest_errors: u64,
}

/// 运行中的插件实例
pub struct PluginInstance {
    store: Store<HostState>,
    bindings: bindings::RynvekPlugin,
    manifest: PluginManifestInfo,
    isolated: Option<PluginFault>,
    stats: PluginStats,
}

impl std::fmt::Debug for PluginInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginInstance")
            .field("manifest", &self.manifest)
            .field("isolated", &self.isolated)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

/// 把 Wasmtime 错误归类为插件故障
fn classify(err: &wasmtime::Error, state: &HostState) -> PluginFault {
    if state.limiter.memory_exceeded {
        return PluginFault::new(
            PluginFaultKind::MemoryLimit,
            format!("线性内存超过 {} MiB 上限", PLUGIN_MEMORY_LIMIT_BYTES >> 20),
        );
    }
    if let Some(v) = &state.capability_violation {
        return PluginFault::new(PluginFaultKind::CapabilityDenied, v.clone());
    }
    match err.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => PluginFault::new(
            PluginFaultKind::GasExhausted,
            format!("单次调用超过 {PLUGIN_GAS_PER_CALL} Gas 预算"),
        ),
        Some(t) => PluginFault::new(PluginFaultKind::Trap, format!("Wasm 陷阱：{t}")),
        None => PluginFault::new(PluginFaultKind::Trap, format!("{err:#}")),
    }
}

impl PluginInstance {
    /// 实例化并调用 `init`；`random_seed` 为宿主随机数密钥（建议由世界种子与插件 id 派生）
    pub fn instantiate(
        runtime: &PluginRuntime,
        module: &PluginModule,
        config_json: &str,
        random_seed: u64,
    ) -> Result<Self, PluginFault> {
        let state = HostState {
            limiter: Limiter { memory_exceeded: false },
            capabilities: BTreeSet::new(),
            random_key: [random_seed as u32, (random_seed >> 32) as u32],
            logs: Vec::new(),
            logs_this_call: 0,
            capability_violation: None,
        };
        let mut store = Store::new(&runtime.engine, state);
        store.limiter(|s| &mut s.limiter);
        store
            .set_fuel(PLUGIN_GAS_PER_CALL)
            .map_err(|e| PluginFault::new(PluginFaultKind::Trap, e.to_string()))?;
        let bindings = bindings::RynvekPlugin::instantiate(&mut store, &module.component, &runtime.linker)
            .map_err(|e| classify(&e, store.data()))?;
        store
            .set_fuel(PLUGIN_GAS_PER_CALL)
            .map_err(|e| PluginFault::new(PluginFaultKind::Trap, e.to_string()))?;
        let res = bindings
            .rynvek_plugin_plugin()
            .call_init(&mut store, config_json)
            .map_err(|e| classify(&e, store.data()))?;
        if let Some(v) = store.data_mut().capability_violation.take() {
            return Err(PluginFault::new(PluginFaultKind::CapabilityDenied, v));
        }
        let m = res.map_err(|e| PluginFault {
            kind: PluginFaultKind::GuestError,
            message: format!("init 失败：{}", e.message),
            guest_code: Some(e.code),
        })?;
        if !is_valid_plugin_id(&m.id) || !is_valid_semver(&m.version) {
            return Err(PluginFault::new(
                PluginFaultKind::InvalidOutput,
                format!("清单 id / version 非法：{} {}", m.id, m.version),
            ));
        }
        let mut caps = m.capabilities.clone();
        caps.sort_by_key(|c| cap_bit(*c));
        caps.dedup();
        store.data_mut().capabilities = caps.iter().map(|c| cap_bit(*c)).collect();
        Ok(Self {
            store,
            bindings,
            manifest: PluginManifestInfo {
                id: m.id,
                version: m.version,
                capabilities: caps,
            },
            isolated: None,
            stats: PluginStats::default(),
        })
    }

    pub fn manifest(&self) -> &PluginManifestInfo {
        &self.manifest
    }

    /// 若已被隔离，返回导致隔离的故障
    pub fn isolated(&self) -> Option<&PluginFault> {
        self.isolated.as_ref()
    }

    pub fn stats(&self) -> PluginStats {
        self.stats
    }

    /// 取出累积的日志
    pub fn drain_logs(&mut self) -> Vec<(LogLevel, String)> {
        std::mem::take(&mut self.store.data_mut().logs)
    }

    /// 调用 `on-tick`。未声明 `observe` 能力时宿主只传 tick 与阵营（单位 / 建筑 / 资源清零）。
    /// 返回待提交的命令 JSON；隔离类故障会使实例永久失效。
    pub fn on_tick(&mut self, mut view: FactionView) -> Result<Vec<String>, PluginFault> {
        if let Some(f) = &self.isolated {
            return Err(f.clone());
        }
        if !self.manifest.has(Capability::Observe) {
            view = FactionView {
                tick: view.tick,
                faction: view.faction,
                biomass_milli: 0,
                flux_milli: 0,
                neurogel_milli: 0,
                supply_used: 0,
                supply_cap: 0,
                score_milli: 0,
                units: Vec::new(),
                buildings: Vec::new(),
            };
        }
        let result = self.call_on_tick(&view);
        if let Err(f) = &result {
            if f.isolates() {
                self.isolated = Some(f.clone());
            } else {
                self.stats.guest_errors += 1;
            }
        }
        result
    }

    fn call_on_tick(&mut self, view: &FactionView) -> Result<Vec<String>, PluginFault> {
        self.store.data_mut().logs_this_call = 0;
        self.store
            .set_fuel(PLUGIN_GAS_PER_CALL)
            .map_err(|e| PluginFault::new(PluginFaultKind::Trap, e.to_string()))?;
        self.stats.calls += 1;
        let res = self.bindings.rynvek_plugin_plugin().call_on_tick(&mut self.store, view);
        let used = PLUGIN_GAS_PER_CALL - self.store.get_fuel().unwrap_or(0);
        self.stats.gas_last_call = used;
        self.stats.gas_max_call = self.stats.gas_max_call.max(used);
        let res = res.map_err(|e| classify(&e, self.store.data()))?;
        if let Some(v) = self.store.data_mut().capability_violation.take() {
            return Err(PluginFault::new(PluginFaultKind::CapabilityDenied, v));
        }
        let commands = res.map_err(|e| PluginFault {
            kind: PluginFaultKind::GuestError,
            message: e.message,
            guest_code: Some(e.code),
        })?;
        if !commands.is_empty() && !self.manifest.has(Capability::Command) {
            return Err(PluginFault::new(
                PluginFaultKind::CapabilityDenied,
                "未声明 command 能力却返回命令",
            ));
        }
        if commands.len() > PLUGIN_MAX_COMMANDS_PER_TICK {
            return Err(PluginFault::new(
                PluginFaultKind::InvalidOutput,
                format!(
                    "单次返回 {} 条命令，超过上限 {PLUGIN_MAX_COMMANDS_PER_TICK}",
                    commands.len()
                ),
            ));
        }
        if let Some(c) = commands
            .iter()
            .find(|c| c.payload_json.len() > PLUGIN_MAX_COMMAND_BYTES)
        {
            return Err(PluginFault::new(
                PluginFaultKind::InvalidOutput,
                format!(
                    "命令 JSON {} 字节，超过上限 {PLUGIN_MAX_COMMAND_BYTES}",
                    c.payload_json.len()
                ),
            ));
        }
        self.stats.commands_emitted += commands.len() as u64;
        Ok(commands.into_iter().map(|c| c.payload_json).collect())
    }
}

/// 把一次调用产生的命令 JSON 标记为契约违规（适配层解析失败时调用），并隔离实例
impl PluginInstance {
    pub fn reject_output(&mut self, message: impl Into<String>) -> PluginFault {
        let f = PluginFault::new(PluginFaultKind::InvalidOutput, message);
        self.isolated = Some(f.clone());
        f
    }
}
