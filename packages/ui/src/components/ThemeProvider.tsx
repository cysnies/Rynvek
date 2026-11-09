// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { type ReactNode, createContext, useContext, useMemo, useState } from "react";
import { type ThemeLayer, resolveTheme, themeStylesheet, toStyleObject } from "../theme.js";
import type { ThemeName, TokenTable } from "../tokens.js";
import { COMPONENT_STYLESHEET } from "./styles.js";

export interface ThemeContextValue {
  readonly theme: ThemeName;
  readonly tokens: TokenTable;
  setTheme(theme: ThemeName): void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

export interface ThemeProviderProps {
  /** 初始主题（非受控）；与 `theme` 二选一 */
  readonly defaultTheme?: ThemeName;
  /** 受控主题 */
  readonly theme?: ThemeName;
  readonly onThemeChange?: (theme: ThemeName) => void;
  /** 世界 / 阵营 / 可视化 / 无障碍覆盖层 */
  readonly layers?: readonly ThemeLayer[];
  readonly children?: ReactNode;
}

/**
 * 主题根节点：输出 `data-rv-theme` 属性切换基础主题，把覆盖层解析结果写成内联 CSS 变量，
 * 并注入一次三主题样式表与组件样式表。
 */
export function ThemeProvider({ defaultTheme = "neutral-dark", theme, onThemeChange, layers, children }: ThemeProviderProps) {
  const [inner, setInner] = useState<ThemeName>(defaultTheme);
  const active = theme ?? inner;
  const tokens = useMemo(() => resolveTheme({ base: active, layers: layers ?? [] }), [active, layers]);
  // 只把覆盖层改动过的令牌写成内联样式，基础值由 [data-rv-theme] 样式表提供
  const style = useMemo(() => {
    if (!layers || layers.length === 0) return undefined;
    const base = resolveTheme({ base: active });
    const all = toStyleObject(tokens);
    return Object.fromEntries(Object.entries(all).filter(([k, v]) => base[k as keyof TokenTable] !== v));
  }, [active, layers, tokens]);
  const value = useMemo<ThemeContextValue>(
    () => ({
      theme: active,
      tokens,
      setTheme(next) {
        if (theme === undefined) setInner(next);
        onThemeChange?.(next);
      },
    }),
    [active, tokens, theme, onThemeChange],
  );
  return (
    <ThemeContext.Provider value={value}>
      <style data-rv-stylesheet="">{`${themeStylesheet()}\n${COMPONENT_STYLESHEET}`}</style>
      <div className="rv-root" data-rv-theme={active} style={style}>
        {children}
      </div>
    </ThemeContext.Provider>
  );
}

/** 读取当前主题；必须位于 `ThemeProvider` 之内 */
export function useTheme(): ThemeContextValue {
  const v = useContext(ThemeContext);
  if (!v) throw new Error("useTheme 必须在 <ThemeProvider> 内使用");
  return v;
}
