// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 服务器裁剪观察 → 既有 WorldRenderer 场景。这里只持有只读观察，不实例化 ArenaWorld。
//! 地形纹理与幽灵建筑后续接入；可见格背景不是随机地形，未知格不上传实体或拾取 ID。

use crate::EngineError;
use rynvek_core::{error::RynvekErrorCode, fixed::Q32_32};
use rynvek_render::{FrameScene, InstanceData, InstanceShape, RenderLayer, RenderLayerMask, Rgba8, WorldPalette};
use rynvek_rts::{buildings::building_spec, fog::VISIBILITY_CELL_TILES, observation::FactionObservation};

const MAX_VISIBILITY_CELLS: usize = 65_536;

fn dimensions(obs: &FactionObservation) -> (u32, u32) {
    (
        obs.visibility_width * VISIBILITY_CELL_TILES,
        (obs.visibility.len() / obs.visibility_width as usize) as u32 * VISIBILITY_CELL_TILES,
    )
}

/// 在替换上一帧前完整校验；格式错误不允许部分更新 Tick 或场景。
pub fn validate(obs: &FactionObservation) -> Result<(), EngineError> {
    let invalid = || EngineError {
        code: RynvekErrorCode::DimensionMismatch,
        message: "阵营观察的可见性网格或实体坐标非法".into(),
    };
    if obs.visibility_width == 0
        || obs.visibility_width > 256
        || obs.visibility.is_empty()
        || obs.visibility.len() > MAX_VISIBILITY_CELLS
        || obs.visibility.len() % obs.visibility_width as usize != 0
        || obs.visibility.iter().any(|v| *v > 2)
    {
        return Err(invalid());
    }
    let (width, height) = dimensions(obs);
    for unit in &obs.units {
        // 判界与迷雾索引保留 Q32.32 精度，避免舍入到相邻可见格。
        if unit.position.x.0 < 0
            || unit.position.y.0 < 0
            || unit.position.x.0 >= (i64::from(width) << 32)
            || unit.position.y.0 >= (i64::from(height) << 32)
        {
            return Err(invalid());
        }
    }
    for building in &obs.buildings {
        let size = building_spec(building.kind).footprint;
        if building.origin.0.checked_add(size).is_none_or(|x| x > width)
            || building.origin.1.checked_add(size).is_none_or(|y| y > height)
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn position(p: rynvek_rts::math::Vec2) -> [f32; 2] {
    let scale = Q32_32::ONE.0 as f64;
    [(p.x.0 as f64 / scale) as f32, (p.y.0 as f64 / scale) as f32]
}

fn visible(obs: &FactionObservation, x: u32, y: u32) -> bool {
    let index =
        (y / VISIBILITY_CELL_TILES) as usize * obs.visibility_width as usize + (x / VISIBILITY_CELL_TILES) as usize;
    obs.visibility.get(index) == Some(&1)
}

/// 已裁剪的实体再作保守过滤，且不接受前端图层开关取消该过滤。
/// 不绑定 SimulationEngine 的场缓冲，不把 RTS EntitySerial 当成 dense-index 拾取 ID。
pub fn scene(obs: &FactionObservation, mask: RenderLayerMask) -> FrameScene {
    let layers = RenderLayerMask(
        mask.0
            & (RenderLayerMask::NONE
                .with(RenderLayer::Structures)
                .with(RenderLayer::Organisms))
            .0,
    );
    let mut scene = FrameScene::new(obs.tick, layers);
    // 防止调用者绕开 validate 直接构建场景；非法帧只产生空画面。
    if validate(obs).is_err() {
        return scene;
    }
    let cell = VISIBILITY_CELL_TILES as f32;
    for (index, state) in obs.visibility.iter().enumerate() {
        if *state == 0 {
            continue;
        }
        let x = (index % obs.visibility_width as usize) as f32 * cell;
        let y = (index / obs.visibility_width as usize) as f32 * cell;
        scene.push(InstanceData::new(
            RenderLayer::Structures,
            [x + cell / 2.0, y + cell / 2.0],
            cell,
            if *state == 1 {
                Rgba8([34, 43, 37, 255])
            } else {
                Rgba8([17, 21, 19, 255])
            },
            InstanceShape::Square,
        ));
    }
    for building in &obs.buildings {
        let size = building_spec(building.kind).footprint;
        let (x, y) = building.origin;
        if building.faction != obs.faction && !(y..y + size).any(|cy| (x..x + size).any(|cx| visible(obs, cx, cy))) {
            continue;
        }
        scene.push(InstanceData::new(
            RenderLayer::Structures,
            [x as f32 + size as f32 / 2.0, y as f32 + size as f32 / 2.0],
            size as f32,
            WorldPalette::faction(building.faction.0 as usize),
            InstanceShape::Square,
        ));
    }
    for unit in &obs.units {
        let p = position(unit.position);
        if unit.faction != obs.faction
            && !visible(
                obs,
                (unit.position.x.0 >> 32) as u32,
                (unit.position.y.0 >> 32) as u32,
            )
        {
            continue;
        }
        scene.push(InstanceData::new(
            RenderLayer::Organisms,
            p,
            0.8,
            WorldPalette::faction(unit.faction.0 as usize),
            InstanceShape::Circle,
        ));
    }
    scene
}
