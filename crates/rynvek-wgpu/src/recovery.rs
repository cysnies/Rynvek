// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! DeviceLost 5 步恢复状态机（规范 11.4 `ErrorRecoveryAction::RecoverOrDegradeBackend`：
//! 「触发 GPU DeviceLost 5 步恢复机或自动降级至 CPU Reference 后端」）。
//!
//! 规范只冻结了「5 步」与「降级至 CPU Reference」，步骤内容按最佳实践冻结（ERRATA E-014）：
//!
//! 1. **Quarantine（隔离）**：立即停止向该设备提交任何工作，丢弃在途的 `CandidateState[t]`；
//!    权威状态始终是 CPU 侧的 `Committed[t-1]`，因此不会丢失已提交数据；
//! 2. **ReleaseResources（释放）**：销毁旧设备上的全部缓冲、管线、BindGroup 与回读环；
//! 3. **ReacquireDevice（重获）**：重新请求 Adapter 与 Device（WebGPU 规定同一 Adapter 只能
//!    `requestDevice` 一次），并重新判定能力档位；失败计一次尝试；
//! 4. **RebuildAndRestore（重建）**：在新设备上重建管线与分段缓冲，并从 `Committed[t-1]` 重新上传状态；
//! 5. **VerifyAndResume（校验）**：执行一次 CPU/GPU 差分探针（Philox 已知向量），通过后从被丢弃的 Tick `t` 继续。
//!
//! 第 3–5 步任一失败计一次尝试并回到第 2 步；累计失败达到 `max_attempts` 时进入 [`RecoveryPhase::Degraded`]，
//! 后续全部计算改由 CPU Reference 后端执行（确定性结果不变，仅性能下降）。
//! 恢复过程中再次丢失设备会回到第 1 步。所有非法转移都会被拒绝，保证驱动代码按序执行。

use core::fmt;

use rynvek_compute::CapabilityProfile;

/// 状态机所处阶段
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryPhase {
    /// 设备可用，允许提交 GPU 工作
    Healthy,
    /// 第 1 步已完成：等待释放资源
    Quarantined,
    /// 第 2 步已完成：等待重获设备
    Released,
    /// 第 3 步已完成：等待重建与恢复状态
    Reacquired,
    /// 第 4 步已完成：等待差分校验
    Rebuilt,
    /// 恢复失败次数耗尽：永久降级至 CPU Reference
    Degraded,
}

/// 5 个恢复步骤
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryStep {
    Quarantine = 1,
    ReleaseResources = 2,
    ReacquireDevice = 3,
    RebuildAndRestore = 4,
    VerifyAndResume = 5,
}

/// 驱动状态机的事件
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryEvent {
    DeviceLost { tick: u64 },
    ResourcesReleased,
    DeviceReacquired { profile: CapabilityProfile },
    ReacquireFailed,
    StateRestored,
    RestoreFailed,
    VerificationPassed,
    VerificationFailed,
}

/// 非法转移
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidRecoveryTransition {
    pub phase: RecoveryPhase,
    pub event: RecoveryEvent,
}

impl fmt::Display for InvalidRecoveryTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "DeviceLost 恢复机在 {:?} 阶段收到非法事件 {:?}",
            self.phase, self.event
        )
    }
}

impl std::error::Error for InvalidRecoveryTransition {}

/// 一次转移的结果
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// 进入下一步
    Next(RecoveryStep),
    /// 恢复完成：从 `resume_tick` 继续（重新计算被丢弃的候选帧）
    Resumed { resume_tick: u64, generation: u32 },
    /// 降级至 CPU Reference
    Degraded,
}

/// DeviceLost 5 步恢复状态机
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceLostRecovery {
    phase: RecoveryPhase,
    /// 成功创建过的设备代数（初始设备为 0，每次恢复成功 +1）
    generation: u32,
    failed_attempts: u32,
    max_attempts: u32,
    lost_at_tick: Option<u64>,
    profile: Option<CapabilityProfile>,
    /// 累计丢失次数
    pub total_losses: u32,
}

impl DeviceLostRecovery {
    /// 默认最多 3 次失败尝试后降级
    pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;

    pub fn new(max_attempts: u32) -> Self {
        Self {
            phase: RecoveryPhase::Healthy,
            generation: 0,
            failed_attempts: 0,
            max_attempts: max_attempts.max(1),
            lost_at_tick: None,
            profile: None,
            total_losses: 0,
        }
    }

