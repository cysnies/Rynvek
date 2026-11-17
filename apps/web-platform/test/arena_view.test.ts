// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { describe,it,expect } from "vitest";
import type { FactionObservation } from "@rynvek/client";
import { project,unproject,fogAt } from "../src/arena-view.js";
describe("服务器观察的只读显示坐标",()=>{
  it("缩放与纵轴反转后点击仍反投影到同一世界格",()=>{
    for(const zoom of [.35,1,1.8,4]){const v={x:32,y:43,zoom,width:900,height:640};for(const p of [[32,43],[12,19],[80,90]] as const){const s=project(v,p[0],p[1]),back=unproject(v,...s);expect(back[0]).toBeCloseTo(p[0]);expect(back[1]).toBeCloseTo(p[1]);}}
  });
  it("未知、越界与缺失格绝不当作可见",()=>{
    const obs={visibility_width:2,visibility:[1,2,0,1]} as unknown as FactionObservation;
    expect(fogAt(obs,1,1)).toBe(1);expect(fogAt(obs,5,1)).toBe(2);expect(fogAt(obs,1,5)).toBe(0);expect(fogAt(obs,-1,1)).toBe(0);expect(fogAt(obs,8,1)).toBe(0);expect(fogAt(obs,1,20)).toBe(0);
  });
});
