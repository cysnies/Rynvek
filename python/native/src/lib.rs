// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Python `rynvek` 包的原生扩展 `rynvek._native`。
//!
//! - [`ArenaSim`]：持有一个权威 `ArenaWorld`，按 Tick 推进、执行宏动作 / 原始命令、读取状态根；
//! - 观测与实验结果通过 Arrow C Data Interface 交给 `pyarrow`：Rust 侧分配的列缓冲区
//!   由 `pyarrow` 直接接管（只移交指针与释放回调，不复制数据），即规范 1.1 所述“零拷贝导出”；
//! - [`run_experiment`]：执行 Lab 批量实验（释放 GIL、多线程并行），可导出 Arrow 表或 Parquet 字节。
//!
//! 所有数值均为定点整数（`*_milli`、`*_raw` Q32.32），与服务端 Parquet 列结构完全一致。

use arrow::array::{Array, ArrayData, RecordBatch, StructArray};
use arrow::ffi::{FFI_ArrowArray, FFI_ArrowSchema};
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use rynvek_core::error::RynvekErrorCode;
use rynvek_rts::commands::RtsCommand;
use rynvek_rts::lab::arrow_io::{self, LabTable};
use rynvek_rts::lab::policy::{self, MacroAction, PolicyKind};
use rynvek_rts::lab::{self, ExperimentResult, ExperimentSpec, FEATURE_NAMES};
use rynvek_rts::{ArenaWorld, FactionId, MatchConfig, MatchMode};

create_exception!(
    _native,
    RynvekError,
    PyException,
    "携带跨语言错误码的 Rynvek 错误；args = (code: int, name: str, message: str)"
);

/// 构造 `RynvekError(code, name, message)`
fn rynvek_err(code: RynvekErrorCode, message: impl Into<String>) -> PyErr {
    RynvekError::new_err((code.code(), format!("{code:?}"), message.into()))
}

fn arrow_err(e: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(format!("Arrow 编码失败：{e}"))
}

fn parse_policy(name: &str) -> PyResult<PolicyKind> {
    match name {
        "idle" => Ok(PolicyKind::Idle),
        "economy" => Ok(PolicyKind::Economy),
        "ecology" => Ok(PolicyKind::Ecology),
        other => Err(PyValueError::new_err(format!(
            "未知策略 {other:?}（可选 idle / economy / ecology）"
        ))),
    }
}

/// 经 Arrow C Data Interface 把 Rust 数组移交给 `pyarrow.<class>._import_from_c`。
///
/// `FFI_ArrowArray` 只携带缓冲区指针与释放回调；pyarrow 导入时把回调“移走”（置空原结构的
/// `release`），之后缓冲区生命周期由 pyarrow 管理，Rust 侧随后析构空结构不会重复释放。
/// 全程只传递结构体地址，不复制列数据，也无需 `unsafe`。
fn export_to_pyarrow(py: Python<'_>, class: &str, data: &ArrayData) -> PyResult<PyObject> {
    let array = FFI_ArrowArray::new(data);
    let schema = FFI_ArrowSchema::try_from(data.data_type()).map_err(arrow_err)?;
    let array_addr = std::ptr::addr_of!(array) as usize;
    let schema_addr = std::ptr::addr_of!(schema) as usize;
    let obj = py
        .import("pyarrow")?
        .getattr(class)?
        .call_method1("_import_from_c", (array_addr, schema_addr))?;
    Ok(obj.unbind())
}

/// `RecordBatch` → `pyarrow.RecordBatch`（以 StructArray 形式移交）
fn batch_to_pyarrow(py: Python<'_>, batch: RecordBatch) -> PyResult<PyObject> {
    let data = StructArray::from(batch).into_data();
    export_to_pyarrow(py, "RecordBatch", &data)
}

/// 单个竞技场仿真句柄（非线程安全；每个 Python 环境实例持有一个）
#[pyclass(module = "rynvek._native", unsendable)]
#[derive(Debug)]
pub struct ArenaSim {
    world: ArenaWorld,
    config: MatchConfig,
}

impl ArenaSim {
    fn player(&self, faction: u8) -> PyResult<FactionId> {
        let id = FactionId(faction);
        if self.world.factions.iter().any(|f| f.id == id) {
            Ok(id)
        } else {
            Err(PyValueError::new_err(format!("阵营 {faction} 不是本局玩家阵营")))
        }
    }
}

