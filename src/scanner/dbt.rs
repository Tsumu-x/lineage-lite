use std::path::Path;

use regex::Regex;

use crate::error::Result;
use crate::graph::node::{EdgeRelation, LineageEdge, NodeId};

use super::Scanner;

/// 透過尋找 ref() 和 source() 呼叫，從 dbt model 檔案中提取 lineage 的掃描器。
pub struct DbtScanner;

impl Scanner for DbtScanner {
    fn extensions(&self) -> &[&str] {
        // dbt model 也是 .sql 檔案，但透過內容（Jinja 語法）來偵測
        // ScanOrchestrator 會對 .sql 檔案嘗試所有 scanner
        &["sql"]
    }

    fn scan_file(&self, path: &Path, content: &str) -> Result<Vec<LineageEdge>> {
        // 只處理包含 dbt Jinja 語法的檔案
        if !content.contains("{{") {
            return Ok(Vec::new());
        }

        let target_id = model_name_from_path(path);
        let mut edges = Vec::new();

        // Extract ref() calls: {{ ref('model_name') }}
        let ref_re = Regex::new(r#"\{\{\s*ref\(\s*['"]([\w.]+)['"]\s*\)\s*\}\}"#).unwrap();
        for cap in ref_re.captures_iter(content) {
            let model_name = &cap[1];
            edges.push(LineageEdge {
                source: NodeId::from(model_name),
                target: NodeId(target_id.clone()),
                relation: EdgeRelation::DbtRef,
                source_file: path.to_path_buf(),
                line_number: byte_offset_to_line(content, cap.get(0).unwrap().start()),
            });
        }

        // Extract source() calls: {{ source('source_name', 'table_name') }}
        let source_re = Regex::new(
            r#"\{\{\s*source\(\s*['"]([\w.]+)['"],\s*['"]([\w.]+)['"]\s*\)\s*\}\}"#,
        )
        .unwrap();
        for cap in source_re.captures_iter(content) {
            let source_name = &cap[1];
            let table_name = &cap[2];
            let source_id = format!("{}.{}", source_name, table_name);
            edges.push(LineageEdge {
                source: NodeId::from(source_id.as_str()),
                target: NodeId(target_id.clone()),
                relation: EdgeRelation::DbtSource,
                source_file: path.to_path_buf(),
                line_number: byte_offset_to_line(content, cap.get(0).unwrap().start()),
            });
        }

        Ok(edges)
    }
}

/// 從檔案路徑推導 model 名稱。
/// 例如 `models/staging/stg_orders.sql` → `stg_orders`
fn model_name_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_lowercase()
}

/// 將 byte offset 轉換為 1-based 行號。
fn byte_offset_to_line(content: &str, offset: usize) -> Option<usize> {
    Some(content[..offset].chars().filter(|&c| c == '\n').count() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(sql: &str) -> Vec<LineageEdge> {
        let scanner = DbtScanner;
        scanner
            .scan_file(Path::new("models/staging/stg_orders.sql"), sql)
            .expect("scan failed")
    }

    #[test]
    fn test_ref_single() {
        let edges = scan("SELECT * FROM {{ ref('raw_orders') }}");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, NodeId::from("raw_orders"));
        assert_eq!(edges[0].target.0, "stg_orders");
        assert_eq!(edges[0].relation, EdgeRelation::DbtRef);
    }

    #[test]
    fn test_ref_double_quotes() {
        let edges = scan(r#"SELECT * FROM {{ ref("raw_orders") }}"#);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, NodeId::from("raw_orders"));
    }

    #[test]
    fn test_multiple_refs() {
        let edges = scan(
            "SELECT o.*, p.amount \
             FROM {{ ref('stg_orders') }} o \
             JOIN {{ ref('stg_payments') }} p ON o.id = p.order_id",
        );
        assert_eq!(edges.len(), 2);
        let sources: Vec<_> = edges.iter().map(|e| e.source.0.as_str()).collect();
        assert!(sources.contains(&"stg_orders"));
        assert!(sources.contains(&"stg_payments"));
    }

    #[test]
    fn test_source() {
        let edges = scan("SELECT * FROM {{ source('raw', 'orders') }}");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, NodeId::from("raw.orders"));
        assert_eq!(edges[0].relation, EdgeRelation::DbtSource);
    }

    #[test]
    fn test_mixed_ref_and_source() {
        let edges = scan(
            "SELECT * FROM {{ source('raw', 'orders') }} o \
             JOIN {{ ref('dim_users') }} u ON o.user_id = u.id",
        );
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn test_no_jinja_returns_empty() {
        let edges = scan("SELECT * FROM raw.orders");
        assert!(edges.is_empty());
    }

    #[test]
    fn test_line_numbers() {
        let edges = scan("-- comment\nSELECT * FROM {{ ref('raw_orders') }}");
        assert_eq!(edges[0].line_number, Some(2));
    }
}
