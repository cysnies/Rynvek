// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 三种标准神经图存储布局（GAP-08 / 规范 05 `TopologyStorageClass`）。
//!
//! 规范只冻结了三种布局的名称与适用范围；具体物理排布按最佳实践设计：
//!
//! - `DenseTiledMatrix`：`16×16` 瓦片按行主序依次排列，瓦片内部同样行主序，尾部零填充；
//! - `BlockSparseCSR`：`8×8` 块的块级 CSR，仅保存含非零元素的块；
//! - `PagedDynamicAdjacency`：每个突触后神经元固定 32 个 `(pre, weight)` 槽位。
//!
//! 三种布局都实现 [`SynapticMatVec`]，并且**逐项**执行 `(w * x) >> 8` 后再做整数求和：
//! 整数加法满足交换律与结合律，因此无论遍历顺序如何，三种布局的结果都位级相同。

use rynvek_core::error::RynvekErrorCode;

use crate::runtime::TopologyStorageClass;

/// `DenseTiledMatrix` 的瓦片边长
pub const DENSE_TILE_DIM: usize = 16;
/// `BlockSparseCSR` 的块边长
pub const SPARSE_BLOCK_DIM: usize = 8;
/// `PagedDynamicAdjacency` 每神经元的突触槽位数
pub const SYNAPSE_SLOTS_PER_NEURON: usize = 32;
/// `DenseTiledMatrix` 的神经元数上限
pub const DENSE_MAX_NEURONS: usize = 64;
/// `BlockSparseCSR` 的神经元数上限
pub const BLOCK_SPARSE_MAX_NEURONS: usize = 4096;
/// 空槽位标记
const EMPTY_SLOT: u16 = u16::MAX;

/// 按 GAP-08 规则选择存储布局
pub fn select_storage_class(
    neuron_count: usize,
    dynamic_topology: bool,
) -> Result<TopologyStorageClass, RynvekErrorCode> {
    if neuron_count > BLOCK_SPARSE_MAX_NEURONS {
        return Err(RynvekErrorCode::NeuralTopologyCapacityExceeded);
    }
    Ok(if dynamic_topology {
        TopologyStorageClass::PagedDynamicAdjacency
    } else if neuron_count <= DENSE_MAX_NEURONS {
        TopologyStorageClass::DenseTiledMatrix
    } else {
        TopologyStorageClass::BlockSparseCSR
    })
}

/// 突触矩阵-向量乘：`y[post] += Σ_pre (w[post][pre] * x[pre]) >> 8`
pub trait SynapticMatVec {
    fn neuron_count(&self) -> usize;
    /// 读取单个权重（不存在的连接返回 0）
    fn weight(&self, post: usize, pre: usize) -> i16;
    /// 累加到 `y`（长度须等于神经元数）
    fn matvec_accumulate_q8(&self, x_q8: &[i16], y: &mut [i32]);
}

#[inline]
fn syn_term(w: i16, x: i16) -> i32 {
    ((w as i32) * (x as i32)) >> 8
}

/// 16×16 分块稠密矩阵
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DenseTiledMatrix {
    n: usize,
    tiles_per_axis: usize,
    data: Vec<i16>,
}

impl DenseTiledMatrix {
    pub fn zeros(n: usize) -> Self {
        let tiles_per_axis = n.div_ceil(DENSE_TILE_DIM);
        Self {
            n,
            tiles_per_axis,
            data: vec![0; tiles_per_axis * tiles_per_axis * DENSE_TILE_DIM * DENSE_TILE_DIM],
        }
    }

    /// 由行主序稠密矩阵 `dense[post * n + pre]` 构建
    pub fn from_dense(n: usize, dense: &[i16]) -> Self {
        assert_eq!(dense.len(), n * n, "稠密矩阵尺寸不匹配");
        let mut m = Self::zeros(n);
        for post in 0..n {
            for pre in 0..n {
                m.set(post, pre, dense[post * n + pre]);
            }
        }
        m
    }

    #[inline]
    fn offset(&self, post: usize, pre: usize) -> usize {
        let (tr, tc) = (post / DENSE_TILE_DIM, pre / DENSE_TILE_DIM);
        let (ir, ic) = (post % DENSE_TILE_DIM, pre % DENSE_TILE_DIM);
        (tr * self.tiles_per_axis + tc) * DENSE_TILE_DIM * DENSE_TILE_DIM + ir * DENSE_TILE_DIM + ic
    }

