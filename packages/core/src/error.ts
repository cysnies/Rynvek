// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

/**
 * 跨语言统一错误码注册表（规范 11 §11.4）。
 *
 * 数值与 Rust `rynvek_core::error::RynvekErrorCode` 逐项一致，单元测试会直接解析 Rust 源码进行比对。
 */

export const RynvekErrorCode = {
  // --- 0x1000：内核、坐标与数值错误 ---
  NumericNaNOrInfDetected: 0x1001,
  FixedPointOverflow: 0x1002,
  CoordinateOutOfCanonicalRange: 0x1003,
  DimensionMismatch: 0x1004,

  // --- 0x2000：ECS、句柄与内存错误 ---
  StaleEntityHandleGeneration: 0x2001,
  EntityNotFoundOrTombstoned: 0x2002,
  ArchetypeComponentMissing: 0x2003,
  WorldMemoryBudgetExceeded: 0x2004,
  StructuralMutationConflict: 0x2005,

  // --- 0x3000：仿真、时钟与阶段 DAG 错误 ---
  PhaseDependencyCycleDetected: 0x3001,
  UnresolvedWriteConflict: 0x3002,
  SimulationInvariantViolated: 0x3003,
  TickDeadlineExceeded: 0x3004,

  // --- 0x4000：神经汤与连续场错误 ---
  NeuralTopologyCapacityExceeded: 0x4001,
  DaleLawSignViolation: 0x4002,
  FieldCflStabilityViolation: 0x4003,
  ConservationLawMismatch: 0x4004,

  // --- 0x5000：规则编译器、QuickJS 与 WASM 沙箱错误 ---
  RuleStaticTypeCheckFailed: 0x5001,
  RuleUndeclaredWriteAccess: 0x5002,
  RuleLoopBoundExceeded: 0x5003,
  SandboxGasBudgetExhausted: 0x5004,
  SandboxMemoryLimitExceeded: 0x5005,
  PluginCapabilityDenied: 0x5006,

  // --- 0x6000：WebGPU、计算与渲染错误 ---
  WebGpuAdapterUnavailable: 0x6001,
  GpuDeviceLost: 0x6002,
  GpuBufferAllocationFailed: 0x6003,
  WgslShaderValidationFailed: 0x6004,

  // --- 0x7000：存储、快照、日志与迁移错误 ---
  InvalidFileMagicOrVersion: 0x7001,
  Blake3ChecksumMismatch: 0x7002,
  JournalTornWriteTruncated: 0x7003,
  SchemaMigrationPathNotFound: 0x7004,
  StalePartitionFencingToken: 0x7005,

  // --- 0x8000：网络、命令、对局与安全错误 ---
  UnauthorizedCapabilityToken: 0x8001,
  DuplicateCommandIdempotencyKey: 0x8002,
  CommandExpiredOrFutureTooFar: 0x8003,
  CommandRateLimitExceeded: 0x8004,
  InsufficientFactionResources: 0x8005,
  InvalidBuildingPlacementGrid: 0x8006,

  // --- 0x9000：资源包、Hub 与资产管线错误 ---
  InvalidEd25519Signature: 0x9001,
  RevokedPackageBlocked: 0x9002,
  DependencyLockConflict: 0x9003,
  UnsafeRviewShaderRejected: 0x9004,
} as const;

/** 错误码名称 */
export type RynvekErrorName = keyof typeof RynvekErrorCode;
/** 错误码数值 */
export type RynvekErrorCode = (typeof RynvekErrorCode)[RynvekErrorName];

/** 强制故障恢复动作（与 Rust `ErrorRecoveryAction` 数值一致） */
export const ErrorRecoveryAction = {
  /** 仅拒绝当前单条非法玩家/Agent 命令，世界继续正常推进 */
  RejectCommandOnly: 0,
  /** 立即隔离并禁用超限的第三方规则或 WASM 插件，回退该插件当帧写入 */
  IsolateFaultyExtension: 1,
  /** 丢弃当前未提交的 CandidateState[t]，回滚至 Committed[t-1] 并告警 */
  RollbackTickCandidate: 2,
  /** 触发 GPU DeviceLost 5 步恢复机或自动降级至 CPU Reference 后端 */
  RecoverOrDegradeBackend: 3,
  /** 从最近 .rsnap + .rjournal 执行完整崩溃恢复状态机 */
  TriggerStorageRecovery: 4,
} as const;

