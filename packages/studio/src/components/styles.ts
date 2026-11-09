// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 可停靠工作区样式：只引用 `--rv-*` 令牌。

export const DOCK_STYLESHEET = `
.rv-dock { position: relative; display: flex; flex: 1; min-width: 0; min-height: 0; }
.rv-dock-split { display: flex; flex: 1; min-width: 0; min-height: 0; }
.rv-dock-split[data-direction="row"] { flex-direction: row; }
.rv-dock-split[data-direction="column"] { flex-direction: column; }
.rv-dock-pane { display: flex; min-width: 0; min-height: 0; flex-basis: 0; }
.rv-dock-separator { flex: 0 0 4px; background: var(--rv-color-border); cursor: col-resize; z-index: var(--rv-z-dock); }
.rv-dock-split[data-direction="column"] > .rv-dock-separator { cursor: row-resize; }
.rv-dock-separator:hover, .rv-dock-separator:focus-visible { background: var(--rv-color-accent); outline: none; }
.rv-dock-tabs { position: relative; display: flex; flex: 1; flex-direction: column; min-width: 0; min-height: 0; background: var(--rv-color-surface); border: var(--rv-border-width) solid var(--rv-color-border); }
.rv-dock-tab { display: inline-flex; align-items: center; gap: var(--rv-space-1); }
.rv-dock-close { border: 0; background: transparent; color: var(--rv-color-text-muted); cursor: pointer; font: inherit; }
.rv-dock-close:hover { color: var(--rv-color-text); }
.rv-dock-body { flex: 1; min-height: 0; overflow: auto; }
.rv-dock-drop { position: absolute; z-index: var(--rv-z-overlay); background: color-mix(in srgb, var(--rv-color-accent) 18%, transparent); border: 1px dashed var(--rv-color-accent); }
.rv-dock-drop[data-dock-position="center"] { inset: 25%; }
.rv-dock-drop[data-dock-position="left"] { top: 0; bottom: 0; left: 0; width: 25%; }
.rv-dock-drop[data-dock-position="right"] { top: 0; bottom: 0; right: 0; width: 25%; }
.rv-dock-drop[data-dock-position="top"] { left: 0; right: 0; top: 0; height: 25%; }
.rv-dock-drop[data-dock-position="bottom"] { left: 0; right: 0; bottom: 0; height: 25%; }
.rv-studio { display: flex; flex-direction: column; height: 100%; }
`.trim();
