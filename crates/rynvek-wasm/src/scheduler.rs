// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 固定步长 Tick 调度器：把不规则的帧回调间隔折算为整数 Tick 数。
//!
//! 仿真只以整数 Tick 推进（确定性不依赖帧时间）；帧时间只决定“本帧推进几个 Tick”。
//! 单帧追赶上限防止标签页切回时的“死亡螺旋”，超出部分直接丢弃而非累积。

/// 单帧最多追赶的 Tick 数
pub const MAX_CATCH_UP_TICKS: u32 = 4;

/// 固定步长累加器
#[derive(Clone, Debug, PartialEq)]
pub struct TickScheduler {
    tick_rate: u32,
    accumulator_ms: f64,
    last_now_ms: Option<f64>,
}

impl TickScheduler {
    pub fn new(tick_rate: u32) -> Self {
        Self {
            tick_rate: tick_rate.clamp(1, 240),
            accumulator_ms: 0.0,
            last_now_ms: None,
        }
    }

    pub fn tick_rate(&self) -> u32 {
        self.tick_rate
    }

    /// 单个 Tick 的时长（毫秒）
    pub fn tick_ms(&self) -> f64 {
        1000.0 / f64::from(self.tick_rate)
    }

    /// 修改 Tick 率并清空累加器
    pub fn set_tick_rate(&mut self, tick_rate: u32) {
        *self = Self::new(tick_rate);
    }

    /// 暂停 / 恢复时调用：丢弃累计时间与时间基准，避免恢复瞬间补跑
    pub fn reset(&mut self) {
        self.accumulator_ms = 0.0;
        self.last_now_ms = None;
    }

    /// 以单调时钟 `now_ms` 推进，返回本帧应执行的 Tick 数
    pub fn advance(&mut self, now_ms: f64) -> u32 {
        let Some(last) = self.last_now_ms.replace(now_ms) else {
            return 0;
        };
        // 时钟回拨或非有限值一律视为零间隔
        let dt = now_ms - last;
        if dt.is_finite() && dt > 0.0 {
            self.accumulator_ms += dt;
        }
        let step = self.tick_ms();
        let mut ticks = 0;
        while self.accumulator_ms + 1e-9 >= step && ticks < MAX_CATCH_UP_TICKS {
            self.accumulator_ms -= step;
            ticks += 1;
        }
        if ticks == MAX_CATCH_UP_TICKS && self.accumulator_ms >= step {
            // 积压超过追赶上限：整体丢弃（不补跑，也避免浮点取模把余量留在一个 Tick 边缘）
            self.accumulator_ms = 0.0;
        }
        ticks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_frame_only_sets_the_time_base() {
        let mut s = TickScheduler::new(30);
        assert_eq!(s.advance(1000.0), 0);
        assert_eq!(s.advance(1000.0 + 1000.0 / 30.0), 1);
    }

    #[test]
    fn sixty_hz_frames_at_thirty_tps_alternate_zero_and_one_tick() {
        let mut s = TickScheduler::new(30);
        s.advance(0.0);
        let total: u32 = (1..=60).map(|i| s.advance(f64::from(i) * 1000.0 / 60.0)).sum();
        assert_eq!(total, 30);
    }

    #[test]
    fn catch_up_is_capped_and_backlog_discarded() {
        let mut s = TickScheduler::new(30);
        s.advance(0.0);
        assert_eq!(s.advance(10_000.0), MAX_CATCH_UP_TICKS);
        // 积压被丢弃：紧接着的一帧不会继续补跑
        assert_eq!(s.advance(10_001.0), 0);
    }

    #[test]
    fn clock_going_backwards_and_reset_never_run_ticks() {
        let mut s = TickScheduler::new(30);
        s.advance(500.0);
        assert_eq!(s.advance(100.0), 0);
        assert_eq!(s.advance(f64::NAN), 0);
        s.reset();
        assert_eq!(s.advance(5_000.0), 0);
        assert_eq!(s.tick_rate(), 30);
        s.set_tick_rate(0);
        assert_eq!(s.tick_rate(), 1);
    }
}
