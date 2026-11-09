// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { readFileSync } from "node:fs";
import { RynvekError, RynvekErrorCode } from "@rynvek/core";
import { describe, expect, it } from "vitest";
import {
  RVIEW_ENTRY_KINDS,
  RVIEW_ENTRY_PATH_PATTERN,
  RVIEW_HEADER_BYTES,
  RVIEW_LICENSES,
  RVIEW_MANIFEST_SCHEMA,
  RVIEW_PACKAGE_ID_PATTERN,
  TrustLevel,
  blake3Digest,
  loadRview,
  readRview,
  readRviewHeader,
  rviewPackageKey,
  toHex,
  writeRview,
  type DependencyLock,
} from "../src/index.js";
import { jsonEntry, sampleInput, samplePack } from "./fixtures.js";

function codeOf(fn: () => unknown): number | undefined {
  try {
    fn();
  } catch (e) {
    if (e instanceof RynvekError) return e.code;
    throw e;
  }
  return undefined;
}

/** 修改一个字节后返回副本 */
function patched(bytes: Uint8Array, offset: number, value: number): Uint8Array {
  const copy = bytes.slice();
  copy[offset] = value;
  return copy;
}

describe(".rview 容器", () => {
  it("64 字节小端文件头，魔数 RYVW，字段与清单一致", () => {
    const bytes = samplePack();
    expect(Array.from(bytes.subarray(0, 4))).toEqual([0x52, 0x59, 0x56, 0x57]);
    const h = readRviewHeader(bytes);
    expect(h).toMatchObject({ formatVersion: 1, flags: 0, entryCount: 5, totalBytes: bytes.length });
    const view = new DataView(bytes.buffer);
    expect(view.getUint16(8, true)).toBe(RVIEW_HEADER_BYTES);
    expect(h.packageKey).toEqual(rviewPackageKey("rynvek.visual.industrial-default"));
    expect((RVIEW_HEADER_BYTES + 32 + Math.ceil(h.manifestBytes / 8) * 8) % 8).toBe(0);
  });

  it("往返读写，条目按路径排序、8 字节对齐，输出逐字节确定", () => {
    const bytes = samplePack();
    const archive = readRview(bytes);
    expect(archive.manifest.schema).toBe(RVIEW_MANIFEST_SCHEMA);
    expect(archive.manifest.entries.map((e) => e.path)).toEqual([
      "palette/factions.json",
      "shaders/glow.wgsl",
      "sprites/atlas.json",
      "sprites/atlas.png",
      "theme/biome.json",
    ]);
    for (const e of archive.manifest.entries) {
      expect(e.offset % 8).toBe(0);
      expect(toHex(blake3Digest(archive.files.get(e.path) ?? new Uint8Array()))).toBe(e.blake3_256);
    }
    const reversed = sampleInput();
    expect(writeRview({ ...reversed, entries: [...reversed.entries].reverse() })).toEqual(bytes);
  });

  it("魔数、版本、标志位、保留字段、截断与尾随数据 → 0x7001", () => {
    const bytes = samplePack();
    const E = RynvekErrorCode.InvalidFileMagicOrVersion;
    expect(codeOf(() => readRview(patched(bytes, 0, 0x4e)))).toBe(E);
    expect(codeOf(() => readRview(patched(bytes, 4, 2)))).toBe(E);
    expect(codeOf(() => readRview(patched(bytes, 6, 1)))).toBe(E);
    expect(codeOf(() => readRview(patched(bytes, 12, 1)))).toBe(E);
    expect(codeOf(() => readRview(patched(bytes, 8, 32)))).toBe(E);
    expect(codeOf(() => readRview(bytes.subarray(0, bytes.length - 1)))).toBe(E);
    const trailing = new Uint8Array(bytes.length + 8);
    trailing.set(bytes);
    expect(codeOf(() => readRview(trailing))).toBe(E);
    expect(codeOf(() => readRview(new Uint8Array(10)))).toBe(E);
  });

  it("任意内容字节被篡改 → 0x7002", () => {
    const bytes = samplePack();
    const manifestStart = RVIEW_HEADER_BYTES + 32;
    expect(codeOf(() => readRview(patched(bytes, manifestStart + 3, 0x20)))).toBe(RynvekErrorCode.Blake3ChecksumMismatch);
    expect(codeOf(() => readRview(patched(bytes, bytes.length - 2, 0xff)))).toBe(RynvekErrorCode.Blake3ChecksumMismatch);
    expect(codeOf(() => readRview(patched(bytes, RVIEW_HEADER_BYTES, (bytes[RVIEW_HEADER_BYTES] ?? 0) ^ 1)))).toBe(
      RynvekErrorCode.Blake3ChecksumMismatch,
    );
  });

  it("写入时拒绝非法 ID / 版本 / 路径 / 媒体类型 / 许可证 / 重复路径", () => {
    const E = RynvekErrorCode.InvalidFileMagicOrVersion;
    expect(codeOf(() => samplePack({ id: "Rynvek Visual" }))).toBe(E);
    expect(codeOf(() => samplePack({ version: "1.0" }))).toBe(E);
    expect(codeOf(() => samplePack({ license: "MIT" as never }))).toBe(E);
    expect(codeOf(() => samplePack({ entries: [jsonEntry("../x.json", "palette", { colors: {} })] }))).toBe(E);
    expect(codeOf(() => samplePack({ entries: [jsonEntry("a/./x.json", "palette", { colors: {} })] }))).toBe(E);
    expect(codeOf(() => samplePack({ entries: [jsonEntry("shader.json", "shader", {})] }))).toBe(E);
    const dup = jsonEntry("p.json", "palette", { colors: {} });
    expect(codeOf(() => samplePack({ entries: [dup, dup] }))).toBe(E);
  });

  it("JSON Schema 与实现常量一致", () => {
    const schema = JSON.parse(readFileSync(new URL("../../../protocols/schemas/rview.schema.json", import.meta.url), "utf8"));
    expect(schema.properties.schema.const).toBe(RVIEW_MANIFEST_SCHEMA);
    expect(schema.properties.license.enum).toEqual(RVIEW_LICENSES);
    expect(schema.$defs.entry.properties.kind.enum).toEqual(RVIEW_ENTRY_KINDS);
    expect(new RegExp(schema.properties.id.pattern).source).toBe(RVIEW_PACKAGE_ID_PATTERN.source);
    expect(new RegExp(schema.$defs.entry.properties.path.pattern).source).toBe(RVIEW_ENTRY_PATH_PATTERN.source);
    expect(RVIEW_PACKAGE_ID_PATTERN.test("rynvek.visual.industrial-default")).toBe(true);
  });
});