#[pymethods]
impl ArenaSim {
    /// `ArenaSim(mode="duel"|"pve", seed=0, players=None)`
    #[new]
    #[pyo3(signature = (mode = "duel", seed = 0, players = None))]
    fn new(mode: &str, seed: u64, players: Option<u8>) -> PyResult<Self> {
        let config = match (mode, players) {
            ("duel", None | Some(2)) => MatchConfig::duel(seed),
            ("duel", Some(n)) => return Err(PyValueError::new_err(format!("双人局玩家数固定为 2，收到 {n}"))),
            ("pve", None) => MatchConfig::pve(seed, 1),
            ("pve", Some(n @ 1..=2)) => MatchConfig::pve(seed, n),
            ("pve", Some(n)) => return Err(PyValueError::new_err(format!("PvE 玩家数必须为 1 或 2，收到 {n}"))),
            (other, _) => return Err(PyValueError::new_err(format!("未知模式 {other:?}（可选 duel / pve）"))),
        };
        Ok(Self {
            world: ArenaWorld::new(config.clone()),
            config,
        })
    }

    /// 从 `.rworld` 包字节创建（校验容器、内容哈希与 `world.json`）
    #[staticmethod]
    fn from_rworld(data: &[u8]) -> PyResult<Self> {
        let bundle = rynvek_world::WorldBundle::from_bytes(data).map_err(|e| rynvek_err(e.code, e.detail))?;
        Ok(Self {
            world: bundle.instantiate(),
            config: bundle.match_config,
        })
    }

    /// 以同一配置（可换种子）重置到第 0 Tick
    #[pyo3(signature = (seed = None))]
    fn reset(&mut self, seed: Option<u64>) {
        if let Some(s) = seed {
            self.config.map_seed = s;
            self.config.world_seed = s;
        }
        self.world = ArenaWorld::new(self.config.clone());
    }

    #[getter]
    fn tick(&self) -> u64 {
        self.world.tick
    }

    #[getter]
    fn mode(&self) -> &'static str {
        match self.config.mode {
            MatchMode::PvpRanked => "duel",
            MatchMode::PveSurvival => "pve",
        }
    }

    #[getter]
    fn seed(&self) -> u64 {
        self.config.map_seed
    }

    /// 玩家阵营编号（不含野生虫潮）
    #[getter]
    fn players(&self) -> Vec<u32> {
        self.world.factions.iter().map(|f| u32::from(f.id.0)).collect()
    }

    fn is_over(&self) -> bool {
        self.world.is_over()
    }

    /// 当前权威状态根（BLAKE3-256，32 字节）
    fn state_root<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.world.state_hash().0)
    }

    /// 对局结果：`None`（进行中）或 `{"winner": int|None, "reason": str}`
    fn outcome<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyDict>>> {
        let Some(o) = self.world.outcome.as_ref() else {
            return Ok(None);
        };
        let (winner, reason) = lab::outcome_parts(Some(o));
        let d = PyDict::new(py);
        d.set_item("winner", winner)?;
        d.set_item("reason", reason)?;
        Ok(Some(d))
    }

    /// 推进 `n` 个 Tick（对局结束即停止）；返回实际推进数
    #[pyo3(signature = (n = 1))]
    fn step(&mut self, py: Python<'_>, n: u64) -> u64 {
        let world = &mut self.world;
        py.allow_threads(|| {
            let mut done = 0;
            while done < n && !world.is_over() {
                world.step();
                done += 1;
            }
            done
        })
    }

    /// 执行离散宏动作（Gymnasium `Discrete(9)` 动作值）；返回 `(accepted, rejected)` 命令数
    fn apply_macro(&mut self, faction: u8, action: u32) -> PyResult<(u32, u32)> {
        let f = self.player(faction)?;
        let a = MacroAction::from_index(action)
            .ok_or_else(|| PyValueError::new_err(format!("宏动作编号必须在 0..{}", MacroAction::ALL.len())))?;
        let o = policy::apply_macro(&mut self.world, f, a);
        Ok((o.accepted, o.rejected))
    }

    /// 让内置脚本策略在当前 Tick 行动（只在 30 Tick 决策边界生效）
    fn run_policy(&mut self, faction: u8, policy: &str) -> PyResult<(u32, u32)> {
        let f = self.player(faction)?;
        let o = policy::run_policy(&mut self.world, f, parse_policy(policy)?);
        Ok((o.accepted, o.rejected))
    }

    /// 执行一条原始权威命令（JSON，与 WebSocket / 插件命令同一格式）
    fn apply_command(&mut self, faction: u8, command_json: &str) -> PyResult<()> {
        let f = self.player(faction)?;
        let cmd: RtsCommand = serde_json::from_str(command_json).map_err(|e| {
            rynvek_err(
                RynvekErrorCode::RuleStaticTypeCheckFailed,
                format!("命令 JSON 非法：{e}"),
            )
        })?;
        self.world
            .apply_command(f, &cmd)
            .map(|_| ())
            .map_err(|e| rynvek_err(e.code(), e.to_string()))
    }

    /// 阵营特征向量（`pyarrow.Int64Array`，长度 24，列名见 `FEATURE_NAMES`；遵守战争迷雾）
    fn features(&self, py: Python<'_>, faction: u8) -> PyResult<PyObject> {
        let f = self.player(faction)?;
        let v = lab::faction_features(&self.world, f)
            .ok_or_else(|| PyValueError::new_err(format!("阵营 {faction} 无观测")))?;
        export_to_pyarrow(py, "Array", &arrow_io::features_array(v).to_data())
    }

    /// 该阵营可见单位的列式视图（`pyarrow.RecordBatch`）
    fn units(&self, py: Python<'_>, faction: u8) -> PyResult<PyObject> {
        let f = self.player(faction)?;
        let obs = self
            .world
            .observe(f)
            .ok_or_else(|| PyValueError::new_err(format!("阵营 {faction} 无观测")))?;
        batch_to_pyarrow(py, arrow_io::units_batch(&obs).map_err(arrow_err)?)
    }

    fn __repr__(&self) -> String {
        format!(
            "ArenaSim(mode={:?}, seed={}, tick={})",
            self.mode(),
            self.config.map_seed,
            self.world.tick
        )
    }
}