    pub fn set(&mut self, post: usize, pre: usize, w: i16) {
        let off = self.offset(post, pre);
        self.data[off] = w;
    }
}

impl SynapticMatVec for DenseTiledMatrix {
    fn neuron_count(&self) -> usize {
        self.n
    }

    fn weight(&self, post: usize, pre: usize) -> i16 {
        self.data[self.offset(post, pre)]
    }

    fn matvec_accumulate_q8(&self, x_q8: &[i16], y: &mut [i32]) {
        assert_eq!(x_q8.len(), self.n);
        assert_eq!(y.len(), self.n);
        // 以瓦片为单位遍历，提升缓存局部性；越界（填充区）行列直接跳过
        for tr in 0..self.tiles_per_axis {
            for tc in 0..self.tiles_per_axis {
                let base = (tr * self.tiles_per_axis + tc) * DENSE_TILE_DIM * DENSE_TILE_DIM;
                for ir in 0..DENSE_TILE_DIM {
                    let post = tr * DENSE_TILE_DIM + ir;
                    if post >= self.n {
                        break;
                    }
                    let row = &self.data[base + ir * DENSE_TILE_DIM..base + (ir + 1) * DENSE_TILE_DIM];
                    let mut acc = 0i32;
                    for (ic, &w) in row.iter().enumerate() {
                        let pre = tc * DENSE_TILE_DIM + ic;
                        if pre >= self.n {
                            break;
                        }
                        acc += syn_term(w, x_q8[pre]);
                    }
                    y[post] += acc;
                }
            }
        }
    }
}

/// 8×8 块稀疏压缩行矩阵（拓扑仅在 Epoch 边界由 `from_dense` 重建）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockSparseCsr {
    n: usize,
    /// 长度 = 块行数 + 1
    block_row_offsets: Vec<u32>,
    /// 每个非零块的块列号（块行内升序）
    block_col_indices: Vec<u32>,
    /// 每块 64 个权重，块内行主序
    block_values: Vec<i16>,
}

impl BlockSparseCsr {
    pub fn from_dense(n: usize, dense: &[i16]) -> Self {
        assert_eq!(dense.len(), n * n, "稠密矩阵尺寸不匹配");
        let blocks = n.div_ceil(SPARSE_BLOCK_DIM);
        let mut block_row_offsets = Vec::with_capacity(blocks + 1);
        let mut block_col_indices = Vec::new();
        let mut block_values = Vec::new();
        block_row_offsets.push(0);
        for br in 0..blocks {
            for bc in 0..blocks {
                let mut block = [0i16; SPARSE_BLOCK_DIM * SPARSE_BLOCK_DIM];
                let mut nonzero = false;
                for ir in 0..SPARSE_BLOCK_DIM {
                    for ic in 0..SPARSE_BLOCK_DIM {
                        let (post, pre) = (br * SPARSE_BLOCK_DIM + ir, bc * SPARSE_BLOCK_DIM + ic);
                        if post < n && pre < n {
                            let w = dense[post * n + pre];
                            block[ir * SPARSE_BLOCK_DIM + ic] = w;
                            nonzero |= w != 0;
                        }
                    }
                }
                if nonzero {
                    block_col_indices.push(bc as u32);
                    block_values.extend_from_slice(&block);
                }
            }
            block_row_offsets.push(block_col_indices.len() as u32);
        }
        Self {
            n,
            block_row_offsets,
            block_col_indices,
            block_values,
        }
    }

    /// 已存储的非零块数
    pub fn stored_block_count(&self) -> usize {
        self.block_col_indices.len()
    }
}

impl SynapticMatVec for BlockSparseCsr {
    fn neuron_count(&self) -> usize {
        self.n
    }

    fn weight(&self, post: usize, pre: usize) -> i16 {
        let (br, bc) = (post / SPARSE_BLOCK_DIM, pre / SPARSE_BLOCK_DIM);
        let start = self.block_row_offsets[br] as usize;
        let end = self.block_row_offsets[br + 1] as usize;
        match self.block_col_indices[start..end].binary_search(&(bc as u32)) {
            Ok(k) => {
                let block = start + k;
                let local = (post % SPARSE_BLOCK_DIM) * SPARSE_BLOCK_DIM + pre % SPARSE_BLOCK_DIM;
                self.block_values[block * SPARSE_BLOCK_DIM * SPARSE_BLOCK_DIM + local]
            }
            Err(_) => 0,
        }
    }