describe("loadRview", () => {
  it("按类型解码色板、主题、着色器与精灵图集", () => {
    const pkg = loadRview(samplePack(), { trustLevel: TrustLevel.UntrustedEphemeral });
    expect(pkg.palettes.get("palette/factions.json")?.colors).toEqual({
      "faction-1": "#E69F00",
      "faction-2": "#56B4E9",
      "heat-low": "#0072B2",
    });
    expect(pkg.themes.get("theme/biome.json")?.tokens["--rv-world-bg"]).toBe("#101412");
    expect(pkg.shaders.get("shaders/glow.wgsl")?.review).toMatchObject({ entryPoint: "fs_main", instructionCount: 6, budget: 512 });
    expect(pkg.sprites.get("sprites/atlas.png")?.format).toBe("png");
    expect(pkg.sprites.get("sprites/atlas.json")).toEqual({
      format: "frames",
      image: "sprites/atlas.png",
      frames: { extractor: { x: 0, y: 0, w: 32, h: 32 } },
    });
  });

  it("内容格式错误 → 0x7001", () => {
    const E = RynvekErrorCode.InvalidFileMagicOrVersion;
    const load = (entries: Parameters<typeof samplePack>[0]) => () =>
      loadRview(samplePack(entries), { trustLevel: TrustLevel.OfficialSigned });
    expect(codeOf(load({ entries: [jsonEntry("p.json", "palette", { colors: { a: "red" } })] }))).toBe(E);
    expect(codeOf(load({ entries: [jsonEntry("t.json", "theme", { tokens: { "--foreign-bg": "#000000" } })] }))).toBe(E);
    expect(codeOf(load({ entries: [jsonEntry("s.json", "sprite_atlas", { image: "missing.png", frames: {} })] }))).toBe(E);
    expect(
      codeOf(load({ entries: [{ path: "a.png", kind: "sprite_atlas", media_type: "image/png", data: new Uint8Array([1, 2, 3]) }] })),
    ).toBe(E);
  });

  it("依赖锁：一致时通过；缺失、类型、版本、摘要冲突 → 0x9003", () => {
    const bytes = samplePack();
    const hash = toHex(blake3Digest(bytes));
    const lock = (pkg: Partial<DependencyLock["packages"][number]>): DependencyLock => ({
      lockfile_version: 1,
      packages: [{ id: "rynvek.visual.industrial-default", version: "1.0.0", kind: "rview", blake3_256: hash, dependencies: [], ...pkg }],
    });
    const opts = (l: DependencyLock) => ({ trustLevel: TrustLevel.OfficialSigned, lock: l });
    expect(loadRview(bytes, opts(lock({}))).fileBlake3).toBe(hash);
    const C = RynvekErrorCode.DependencyLockConflict;
    expect(codeOf(() => loadRview(bytes, opts(lock({ id: "rynvek.visual.other" }))))).toBe(C);
    expect(codeOf(() => loadRview(bytes, opts(lock({ kind: "rpack" }))))).toBe(C);
    expect(codeOf(() => loadRview(bytes, opts(lock({ version: "1.0.1" }))))).toBe(C);
    expect(codeOf(() => loadRview(bytes, opts(lock({ blake3_256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" }))))).toBe(C);
  });
});