/// Lab 实验结果（四张当前列结构的表）
#[pyclass(module = "rynvek._native", frozen)]
#[derive(Debug)]
pub struct Experiment {
    result: ExperimentResult,
}

impl Experiment {
    fn batch_of(&self, table: &str) -> PyResult<RecordBatch> {
        let t = LabTable::parse(table).ok_or_else(|| {
            PyValueError::new_err(format!("未知表 {table:?}（可选 runs / metrics / lineage / energy）"))
        })?;
        arrow_io::table_batch(&self.result, t).map_err(arrow_err)
    }
}

#[pymethods]
impl Experiment {
    /// 表的 `pyarrow.RecordBatch`（零拷贝移交）
    fn table(&self, py: Python<'_>, name: &str) -> PyResult<PyObject> {
        batch_to_pyarrow(py, self.batch_of(name)?)
    }

    /// 表的 Parquet 字节（ZSTD；与服务端 `GET .../parquet?table=` 逐字节一致）
    fn parquet<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyBytes>> {
        let batch = self.batch_of(name)?;
        let bytes = py.allow_threads(|| arrow_io::to_parquet(&batch)).map_err(|e| {
            rynvek_err(
                RynvekErrorCode::JournalTornWriteTruncated,
                format!("Parquet 写出失败：{e}"),
            )
        })?;
        Ok(PyBytes::new(py, &bytes))
    }

    #[getter]
    fn run_count(&self) -> usize {
        self.result.runs.len()
    }

    fn __repr__(&self) -> String {
        format!(
            "Experiment(runs={}, metrics={}, lineage={})",
            self.result.runs.len(),
            self.result.metrics.len(),
            self.result.lineage.len()
        )
    }
}

/// 执行 Lab 实验（规格 JSON 与 `POST /api/v1/lab/experiments` 的实验部分相同）
#[pyfunction]
fn run_experiment(py: Python<'_>, spec_json: &str) -> PyResult<Experiment> {
    let spec: ExperimentSpec =
        serde_json::from_str(spec_json).map_err(|e| PyValueError::new_err(format!("实验规格 JSON 非法：{e}")))?;
    spec.validate().map_err(PyValueError::new_err)?;
    let result = py.allow_threads(|| lab::run_experiment(&spec, &|_, _| {}));
    Ok(Experiment { result })
}

/// 原生扩展模块 `rynvek._native`
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("RynvekError", m.py().get_type::<RynvekError>())?;
    m.add_class::<ArenaSim>()?;
    m.add_class::<Experiment>()?;
    m.add_function(wrap_pyfunction!(run_experiment, m)?)?;
    m.add("FEATURE_NAMES", FEATURE_NAMES.to_vec())?;
    m.add(
        "MACRO_ACTIONS",
        MacroAction::ALL.iter().map(|a| a.name()).collect::<Vec<_>>(),
    )?;
    m.add("POLICY_INTERVAL_TICKS", policy::POLICY_INTERVAL_TICKS)?;
    m.add("MAX_TICKS", lab::MAX_TICKS)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
