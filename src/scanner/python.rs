use std::path::Path;

use regex::Regex;

use crate::error::Result;
use crate::graph::node::{EdgeRelation, LineageEdge, NodeId};

use super::Scanner;

/// 透過尋找 read_sql / to_sql / saveAsTable 等 pattern，
/// 從 Python ETL 腳本中提取 lineage 的掃描器。
pub struct PythonScanner;

impl Scanner for PythonScanner {
    fn extensions(&self) -> &[&str] {
        &["py"]
    }

    fn scan_file(&self, path: &Path, content: &str) -> Result<Vec<LineageEdge>> {
        let node_name = python_node_name(path);
        let mut edges = Vec::new();

        // 來源（讀取）：pd.read_sql("table", ...) 或 read_sql_table("table", ...)
        let read_patterns = [
            Regex::new(r#"read_sql(?:_table|_query)?\s*\(\s*["']([^"']+)["']"#).unwrap(),
            Regex::new(r#"spark\.table\s*\(\s*["']([^"']+)["']"#).unwrap(),
            Regex::new(r#"\.sql\s*\(\s*["'](?i:SELECT\s.*?FROM\s+)([a-zA-Z_][\w.]*)"#).unwrap(),
        ];

        for re in &read_patterns {
            for cap in re.captures_iter(content) {
                let table = &cap[1];
                // 跳過不是表名的原始 SQL 字串
                if looks_like_table_name(table) {
                    edges.push(LineageEdge {
                        source: NodeId::from(table),
                        target: NodeId(node_name.clone()),
                        relation: EdgeRelation::PythonReadWrite,
                        source_file: path.to_path_buf(),
                        line_number: byte_offset_to_line(content, cap.get(0).unwrap().start()),
                    });
                }
            }
        }

        // 目標（寫入）：.to_sql("table", ...) 或 .saveAsTable("table")
        let write_patterns = [
            Regex::new(r#"\.to_sql\s*\(\s*["']([^"']+)["']"#).unwrap(),
            Regex::new(r#"\.saveAsTable\s*\(\s*["']([^"']+)["']"#).unwrap(),
            Regex::new(r#"\.insertInto\s*\(\s*["']([^"']+)["']"#).unwrap(),
            Regex::new(r#"\.write\b.*?\.save\s*\(\s*["']([^"']+)["']"#).unwrap(),
        ];

        for re in &write_patterns {
            for cap in re.captures_iter(content) {
                let table = &cap[1];
                if looks_like_table_name(table) {
                    edges.push(LineageEdge {
                        source: NodeId(node_name.clone()),
                        target: NodeId::from(table),
                        relation: EdgeRelation::PythonReadWrite,
                        source_file: path.to_path_buf(),
                        line_number: byte_offset_to_line(content, cap.get(0).unwrap().start()),
                    });
                }
            }
        }

        Ok(edges)
    }
}

/// 從 Python 檔案路徑推導節點名稱。
/// 例如 `etl/sync_payments.py` → `etl/sync_payments`
fn python_node_name(path: &Path) -> String {
    path.with_extension("")
        .to_string_lossy()
        .to_string()
        .to_lowercase()
}

/// 檢查字串是否看起來像表名（非完整 SQL 查詢或檔案路徑）。
fn looks_like_table_name(s: &str) -> bool {
    !s.contains(' ') && !s.contains('/') && !s.contains('\\') && s.len() < 128
}

fn byte_offset_to_line(content: &str, offset: usize) -> Option<usize> {
    Some(content[..offset].chars().filter(|&c| c == '\n').count() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(py: &str) -> Vec<LineageEdge> {
        let scanner = PythonScanner;
        scanner
            .scan_file(Path::new("etl/sync_job.py"), py)
            .expect("scan failed")
    }

    #[test]
    fn test_pandas_read_sql() {
        let edges = scan(r#"df = pd.read_sql("raw.events", con=engine)"#);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, NodeId::from("raw.events"));
        assert_eq!(edges[0].target.0, "etl/sync_job");
    }

    #[test]
    fn test_pandas_to_sql() {
        let edges = scan(r#"df.to_sql("staging.events", con=engine, if_exists="replace")"#);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source.0, "etl/sync_job");
        assert_eq!(edges[0].target, NodeId::from("staging.events"));
    }

    #[test]
    fn test_spark_save_as_table() {
        let edges = scan(r#"df.write.mode("overwrite").saveAsTable("mart.orders")"#);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].target, NodeId::from("mart.orders"));
    }

    #[test]
    fn test_spark_table_read() {
        let edges = scan(r#"df = spark.table("raw.events")"#);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, NodeId::from("raw.events"));
    }

    #[test]
    fn test_spark_insert_into() {
        let edges = scan(r#"df.write.insertInto("staging.events")"#);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].target, NodeId::from("staging.events"));
    }

    #[test]
    fn test_full_etl_script() {
        let script = r#"
import pandas as pd
from sqlalchemy import create_engine

engine = create_engine("postgresql://...")

# Read
orders = pd.read_sql("raw.orders", con=engine)
payments = pd.read_sql("raw.payments", con=engine)

# Transform
merged = orders.merge(payments, on="order_id")

# Write
merged.to_sql("staging.order_payments", con=engine, if_exists="replace")
"#;
        let edges = scan(script);
        assert_eq!(edges.len(), 3);

        let sources: Vec<_> = edges
            .iter()
            .filter(|e| e.target.0 == "etl/sync_job")
            .map(|e| e.source.0.as_str())
            .collect();
        assert!(sources.contains(&"raw.orders"));
        assert!(sources.contains(&"raw.payments"));

        let sinks: Vec<_> = edges
            .iter()
            .filter(|e| e.source.0 == "etl/sync_job")
            .map(|e| e.target.0.as_str())
            .collect();
        assert!(sinks.contains(&"staging.order_payments"));
    }

    #[test]
    fn test_no_matches() {
        let edges = scan("print('hello world')");
        assert!(edges.is_empty());
    }

    #[test]
    fn test_read_sql_query() {
        let edges = scan(r#"df = pd.read_sql_query("raw.analytics", engine)"#);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, NodeId::from("raw.analytics"));
    }
}