export type ErrorRecoveryAction = (typeof ErrorRecoveryAction)[keyof typeof ErrorRecoveryAction];

const NAME_BY_CODE: ReadonlyMap<number, RynvekErrorName> = new Map(
  (Object.entries(RynvekErrorCode) as [RynvekErrorName, RynvekErrorCode][]).map(([name, code]) => [code, name]),
);

const RECOVERY_BY_CODE: ReadonlyMap<RynvekErrorCode, ErrorRecoveryAction> = new Map<
  RynvekErrorCode,
  ErrorRecoveryAction
>([
  [RynvekErrorCode.UnauthorizedCapabilityToken, ErrorRecoveryAction.RejectCommandOnly],
  [RynvekErrorCode.DuplicateCommandIdempotencyKey, ErrorRecoveryAction.RejectCommandOnly],
  [RynvekErrorCode.CommandExpiredOrFutureTooFar, ErrorRecoveryAction.RejectCommandOnly],
  [RynvekErrorCode.CommandRateLimitExceeded, ErrorRecoveryAction.RejectCommandOnly],
  [RynvekErrorCode.InsufficientFactionResources, ErrorRecoveryAction.RejectCommandOnly],
  [RynvekErrorCode.InvalidBuildingPlacementGrid, ErrorRecoveryAction.RejectCommandOnly],
  [RynvekErrorCode.SandboxGasBudgetExhausted, ErrorRecoveryAction.IsolateFaultyExtension],
  [RynvekErrorCode.SandboxMemoryLimitExceeded, ErrorRecoveryAction.IsolateFaultyExtension],
  [RynvekErrorCode.PluginCapabilityDenied, ErrorRecoveryAction.IsolateFaultyExtension],
  [RynvekErrorCode.NumericNaNOrInfDetected, ErrorRecoveryAction.RollbackTickCandidate],
  [RynvekErrorCode.SimulationInvariantViolated, ErrorRecoveryAction.RollbackTickCandidate],
  [RynvekErrorCode.ConservationLawMismatch, ErrorRecoveryAction.RollbackTickCandidate],
  [RynvekErrorCode.WebGpuAdapterUnavailable, ErrorRecoveryAction.RecoverOrDegradeBackend],
  [RynvekErrorCode.GpuDeviceLost, ErrorRecoveryAction.RecoverOrDegradeBackend],
  [RynvekErrorCode.GpuBufferAllocationFailed, ErrorRecoveryAction.RecoverOrDegradeBackend],
  [RynvekErrorCode.Blake3ChecksumMismatch, ErrorRecoveryAction.TriggerStorageRecovery],
  [RynvekErrorCode.JournalTornWriteTruncated, ErrorRecoveryAction.TriggerStorageRecovery],
]);

/** 由数值反查错误码名称；未注册时返回 `undefined` */
export function errorNameOf(code: number): RynvekErrorName | undefined {
  return NAME_BY_CODE.get(code);
}

/** 规范强制的恢复动作；规范未规定时返回 `undefined` */
export function recoveryActionOf(code: RynvekErrorCode): ErrorRecoveryAction | undefined {
  return RECOVERY_BY_CODE.get(code);
}

/** 统一展示格式：`RV-0x1003 CoordinateOutOfCanonicalRange`（与 Rust `Display` 一致） */
export function formatErrorCode(code: RynvekErrorCode): string {
  const hex = code.toString(16).toUpperCase().padStart(4, "0");
  return `RV-0x${hex} ${errorNameOf(code) ?? "Unknown"}`;
}

/** 携带统一错误码的异常 */
export class RynvekError extends Error {
  public readonly code: RynvekErrorCode;

  public constructor(code: RynvekErrorCode, detail?: string) {
    super(detail === undefined ? formatErrorCode(code) : `${formatErrorCode(code)}: ${detail}`);
    this.name = "RynvekError";
    this.code = code;
  }
}
