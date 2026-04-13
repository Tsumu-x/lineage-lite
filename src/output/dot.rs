use std::collections::HashSet;
use std::io::Write;

use crate::error::Result;
use crate::graph::node::{LineageEdge, Node};

use super::Renderer;

/// 以 Graphviz DOT 格式渲染 lineage graph。
pub struct DotRenderer;

impl Renderer for DotRenderer {
    fn render_edges(&self, edges: &[&LineageEdge], writer: &mut dyn Write) -> Result<()> {
        writeln!(writer, "digraph lineage {{")?;
        writeln!(writer, "    rankdir=LR;")?;
        writeln!(writer, "    node [shape=box, style=filled, fillcolor=lightyellow];")?;
        writeln!(writer)?;

        // 收集唯一的節點名稱用於宣告
        let mut nodes = HashSet::new();
        for edge in edges {
            nodes.insert(&edge.source.0);
            nodes.insert(&edge.target.0);
        }

        for node_name in &nodes {
            writeln!(writer, "    \"{}\" [label=\"{}\"];", node_name, node_name)?;
        }
        writeln!(writer)?;

        for edge in edges {
            writeln!(
                writer,
                "    \"{}\" -> \"{}\" [label=\"{}\"];",
                edge.source.0, edge.target.0, edge.relation
            )?;
        }

        writeln!(writer, "}}")?;
        Ok(())
    }

    fn render_nodes(&self, nodes: &[&Node], writer: &mut dyn Write) -> Result<()> {
        writeln!(writer, "digraph lineage {{")?;
        writeln!(writer, "    rankdir=LR;")?;
        writeln!(writer, "    node [shape=box, style=filled, fillcolor=lightyellow];")?;
        writeln!(writer)?;

        for node in nodes {
            writeln!(
                writer,
                "    \"{}\" [label=\"{}\\n({})\"];",
                node.id.0, node.id.0, node.kind
            )?;
        }

        writeln!(writer, "}}")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::node::{EdgeRelation, NodeId};
    use std::path::PathBuf;

    #[test]
    fn test_dot_edges_output() {
        let edge = LineageEdge {
            source: NodeId::from("raw.orders"),
            target: NodeId::from("stg.orders"),
            relation: EdgeRelation::CreateTableAs,
            source_file: PathBuf::from("test.sql"),
            line_number: None,
        };

        let renderer = DotRenderer;
        let mut buf = Vec::new();
        renderer.render_edges(&[&edge], &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("digraph lineage"));
        assert!(output.contains("\"raw.orders\" -> \"stg.orders\""));
        assert!(output.contains("CREATE TABLE AS"));
    }
}
