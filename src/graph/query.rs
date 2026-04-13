use std::collections::HashMap;

use super::node::{NodeKind, NodeId};
use super::LineageGraph;

/// 統計報表。
#[derive(Debug)]
pub struct StatsReport {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub nodes_by_kind: HashMap<String, usize>,
    pub column_matches: Vec<ColumnMatch>,
}

/// 某個 node 中包含指定欄位的匹配結果。
#[derive(Debug)]
pub struct ColumnMatch {
    pub node_id: NodeId,
    pub kind: NodeKind,
    pub matched_columns: Vec<String>,
    pub source_file: String,
}

impl LineageGraph {
    /// 產生統計報表，可選擇性地依欄位名稱過濾。
    pub fn stats(&self, column_filter: Option<&str>) -> StatsReport {
        let mut nodes_by_kind: HashMap<String, usize> = HashMap::new();
        let mut column_matches = Vec::new();

        for node in self.nodes() {
            *nodes_by_kind
                .entry(format!("{}", node.kind))
                .or_insert(0) += 1;

            if let Some(filter) = column_filter {
                let filter_lower = filter.to_lowercase();
                let matched: Vec<String> = node
                    .columns
                    .iter()
                    .filter(|c| c.contains(&filter_lower))
                    .cloned()
                    .collect();

                if !matched.is_empty() {
                    column_matches.push(ColumnMatch {
                        node_id: node.id.clone(),
                        kind: node.kind.clone(),
                        matched_columns: matched,
                        source_file: node.source_file.display().to_string(),
                    });
                }
            }
        }

        StatsReport {
            total_nodes: self.node_count(),
            total_edges: self.edge_count(),
            nodes_by_kind,
            column_matches,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::node::{EdgeRelation, LineageEdge, Node};
    use std::path::PathBuf;

    fn build_graph_with_columns() -> LineageGraph {
        let mut g = LineageGraph::new();
        g.add_node(Node {
            id: NodeId::from("raw.users"),
            kind: NodeKind::SqlTable,
            source_file: PathBuf::from("users.sql"),
            columns: vec!["id".into(), "email".into(), "name".into()],
        });
        g.add_node(Node {
            id: NodeId::from("raw.orders"),
            kind: NodeKind::SqlTable,
            source_file: PathBuf::from("orders.sql"),
            columns: vec!["id".into(), "amount".into(), "user_id".into()],
        });
        g.add_node(Node {
            id: NodeId::from("mart.customers"),
            kind: NodeKind::DbtModel,
            source_file: PathBuf::from("customers.sql"),
            columns: vec!["user_id".into(), "email".into(), "total_orders".into()],
        });
        g.add_edge(LineageEdge {
            source: NodeId::from("raw.users"),
            target: NodeId::from("mart.customers"),
            relation: EdgeRelation::DbtRef,
            source_file: PathBuf::from("customers.sql"),
            line_number: None,
        })
        .unwrap();
        g
    }

    #[test]
    fn test_stats_without_filter() {
        let g = build_graph_with_columns();
        let report = g.stats(None);
        assert_eq!(report.total_nodes, 3);
        assert_eq!(report.total_edges, 1);
        assert_eq!(*report.nodes_by_kind.get("SQL Table").unwrap(), 2);
        assert_eq!(*report.nodes_by_kind.get("dbt Model").unwrap(), 1);
        assert!(report.column_matches.is_empty());
    }

    #[test]
    fn test_stats_filter_email() {
        let g = build_graph_with_columns();
        let report = g.stats(Some("email"));
        assert_eq!(report.column_matches.len(), 2);
        let matched_nodes: Vec<&str> = report
            .column_matches
            .iter()
            .map(|m| m.node_id.0.as_str())
            .collect();
        assert!(matched_nodes.contains(&"raw.users"));
        assert!(matched_nodes.contains(&"mart.customers"));
    }

    #[test]
    fn test_stats_filter_no_match() {
        let g = build_graph_with_columns();
        let report = g.stats(Some("phone"));
        assert!(report.column_matches.is_empty());
    }
}
