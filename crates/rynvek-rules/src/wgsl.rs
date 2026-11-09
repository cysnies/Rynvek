// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Backend A：`emit_wgsl_compute_shader(&RuleModule) -> String`（GPU Tier-0，规范 08 §8.1）。
//!
//! 生成的着色器使用规范 01 §1.1 冻结的线性 Workgroup `128×1×1`，资源绑定约定：
//!
//! | Group | Binding | 资源 |
//! | :-- | :-- | :-- |
//! | 0 | 0 | `RuleParams`（uniform：实体数、Tick、世界种子） |
//! | 0 | 1 | `entity_serials: array<vec2<u32>>` |
//! | 1 | k | 槽位 k 的输入组件列（只读，仅当被读取） |
//! | 2 | k | 槽位 k 的输出组件列（读写，仅当被写入；宿主须预先拷入输入值） |
//! | 3 | 0/1 | `field_samples` / `field_deposits`（`[channel * count + entity]`） |
//! | 3 | 2/3 | `neighbor_offsets` / `neighbor_indices`（CSR） |
//!
//! `Bool` / `I32` / `Q8_8` 列以 `i32` 存储，`Q32_32` 列以 `vec2<u32>(lo, hi)` 存储。
//! 每条 IR 指令生成一条与 [`crate::semantics`] 位级一致的语句。

use std::fmt::Write as _;

use crate::cpu::RULE_PHASE_ID;
use crate::ir::{Instr, Opcode, RuleModule, ValueType};
use rynvek_core::rng::RandomKey;

/// 规范 01 §1.1：线性 Workgroup 尺寸
pub const RULE_WORKGROUP_SIZE: u32 = 128;
/// 入口函数名
pub const RULE_ENTRY_POINT: &str = "rule_main";

const PRELUDE: &str = include_str!("wgsl_prelude.wgsl");

fn wgsl_type(ty: ValueType) -> &'static str {
    match ty {
        ValueType::Bool => "bool",
        ValueType::Q32_32 => "vec2<u32>",
        _ => "i32",
    }
}

fn storage_elem(ty: ValueType) -> &'static str {
    if ty == ValueType::Q32_32 { "vec2<u32>" } else { "i32" }
}

fn q32_literal(raw: i64) -> String {
    let bits = raw as u64;
    format!("vec2<u32>({}u, {}u)", bits as u32, (bits >> 32) as u32)
}

fn literal(ty: ValueType, raw: i64) -> String {
    match ty {
        ValueType::Bool => (raw != 0).to_string(),
        ValueType::Q32_32 => q32_literal(raw),
        _ => format!("i32({raw})"),
    }
}

fn zero(ty: ValueType) -> String {
    literal(ty, 0)
}

struct Emitter<'m> {
    module: &'m RuleModule,
    out: String,
    depth: usize,
}