    fn matvec_accumulate_q8(&self, x_q8: &[i16], y: &mut [i32]) {
        assert_eq!(x_q8.len(), self.n);
        assert_eq!(y.len(), self.n);
        for br in 0..self.block_row_offsets.len() - 1 {
            let start = self.block_row_offsets[br] as usize;
            let end = self.block_row_offsets[br + 1] as usize;
            for block in start..end {
                let bc = self.block_col_indices[block] as usize;
                let values = &self.block_values
                    [block * SPARSE_BLOCK_DIM * SPARSE_BLOCK_DIM..(block + 1) * SPARSE_BLOCK_DIM * SPARSE_BLOCK_DIM];
                for ir in 0..SPARSE_BLOCK_DIM {
                    let post = br * SPARSE_BLOCK_DIM + ir;
                    if post >= self.n {
                        break;
                    }
                    let mut acc = 0i32;
                    for ic in 0..SPARSE_BLOCK_DIM {
                        let pre = bc * SPARSE_BLOCK_DIM + ic;
                        if pre >= self.n {
                            break;
                        }
                        acc += syn_term(values[ir * SPARSE_BLOCK_DIM + ic], x_q8[pre]);
                    }
                    y[post] += acc;
                }
            }
        }
    }
}

/// 每神经元 32 槽位的动态邻接页
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PagedDynamicAdjacency {
    n: usize,
    /// `slots[post * 32 + s] = (pre, weight)`；`pre == u16::MAX` 表示空槽
    slots: Vec<(u16, i16)>,
}

impl PagedDynamicAdjacency {
    pub fn new(n: usize) -> Result<Self, RynvekErrorCode> {
        if n >= EMPTY_SLOT as usize {
            return Err(RynvekErrorCode::NeuralTopologyCapacityExceeded);
        }
        Ok(Self {
            n,
            slots: vec![(EMPTY_SLOT, 0); n * SYNAPSE_SLOTS_PER_NEURON],
        })
    }

    pub fn from_dense(n: usize, dense: &[i16]) -> Result<Self, RynvekErrorCode> {
        assert_eq!(dense.len(), n * n, "稠密矩阵尺寸不匹配");
        let mut m = Self::new(n)?;
        for post in 0..n {
            for pre in 0..n {
                let w = dense[post * n + pre];
                if w != 0 {
                    m.add_synapse(post, pre, w)?;
                }
            }
        }
        Ok(m)
    }

    fn page(&self, post: usize) -> &[(u16, i16)] {
        &self.slots[post * SYNAPSE_SLOTS_PER_NEURON..(post + 1) * SYNAPSE_SLOTS_PER_NEURON]
    }

    fn page_mut(&mut self, post: usize) -> &mut [(u16, i16)] {
        &mut self.slots[post * SYNAPSE_SLOTS_PER_NEURON..(post + 1) * SYNAPSE_SLOTS_PER_NEURON]
    }

    /// 新增或覆盖突触；页满时返回 `NeuralTopologyCapacityExceeded`
    pub fn add_synapse(&mut self, post: usize, pre: usize, w: i16) -> Result<(), RynvekErrorCode> {
        if post >= self.n || pre >= self.n {
            return Err(RynvekErrorCode::DimensionMismatch);
        }
        let pre16 = pre as u16;
        let page = self.page_mut(post);
        if let Some(slot) = page.iter_mut().find(|s| s.0 == pre16) {
            slot.1 = w;
            return Ok(());
        }
        // 取第一个空槽，保证槽位分配次序确定
        match page.iter_mut().find(|s| s.0 == EMPTY_SLOT) {
            Some(slot) => {
                *slot = (pre16, w);
                Ok(())
            }
            None => Err(RynvekErrorCode::NeuralTopologyCapacityExceeded),
        }
    }

