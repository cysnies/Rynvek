// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * 命令面板的子序列模糊匹配：查询字符须按顺序出现在候选中（大小写不敏感）。
 * 评分规则：连续命中、词首命中（`.`、`_`、空格、`-` 之后或首字符）加分，跨越间隔减分；
 * 同分时保持原始顺序（稳定），结果完全确定。
 */

export interface FuzzyResult<T> {
  readonly item: T;
  readonly score: number;
  /** 命中字符在候选文本中的下标 */
  readonly positions: readonly number[];
}

const BOUNDARY = new Set([".", "_", " ", "-", "/"]);

/** 单个候选打分；不匹配返回 null */
export function fuzzyScore(query: string, text: string): { score: number; positions: number[] } | null {
  const q = query.trim().toLowerCase();
  if (q.length === 0) return { score: 0, positions: [] };
  const t = text.toLowerCase();
  const positions: number[] = [];
  let score = 0;
  let ti = 0;
  let prev = -2;
  for (const ch of q) {
    const found = t.indexOf(ch, ti);
    if (found < 0) return null;
    const atBoundary = found === 0 || BOUNDARY.has(t[found - 1]!);
    score += 1;
    if (found === prev + 1) score += 4;
    if (atBoundary) score += 3;
    score -= Math.min(3, found - ti) * 0.25;
    positions.push(found);
    prev = found;
    ti = found + 1;
  }
  return { score, positions };
}

/** 过滤并排序候选 */
export function fuzzyFilter<T>(query: string, items: readonly T[], text: (item: T) => string): FuzzyResult<T>[] {
  const out: (FuzzyResult<T> & { index: number })[] = [];
  items.forEach((item, index) => {
    const r = fuzzyScore(query, text(item));
    if (r) out.push({ item, score: r.score, positions: r.positions, index });
  });
  out.sort((a, b) => b.score - a.score || a.index - b.index);
  return out.map(({ item, score, positions }) => ({ item, score, positions }));
}