impl Emitter<'_> {
    fn line(&mut self, text: &str) {
        for _ in 0..self.depth {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn v(idx: u16) -> String {
        format!("v{idx}")
    }

    fn ty_of(&self, idx: u16) -> ValueType {
        self.module.instrs[idx as usize].ty
    }

    fn load_component(&self, slot: usize, entity: &str) -> String {
        let ty = self.module.components[slot].ty;
        let raw = format!("comp_in_{slot}[{entity}]");
        match ty {
            ValueType::Bool => format!("({raw} != 0)"),
            ValueType::Q8_8 => format!("clamp({raw}, -32768, 32767)"),
            _ => raw,
        }
    }

    /// 纯表达式指令的 WGSL 表达式
    fn expr(&self, ins: &Instr) -> String {
        use Opcode::*;
        let (a, b, c) = (Self::v(ins.a), Self::v(ins.b), Self::v(ins.c));
        let q32 = ins.ty == ValueType::Q32_32;
        let q8 = ins.ty == ValueType::Q8_8;
        match ins.op {
            ConstBool | ConstI32 | ConstQ8_8 | ConstQ32_32 => literal(ins.ty, ins.imm),
            Add if q32 => format!("q32_add_sat({a}, {b})"),
            Sub if q32 => format!("q32_sub_sat({a}, {b})"),
            Mul if q32 => format!("q32_mul({a}, {b})"),
            Add if q8 => format!("clamp({a} + {b}, -32768, 32767)"),
            Sub if q8 => format!("clamp({a} - {b}, -32768, 32767)"),
            Mul if q8 => format!("clamp(({a} * {b}) >> 8u, -32768, 32767)"),
            Div if q8 => format!("select(clamp(({a} << 8u) / {b}, -32768, 32767), 0, {b} == 0)"),
            Add => format!("({a} + {b})"),
            Sub => format!("({a} - {b})"),
            Mul => format!("({a} * {b})"),
            Div => format!("select({a} / {b}, 0, {b} == 0)"),
            Min if q32 => format!("q32_min({a}, {b})"),
            Max if q32 => format!("q32_max({a}, {b})"),
            Min => format!("min({a}, {b})"),
            Max => format!("max({a}, {b})"),
            Neg if q32 => format!("q32_sub_sat(vec2<u32>(0u, 0u), {a})"),
            Neg if q8 => format!("clamp(-{a}, -32768, 32767)"),
            Neg => format!("(-{a})"),
            Abs if q32 => format!("q32_abs({a})"),
            Abs if q8 => format!("min(abs({a}), 32767)"),
            Abs => format!("abs({a})"),
            Clamp if q32 => format!("q32_min(q32_max({a}, {b}), {c})"),
            Clamp => format!("min(max({a}, {b}), {c})"),
            CmpEq | CmpNe | CmpLt | CmpLe | CmpGt | CmpGe if self.ty_of(ins.a) == ValueType::Q32_32 => match ins.op {
                CmpEq => format!("all({a} == {b})"),
                CmpNe => format!("any({a} != {b})"),
                CmpLt => format!("q32_lt({a}, {b})"),
                CmpLe => format!("!q32_lt({b}, {a})"),
                CmpGt => format!("q32_lt({b}, {a})"),
                _ => format!("!q32_lt({a}, {b})"),
            },
            CmpEq => format!("({a} == {b})"),
            CmpNe => format!("({a} != {b})"),
            CmpLt => format!("({a} < {b})"),
            CmpLe => format!("({a} <= {b})"),
            CmpGt => format!("({a} > {b})"),
            CmpGe => format!("({a} >= {b})"),
            And => format!("({a} && {b})"),
            Or => format!("({a} || {b})"),
            Not => format!("(!{a})"),
            Select => format!("select({c}, {b}, {a})"),
            CastI32 | CastQ8_8 | CastQ32_32 => self.cast(self.ty_of(ins.a), ins.ty, &a),
            LoadComponent => self.load_component(ins.imm as usize, "idx"),
            SampleField => format!(
                "clamp(field_samples[{}u * rule_params.entity_count + idx], -32768, 32767)",
                ins.imm
            ),
            NeighborCount => "i32(neighbor_offsets[idx + 1u] - neighbor_offsets[idx])".to_string(),
            _ => unreachable!("{:?} 不是纯表达式指令", ins.op),
        }
    }

    fn cast(&self, from: ValueType, to: ValueType, a: &str) -> String {
        use ValueType::*;
        match (from, to) {
            (x, y) if x == y => a.to_string(),
            (Bool, I32) => format!("select(0, 1, {a})"),
            (I32, Q8_8) => format!("(clamp({a}, -128, 127) << 8u)"),
            (I32, Q32_32) => format!("vec2<u32>(0u, bitcast<u32>({a}))"),
            (Q8_8, I32) => format!("({a} >> 8u)"),
            (Q8_8, Q32_32) => format!("q32_from_q8({a})"),
            (Q32_32, I32) => format!("bitcast<i32>({a}.y)"),
            (Q32_32, Q8_8) => format!("q32_to_q8({a})"),
            _ => unreachable!("Pass 1 已拒绝的转换 {from:?} → {to:?}"),
        }
    }

    fn emit_bindings(&mut self) {
        let m = self.module;
        self.line("@group(0) @binding(0) var<uniform> rule_params: RuleParams;");
        self.line("@group(0) @binding(1) var<storage, read> entity_serials: array<vec2<u32>>;");
        for (slot, c) in m.components.iter().enumerate() {
            let elem = storage_elem(c.ty);
            if c.read {
                self.line(&format!(
                    "@group(1) @binding({slot}) var<storage, read> comp_in_{slot}: array<{elem}>; // {}",
                    c.name
                ));
            }
            if c.write {
                self.line(&format!(
                    "@group(2) @binding({slot}) var<storage, read_write> comp_out_{slot}: array<{elem}>; // {}",
                    c.name
                ));
            }
        }
        if m.uses(Opcode::SampleField) {
            self.line("@group(3) @binding(0) var<storage, read> field_samples: array<i32>;");
        }
        if m.uses(Opcode::EmitFieldDeposit) {
            self.line("@group(3) @binding(1) var<storage, read_write> field_deposits: array<i32>;");
        }
        if m.uses(Opcode::NeighborCount) || m.uses(Opcode::LoadNeighborComponent) {
            self.line("@group(3) @binding(2) var<storage, read> neighbor_offsets: array<u32>;");
            self.line("@group(3) @binding(3) var<storage, read> neighbor_indices: array<u32>;");
        }
    }

    fn emit_body(&mut self) {
        let instrs = &self.module.instrs;
        for (i, ins) in instrs.iter().enumerate() {
            let name = format!("v{i}");
            let ty = wgsl_type(ins.ty);
            match ins.op {
                Opcode::Phi => {} // 已在 BoundedForBegin 处声明为 var
                Opcode::BoundedForBegin => {
                    for (p, phi) in instrs
                        .iter()
                        .enumerate()
                        .skip(i + 1)
                        .take_while(|(_, x)| x.op == Opcode::Phi)
                    {
                        let line = format!("var v{p}: {} = {};", wgsl_type(phi.ty), Self::v(phi.a));
                        self.line(&line);
                    }
                    self.line(&format!(
                        "for (var {name}: i32 = 0; {name} < {}; {name} = {name} + 1) {{",
                        ins.imm
                    ));
                    self.depth += 1;
                }
                Opcode::BoundedForEnd => {
                    let begin = ins.a as usize;
                    let phis: Vec<usize> = (begin + 1..i).take_while(|&p| instrs[p].op == Opcode::Phi).collect();
                    for &p in &phis {
                        self.line(&format!("let phi_next_{p} = {};", Self::v(instrs[p].b)));
                    }
                    for &p in &phis {
                        self.line(&format!("v{p} = phi_next_{p};"));
                    }
                    self.depth -= 1;
                    self.line("}");
                }
                Opcode::EmitComponentWrite => {
                    let slot = ins.imm;
                    let value = if self.module.components[slot as usize].ty == ValueType::Bool {
                        format!("select(0, 1, {})", Self::v(ins.a))
                    } else {
                        Self::v(ins.a)
                    };
                    self.line(&format!("comp_out_{slot}[idx] = {value};"));
                }
                Opcode::EmitFieldDeposit => {
                    self.line(&format!(
                        "let deposit_at_{i} = {}u * rule_params.entity_count + idx;",
                        ins.imm
                    ));
                    self.line(&format!(
                        "field_deposits[deposit_at_{i}] = clamp(field_deposits[deposit_at_{i}] + {}, -32768, 32767);",
                        Self::v(ins.a)
                    ));
                }
                Opcode::LoadNeighborComponent => {
                    let k = Self::v(ins.a);
                    let slot = ins.imm as usize;
                    self.line(&format!("let nb_start_{i} = neighbor_offsets[idx];"));
                    self.line(&format!(
                        "let nb_count_{i} = i32(neighbor_offsets[idx + 1u] - nb_start_{i});"
                    ));
                    self.line(&format!("var {name}: {ty} = {};", zero(ins.ty)));
                    self.line(&format!("if ({k} >= 0 && {k} < nb_count_{i}) {{"));
                    self.depth += 1;
                    let load = self.load_component(slot, &format!("neighbor_indices[nb_start_{i} + u32({k})]"));
                    self.line(&format!("{name} = {load};"));
                    self.depth -= 1;
                    self.line("}");
                }
                Opcode::RandomQ8_8 => {
                    let stream_and_phase = RandomKey::pack_stream_and_phase(ins.imm as u16, RULE_PHASE_ID);
                    self.line(&format!(
                        "let rng_{i} = philox4x32_10(vec4<u32>(rng_counter, {stream_and_phase}u, entity_serials[idx].x, entity_serials[idx].y), \
                         vec2<u32>(rule_params.seed_lo ^ rule_params.tick_lo, rule_params.seed_hi ^ rule_params.tick_hi));"
                    ));
                    self.line("rng_counter = rng_counter + 1u;");
                    self.line(&format!("let {name}: i32 = i32(rng_{i}.x >> 24u);"));
                }
                _ => {
                    let expr = self.expr(ins);
                    self.line(&format!("let {name}: {ty} = {expr};"));
                }
            }
        }
    }
}

/// 将已验证的规则模块转译为 WGSL 计算着色器
pub fn emit_wgsl_compute_shader(module: &RuleModule) -> String {
    let mut e = Emitter {
        module,
        out: String::new(),
        depth: 0,
    };
    e.out.push_str(PRELUDE);
    let _ = writeln!(
        e.out,
        "\n// 规则 `{}`：由 rynvek-rules 从 Typed SSA Rule IR 生成",
        module.name
    );
    e.out.push('\n');
    e.emit_bindings();
    e.out.push('\n');
    e.line(&format!("@compute @workgroup_size({RULE_WORKGROUP_SIZE}, 1, 1)"));
    e.line(&format!(
        "fn {RULE_ENTRY_POINT}(@builtin(global_invocation_id) gid: vec3<u32>) {{"
    ));
    e.depth += 1;
    e.line("let idx = gid.x;");
    e.line("if (idx >= rule_params.entity_count) {");
    e.line("    return;");
    e.line("}");
    e.line("var rng_counter: u32 = 0u;");
    e.emit_body();
    e.depth -= 1;
    e.line("}");
    e.out
}

#[cfg(test)]
mod q32_mirror_tests {
    //! 将前导中 `vec2<u32>` 模拟 Q32.32 的辅助函数逐行移植为 Rust（同样只用 u32 运算），
    //! 与 CPU 后端的 `semantics` 精确语义逐位比对，保证两后端结果一致（GPU 不可用时的等价性证据）。

    use crate::ir::{Opcode, ValueType};
    use crate::semantics;

    type V2 = (u32, u32); // (.x = lo, .y = hi)

    const Q32_MAX: V2 = (0xFFFF_FFFF, 0x7FFF_FFFF);
    const Q32_MIN: V2 = (0, 0x8000_0000);

    fn split(v: i64) -> V2 {
        (v as u64 as u32, ((v as u64) >> 32) as u32)
    }

    fn join(v: V2) -> i64 {
        (((v.1 as u64) << 32) | v.0 as u64) as i64
    }

    fn mul_hi_lo_u32(a: u32, b: u32) -> V2 {
        let (a_lo, a_hi, b_lo, b_hi) = (a & 0xFFFF, a >> 16, b & 0xFFFF, b >> 16);
        let (p0, p1, p2, p3) = (a_lo * b_lo, a_lo * b_hi, a_hi * b_lo, a_hi * b_hi);
        let mid = (p0 >> 16) + (p1 & 0xFFFF) + (p2 & 0xFFFF);
        let lo = (p0 & 0xFFFF) | ((mid & 0xFFFF) << 16);
        let hi = p3.wrapping_add(p1 >> 16).wrapping_add(p2 >> 16).wrapping_add(mid >> 16);
        (lo, hi)
    }

    fn is_neg(a: V2) -> bool {
        (a.1 >> 31) == 1
    }

    fn add_sat(a: V2, b: V2) -> V2 {
        let lo = a.0.wrapping_add(b.0);
        let carry = (lo < a.0) as u32;
        let hi = a.1.wrapping_add(b.1).wrapping_add(carry);
        if (((a.1 ^ hi) & (b.1 ^ hi)) >> 31) == 1 {
            return if is_neg(a) { Q32_MIN } else { Q32_MAX };
        }
        (lo, hi)
    }

    fn sub_sat(a: V2, b: V2) -> V2 {
        let lo = a.0.wrapping_sub(b.0);
        let borrow = (a.0 < b.0) as u32;
        let hi = a.1.wrapping_sub(b.1).wrapping_sub(borrow);
        if (((a.1 ^ b.1) & (a.1 ^ hi)) >> 31) == 1 {
            return if is_neg(a) { Q32_MIN } else { Q32_MAX };
        }
        (lo, hi)
    }

    fn lt(a: V2, b: V2) -> bool {
        let (ah, bh) = (a.1 as i32, b.1 as i32);
        ah < bh || (ah == bh && a.0 < b.0)
    }

    fn abs(a: V2) -> V2 {
        if is_neg(a) { sub_sat((0, 0), a) } else { a }
    }

    fn add_with_carry(x: u32, y: u32) -> V2 {
        let s = x.wrapping_add(y);
        (s, (s < x) as u32)
    }

    fn mul(a: V2, b: V2) -> V2 {
        let ll = mul_hi_lo_u32(a.0, b.0);
        let lh = mul_hi_lo_u32(a.0, b.1);
        let hl = mul_hi_lo_u32(a.1, b.0);
        let hh = mul_hi_lo_u32(a.1, b.1);
        let s0 = add_with_carry(ll.1, lh.0);
        let s1 = add_with_carry(s0.0, hl.0);
        let mut hi =
            lh.1.wrapping_add(hl.1)
                .wrapping_add(hh.0)
                .wrapping_add(s0.1)
                .wrapping_add(s1.1);
        if is_neg(a) {
            hi = hi.wrapping_sub(b.0);
        }
        if is_neg(b) {
            hi = hi.wrapping_sub(a.0);
        }
        (s1.0, hi)
    }

    fn from_q8(v: i32) -> V2 {
        ((v as u32) << 24, (v >> 8) as u32)
    }

    fn to_q8(a: V2) -> i32 {
        let hi = a.1 as i32;
        if hi >= 128 {
            return 32767;
        }
        if hi < -128 {
            return -32768;
        }
        (hi << 8) | (a.0 >> 24) as i32
    }

    /// 确定性的 SplitMix64 测试输入流（含边界值）
    fn samples() -> Vec<i64> {
        let mut out = vec![
            0,
            1,
            -1,
            i64::MAX,
            i64::MIN,
            1 << 32,
            -(1 << 32),
            (1 << 31) - 1,
            1 << 40,
            -(1 << 40) + 7,
        ];
        let mut s = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..400 {
            s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = s;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            // 一半样本缩小到常见量级，覆盖不溢出路径
            out.push(if z & 1 == 0 { z as i64 } else { (z as i64) >> 20 });
        }
        out
    }

    #[test]
    fn q32_helpers_match_cpu_semantics_bit_for_bit() {
        let xs = samples();
        for &a in &xs {
            assert_eq!(
                join(abs(split(a))),
                semantics::unary(Opcode::Abs, ValueType::Q32_32, a),
                "abs {a}"
            );
            assert_eq!(
                to_q8(split(a)) as i64,
                semantics::cast(ValueType::Q32_32, ValueType::Q8_8, a),
                "to_q8 {a}"
            );
            for &b in xs.iter().step_by(7) {
                let (va, vb) = (split(a), split(b));
                let q = ValueType::Q32_32;
                assert_eq!(
                    join(add_sat(va, vb)),
                    semantics::binary(Opcode::Add, q, a, b),
                    "add {a} {b}"
                );
                assert_eq!(
                    join(sub_sat(va, vb)),
                    semantics::binary(Opcode::Sub, q, a, b),
                    "sub {a} {b}"
                );
                assert_eq!(
                    join(mul(va, vb)),
                    semantics::binary(Opcode::Mul, q, a, b),
                    "mul {a} {b}"
                );
                assert_eq!(lt(va, vb) as i64, semantics::compare(Opcode::CmpLt, a, b), "lt {a} {b}");
            }
        }
        for v in i16::MIN..=i16::MAX {
            let v = v as i64;
            assert_eq!(
                join(from_q8(v as i32)),
                semantics::cast(ValueType::Q8_8, ValueType::Q32_32, v)
            );
        }
    }

    #[test]
    fn wgsl_mul_hi_lo_matches_u64_product() {
        for &a in &[0u32, 1, 0xFFFF, 0x1_0000, 0xFFFF_FFFF, 0xD251_1F53, 0xCD9E_8D57] {
            for &b in &[0u32, 3, 0xFFFF_FFFF, 0x8000_0000, 0x9E37_79B9] {
                let p = a as u64 * b as u64;
                assert_eq!(mul_hi_lo_u32(a, b), (p as u32, (p >> 32) as u32));
            }
        }
    }
}
