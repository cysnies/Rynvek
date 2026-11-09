// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// `.rview` 视觉包预览面板：清单、色板色块、着色器审查结论，以及应用到五层主题时被剔除的令牌。

import type { RviewPackage } from "@rynvek/assets";
import { rviewThemeLayers } from "@rynvek/visualization";

export interface RviewPreviewProps {
  readonly pkg: RviewPackage;
}

const REASON_TEXT = {
  unknown_token: "未知令牌",
  application_layer_untrusted: "非官方包不能改写应用层",
  unsafe_value: "值不安全",
} as const;

export function RviewPreview({ pkg }: RviewPreviewProps) {
  const { layers, rejected } = rviewThemeLayers(pkg);
  const m = pkg.manifest;
  return (
    <div className="rv-rview-preview">
      <dl>
        <dt>包</dt>
        <dd className="rv-mono">
          {m.id}@{m.version}
        </dd>
        <dt>名称</dt>
        <dd>{m.name}</dd>
        <dt>许可证</dt>
        <dd>{m.license}</dd>
        <dt>信任等级</dt>
        <dd>TrustLevel {pkg.trustLevel}</dd>
        <dt>BLAKE3</dt>
        <dd className="rv-mono">{pkg.fileBlake3}</dd>
      </dl>
      <section aria-label="色板">
        {[...pkg.palettes].map(([path, palette]) => (
          <ul key={path} aria-label={path}>
            {Object.entries(palette.colors).map(([name, color]) => (
              <li key={name} data-color={color}>
                <span aria-hidden="true" style={{ display: "inline-block", width: 12, height: 12, background: color }} /> {name}{" "}
                <code>{color}</code>
              </li>
            ))}
          </ul>
        ))}
      </section>
      <section aria-label="着色器">
        <ul>
          {[...pkg.shaders].map(([path, s]) => (
            <li key={path}>
              <code>{path}</code> {s.review.entryPoint}：{s.review.instructionCount}
              {s.review.budget === null ? "" : ` / ${s.review.budget}`} 条指令
            </li>
          ))}
        </ul>
      </section>
      <section aria-label="主题层">
        <ul>
          {layers.map((l) => (
            <li key={l.kind}>
              {l.kind}：{Object.keys(l.tokens).length} 个令牌
            </li>
          ))}
        </ul>
        {rejected.length > 0 ? (
          <ul aria-label="被剔除的令牌">
            {rejected.map((r) => (
              <li key={`${r.entry}:${r.token}`}>
                <code>{r.token}</code>：{REASON_TEXT[r.reason]}
              </li>
            ))}
          </ul>
        ) : null}
      </section>
    </div>
  );
}
