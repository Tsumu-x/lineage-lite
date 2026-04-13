pub mod dot;
pub mod html;
pub mod table;

use std::io::Write;

use crate::error::Result;
use crate::graph::node::{LineageEdge, Node};

/// lineage 結果的輸出格式。
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Table,
    Dot,
    Html,
}

/// 渲染 lineage 資料的 trait。
pub trait Renderer {
    fn render_edges(&self, edges: &[&LineageEdge], writer: &mut dyn Write) -> Result<()>;
    fn render_nodes(&self, nodes: &[&Node], writer: &mut dyn Write) -> Result<()>;
}

/// 取得指定格式的 renderer（不包含 Html，因為 Html 需要完整的 graph）。
pub fn get_renderer(format: &OutputFormat) -> Box<dyn Renderer> {
    match format {
        OutputFormat::Table => Box::new(table::TableRenderer),
        OutputFormat::Dot => Box::new(dot::DotRenderer),
        OutputFormat::Html => Box::new(table::TableRenderer), // fallback，實際 Html 在 CLI 中特殊處理
    }
}
