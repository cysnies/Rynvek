// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 服务器裁剪观察 → 既有 WorldRenderer 场景。这里只持有只读观察，不实例化 ArenaWorld。
//! 静态地图与选择叠层沿用同一 FrameScene；未知格不上传地形种类，隐藏敌人不进入实例。

use crate::EngineError;
use rynvek_core::{error::RynvekErrorCode, fixed::Q32_32};
use rynvek_render::{FrameScene, InstanceData, InstanceShape, RenderLayer, RenderLayerMask, Rgba8, WorldPalette};
use rynvek_rts::{buildings::building_spec, fog::VISIBILITY_CELL_TILES, observation::FactionObservation};

#[derive(Debug, Default)]
pub(crate) struct Selection {
    pub units: std::collections::BTreeSet<u32>,
    pub building: Option<u32>,
    pub drag: Option<[[f32; 2]; 2]>,
    pub placement: Option<crate::protocol::ArenaPlacement>,
}

pub(crate) fn validate_map(map: &crate::protocol::ArenaMapData) -> Result<(), EngineError> {
    if map.width == 0
        || map.height == 0
        || map.width > 1024
        || map.height > 1024
        || map.tiles.len() != map.width as usize * map.height as usize
        || map.tiles.iter().any(|tile| *tile > 4)
        || rynvek_core::hash::Blake3Hash::of(&map.tiles).to_hex() != map.tiles_hash
    {
        return Err(EngineError {
            code: RynvekErrorCode::DimensionMismatch,
            message: "静态地图尺寸、地形或BLAKE3不符".into(),
        });
    }
    Ok(())
}

pub(crate) fn map_matches(map: &crate::protocol::ArenaMapData, obs: &FactionObservation) -> bool {
    map.width.div_ceil(VISIBILITY_CELL_TILES) == obs.visibility_width
        && map.height.div_ceil(VISIBILITY_CELL_TILES) as usize * obs.visibility_width as usize == obs.visibility.len()
}

const TERRAIN_COLORS: [[u8; 4]; 5] = [
    [39, 54, 43, 255],
    [59, 69, 71, 255],
    [83, 108, 50, 255],
    [110, 59, 40, 255],
    [23, 29, 27, 255],
];
const SELECTION_COLOR: Rgba8 = Rgba8([228, 255, 158, 255]);

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
    presentation_scene(obs, mask, None, &Selection::default())
}

/// 带静态地图/选择的同一场景构建路径，表现状态不能反向修改观察。
pub(crate) fn presentation_scene(
    obs: &FactionObservation,
    mask: RenderLayerMask,
    map: Option<&crate::protocol::ArenaMapData>,
    selection: &Selection,
) -> FrameScene {
    let mut allowed = RenderLayerMask::NONE
        .with(RenderLayer::Structures)
        .with(RenderLayer::Organisms);
    if map.is_some() {
        allowed = allowed.with(RenderLayer::Terrain);
    }
    if !selection.units.is_empty()
        || selection.building.is_some()
        || selection.drag.is_some()
        || selection.placement.is_some()
    {
        allowed = allowed.with(RenderLayer::Overlay);
    }
    let layers = RenderLayerMask(mask.0 & allowed.0);
    let mut scene = FrameScene::new(obs.tick, layers);
    // 防止调用者绕开 validate 直接构建场景；非法帧只产生空画面。
    if validate(obs).is_err() {
        return scene;
    }
    let cell = VISIBILITY_CELL_TILES as f32;
    for (index, state) in obs.visibility.iter().enumerate() {
        let x = (index % obs.visibility_width as usize) as f32 * cell;
        let y = (index / obs.visibility_width as usize) as f32 * cell;
        if let Some(map) = map {
            // 未知格只上传统一不透明底色；关闭迷雾层也不能泄漏真实地形。
            if *state == 0 {
                scene.push(InstanceData::new(
                    RenderLayer::Terrain,
                    [x + cell / 2.0, y + cell / 2.0],
                    cell,
                    WorldPalette::NEUTRAL_DARK.background,
                    InstanceShape::Square,
                ));
                continue;
            }
            for ty in y as u32..(y as u32 + VISIBILITY_CELL_TILES).min(map.height) {
                for tx in x as u32..(x as u32 + VISIBILITY_CELL_TILES).min(map.width) {
                    let mut color = TERRAIN_COLORS[map.tiles[(ty * map.width + tx) as usize] as usize];
                    if *state == 2 {
                        for c in &mut color[..3] {
                            *c = (*c as u16 * 32 / 100) as u8;
                        }
                    }
                    scene.push(InstanceData::new(
                        RenderLayer::Terrain,
                        [tx as f32 + 0.5, ty as f32 + 0.5],
                        1.0,
                        Rgba8(color),
                        InstanceShape::Square,
                    ));
                }
            }
        } else if *state != 0 {
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
    }
    if let Some([a, b]) = selection.drag {
        scene.push(InstanceData::rectangle_outline(
            [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0],
            [(a[0] - b[0]).abs(), (a[1] - b[1]).abs()],
            Rgba8([200, 239, 160, 255]),
        ));
    }
    if let Some(placement) = &selection.placement {
        let size = building_spec(placement.kind).footprint;
        let [x, y] = placement.origin;
        let (width, height) = map.map_or_else(|| dimensions(obs), |m| (m.width, m.height));
        // 只按公开地图边界裁剪，固定中性黄；不猜资源/科技/碰撞，也不探查隐藏实体。
        if x.checked_add(size).is_some_and(|end| end <= width) && y.checked_add(size).is_some_and(|end| end <= height) {
            scene.push(InstanceData::rectangle_outline(
                [x as f32 + size as f32 / 2.0, y as f32 + size as f32 / 2.0],
                [size as f32; 2],
                Rgba8([234, 214, 134, 255]),
            ));
        }
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
    for building in &obs.buildings {
        if building.faction == obs.faction && selection.building == Some(building.id.0) {
            let size = building_spec(building.kind).footprint as f32;
            scene.push(InstanceData::new(
                RenderLayer::Overlay,
                [
                    building.origin.0 as f32 + size / 2.0,
                    building.origin.1 as f32 + size / 2.0,
                ],
                size * 1.45,
                SELECTION_COLOR,
                InstanceShape::Ring,
            ));
        }
    }
    for unit in &obs.units {
        let p = position(unit.position);
        if unit.faction != obs.faction
            && !visible(obs, (unit.position.x.0 >> 32) as u32, (unit.position.y.0 >> 32) as u32)
        {
            continue;
        }
        if unit.faction == obs.faction && selection.units.contains(&unit.id.0) {
            scene.push(InstanceData::new(
                RenderLayer::Overlay,
                p,
                1.25,
                SELECTION_COLOR,
                InstanceShape::Ring,
            ));
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
