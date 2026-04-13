use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::error::{LineageError, Result};
use crate::graph::node::{EdgeRelation, LineageEdge, Node, NodeId, NodeKind};
use crate::graph::LineageGraph;

use super::{ScanMetadata, StorageBackend};

/// 基於 SQLite 的 lineage graph 儲存後端。
pub struct SqliteStorage {
    path: PathBuf,
}

impl SqliteStorage {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn open(&self) -> Result<Connection> {
        let conn = Connection::open(&self.path).map_err(LineageError::Storage)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(LineageError::Storage)?;
        Ok(conn)
    }

    fn create_tables(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                source_file TEXT NOT NULL,
                columns_json TEXT DEFAULT '[]'
            );
            CREATE TABLE IF NOT EXISTS edges (
                source_id TEXT NOT NULL REFERENCES nodes(id),
                target_id TEXT NOT NULL REFERENCES nodes(id),
                relation TEXT NOT NULL,
                source_file TEXT NOT NULL,
                line_number INTEGER,
                PRIMARY KEY (source_id, target_id, relation)
            );
            CREATE TABLE IF NOT EXISTS scan_metadata (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scanned_at TEXT NOT NULL,
                root_path TEXT NOT NULL,
                file_count INTEGER NOT NULL,
                node_count INTEGER NOT NULL,
                edge_count INTEGER NOT NULL
            );",
        )
        .map_err(LineageError::Storage)?;
        Ok(())
    }
}

impl StorageBackend for SqliteStorage {
    fn save(&self, graph: &LineageGraph, metadata: &ScanMetadata) -> Result<()> {
        let conn = self.open()?;
        Self::create_tables(&conn)?;

        // 清除先前的資料
        conn.execute_batch("DELETE FROM edges; DELETE FROM nodes; DELETE FROM scan_metadata;")
            .map_err(LineageError::Storage)?;

        // 寫入節點
        let mut node_stmt = conn
            .prepare(
                "INSERT INTO nodes (id, kind, source_file, columns_json) VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(LineageError::Storage)?;

        for node in graph.nodes() {
            let columns_json = serde_json::to_string(&node.columns).unwrap_or_default();
            node_stmt
                .execute(params![
                    node.id.0,
                    format!("{:?}", node.kind),
                    node.source_file.display().to_string(),
                    columns_json,
                ])
                .map_err(LineageError::Storage)?;
        }

        // 寫入邊
        let mut edge_stmt = conn
            .prepare(
                "INSERT OR IGNORE INTO edges (source_id, target_id, relation, source_file, line_number) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(LineageError::Storage)?;

        for edge in graph.edges() {
            edge_stmt
                .execute(params![
                    edge.source.0,
                    edge.target.0,
                    format!("{:?}", edge.relation),
                    edge.source_file.display().to_string(),
                    edge.line_number,
                ])
                .map_err(LineageError::Storage)?;
        }

        // 寫入 metadata
        conn.execute(
            "INSERT INTO scan_metadata (scanned_at, root_path, file_count, node_count, edge_count) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                metadata.scanned_at,
                metadata.root_path,
                metadata.file_count,
                metadata.node_count,
                metadata.edge_count,
            ],
        )
        .map_err(LineageError::Storage)?;

        Ok(())
    }

    fn load(&self) -> Result<(LineageGraph, ScanMetadata)> {
        let conn = self.open()?;

        let mut graph = LineageGraph::new();

        // 載入節點
        let mut node_stmt = conn
            .prepare("SELECT id, kind, source_file, columns_json FROM nodes")
            .map_err(LineageError::Storage)?;

        let nodes = node_stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let kind_str: String = row.get(1)?;
                let source_file: String = row.get(2)?;
                let columns_json: String = row.get::<_, String>(3).unwrap_or_default();
                Ok((id, kind_str, source_file, columns_json))
            })
            .map_err(LineageError::Storage)?;

        for node_result in nodes {
            let (id, kind_str, source_file, columns_json) =
                node_result.map_err(LineageError::Storage)?;
            let kind = parse_node_kind(&kind_str);
            let columns: Vec<String> =
                serde_json::from_str(&columns_json).unwrap_or_default();
            graph.add_node(Node {
                id: NodeId(id),
                kind,
                source_file: PathBuf::from(source_file),
                columns,
            });
        }

        // 載入邊
        let mut edge_stmt = conn
            .prepare("SELECT source_id, target_id, relation, source_file, line_number FROM edges")
            .map_err(LineageError::Storage)?;