    /// 删除突触，返回是否存在
    pub fn remove_synapse(&mut self, post: usize, pre: usize) -> bool {
        if post >= self.n {
            return false;
        }
        let pre16 = pre as u16;
        match self.page_mut(post).iter_mut().find(|s| s.0 == pre16) {
            Some(slot) => {
                *slot = (EMPTY_SLOT, 0);
                true
            }
            None => false,
        }
    }

    pub fn synapse_count(&self, post: usize) -> usize {
        self.page(post).iter().filter(|s| s.0 != EMPTY_SLOT).count()
    }
}

impl SynapticMatVec for PagedDynamicAdjacency {
    fn neuron_count(&self) -> usize {
        self.n
    }

    fn weight(&self, post: usize, pre: usize) -> i16 {
        self.page(post).iter().find(|s| s.0 == pre as u16).map_or(0, |s| s.1)
    }

    fn matvec_accumulate_q8(&self, x_q8: &[i16], y: &mut [i32]) {
        assert_eq!(x_q8.len(), self.n);
        assert_eq!(y.len(), self.n);
        for (post, out) in y.iter_mut().enumerate() {
            let mut acc = 0i32;
            for &(pre, w) in self.page(post) {
                if pre != EMPTY_SLOT {
                    acc += syn_term(w, x_q8[pre as usize]);
                }
            }
            *out += acc;
        }
    }
}

/// 统一封装的神经图存储
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NeuralGraphStorage {
    DenseTiledMatrix(DenseTiledMatrix),
    BlockSparseCSR(BlockSparseCsr),
    PagedDynamicAdjacency(PagedDynamicAdjacency),
}

impl NeuralGraphStorage {
    /// 按存储类别由稠密矩阵构建
    pub fn build(class: TopologyStorageClass, n: usize, dense: &[i16]) -> Result<Self, RynvekErrorCode> {
        Ok(match class {
            TopologyStorageClass::DenseTiledMatrix => Self::DenseTiledMatrix(DenseTiledMatrix::from_dense(n, dense)),
            TopologyStorageClass::BlockSparseCSR => Self::BlockSparseCSR(BlockSparseCsr::from_dense(n, dense)),
            TopologyStorageClass::PagedDynamicAdjacency => {
                Self::PagedDynamicAdjacency(PagedDynamicAdjacency::from_dense(n, dense)?)
            }
        })
    }

    pub fn class(&self) -> TopologyStorageClass {
        match self {
            Self::DenseTiledMatrix(_) => TopologyStorageClass::DenseTiledMatrix,
            Self::BlockSparseCSR(_) => TopologyStorageClass::BlockSparseCSR,
            Self::PagedDynamicAdjacency(_) => TopologyStorageClass::PagedDynamicAdjacency,
        }
    }

    fn inner(&self) -> &dyn SynapticMatVec {
        match self {
            Self::DenseTiledMatrix(m) => m,
            Self::BlockSparseCSR(m) => m,
            Self::PagedDynamicAdjacency(m) => m,
        }
    }
}

impl SynapticMatVec for NeuralGraphStorage {
    fn neuron_count(&self) -> usize {
        self.inner().neuron_count()
    }

    fn weight(&self, post: usize, pre: usize) -> i16 {
        self.inner().weight(post, pre)
    }

    fn matvec_accumulate_q8(&self, x_q8: &[i16], y: &mut [i32]) {
        self.inner().matvec_accumulate_q8(x_q8, y);
    }
}

/// 突触前神经元的 Dale 符号
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaleSign {
    Excitatory,
    Inhibitory,
}

