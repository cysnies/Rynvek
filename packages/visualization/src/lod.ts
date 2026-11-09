// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

// L0–L4 五级多尺度神经可视化（项目文档 6.1），阈值与 `rynvek-render::NeuralLod` 一致（ERRATA E-015）。
// UI 侧（Inspector、图例、状态栏）用同一套判定，保证与 GPU 渲染层所见一致。

import { RenderLayer, layerMask } from "@rynvek/engine";

export type NeuralLod = 0 | 1 | 2 | 3 | 4;

export const NEURAL_LODS: readonly NeuralLod[] = [0, 1, 2, 3, 4] as const;

/** 各级下限阈值（每世界格像素数），与 Rust `NeuralLod::THRESHOLDS` 逐项一致 */
export const NEURAL_LOD_THRESHOLDS: readonly number[] = [0, 2, 8, 32, 128] as const;

/** 32×32 像素网格下的 1:1 缩放 */
export const PX_PER_CELL_NATIVE = 32;

/** 各级展示内容（项目文档 6.1） */
export const NEURAL_LOD_CONTENT: Readonly<Record<NeuralLod, string>> = {
  0: "远景全图：群体活动密度热力云与阵营控制边界",
  1: "中景战区：多细胞单位簇几何外框、朝向与 Kuramoto 同步脉冲环",
  2: "近景集群：个体之间的粘附键与间隙连接电耦合光束",
  3: "微观个体：选中生命体内部的神经元节点与兴奋/抑制突触图",
  4: "突触检视：Inspector 浮层实时绘制单条突触的膜电位波形、权重与资格迹",
};

/** 由缩放倍率选级（与 Rust `NeuralLod::for_zoom` 相同：取不超过 px 的最大阈值） */
export function neuralLodFor(pxPerCell: number): NeuralLod {
  let lod: NeuralLod = 0;
  for (const l of NEURAL_LODS) {
    if (pxPerCell >= (NEURAL_LOD_THRESHOLDS[l] ?? Infinity)) lod = l;
  }
  return lod;
}

/** 该级别默认启用的渲染层位集（与 Rust `NeuralLod::visible_layers` 一致） */
export function visibleLayersFor(lod: NeuralLod): number {
  const base = [RenderLayer.Terrain, RenderLayer.FieldHeatmap, RenderLayer.Structures, RenderLayer.FogOfWar, RenderLayer.Overlay];
  switch (lod) {
    case 0:
      return layerMask(base);
    case 1:
      return layerMask([...base, RenderLayer.Organisms]);
    case 2:
      return layerMask([...base, RenderLayer.Organisms, RenderLayer.Bonds]);
    default:
      return layerMask([...base, RenderLayer.Organisms, RenderLayer.Bonds, RenderLayer.NeuralOverlay]);
  }
}
