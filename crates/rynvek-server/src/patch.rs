// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 复用领域 `WorldPatch`，避免服务器校验模型与真实执行模型分裂。
//! 以及路由 `08` 的校验：安全分级映射、"声明不得低于所需"、生效时机。

use crate::error::ApiError;
use rynvek_core::error::RynvekErrorCode;
pub use rynvek_world::{HlcTimestamp, LiveEditSafetyClass, WorldPatch, WorldPatchOp};

/// 单个补丁的操作数上限
pub const MAX_PATCH_OPERATIONS: usize = 4096;

/// 一个操作所需的最低安全分级（E-021 映射表）
pub fn required_class(op: &WorldPatchOp) -> LiveEditSafetyClass {
    use LiveEditSafetyClass as C;
    match op {
        WorldPatchOp::SetWorldParam { key_path, .. } => {
            match key_path.split('.').next().unwrap_or_default() {
                "presentation" | "visual" | "ui" | "audio" => C::HotSafePresentation,
                "field" | "economy" | "buildings" | "units" => C::TickBoundarySafe,
                "neural" | "rules" | "ecology" => C::EpochBoundarySafe,
                // `world.*` 与所有未登记命名空间：最保守
                _ => C::RequiresForkRestart,
            }
        }
        WorldPatchOp::PaintTerrainRect { .. } | WorldPatchOp::RemoveEntityTemplate { .. } => C::TickBoundarySafe,
        WorldPatchOp::UpsertSeedGenome { .. } | WorldPatchOp::UpdateRuleModule { .. } | WorldPatchOp::RemoveRuleModule { .. } => {
            C::EpochBoundarySafe
        }
    }
}

fn invalid(message: impl Into<String>) -> ApiError {
    // E-021：补丁格式错误复用 0x3003
    ApiError::new(RynvekErrorCode::SimulationInvariantViolated, message)
        .with_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY)
}

/// 整体校验（原子：任一操作不合格则整个补丁被拒绝）
pub fn validate(patch: &WorldPatch) -> Result<LiveEditSafetyClass, ApiError> {
    if patch.operations.is_empty() {
        return Err(invalid("operations 不能为空"));
    }
    if patch.operations.len() > MAX_PATCH_OPERATIONS {
        return Err(invalid(format!("operations 超过 {MAX_PATCH_OPERATIONS} 条")));
    }
    let mut required = LiveEditSafetyClass::HotSafePresentation;
    for (i, op) in patch.operations.iter().enumerate() {
        match op {
            WorldPatchOp::SetWorldParam {
                key_path,
                new_value_json,
            } => {
                if key_path.is_empty() || key_path.split('.').any(str::is_empty) {
                    return Err(invalid(format!("第 {i} 条：key_path 非法")));
                }
                if serde_json::from_str::<serde_json::Value>(new_value_json).is_err() {
                    return Err(invalid(format!("第 {i} 条：new_value_json 不是合法 JSON")));
                }
            }
            WorldPatchOp::PaintTerrainRect {
                x0, y0, width, height, ..
            } => {
                if *x0 < 0 || *y0 < 0 || *width == 0 || *height == 0 {
                    return Err(invalid(format!("第 {i} 条：涂刷矩形越界")));
                }
            }
            WorldPatchOp::UpsertSeedGenome { genome_slug, .. } if genome_slug.is_empty() => {
                return Err(invalid(format!("第 {i} 条：genome_slug 为空")));
            }
            WorldPatchOp::UpdateRuleModule { module_id, .. } | WorldPatchOp::RemoveRuleModule { module_id }
                if !rynvek_rts::world::authoring::valid_authoring_key(module_id) => {
                return Err(invalid(format!("第 {i} 条：module_id 非法")));
            }
            _ => {}
        }
        required = required.max(required_class(op));
    }
    if patch.safety_class < required {
        return Err(invalid(format!(
            "补丁声明 {:?} 低于所需 {required:?}（声明可以更保守，不能更低）",
            patch.safety_class
        )));
    }
    Ok(required)
}

/// 生效时机描述：`(scheduled_for, effective_tick)`
pub fn schedule(class: LiveEditSafetyClass, current_tick: u64) -> (&'static str, Option<u64>) {
    match class {
        LiveEditSafetyClass::HotSafePresentation => ("next_frame", None),
        LiveEditSafetyClass::TickBoundarySafe => ("next_tick", Some(current_tick + 1)),
        // 严格晚于当前 Tick 的下一个 tick % 4 == 0
        LiveEditSafetyClass::EpochBoundarySafe => ("next_epoch", Some((current_tick / 4 + 1) * 4)),
        LiveEditSafetyClass::RequiresForkRestart => ("world_fork", None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(class: LiveEditSafetyClass, ops: Vec<WorldPatchOp>) -> WorldPatch {
        WorldPatch {
            patch_id: [1; 16],
            base_authoring_revision: 0,
            hlc: HlcTimestamp {
                wall_time_ms: 1,
                logical_counter: 0,
                author_node_id: 7,
            },
            safety_class: class,
            operations: ops,
        }
    }

    fn param(k: &str) -> WorldPatchOp {
        WorldPatchOp::SetWorldParam {
            key_path: k.into(),
            new_value_json: "1".into(),
        }
    }

    #[test]
    fn declared_class_may_be_conservative_but_never_lower() {
        use LiveEditSafetyClass as C;
        assert_eq!(
            validate(&patch(C::EpochBoundarySafe, vec![param("visual.tint")])).unwrap(),
            C::HotSafePresentation
        );
        let e = validate(&patch(
            C::TickBoundarySafe,
            vec![param("economy.cost"), param("rules.x")],
        ))
        .unwrap_err();
        assert_eq!(e.code, RynvekErrorCode::SimulationInvariantViolated);
        assert_eq!(
            validate(&patch(C::RequiresForkRestart, vec![param("world.size")])).unwrap(),
            C::RequiresForkRestart
        );
    }

    #[test]
    fn serde_shape_matches_e021() {
        let p = patch(LiveEditSafetyClass::EpochBoundarySafe, vec![param("neural.lr")]);
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["safety_class"], "EpochBoundarySafe");
        assert_eq!(v["patch_id"][0], 1);
        assert!(v["operations"][0]["SetWorldParam"].is_object());
    }

    #[test]
    fn epoch_boundary_is_strictly_after_current_tick() {
        assert_eq!(schedule(LiveEditSafetyClass::EpochBoundarySafe, 8).1, Some(12));
        assert_eq!(schedule(LiveEditSafetyClass::EpochBoundarySafe, 9).1, Some(12));
    }
}
