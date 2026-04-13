pub mod dbt;
pub mod python;
pub mod sql;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::Result;
use crate::graph::node::{ColumnLineage, LineageEdge};

/// 掃描原始碼檔案並提取 lineage edges 的 trait。
pub trait Scanner: Send + Sync {
    /// 此 scanner 處理的副檔名（例如 ["sql"]）。
    fn extensions(&self) -> &[&str];

    /// 掃描單一檔案，回傳所有發現的 lineage edges。
    fn scan_file(&self, path: &Path, content: &str) -> Result<Vec<LineageEdge>>;
}

/// 負責遍歷目錄樹，將檔案分派給對應 scanner 處理。
pub struct ScanOrchestrator {
    scanners: Vec<Box<dyn Scanner>>,
}

impl ScanOrchestrator {
    pub fn new(scanners: Vec<Box<dyn Scanner>>) -> Self {
        Self { scanners }
    }

    /// 建立包含所有內建 scanner 的預設 orchestrator。
    pub fn default_scanners() -> Self {
        Self::new(vec![
            Box::new(dbt::DbtScanner),
            Box::new(sql::SqlScanner),
            Box::new(python::PythonScanner),
        ])
    }

    /// 掃描目錄樹，回傳所有發現的 edges、欄位資訊、以及掃描的檔案數。
    pub fn scan_directory(&self, root: &Path) -> Result<ScanResult> {
        let mut edges = Vec::new();
        let mut files_scanned = 0u64;
        let mut column_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut col_lineage: Vec<ColumnLineage> = Vec::new();

        // 這個函式是整個掃描流程的主幹：
        // 走訪目錄 -> 找到適合的 scanner -> 收集 table-level edge
        // -> 對 SQL 額外補 column 資訊與 column lineage。
        for entry in WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

            let matching_scanners = self.find_scanners(ext);
            if matching_scanners.is_empty() {
                continue;
            }

            let content = std::fs::read_to_string(path)?;
            files_scanned += 1;

            for scanner in matching_scanners {
                let file_edges = scanner.scan_file(path, &content)?;
                edges.extend(file_edges);
            }

            // table-level lineage 是透過 Scanner trait 統一抽象出來的；
            // 但 column-level lineage 目前只有 SQL 走 AST 才能 reasonably 提取，
            // 所以這裡用額外分支補上第二層分析結果。
            if ext.eq_ignore_ascii_case("sql") {
                let cols = sql::extract_columns_from_sql(&content);
                for (table, columns) in cols {
                    column_map
                        .entry(table)
                        .or_default()
                        .extend(columns);
                }
                col_lineage.extend(sql::extract_column_lineage(&content));
            }
        }

        Ok(ScanResult {
            edges,
            files_scanned,
            root: root.to_path_buf(),
            column_map,
            col_lineage,
        })
    }

    /// 找出所有能處理指定副檔名的 scanner。
    fn find_scanners(&self, ext: &str) -> Vec<&dyn Scanner> {
        self.scanners
            .iter()
            .filter(|s| s.extensions().iter().any(|e| e.eq_ignore_ascii_case(ext)))
            .map(|s| s.as_ref())
            .collect()
    }
}

/// 目錄掃描的結果。
pub struct ScanResult {
    pub edges: Vec<LineageEdge>,
    pub files_scanned: u64,
    pub root: PathBuf,
    /// 表名 → 欄位名稱列表（從 SQL SELECT list 提取）。
    pub column_map: HashMap<String, Vec<String>>,
    /// Column-level lineage（每個 output column 的來源追蹤）。
    pub col_lineage: Vec<ColumnLineage>,
}
