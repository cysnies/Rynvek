// crates/rynvek-wgpu/shaders/common/bindings.wgsl
// SPDX-License-Identifier: AGPL-3.0-only
// BindGroup 0–3 公共约定（见 rynvek-wgpu `binding` 模块）：
//   @group(0) Frame     ：本次调度的参数 Uniform（binding 0）与诊断计数器 s_diagnostics（binding 15）
//   @group(1) StateIn   ：只读输入状态
//   @group(2) StateOut  ：读写输出状态
//   @group(3) Auxiliary ：辅助数据（场采样/沉积、CSR 邻居表、统计摘要）
// s_diagnostics[0]：任何内核捕获到的定点越界 / 非法输入计数（INV-01 要求每 Tick 为 0）。

@group(0) @binding(15) var<storage, read_write> s_diagnostics: array<atomic<u32>, 4>;

fn rv_report_diagnostic() {
    atomicAdd(&s_diagnostics[0], 1u);
}

// 以 i32 承载的 Q8.8 原始值必须落在 i16 范围内，否则记一次诊断并饱和
fn rv_checked_q8(v: i32) -> i32 {
    if (v < -32768 || v > 32767) {
        rv_report_diagnostic();
        return clamp(v, -32768, 32767);
    }
    return v;
}
