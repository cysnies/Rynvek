// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 测试用的极简规则图构建器（对应 TS 侧 `Kernel.define` 的子集）。

#![allow(dead_code)]

use rynvek_rules::{ComponentDecl, GraphNode, RULE_GRAPH_FORMAT, UntypedRuleGraph, ValueType};

#[derive(Default)]
pub struct G {
    pub reads: Vec<ComponentDecl>,
    pub writes: Vec<ComponentDecl>,
    pub nodes: Vec<GraphNode>,
}

impl G {
    pub fn read(mut self, component: &str, ty: ValueType) -> Self {
        self.reads.push(ComponentDecl {
            component: component.into(),
            ty,
        });
        self
    }

    pub fn write(mut self, component: &str, ty: ValueType) -> Self {
        self.writes.push(ComponentDecl {
            component: component.into(),
            ty,
        });
        self
    }

    pub fn push(&mut self, node: GraphNode) -> u32 {
        self.nodes.push(node);
        (self.nodes.len() - 1) as u32
    }

    pub fn op(&mut self, op: &str, args: &[u32]) -> u32 {
        self.push(GraphNode {
            op: op.into(),
            args: args.to_vec(),
            ..GraphNode::default()
        })
    }

    pub fn konst(&mut self, op: &str, imm: impl ToString) -> u32 {
        self.push(GraphNode {
            op: op.into(),
            imm: Some(imm.to_string()),
            ..GraphNode::default()
        })
    }

    pub fn q8(&mut self, raw: i16) -> u32 {
        self.konst("ConstQ8_8", raw)
    }

    pub fn i32(&mut self, v: i32) -> u32 {
        self.konst("ConstI32", v)
    }

    pub fn comp(&mut self, op: &str, component: &str, args: &[u32]) -> u32 {
        self.push(GraphNode {
            op: op.into(),
            args: args.to_vec(),
            component: Some(component.into()),
            ..GraphNode::default()
        })
    }

    pub fn channel(&mut self, op: &str, channel: &str, args: &[u32]) -> u32 {
        self.push(GraphNode {
            op: op.into(),
            args: args.to_vec(),
            channel: Some(channel.into()),
            ..GraphNode::default()
        })
    }

    pub fn loop_begin(&mut self, iters: u32) -> u32 {
        self.push(GraphNode {
            op: "BoundedForBegin".into(),
            max_iters: Some(iters),
            ..GraphNode::default()
        })
    }

    /// Phi 的回边先占位，循环体构建完后用 `set_phi_next` 回填
    pub fn phi(&mut self, init: u32) -> u32 {
        self.op("Phi", &[init, u32::MAX])
    }

    pub fn set_phi_next(&mut self, phi: u32, next: u32) {
        self.nodes[phi as usize].args[1] = next;
    }

    pub fn loop_end(&mut self) -> u32 {
        self.op("BoundedForEnd", &[])
    }

    pub fn build(self, name: &str) -> UntypedRuleGraph {
        UntypedRuleGraph {
            format: RULE_GRAPH_FORMAT.into(),
            name: name.into(),
            reads: self.reads,
            writes: self.writes,
            nodes: self.nodes,
        }
    }
}
