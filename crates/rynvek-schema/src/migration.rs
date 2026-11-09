// crates/rynvek-schema/src/migration.rs
// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Schema 有向迁移图（`MigrationGraph`）最短升级路径搜索（规范 10 §10.3.2、GAP-28）。

use std::collections::{BTreeMap, VecDeque};

/// Schema 修订标识：`(领域名, 版本号)`
pub type SchemaRevisionId = (String, u32); // (domain_name, version_num)

/// 迁移图中的一条有向边：把 `from_ver` 版本的文档原地变换为 `to_ver` 版本
#[derive(Debug)]
pub struct MigrationEdge {
    pub from_ver: u32,
    pub to_ver: u32,
    /// 该迁移是否无信息损失
    pub is_lossless: bool,
    /// 原地变换函数；失败时返回人类可读的原因
    pub transform_fn: fn(&mut serde_json::Value) -> Result<(), String>,
}

/// 按领域分组的 Schema 迁移图
#[derive(Debug, Default)]
pub struct SchemaMigrationGraph {
    pub edges_by_domain: BTreeMap<String, Vec<MigrationEdge>>,
}

impl SchemaMigrationGraph {
    /// 向指定领域追加一条迁移边，返回其在该领域边表中的下标
    pub fn add_edge(&mut self, domain: &str, edge: MigrationEdge) -> usize {
        let edges = self.edges_by_domain.entry(domain.to_owned()).or_default();
        edges.push(edge);
        edges.len() - 1
    }

    /// 使用确定性 BFS 寻找从存档版本 `from_ver` 到当前引擎版本 `target_ver` 的最短无损迁移路径
    pub fn find_migration_path(&self, domain: &str, from_ver: u32, target_ver: u32) -> Result<Vec<usize>, String> {
        if from_ver == target_ver {
            return Ok(Vec::new());
        }
        let edges = self
            .edges_by_domain
            .get(domain)
            .ok_or_else(|| format!("Unknown schema domain: {domain}"))?;

        let mut queue = VecDeque::new();
        let mut parent_map: BTreeMap<u32, (u32, usize)> = BTreeMap::new();
        queue.push_back(from_ver);

        while let Some(curr) = queue.pop_front() {
            if curr == target_ver {
                let mut path = Vec::new();
                let mut trace = target_ver;
                while trace != from_ver {
                    let &(prev, edge_idx) = parent_map.get(&trace).unwrap();
                    path.push(edge_idx);
                    trace = prev;
                }
                path.reverse();
                return Ok(path);
            }
            // 按边的注册顺序遍历，保证同长度路径的选择在所有平台上完全一致
            for (idx, edge) in edges.iter().enumerate() {
                if edge.from_ver == curr && !parent_map.contains_key(&edge.to_ver) {
                    parent_map.insert(edge.to_ver, (curr, idx));
                    queue.push_back(edge.to_ver);
                }
            }
        }
        Err(format!(
            "No valid schema migration path for {domain} from v{from_ver} to v{target_ver}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop(_: &mut serde_json::Value) -> Result<(), String> {
        Ok(())
    }

    fn edge(from_ver: u32, to_ver: u32) -> MigrationEdge {
        MigrationEdge {
            from_ver,
            to_ver,
            is_lossless: true,
            transform_fn: noop,
        }
    }

    fn sample_graph() -> SchemaMigrationGraph {
        let mut graph = SchemaMigrationGraph::default();
        graph.add_edge("rynvek.world", edge(1, 2)); // 0
        graph.add_edge("rynvek.world", edge(2, 3)); // 1
        graph.add_edge("rynvek.world", edge(3, 4)); // 2
        graph.add_edge("rynvek.world", edge(1, 3)); // 3：捷径
        graph
    }

    #[test]
    fn same_version_needs_no_migration() {
        assert_eq!(sample_graph().find_migration_path("rynvek.world", 2, 2), Ok(vec![]));
    }

    #[test]
    fn bfs_prefers_shortest_path() {
        assert_eq!(sample_graph().find_migration_path("rynvek.world", 1, 4), Ok(vec![3, 2]));
        assert_eq!(sample_graph().find_migration_path("rynvek.world", 2, 4), Ok(vec![1, 2]));
    }

    #[test]
    fn missing_path_and_domain_are_errors() {
        let graph = sample_graph();
        assert!(graph.find_migration_path("rynvek.world", 4, 1).is_err());
        assert_eq!(
            graph.find_migration_path("rynvek.unknown", 1, 2),
            Err("Unknown schema domain: rynvek.unknown".to_owned())
        );
    }
}
