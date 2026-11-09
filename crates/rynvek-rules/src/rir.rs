// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `.rir` 二进制：已验证的 Typed SSA Rule IR 模块（魔数 `"RYIR"`，规范 08 §8.1）。
//!
//! 规范只冻结了魔数与 36 条指令；具体布局按最佳实践设计（Little-Endian）：
//!
//! ```text
//! 0   magic "RYIR"          4B
//! 4   version u16 (=1)      2B
//! 6   opcode_count u16 (=36) 2B   —— 指令集版本自检
//! 8   instr_count u32
//! 12  component_count u32
//! 16  gas_estimate u64
//! 24  name_len u16 | field_write_mask u8 | reserved u8
//! 28  reserved u32
//! 32  name (UTF-8)
//!     components × [name_len u16 | name | ty u8 | access u8 (bit0=read, bit1=write)]
//!     零填充到 16 字节对齐
//!     instrs × 16B（与 CPU 字节码同一编码）
//!     BLAKE3-256(之前全部字节) 32B
//! ```

use rynvek_core::error::RynvekErrorCode;
use rynvek_core::hash::Blake3Hash;

use crate::compiler::{CompileOptions, compile_rule_graph};
use crate::error::{RuleError, RuleResult};
use crate::ir::{ComponentSlot, Instr, Opcode, RuleModule, ValueType};

pub const RIR_MAGIC: [u8; 4] = *b"RYIR";
pub const RIR_VERSION: u16 = 1;
const HEADER_LEN: usize = 32;
const DIGEST_LEN: usize = 32;

fn corrupt(msg: &str) -> RuleError {
    RuleError::new(RynvekErrorCode::InvalidFileMagicOrVersion, msg)
}

/// 编码为 `.rir` 字节
pub fn encode_rir(module: &RuleModule) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&RIR_MAGIC);
    out.extend_from_slice(&RIR_VERSION.to_le_bytes());
    out.extend_from_slice(&(Opcode::ALL.len() as u16).to_le_bytes());
    out.extend_from_slice(&(module.instrs.len() as u32).to_le_bytes());
    out.extend_from_slice(&(module.components.len() as u32).to_le_bytes());
    out.extend_from_slice(&module.gas_estimate.to_le_bytes());
    out.extend_from_slice(&(module.name.len() as u16).to_le_bytes());
    out.push(module.field_write_mask);
    out.push(0);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(module.name.as_bytes());
    for c in &module.components {
        out.extend_from_slice(&(c.name.len() as u16).to_le_bytes());
        out.extend_from_slice(c.name.as_bytes());
        out.push(c.ty as u8);
        out.push(c.read as u8 | ((c.write as u8) << 1));
    }
    out.resize(out.len().next_multiple_of(Instr::ENCODED_LEN), 0);
    for ins in &module.instrs {
        out.extend_from_slice(&ins.encode());
    }
    let digest = Blake3Hash::of(&out);
    out.extend_from_slice(&digest.0);
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> RuleResult<&'a [u8]> {
        let s = self
            .bytes
            .get(self.pos..self.pos + n)
            .ok_or_else(|| corrupt(".rir 截断"))?;
        self.pos += n;
        Ok(s)
    }

    fn u16(&mut self) -> RuleResult<u16> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }

    fn utf8(&mut self, len: usize) -> RuleResult<String> {
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| corrupt(".rir 名称非 UTF-8"))
    }
}

/// 解码 `.rir` 字节（校验魔数、版本、指令集规模与 BLAKE3 摘要）
pub fn decode_rir(bytes: &[u8]) -> RuleResult<RuleModule> {
    if bytes.len() < HEADER_LEN + DIGEST_LEN || bytes[0..4] != RIR_MAGIC {
        return Err(corrupt("不是 .rir 文件"));
    }
    let (body, digest) = bytes.split_at(bytes.len() - DIGEST_LEN);
    if Blake3Hash::of(body).0[..] != digest[..] {
        return Err(RuleError::new(
            RynvekErrorCode::Blake3ChecksumMismatch,
            ".rir 摘要校验失败",
        ));
    }
    let le_u32 = |o: usize| u32::from_le_bytes(body[o..o + 4].try_into().unwrap_or([0; 4]));
    if u16::from_le_bytes([body[4], body[5]]) != RIR_VERSION
        || u16::from_le_bytes([body[6], body[7]]) as usize != Opcode::ALL.len()
    {
        return Err(corrupt(".rir 版本或指令集不受支持"));
    }
    let instr_count = le_u32(8) as usize;
    let component_count = le_u32(12) as usize;
    let gas_estimate = u64::from_le_bytes(body[16..24].try_into().unwrap_or([0; 8]));
    let name_len = u16::from_le_bytes([body[24], body[25]]) as usize;
    let field_write_mask = body[26];

    let mut r = Reader {
        bytes: body,
        pos: HEADER_LEN,
    };
    let name = r.utf8(name_len)?;
    let mut components = Vec::with_capacity(component_count.min(1024));
    for _ in 0..component_count {
        let len = r.u16()? as usize;
        let cname = r.utf8(len)?;
        let meta = r.take(2)?;
        let ty = ValueType::from_u8(meta[0]).ok_or_else(|| corrupt(".rir 组件类型非法"))?;
        components.push(ComponentSlot {
            name: cname,
            ty,
            read: meta[1] & 1 != 0,
            write: meta[1] & 2 != 0,
        });
    }
    r.pos = r.pos.next_multiple_of(Instr::ENCODED_LEN);
    let mut instrs = Vec::with_capacity(instr_count.min(u16::MAX as usize));
    for _ in 0..instr_count {
        let word: &[u8; Instr::ENCODED_LEN] = r
            .take(Instr::ENCODED_LEN)?
            .try_into()
            .map_err(|_| corrupt(".rir 截断"))?;
        instrs.push(Instr::decode(word).ok_or_else(|| corrupt(".rir 指令非法"))?);
    }
    if r.pos != body.len() {
        return Err(corrupt(".rir 尾部存在多余字节"));
    }
    let module = RuleModule {
        name,
        components,
        field_write_mask,
        instrs,
        gas_estimate,
    };
    // 不信任外部文件：还原为规则图后重新走完整管线，结果必须与文件内容逐项相同
    let recompiled = compile_rule_graph(&module.to_graph(), &CompileOptions::default())?;
    if recompiled != module {
        return Err(corrupt(".rir 内容未通过重新验证"));
    }
    Ok(module)
}
