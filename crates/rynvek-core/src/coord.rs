// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 统一权威坐标：`Chunk(i32, i32) + Q32_32` 局部偏移（规范 02 §2.1、ADR-004）。
//!
//! 规范形式（Canonical Form）要求局部坐标满足 `0 <= local < 256 × 2^32`（不变量 `INV-05`），
//! 越界的局部偏移通过 [`WorldPosition2D::canonicalize`] 进位/借位到相邻 Chunk。

use crate::constants::CHUNK_SIZE_UNITS;
use crate::error::RynvekErrorCode;
use crate::fixed::Q32_32;

/// 单个 Chunk 边长对应的 Q32.32 原始值（`256 << 32`）
pub const CHUNK_SPAN_RAW: i64 = (CHUNK_SIZE_UNITS as i64) << 32;

/// 二维权威世界坐标（D0 定点表示，跨平台位级一致）
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WorldPosition2D {
    /// Chunk 网格 X 坐标
    pub chunk_x: i32,
    /// Chunk 网格 Y 坐标
    pub chunk_y: i32,
    /// Chunk 内局部 X 偏移（规范形式下位于 `[0, 256)` 世界单位）
    pub local_x: Q32_32,
    /// Chunk 内局部 Y 偏移（规范形式下位于 `[0, 256)` 世界单位）
    pub local_y: Q32_32,
}

impl WorldPosition2D {
    /// 构造坐标（不做规范化）
    #[inline]
    pub const fn new(chunk_x: i32, chunk_y: i32, local_x: Q32_32, local_y: Q32_32) -> Self {
        Self {
            chunk_x,
            chunk_y,
            local_x,
            local_y,
        }
    }

    /// 是否满足规范形式（不变量 `INV-05 CoordinateCanonicalBounds`）
    #[inline]
    pub const fn is_canonical(&self) -> bool {
        is_canonical_local(self.local_x) && is_canonical_local(self.local_y)
    }

    /// 将越界的局部偏移进位/借位到相邻 Chunk，得到唯一的规范形式。
    ///
    /// 采用欧几里得除法（向负无穷取整），因此负向越界同样能正确借位。
    /// 若 Chunk 坐标超出 `i32` 范围，返回 [`RynvekErrorCode::CoordinateOutOfCanonicalRange`]。
    pub const fn canonicalize(self) -> Result<Self, RynvekErrorCode> {
        let (chunk_x, local_x) = match carry_axis(self.chunk_x, self.local_x) {
            Some(axis) => axis,
            None => return Err(RynvekErrorCode::CoordinateOutOfCanonicalRange),
        };
        let (chunk_y, local_y) = match carry_axis(self.chunk_y, self.local_y) {
            Some(axis) => axis,
            None => return Err(RynvekErrorCode::CoordinateOutOfCanonicalRange),
        };
        Ok(Self {
            chunk_x,
            chunk_y,
            local_x,
            local_y,
        })
    }

    /// 按位移量平移后返回规范化坐标
    pub const fn translated(self, dx: Q32_32, dy: Q32_32) -> Result<Self, RynvekErrorCode> {
        let local_x = match self.local_x.0.checked_add(dx.0) {
            Some(v) => v,
            None => return Err(RynvekErrorCode::CoordinateOutOfCanonicalRange),
        };
        let local_y = match self.local_y.0.checked_add(dy.0) {
            Some(v) => v,
            None => return Err(RynvekErrorCode::CoordinateOutOfCanonicalRange),
        };
        Self::new(self.chunk_x, self.chunk_y, Q32_32(local_x), Q32_32(local_y)).canonicalize()
    }
}

/// 单轴局部偏移是否位于 `[0, CHUNK_SPAN_RAW)`
#[inline]
const fn is_canonical_local(local: Q32_32) -> bool {
    local.0 >= 0 && local.0 < CHUNK_SPAN_RAW
}

/// 单轴进位：返回规范化后的 `(chunk, local)`；Chunk 溢出 `i32` 时返回 `None`
const fn carry_axis(chunk: i32, local: Q32_32) -> Option<(i32, Q32_32)> {
    let carry = local.0.div_euclid(CHUNK_SPAN_RAW);
    let new_local = local.0.rem_euclid(CHUNK_SPAN_RAW);
    let new_chunk = (chunk as i64) + carry;
    if new_chunk < i32::MIN as i64 || new_chunk > i32::MAX as i64 {
        return None;
    }
    Some((new_chunk as i32, Q32_32(new_local)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_vector_gv002_canonicalizes_both_directions() {
        let raw = WorldPosition2D::new(3, -2, Q32_32(1_108_101_562_368), Q32_32(-4_294_967_296));
        let canonical = raw.canonicalize().expect("应可规范化");
        assert_eq!(
            canonical,
            WorldPosition2D::new(4, -3, Q32_32(8_589_934_592), Q32_32(1_095_216_660_480))
        );
        assert!(canonical.is_canonical());
        assert!(!raw.is_canonical());
    }

    #[test]
    fn canonical_positions_are_unchanged() {
        let pos = WorldPosition2D::new(-7, 9, Q32_32::ZERO, Q32_32(CHUNK_SPAN_RAW - 1));
        assert!(pos.is_canonical());
        assert_eq!(pos.canonicalize(), Ok(pos));
    }

    #[test]
    fn exact_chunk_boundary_carries_to_next_chunk() {
        let pos = WorldPosition2D::new(0, 0, Q32_32(CHUNK_SPAN_RAW), Q32_32(-CHUNK_SPAN_RAW));
        assert_eq!(
            pos.canonicalize(),
            Ok(WorldPosition2D::new(1, -1, Q32_32::ZERO, Q32_32::ZERO))
        );
    }

    #[test]
    fn chunk_overflow_is_rejected() {
        let pos = WorldPosition2D::new(i32::MAX, 0, Q32_32(CHUNK_SPAN_RAW), Q32_32::ZERO);
        assert_eq!(pos.canonicalize(), Err(RynvekErrorCode::CoordinateOutOfCanonicalRange));
        let pos = WorldPosition2D::new(0, i32::MIN, Q32_32::ZERO, Q32_32::NEG_ONE);
        assert_eq!(pos.canonicalize(), Err(RynvekErrorCode::CoordinateOutOfCanonicalRange));
    }

    #[test]
    fn translation_crosses_chunks() {
        let start = WorldPosition2D::new(0, 0, Q32_32::from_int(255), Q32_32::from_int(1));
        let moved = start
            .translated(Q32_32::from_int(2), Q32_32::from_int(-2))
            .expect("应可平移");
        assert_eq!(
            moved,
            WorldPosition2D::new(1, -1, Q32_32::from_int(1), Q32_32::from_int(255))
        );
    }

    #[test]
    fn layout_is_24_bytes() {
        assert_eq!(core::mem::size_of::<WorldPosition2D>(), 24);
        assert_eq!(core::mem::align_of::<WorldPosition2D>(), 8);
    }
}
