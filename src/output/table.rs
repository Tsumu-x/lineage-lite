use std::io::Write;

use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};

use crate::error::Result;
use crate::graph::node::{LineageEdge, Node};

use super::Renderer;

pub struct TableRenderer;

impl Renderer for TableRenderer {
    fn render_edges(&self, edges: &[&LineageEdge], writer: &mut dyn Write) -> Result<()> {
        if edges.is_empty() {
            writeln!(writer, "No edges found.")?;
            return Ok(());
        }

        let mut table = Table::new();
        table.load_preset(UTF8_FULL_CONDENSED);
        table.set_header(vec!["Source", "Target", "Relation", "File"]);

        for edge in edges {
            table.add_row(vec![
                edge.source.to_string(),
                edge.target.to_string(),
                edge.relation.to_string(),
                edge.source_file.display().to_string(),
            ]);
        }

        writeln!(writer, "{table}")?;
        Ok(())
    }

    fn render_nodes(&self, nodes: &[&Node], writer: &mut dyn Write) -> Result<()> {
        if nodes.is_empty() {
            writeln!(writer, "No nodes found.")?;
            return Ok(());
        }

        let mut table = Table::new();
        table.load_preset(UTF8_FULL_CONDENSED);
        table.set_header(vec!["Node", "Kind", "Source File"]);

        for node in nodes {
            table.add_row(vec![
                node.id.to_string(),
                node.kind.to_string(),
                node.source_file.display().to_string(),
            ]);
        }

        writeln!(writer, "{table}")?;
        Ok(())
    }
}
