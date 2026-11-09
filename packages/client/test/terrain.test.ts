// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { blake3 } from "@noble/hashes/blake3.js";
import { describe, expect, it } from "vitest";

import { TerrainGrid, base64Decode, rawToTile, tileCenterRaw, type MapInfo } from "../src/index.js";

const hex = (b: Uint8Array) => Buffer.from(b).toString("hex");

function mapOf(width: number, height: number, tiles: Uint8Array, hash = hex(blake3(tiles))): MapInfo {
  const packed = new Uint8Array(Math.ceil(tiles.length / 2));
  tiles.forEach((t, i) => (packed[i >> 1]! |= i % 2 === 0 ? t : t << 4));
  return {
    size: "Duel_1v1",
    width,
    height,
    seed: 1,
    tiles_hash: hash,
    terrain_b64: Buffer.from(packed).toString("base64"),
    spawn_tiles: [
      [1, 1],
      [width - 2, height - 2],
    ],
  };
}

describe("地形解码", () => {
  it("Base64 符合 RFC 4648 测试向量", () => {
    const vec: [string, string][] = [["", ""], ["Zg==", "f"], ["Zm8=", "fo"], ["Zm9v", "foo"], ["Zm9vYmFy", "foobar"]];
    for (const [i, o] of vec) expect(new TextDecoder().decode(base64Decode(i))).toBe(o);
    expect(() => base64Decode("@@")).toThrow();
  });

  it("4 bit 解包、BLAKE3 校验与 180° 对称检查", () => {
    const w = 6;
    const h = 4;
    const tiles = new Uint8Array(w * h);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) tiles[y * w + x] = (x * 7 + y * 3) % 5;
    // 制造中心对称：后半复制前半的镜像
    for (let i = 0; i < tiles.length / 2; i++) tiles[tiles.length - 1 - i] = tiles[i]!;
    const g = TerrainGrid.decode(mapOf(w, h, tiles));
    expect([...g.tiles]).toEqual([...tiles]);
    expect(g.isPointSymmetric()).toBe(true);
    expect(g.at(5, 3)).toBe(g.at(0, 0));
    tiles[0] = (tiles[0]! + 1) % 5;
    expect(new TerrainGrid(w, h, tiles).isPointSymmetric()).toBe(false);
    expect(() => TerrainGrid.decode(mapOf(w, h, tiles, "00"))).toThrow(/BLAKE3/);
  });

  it("Tile 中心 Q32.32 原始值与 Rust Vec2::tile_center 一致", () => {
    expect(tileCenterRaw(0, 0)).toEqual([2 ** 31, 2 ** 31]);
    expect(tileCenterRaw(255, 3)).toEqual([255 * 2 ** 32 + 2 ** 31, 3 * 2 ** 32 + 2 ** 31]);
    expect(rawToTile(tileCenterRaw(17, 200))).toEqual([17, 200]);
  });
});