    pub fn phase(&self) -> RecoveryPhase {
        self.phase
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    pub fn failed_attempts(&self) -> u32 {
        self.failed_attempts
    }

    /// 重获设备后判定的能力档位
    pub fn reacquired_profile(&self) -> Option<CapabilityProfile> {
        self.profile
    }

    /// 只有 Healthy 时才允许提交 GPU 工作
    pub fn can_submit_gpu_work(&self) -> bool {
        self.phase == RecoveryPhase::Healthy
    }

    pub fn is_degraded(&self) -> bool {
        self.phase == RecoveryPhase::Degraded
    }

    /// 当前等待执行的步骤（Healthy / Degraded 时为 `None`）
    pub fn pending_step(&self) -> Option<RecoveryStep> {
        match self.phase {
            RecoveryPhase::Quarantined => Some(RecoveryStep::ReleaseResources),
            RecoveryPhase::Released => Some(RecoveryStep::ReacquireDevice),
            RecoveryPhase::Reacquired => Some(RecoveryStep::RebuildAndRestore),
            RecoveryPhase::Rebuilt => Some(RecoveryStep::VerifyAndResume),
            RecoveryPhase::Healthy | RecoveryPhase::Degraded => None,
        }
    }

    fn fail_attempt(&mut self) -> RecoveryOutcome {
        self.failed_attempts += 1;
        if self.failed_attempts >= self.max_attempts {
            self.phase = RecoveryPhase::Degraded;
            RecoveryOutcome::Degraded
        } else {
            // 回到第 2 步之后：重新请求设备前，当前（可能半建成的）资源已视为释放
            self.phase = RecoveryPhase::Released;
            RecoveryOutcome::Next(RecoveryStep::ReacquireDevice)
        }
    }

    /// 处理事件
    pub fn handle(&mut self, event: RecoveryEvent) -> Result<RecoveryOutcome, InvalidRecoveryTransition> {
        use RecoveryEvent as E;
        use RecoveryPhase as P;
        let invalid = InvalidRecoveryTransition {
            phase: self.phase,
            event,
        };
        let outcome = match (self.phase, event) {
            (P::Degraded, _) => return Err(invalid),
            (P::Healthy, E::DeviceLost { tick }) => {
                self.total_losses += 1;
                self.failed_attempts = 0;
                self.lost_at_tick = Some(tick);
                self.phase = P::Quarantined;
                RecoveryOutcome::Next(RecoveryStep::ReleaseResources)
            }
            // 恢复途中再次丢失：回到第 1 步，计一次失败；保留最早丢弃的 Tick
            (P::Quarantined | P::Released | P::Reacquired | P::Rebuilt, E::DeviceLost { .. }) => {
                self.total_losses += 1;
                match self.fail_attempt() {
                    RecoveryOutcome::Degraded => RecoveryOutcome::Degraded,
                    _ => {
                        self.phase = P::Quarantined;
                        RecoveryOutcome::Next(RecoveryStep::ReleaseResources)
                    }
                }
            }
            (P::Quarantined, E::ResourcesReleased) => {
                self.phase = P::Released;
                RecoveryOutcome::Next(RecoveryStep::ReacquireDevice)
            }
            (P::Released, E::DeviceReacquired { profile }) => {
                self.profile = Some(profile);
                self.phase = P::Reacquired;
                RecoveryOutcome::Next(RecoveryStep::RebuildAndRestore)
            }
            (P::Released, E::ReacquireFailed) => self.fail_attempt(),
            (P::Reacquired, E::StateRestored) => {
                self.phase = P::Rebuilt;
                RecoveryOutcome::Next(RecoveryStep::VerifyAndResume)
            }
            (P::Reacquired, E::RestoreFailed) | (P::Rebuilt, E::VerificationFailed) => self.fail_attempt(),
            (P::Rebuilt, E::VerificationPassed) => {
                self.generation += 1;
                self.failed_attempts = 0;
                self.phase = P::Healthy;
                let resume_tick = self.lost_at_tick.take().unwrap_or(0);
                RecoveryOutcome::Resumed {
                    resume_tick,
                    generation: self.generation,
                }
            }
            _ => return Err(invalid),
        };
        Ok(outcome)
    }
}

impl Default for DeviceLostRecovery {
    fn default() -> Self {
        Self::new(Self::DEFAULT_MAX_ATTEMPTS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RecoveryEvent as E;

    #[test]
    fn happy_path_walks_all_five_steps_and_resumes_at_lost_tick() {
        let mut m = DeviceLostRecovery::default();
        assert!(m.can_submit_gpu_work());
        assert_eq!(
            m.handle(E::DeviceLost { tick: 42 }).unwrap(),
            RecoveryOutcome::Next(RecoveryStep::ReleaseResources)
        );
        assert!(!m.can_submit_gpu_work());
        assert_eq!(m.pending_step(), Some(RecoveryStep::ReleaseResources));
        assert_eq!(
            m.handle(E::ResourcesReleased).unwrap(),
            RecoveryOutcome::Next(RecoveryStep::ReacquireDevice)
        );
        assert_eq!(
            m.handle(E::DeviceReacquired {
                profile: CapabilityProfile::TierC
            })
            .unwrap(),
            RecoveryOutcome::Next(RecoveryStep::RebuildAndRestore)
        );
        assert_eq!(
            m.handle(E::StateRestored).unwrap(),
            RecoveryOutcome::Next(RecoveryStep::VerifyAndResume)
        );
        assert_eq!(
            m.handle(E::VerificationPassed).unwrap(),
            RecoveryOutcome::Resumed {
                resume_tick: 42,
                generation: 1
            }
        );
        assert!(m.can_submit_gpu_work());
        assert_eq!(m.reacquired_profile(), Some(CapabilityProfile::TierC));
        assert_eq!(m.total_losses, 1);
    }

    #[test]
    fn out_of_order_events_are_rejected() {
        let mut m = DeviceLostRecovery::default();
        assert!(m.handle(E::ResourcesReleased).is_err());
        assert!(m.handle(E::VerificationPassed).is_err());
        m.handle(E::DeviceLost { tick: 1 }).unwrap();
        // 未释放资源就重获设备
        assert!(
            m.handle(E::DeviceReacquired {
                profile: CapabilityProfile::TierB
            })
            .is_err()
        );
        assert_eq!(m.phase(), RecoveryPhase::Quarantined);
    }

    #[test]
    fn repeated_failures_degrade_to_cpu_reference() {
        let mut m = DeviceLostRecovery::new(3);
        m.handle(E::DeviceLost { tick: 7 }).unwrap();
        m.handle(E::ResourcesReleased).unwrap();
        assert_eq!(
            m.handle(E::ReacquireFailed).unwrap(),
            RecoveryOutcome::Next(RecoveryStep::ReacquireDevice)
        );
        m.handle(E::DeviceReacquired {
            profile: CapabilityProfile::TierB,
        })
        .unwrap();
        m.handle(E::StateRestored).unwrap();
        assert_eq!(
            m.handle(E::VerificationFailed).unwrap(),
            RecoveryOutcome::Next(RecoveryStep::ReacquireDevice)
        );
        assert_eq!(m.failed_attempts(), 2);
        assert_eq!(m.handle(E::ReacquireFailed).unwrap(), RecoveryOutcome::Degraded);
        assert!(m.is_degraded());
        assert!(!m.can_submit_gpu_work());
        // 降级为终态
        assert!(m.handle(E::DeviceLost { tick: 8 }).is_err());
    }

    #[test]
    fn loss_during_recovery_restarts_at_step_one_and_keeps_first_tick() {
        let mut m = DeviceLostRecovery::new(5);
        m.handle(E::DeviceLost { tick: 10 }).unwrap();
        m.handle(E::ResourcesReleased).unwrap();
        m.handle(E::DeviceReacquired {
            profile: CapabilityProfile::TierB,
        })
        .unwrap();
        assert_eq!(
            m.handle(E::DeviceLost { tick: 11 }).unwrap(),
            RecoveryOutcome::Next(RecoveryStep::ReleaseResources)
        );
        assert_eq!(m.phase(), RecoveryPhase::Quarantined);
        m.handle(E::ResourcesReleased).unwrap();
        m.handle(E::DeviceReacquired {
            profile: CapabilityProfile::TierB,
        })
        .unwrap();
        m.handle(E::StateRestored).unwrap();
        assert_eq!(
            m.handle(E::VerificationPassed).unwrap(),
            RecoveryOutcome::Resumed {
                resume_tick: 10,
                generation: 1
            }
        );
        assert_eq!(m.total_losses, 2);
        // 成功后失败计数清零，下次丢失重新获得完整的尝试次数
        assert_eq!(m.failed_attempts(), 0);
    }
}
