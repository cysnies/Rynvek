// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

import type { FactionObservation } from "@rynvek/client";
export interface Viewport { x: number; y: number; zoom: number; width: number; height: number }
export const viewScale = (v: Viewport): number => v.width / 64 * v.zoom;
export function project(v: Viewport, x: number, y: number): [number, number] {
  const s=viewScale(v); return [(x-v.x)*s+v.width/2,(v.y-y)*s+v.height/2];
}
export function unproject(v: Viewport, x: number, y: number): [number, number] {
  const s=viewScale(v); return [v.x+(x-v.width/2)/s,v.y-(y-v.height/2)/s];
}
// 未知格默认黑暗；绝不使用前端随机数猜测迷雾中的世界。
export function fogAt(obs: FactionObservation, x: number, y: number): number {
  if(x<0 || y<0) return 0;
  const cx=Math.floor(x/4),cy=Math.floor(y/4);
  if(cx>=obs.visibility_width) return 0;
  return obs.visibility[cy*obs.visibility_width+cx] ?? 0;
}
