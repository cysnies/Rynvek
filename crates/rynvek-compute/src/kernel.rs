// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 计算内核注册表：按稳定 ID 登记可调度内核及其能力门槛。

use std::collections::BTreeMap;

use rynvek_core::error::RynvekErrorCode;

use crate::capability::CapabilityProfile;

/// 内核稳定标识（点分字符串的 BLAKE3 截断不在此层；此处用静态字符串 ID）
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KernelId(pub &'static str);

/// 内核签名：能力门槛与工作组提示
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelSignature {
    pub id: KernelId,
    pub min_profile: CapabilityProfile,
    /// 一维工作组大小提示（规范：线性 Workgroup `128×1×1`）
    pub workgroup_x: u32,
}

/// 内核注册表
#[derive(Clone, Debug, Default)]
pub struct KernelRegistry {
    entries: BTreeMap<&'static str, KernelSignature>,
}

impl KernelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记内核；同 ID 重复登记且签名不同则报冲突
    pub fn register(&mut self, signature: KernelSignature) -> Result<(), RynvekErrorCode> {
        match self.entries.get(signature.id.0) {
            Some(existing) if *existing != signature => Err(RynvekErrorCode::StructuralMutationConflict),
            Some(_) => Ok(()),
            None => {
                self.entries.insert(signature.id.0, signature);
                Ok(())
            }
        }
    }

    pub fn get(&self, id: KernelId) -> Option<&KernelSignature> {
        self.entries.get(id.0)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 安装 CPU Reference 标准内核集合
    pub fn with_cpu_reference_kernels() -> Self {
        let mut reg = Self::new();
        let kernels = [
            ("field.reaction_diffusion_q8", CapabilityProfile::TierD, 128),
            ("spatial.build_uniform_grid_csr", CapabilityProfile::TierD, 128),
            ("sim.tick_phase_noop", CapabilityProfile::TierD, 1),
        ];
        for (id, min_profile, workgroup_x) in kernels {
            reg.register(KernelSignature {
                id: KernelId(id),
                min_profile,
                workgroup_x,
            })
            .expect("内置内核 ID 不得冲突");
        }
        reg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_rejects_conflicting_signatures() {
        let mut reg = KernelRegistry::new();
        let sig = KernelSignature {
            id: KernelId("k"),
            min_profile: CapabilityProfile::TierD,
            workgroup_x: 128,
        };
        reg.register(sig).unwrap();
        let conflict = KernelSignature { workgroup_x: 64, ..sig };
        assert_eq!(reg.register(conflict), Err(RynvekErrorCode::StructuralMutationConflict));
        assert_eq!(KernelRegistry::with_cpu_reference_kernels().len(), 3);
    }
}