        let edges = edge_stmt
            .query_map([], |row| {
                let source: String = row.get(0)?;
                let target: String = row.get(1)?;
                let relation: String = row.get(2)?;
                let source_file: String = row.get(3)?;
                let line_number: Option<usize> = row.get(4)?;
                Ok((source, target, relation, source_file, line_number))
            })
            .map_err(LineageError::Storage)?;

        for edge_result in edges {
            let (source, target, relation_str, source_file, line_number) =
                edge_result.map_err(LineageError::Storage)?;
            let relation = parse_edge_relation(&relation_str);
            graph.add_edge(LineageEdge {
                source: NodeId(source),
                target: NodeId(target),
                relation,
                source_file: PathBuf::from(source_file),
                line_number,
            })?;
        }

        // 載入 metadata
        let metadata = conn
            .query_row(
                "SELECT scanned_at, root_path, file_count, node_count, edge_count \
                 FROM scan_metadata ORDER BY id DESC LIMIT 1",
                [],
                |row| {
                    Ok(ScanMetadata {
                        scanned_at: row.get(0)?,
                        root_path: row.get(1)?,
                        file_count: row.get(2)?,
                        node_count: row.get(3)?,
                        edge_count: row.get(4)?,
                    })
                },
            )
            .map_err(LineageError::Storage)?;

        Ok((graph, metadata))
    }
}

fn parse_node_kind(s: &str) -> NodeKind {
    match s {
        "SqlView" => NodeKind::SqlView,
        "DbtModel" => NodeKind::DbtModel,
        "DbtSource" => NodeKind::DbtSource,
        "PythonEtl" => NodeKind::PythonEtl,
        _ => NodeKind::SqlTable,
    }
}

fn parse_edge_relation(s: &str) -> EdgeRelation {
    match s {
        "JoinOn" => EdgeRelation::JoinOn,
        "InsertInto" => EdgeRelation::InsertInto,
        "CreateTableAs" => EdgeRelation::CreateTableAs,
        "CteReference" => EdgeRelation::CteReference,
        "DbtRef" => EdgeRelation::DbtRef,
        "DbtSource" => EdgeRelation::DbtSource,
        "PythonReadWrite" => EdgeRelation::PythonReadWrite,
        _ => EdgeRelation::SelectFrom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn build_test_graph() -> LineageGraph {
        let mut g = LineageGraph::new();
        g.add_node(Node {
            id: NodeId::from("raw.orders"),
            kind: NodeKind::SqlTable,
            source_file: PathBuf::from("test.sql"),
            columns: vec!["id".to_string(), "amount".to_string()],
        });
        g.add_node(Node {
            id: NodeId::from("stg.orders"),
            kind: NodeKind::SqlTable,
            source_file: PathBuf::from("test.sql"),
            columns: Vec::new(),
        });
        g.add_edge(LineageEdge {
            source: NodeId::from("raw.orders"),
            target: NodeId::from("stg.orders"),
            relation: EdgeRelation::CreateTableAs,
            source_file: PathBuf::from("test.sql"),
            line_number: Some(1),
        })
        .unwrap();
        g
    }

    #[test]
    fn test_sqlite_round_trip() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path());

        let graph = build_test_graph();
        let metadata = ScanMetadata {
            scanned_at: "12345".to_string(),
            root_path: "/test".to_string(),
            file_count: 1,
            node_count: 2,
            edge_count: 1,
        };

        storage.save(&graph, &metadata).unwrap();

        let (loaded_graph, loaded_meta) = storage.load().unwrap();
        assert_eq!(loaded_graph.node_count(), 2);
        assert_eq!(loaded_graph.edge_count(), 1);
        assert_eq!(loaded_meta.root_path, "/test");
        assert_eq!(loaded_meta.file_count, 1);

        // Verify nodes and columns
        let raw = loaded_graph.get_node(&NodeId::from("raw.orders")).unwrap();
        assert_eq!(raw.columns, vec!["id", "amount"]);

        // Verify downstream still works
        let downstream = loaded_graph
            .downstream(&NodeId::from("raw.orders"), None)
            .unwrap();
        assert_eq!(downstream.len(), 1);
        assert_eq!(downstream[0].id, NodeId::from("stg.orders"));
    }

    #[test]
    fn test_sqlite_overwrite_on_resave() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path());

        let graph = build_test_graph();
        let metadata = ScanMetadata {
            scanned_at: "111".to_string(),
            root_path: "/a".to_string(),
            file_count: 1,
            node_count: 2,
            edge_count: 1,
        };

        storage.save(&graph, &metadata).unwrap();
        storage.save(&graph, &metadata).unwrap();

        let (loaded, _) = storage.load().unwrap();
        assert_eq!(loaded.node_count(), 2);
    }
}
