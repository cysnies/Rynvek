// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 组件基础样式：shadcn 风格的 1px 细边框、低圆角、高信噪比暗色工具界面。
 * 所有颜色、间距与动效只经由 `--rv-*` 令牌引用，主题切换无需改动组件。
 */

export const COMPONENT_STYLESHEET = `
.rv-root { background: var(--rv-color-bg); color: var(--rv-color-text); font-family: var(--rv-font-sans); font-size: var(--rv-font-size-md); line-height: var(--rv-line-height); }
.rv-mono, .rv-kbd { font-family: var(--rv-font-mono); }
.rv-button { display: inline-flex; align-items: center; gap: var(--rv-space-2); height: 28px; padding: 0 var(--rv-space-3); border: var(--rv-border-width) solid var(--rv-color-border); border-radius: var(--rv-radius-md); background: var(--rv-color-surface); color: var(--rv-color-text); font: inherit; font-size: var(--rv-font-size-sm); cursor: pointer; transition: background var(--rv-duration-fast) var(--rv-ease), border-color var(--rv-duration-fast) var(--rv-ease); }
.rv-button:hover { border-color: var(--rv-color-border-strong); background: var(--rv-color-surface-raised); }
.rv-button:focus-visible, .rv-tab:focus-visible, .rv-input:focus-visible { outline: 2px solid var(--rv-color-focus-ring); outline-offset: 1px; }
.rv-button[disabled] { opacity: 0.5; cursor: not-allowed; }
.rv-button[data-variant="primary"] { background: var(--rv-color-accent); border-color: var(--rv-color-accent); color: var(--rv-color-accent-text); }
.rv-button[data-variant="primary"]:hover { background: var(--rv-color-accent-hover); }
.rv-button[data-variant="ghost"] { background: transparent; border-color: transparent; }
.rv-button[data-variant="danger"] { color: var(--rv-color-danger); }
.rv-button[data-size="sm"] { height: 22px; padding: 0 var(--rv-space-2); font-size: var(--rv-font-size-xs); }
.rv-panel { display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--rv-color-surface); border: var(--rv-border-width) solid var(--rv-color-border); }
.rv-panel-header { display: flex; align-items: center; justify-content: space-between; height: 28px; padding: 0 var(--rv-space-2); border-bottom: var(--rv-border-width) solid var(--rv-color-border); font-size: var(--rv-font-size-xs); text-transform: uppercase; letter-spacing: 0.04em; color: var(--rv-color-text-muted); }
.rv-panel-body { flex: 1; min-height: 0; overflow: auto; }
.rv-tablist { display: flex; gap: 0; border-bottom: var(--rv-border-width) solid var(--rv-color-border); }
.rv-tab { height: 28px; padding: 0 var(--rv-space-3); border: 0; border-right: var(--rv-border-width) solid var(--rv-color-border); background: transparent; color: var(--rv-color-text-muted); font: inherit; font-size: var(--rv-font-size-sm); cursor: pointer; }
.rv-tab[aria-selected="true"] { color: var(--rv-color-text); box-shadow: inset 0 -2px 0 var(--rv-color-accent); }
.rv-toolbar { display: flex; align-items: center; gap: var(--rv-space-1); padding: var(--rv-space-1) var(--rv-space-2); border-bottom: var(--rv-border-width) solid var(--rv-color-border); background: var(--rv-color-surface); }
.rv-statusbar { display: flex; align-items: center; gap: var(--rv-space-4); height: 22px; padding: 0 var(--rv-space-2); border-top: var(--rv-border-width) solid var(--rv-color-border); background: var(--rv-color-surface); color: var(--rv-color-text-muted); font-family: var(--rv-font-mono); font-size: var(--rv-font-size-xs); }
.rv-kbd { display: inline-block; padding: 0 var(--rv-space-1); border: var(--rv-border-width) solid var(--rv-color-border); border-radius: var(--rv-radius-sm); color: var(--rv-color-text-muted); font-size: var(--rv-font-size-xs); }
.rv-palette-backdrop { position: fixed; inset: 0; z-index: var(--rv-z-palette); display: flex; justify-content: center; padding-top: 12vh; background: rgba(0, 0, 0, 0.5); }
.rv-palette { width: min(560px, 92vw); max-height: 60vh; display: flex; flex-direction: column; background: var(--rv-color-overlay); border: var(--rv-border-width) solid var(--rv-color-border-strong); border-radius: var(--rv-radius-lg); }
.rv-input { height: 36px; padding: 0 var(--rv-space-3); border: 0; border-bottom: var(--rv-border-width) solid var(--rv-color-border); background: transparent; color: var(--rv-color-text); font: inherit; }
.rv-palette-list { margin: 0; padding: var(--rv-space-1); list-style: none; overflow: auto; }
.rv-palette-item { display: flex; justify-content: space-between; padding: var(--rv-space-1) var(--rv-space-2); border-radius: var(--rv-radius-sm); cursor: pointer; }
.rv-palette-item[aria-selected="true"] { background: var(--rv-color-surface-raised); box-shadow: inset 2px 0 0 var(--rv-color-accent); }
.rv-palette-item mark { background: transparent; color: var(--rv-color-accent); }
`.trim();
