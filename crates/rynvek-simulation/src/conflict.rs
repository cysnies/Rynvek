// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `ConflictPolicy` 确定性归约（规范目录 / Phase 08 ActionArbitration）。
//!
//! 规范未给出完整枚举源码；此处冻结最小可测试策略集，并在注释标明为最佳实践补全。

/// 同帧多写冲突的归约策略
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ConflictPolicy {
    /// 保留 dense_index 最小写入者
    LowestDenseIndexWins = 0,
    /// 保留绝对值最大的写入（能量/伤害峰值）
    MaxAbsMagnitudeWins = 1,
    /// 代数求和后钳制
    SaturatingSum = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConflictResolution {
    pub winner_dense: u32,
    pub value: i64,
}

impl ConflictPolicy {
    /// 对候选 `(dense_index, value)` 列表做确定性归约；空列表返回 `None`
    pub fn resolve(self, candidates: &[(u32, i64)]) -> Option<ConflictResolution> {
        if candidates.is_empty() {
            return None;
        }
        match self {
            Self::LowestDenseIndexWins => {
                let (dense, value) = candidates.iter().copied().min_by_key(|(d, _)| *d)?;
                Some(ConflictResolution {
                    winner_dense: dense,
                    value,
                })
            }
            Self::MaxAbsMagnitudeWins => {
                let (dense, value) = candidates.iter().copied().max_by_key(|(d, v)| (v.abs(), !*d))?;
                Some(ConflictResolution {
                    winner_dense: dense,
                    value,
                })
            }
            Self::SaturatingSum => {
                let sum = candidates.iter().fold(0i64, |acc, (_, v)| acc.saturating_add(*v));
                let (dense, _) = candidates.iter().copied().min_by_key(|(d, _)| *d)?;
                Some(ConflictResolution {
                    winner_dense: dense,
                    value: sum,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowest_dense_wins() {
        let r = ConflictPolicy::LowestDenseIndexWins
            .resolve(&[(3, 10), (1, 99), (2, 5)])
            .unwrap();
        assert_eq!(r.winner_dense, 1);
        assert_eq!(r.value, 99);
    }
}
