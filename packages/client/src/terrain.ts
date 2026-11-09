// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * `welcome.map` 地形解码：标准 Base64 → 每字节两格（偶数下标在低半字节）→ `TerrainTileKind` 数组，
 * 并以 BLAKE3 校验与服务端 `ArenaMap::tiles_hash` 一致（ERRATA E-035）。
 */

import { blake3 } from "@noble/hashes/blake3.js";

import type { MapInfo } from "./protocol.js";

/** 地形种类（与 Rust `TerrainTileKind` 的 `repr(u8)` 一致） */
export const TerrainTileKind = {
  OpenSubstrate: 0,
  ConductiveBasalt: 1,
  NutrientMarsh: 2,
  AcidicFissure: 3,
  ImperviousWall: 4,
} as const;
export type TerrainTileKind = (typeof TerrainTileKind)[keyof typeof TerrainTileKind];

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const B64_INDEX = new Map([...B64].map((c, i) => [c, i]));

/** 标准 Base64 解码（不依赖 `atob` / `Buffer`，浏览器与 Node 通用） */
export function base64Decode(text: string): Uint8Array {
  const clean = text.replace(/=+$/, "");
  const out = new Uint8Array(Math.floor((clean.length * 6) / 8));
  let acc = 0;
  let bits = 0;
  let o = 0;
  for (const c of clean) {
    const v = B64_INDEX.get(c);
    if (v === undefined) throw new Error(`非法 Base64 字符 ${JSON.stringify(c)}`);
    acc = (acc << 6) | v;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      out[o++] = (acc >> bits) & 0xff;
    }
  }
  return out;
}

function hex(bytes: Uint8Array): string {
  return [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** 解码后的地形网格 */
export class TerrainGrid {
  constructor(
    readonly width: number,
    readonly height: number,
    readonly tiles: Uint8Array,
  ) {
    if (tiles.length !== width * height) throw new Error("地形尺寸不匹配");
  }

  static decode(map: MapInfo): TerrainGrid {
    const packed = base64Decode(map.terrain_b64);
    const n = map.width * map.height;
    const tiles = new Uint8Array(n);
    for (let i = 0; i < n; i++) {
      const b = packed[i >> 1] ?? 0;
      tiles[i] = i % 2 === 0 ? b & 0x0f : b >> 4;
    }
    const digest = hex(blake3(tiles));
    if (digest !== map.tiles_hash) throw new Error(`地形 BLAKE3 校验失败：${digest} ≠ ${map.tiles_hash}`);
    return new TerrainGrid(map.width, map.height, tiles);
  }

  inBounds(x: number, y: number): boolean {
    return x >= 0 && y >= 0 && x < this.width && y < this.height;
  }

  at(x: number, y: number): TerrainTileKind {
    return this.tiles[y * this.width + x] as TerrainTileKind;
  }

  /** 是否 180° 中心对称：`(x, y)` 与 `(W−1−x, H−1−y)` 同类 */
  isPointSymmetric(): boolean {
    for (let y = 0; y < this.height; y++) {
      for (let x = 0; x < this.width; x++) {
        if (this.at(x, y) !== this.at(this.width - 1 - x, this.height - 1 - y)) return false;
      }
    }
    return true;
  }
}