/// 校验 Dale 定律：兴奋性神经元的出边权重 `>= 0`，抑制性 `<= 0`
pub fn validate_dale_law(storage: &impl SynapticMatVec, signs: &[DaleSign]) -> Result<(), RynvekErrorCode> {
    let n = storage.neuron_count();
    if signs.len() != n {
        return Err(RynvekErrorCode::DimensionMismatch);
    }
    for post in 0..n {
        for (pre, sign) in signs.iter().enumerate() {
            let w = storage.weight(post, pre);
            let ok = match sign {
                DaleSign::Excitatory => w >= 0,
                DaleSign::Inhibitory => w <= 0,
            };
            if !ok {
                return Err(RynvekErrorCode::DaleLawSignViolation);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rynvek_core::rng::RandomKey;

    /// 生成稀疏的确定性随机矩阵（约 1/4 非零）
    fn random_sparse_matrix(n: usize, seed: u64) -> Vec<i16> {
        (0..n * n)
            .map(|i| {
                let r = RandomKey {
                    world_seed: seed,
                    tick: 0,
                    entity_serial: i as u64,
                    stream_and_phase: 0,
                    sub_counter: 0,
                }
                .generate_u32x4();
                if r[0] % 4 == 0 { (r[1] % 1024) as i16 - 512 } else { 0 }
            })
            .collect()
    }

    #[test]
    fn storage_class_selection_follows_gap08() {
        assert_eq!(
            select_storage_class(46, false),
            Ok(TopologyStorageClass::DenseTiledMatrix)
        );
        assert_eq!(
            select_storage_class(64, false),
            Ok(TopologyStorageClass::DenseTiledMatrix)
        );
        assert_eq!(
            select_storage_class(70, false),
            Ok(TopologyStorageClass::BlockSparseCSR)
        );
        assert_eq!(
            select_storage_class(128, true),
            Ok(TopologyStorageClass::PagedDynamicAdjacency)
        );
        assert_eq!(
            select_storage_class(4097, false),
            Err(RynvekErrorCode::NeuralTopologyCapacityExceeded)
        );
    }

    #[test]
    fn three_layouts_produce_bit_identical_matvec() {
        // n=70 非 8/16 整数倍，覆盖填充边界；稀疏度保证每行 ≤ 32 个突触概率极高，失败时换种子
        let n = 70;
        let dense = random_sparse_matrix(n, 0x5EED);
        let x: Vec<i16> = (0..n).map(|i| ((i as i16 * 37) % 512) - 256).collect();

        let mut expected = vec![0i32; n];
        for post in 0..n {
            for pre in 0..n {
                expected[post] += syn_term(dense[post * n + pre], x[pre]);
            }
        }

        for class in [
            TopologyStorageClass::DenseTiledMatrix,
            TopologyStorageClass::BlockSparseCSR,
            TopologyStorageClass::PagedDynamicAdjacency,
        ] {
            let storage = NeuralGraphStorage::build(class, n, &dense).unwrap();
            let mut y = vec![0i32; n];
            storage.matvec_accumulate_q8(&x, &mut y);
            assert_eq!(y, expected, "{class:?} 结果与参照不一致");
            for post in 0..n {
                for pre in 0..n {
                    assert_eq!(storage.weight(post, pre), dense[post * n + pre]);
                }
            }
        }
    }

    #[test]
    fn block_sparse_skips_empty_blocks() {
        let n = 32;
        let mut dense = vec![0i16; n * n];
        dense[0] = 1; // 仅块 (0,0) 非零
        dense[31 * n + 31] = -1; // 仅块 (3,3) 非零
        let m = BlockSparseCsr::from_dense(n, &dense);
        assert_eq!(m.stored_block_count(), 2);
    }

    #[test]
    fn paged_adjacency_add_remove_and_capacity() {
        let mut m = PagedDynamicAdjacency::new(40).unwrap();
        for pre in 0..SYNAPSE_SLOTS_PER_NEURON {
            m.add_synapse(0, pre, 1).unwrap();
        }
        assert_eq!(m.synapse_count(0), 32);
        assert_eq!(
            m.add_synapse(0, 33, 1),
            Err(RynvekErrorCode::NeuralTopologyCapacityExceeded)
        );
        // 覆盖已存在突触不占新槽
        m.add_synapse(0, 5, 9).unwrap();
        assert_eq!(m.weight(0, 5), 9);
        assert!(m.remove_synapse(0, 5));
        m.add_synapse(0, 33, 2).unwrap();
        assert_eq!(m.weight(0, 33), 2);
    }

    #[test]
    fn dale_law_validation_detects_sign_flip() {
        let n = 2;
        let dense = vec![0, -5, 7, 0]; // post0 ← pre1 = -5；post1 ← pre0 = 7
        let m = DenseTiledMatrix::from_dense(n, &dense);
        assert!(validate_dale_law(&m, &[DaleSign::Excitatory, DaleSign::Inhibitory]).is_ok());
        assert_eq!(
            validate_dale_law(&m, &[DaleSign::Excitatory, DaleSign::Excitatory]),
            Err(RynvekErrorCode::DaleLawSignViolation)
        );
    }
}
