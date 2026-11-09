// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 世界画布色板（项目文档 6.1 / GAP-14）。
//!
//! - 深石墨碳黑背景 `#090B0A`、1px 细边框 / 网格线 `#303A34`、工业青柠强调色 `#A3D65C`；
//! - 阵营色采用 Okabe-Ito 色盲安全八色表（GAP-14 冻结的无障碍令牌）。

/// sRGB 8 位 RGBA
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba8(pub [u8; 4]);

impl Rgba8 {
    pub const fn hex(rgb: u32) -> Self {
        Self([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 0xFF])
    }

    pub const fn with_alpha(self, a: u8) -> Self {
        Self([self.0[0], self.0[1], self.0[2], a])
    }

    /// 打包为小端 u32（WGSL `unpack4x8unorm` 的输入格式）
    pub const fn packed(self) -> u32 {
        u32::from_le_bytes(self.0)
    }

    /// 线性化前的归一化分量
    pub fn to_f64(self) -> [f64; 4] {
        self.0.map(|c| f64::from(c) / 255.0)
    }
}

/// Okabe-Ito 色盲安全色（黑、橙、天蓝、蓝绿、黄、蓝、朱红、红紫）
pub const OKABE_ITO: [Rgba8; 8] = [
    Rgba8::hex(0x000000),
    Rgba8::hex(0xE69F00),
    Rgba8::hex(0x56B4E9),
    Rgba8::hex(0x009E73),
    Rgba8::hex(0xF0E442),
    Rgba8::hex(0x0072B2),
    Rgba8::hex(0xD55E00),
    Rgba8::hex(0xCC79A7),
];

/// 世界画布色板
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldPalette {
    pub background: Rgba8,
    pub grid_line: Rgba8,
    pub accent: Rgba8,
    /// 热力图低端 / 高端
    pub heat_low: Rgba8,
    pub heat_high: Rgba8,
    pub fog: Rgba8,
}

impl WorldPalette {
    /// Neutral Dark 默认主题
    pub const NEUTRAL_DARK: Self = Self {
        background: Rgba8::hex(0x090B0A),
        grid_line: Rgba8::hex(0x303A34),
        accent: Rgba8::hex(0xA3D65C),
        heat_low: Rgba8::hex(0x0072B2),
        heat_high: Rgba8::hex(0xF0E442),
        fog: Rgba8([0x09, 0x0B, 0x0A, 0xC0]),
    };

    /// 阵营色（0 号保留给中立，跳过 Okabe-Ito 黑色）
    pub const fn faction(index: usize) -> Rgba8 {
        OKABE_ITO[1 + index % 7]
    }
}

impl Default for WorldPalette {
    fn default() -> Self {
        Self::NEUTRAL_DARK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_colors_and_packing() {
        assert_eq!(WorldPalette::NEUTRAL_DARK.background, Rgba8([0x09, 0x0B, 0x0A, 0xFF]));
        assert_eq!(WorldPalette::NEUTRAL_DARK.accent, Rgba8([0xA3, 0xD6, 0x5C, 0xFF]));
        assert_eq!(Rgba8([1, 2, 3, 4]).packed(), 0x0403_0201);
        assert_eq!(WorldPalette::faction(0), Rgba8::hex(0xE69F00));
        assert_eq!(WorldPalette::faction(7), Rgba8::hex(0xE69F00));
    }
}
