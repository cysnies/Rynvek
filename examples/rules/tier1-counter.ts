// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
// 编译到全局tick函数，无模块/网络/宿主访问；数值为原始整数。
interface ComponentInput {
  tick: string;
  world_seed: string;
  entities: readonly {id:string;serial:number;values:Readonly<Record<string,number>>}[];
}
function tick(input:ComponentInput) {
  return {writes:input.entities.map(e=>({id:e.id,values:{
    "state.total":e.values["state.total"]!+e.values["param.rate"]!
  }}))};
}
