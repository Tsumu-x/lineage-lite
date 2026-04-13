pub mod sqlite;

use std::path::Path;

use crate::error::Result;
use crate::graph::LineageGraph;

/// 持久化與載入 lineage graph 的 trait。
pub trait StorageBackend {
    fn save(&self, graph: &LineageGraph, metadata: &ScanMetadata) -> Result<()>;
    fn load(&self) -> Result<(LineageGraph, ScanMetadata)>;
}

/// 掃描執行的 metadata。
#[derive(Debug, Clone)]
pub struct ScanMetadata {
    pub scanned_at: String,
    pub root_path: String,
    pub file_count: u64,
    pub node_count: usize,
    pub edge_count: usize,
}

impl ScanMetadata {
    pub fn new(root_path: &Path, file_count: u64, graph: &LineageGraph) -> Self {
        Self {
            scanned_at: chrono_now(),
            root_path: root_path.display().to_string(),
            file_count,
            node_count: graph.node_count(),
            edge_count: graph.edge_count(),
        }
    }
}

fn chrono_now() -> String {
    // 不依賴 chrono 的簡易 ISO-8601 時間戳
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}
