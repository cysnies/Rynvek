// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 匹配队列（D328–D333）：按 `(league, ruleset_hash)` 分桶，评分窗口随等待时间放宽。
//!
//! - 初始窗口 ±[`BASE_WINDOW`]，每等待 1 秒放宽 [`WINDOW_GROWTH_PER_S`]，上限 ±[`MAX_WINDOW`]；
//! - 一对候选可配对当且仅当评分差不超过 **双方窗口的较大者**（久等的一方主动放宽，避免长尾饿死）；
//! - 配对顺序：最早入队者优先，其次评分差最小者，再按 `principal_id` 升序（结果确定，便于测试与审计）；
//! - 同一主体同时只能在一个队列中；重复入队视为刷新（保留原入队时间）。

use std::collections::BTreeMap;

use rynvek_security::League;

/// 初始评分窗口
pub const BASE_WINDOW: f64 = 100.0;
/// 每秒放宽量
pub const WINDOW_GROWTH_PER_S: f64 = 25.0;
/// 窗口上限
pub const MAX_WINDOW: f64 = 600.0;

/// 队列条目
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QueueEntry {
    pub principal_id: u64,
    pub rating: f64,
    pub deviation: f64,
    pub enqueued_at_s: u64,
}

impl QueueEntry {
    pub fn window(&self, now_s: u64) -> f64 {
        (BASE_WINDOW + WINDOW_GROWTH_PER_S * now_s.saturating_sub(self.enqueued_at_s) as f64).min(MAX_WINDOW)
    }
}

/// 队列桶键
pub type BucketKey = (League, [u8; 32]);

/// 一次成功配对
#[derive(Clone, Debug, PartialEq)]
pub struct Pairing {
    pub league: League,
    pub ruleset_hash: [u8; 32],
    /// `[阵营 0, 阵营 1]`：先入队者执阵营 0
    pub players: [QueueEntry; 2],
}

/// 匹配器
#[derive(Clone, Debug, Default)]
pub struct Matchmaker {
    buckets: BTreeMap<BucketKey, Vec<QueueEntry>>,
    member_of: BTreeMap<u64, BucketKey>,
}

impl Matchmaker {
    pub fn new() -> Self {
        Self::default()
    }

    /// 入队；返回当前排队位置（从 1 开始）
    pub fn enqueue(&mut self, key: BucketKey, entry: QueueEntry) -> usize {
        if let Some(old) = self.member_of.get(&entry.principal_id).copied() {
            if old == key {
                let bucket = self.buckets.get_mut(&key).expect("成员索引一致");
                let pos = bucket
                    .iter()
                    .position(|e| e.principal_id == entry.principal_id)
                    .expect("成员索引一致");
                bucket[pos].rating = entry.rating;
                bucket[pos].deviation = entry.deviation;
                return pos + 1;
            }
            self.cancel(entry.principal_id);
        }
        let bucket = self.buckets.entry(key).or_default();
        bucket.push(entry);
        self.member_of.insert(entry.principal_id, key);
        bucket.len()
    }

    /// 离队
    pub fn cancel(&mut self, principal_id: u64) -> bool {
        let Some(key) = self.member_of.remove(&principal_id) else {
            return false;
        };
        if let Some(bucket) = self.buckets.get_mut(&key) {
            bucket.retain(|e| e.principal_id != principal_id);
            if bucket.is_empty() {
                self.buckets.remove(&key);
            }
        }
        true
    }

    pub fn is_queued(&self, principal_id: u64) -> bool {
        self.member_of.contains_key(&principal_id)
    }

    pub fn len(&self) -> usize {
        self.member_of.len()
    }

    pub fn is_empty(&self) -> bool {
        self.member_of.is_empty()
    }

    /// 尝试在所有桶中配对，返回全部新配对（已从队列移除）
    pub fn pair(&mut self, now_s: u64) -> Vec<Pairing> {
        let mut out = Vec::new();
        let keys: Vec<BucketKey> = self.buckets.keys().copied().collect();
        for key in keys {
            loop {
                let Some(bucket) = self.buckets.get(&key) else { break };
                let found = Self::best_pair(bucket, now_s);
                let Some((a, b)) = found else { break };
                let (first, second) = (bucket[a], bucket[b]);
                self.cancel(first.principal_id);
                self.cancel(second.principal_id);
                out.push(Pairing {
                    league: key.0,
                    ruleset_hash: key.1,
                    players: [first, second],
                });
            }
        }
        out
    }

    /// 桶内按入队顺序扫描：为最早的可配对者挑选评分差最小的对手
    fn best_pair(bucket: &[QueueEntry], now_s: u64) -> Option<(usize, usize)> {
        for (i, a) in bucket.iter().enumerate() {
            let best = bucket
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .filter(|(_, b)| (a.rating - b.rating).abs() <= a.window(now_s).max(b.window(now_s)))
                .min_by(|(_, x), (_, y)| {
                    (a.rating - x.rating)
                        .abs()
                        .total_cmp(&(a.rating - y.rating).abs())
                        .then(x.principal_id.cmp(&y.principal_id))
                });
            if let Some((j, _)) = best {
                return Some(if i < j { (i, j) } else { (j, i) });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RS: [u8; 32] = [7; 32];

    fn e(id: u64, rating: f64, t: u64) -> QueueEntry {
        QueueEntry {
            principal_id: id,
            rating,
            deviation: 100.0,
            enqueued_at_s: t,
        }
    }

    #[test]
    fn pairs_close_ratings_immediately_and_far_ones_after_waiting() {
        let mut m = Matchmaker::new();
        m.enqueue((League::Human, RS), e(1, 1500.0, 0));
        m.enqueue((League::Human, RS), e(2, 1800.0, 0));
        assert!(m.pair(0).is_empty(), "差 300 > 初始窗口 100");
        assert!(m.pair(7).is_empty(), "7 s 后窗口 275 仍不足");
        let p = m.pair(8);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].players.map(|q| q.principal_id), [1, 2]);
        assert!(m.is_empty());
    }

    #[test]
    fn leagues_and_rulesets_never_mix() {
        let mut m = Matchmaker::new();
        m.enqueue((League::Human, RS), e(1, 1500.0, 0));
        m.enqueue((League::Script, RS), e(2, 1500.0, 0));
        m.enqueue((League::Human, [8; 32]), e(3, 1500.0, 0));
        assert!(m.pair(1000).is_empty());
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn picks_the_closest_opponent_for_the_earliest_entry() {
        let mut m = Matchmaker::new();
        let k = (League::AgentHybrid, RS);
        m.enqueue(k, e(1, 1500.0, 0));
        m.enqueue(k, e(2, 1590.0, 1));
        m.enqueue(k, e(3, 1510.0, 2));
        m.enqueue(k, e(4, 1600.0, 3));
        let p = m.pair(3);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].players.map(|q| q.principal_id), [1, 3]);
        assert_eq!(p[1].players.map(|q| q.principal_id), [2, 4]);
    }

    #[test]
    fn requeue_keeps_position_and_switching_bucket_moves() {
        let mut m = Matchmaker::new();
        let k = (League::Human, RS);
        assert_eq!(m.enqueue(k, e(1, 1500.0, 0)), 1);
        assert_eq!(m.enqueue(k, e(2, 2500.0, 0)), 2);
        assert_eq!(m.enqueue(k, e(1, 1510.0, 9)), 1);
        m.enqueue((League::Script, RS), e(1, 1500.0, 10));
        assert_eq!(m.len(), 2);
        assert!(m.cancel(1));
        assert!(!m.cancel(1));
    }
}
