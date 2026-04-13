pub mod node;
pub mod query;

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;

use crate::error::{LineageError, Result};
use node::{LineageEdge, Node, NodeId};

/// 核心 lineage DAG，封裝 petgraph 並提供以 NodeId 為索引的介面。
pub struct LineageGraph {
    graph: DiGraph<Node, LineageEdge>,
    index: HashMap<NodeId, NodeIndex>,
}

impl LineageGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index: HashMap::new(),
        }
    }

    /// 新增節點到 graph。若已存在相同 id 的節點，回傳既有的 index。
    pub fn add_node(&mut self, node: Node) -> NodeIndex {
        if let Some(&idx) = self.index.get(&node.id) {
            return idx;
        }
        let id = node.id.clone();
        let idx = self.graph.add_node(node);
        self.index.insert(id, idx);
        idx
    }

    /// 確保指定 id 的節點存在，若不存在則建立 placeholder。
    pub fn ensure_node(&mut self, id: &NodeId, kind: node::NodeKind, source_file: &std::path::Path) -> NodeIndex {
        if let Some(&idx) = self.index.get(id) {
            return idx;
        }
        // 掃描器先找到的是 edge，而不是完整 node 物件；
        // 因此建圖時要先補齊 source / target node，後面才能安全 add_edge。
        self.add_node(Node {
            id: id.clone(),
            kind,
            source_file: source_file.to_path_buf(),
            columns: Vec::new(),
        })
    }

    /// 新增一條邊到 graph。
    pub fn add_edge(&mut self, edge: LineageEdge) -> Result<()> {
        let source_idx = self.index.get(&edge.source).ok_or_else(|| {
            LineageError::NodeNotFound(edge.source.to_string())
        })?;
        let target_idx = self.index.get(&edge.target).ok_or_else(|| {
            LineageError::NodeNotFound(edge.target.to_string())
        })?;
        self.graph.add_edge(*source_idx, *target_idx, edge);
        Ok(())
    }

    /// 依 id 取得節點。
    pub fn get_node(&self, id: &NodeId) -> Option<&Node> {
        self.index.get(id).map(|&idx| &self.graph[idx])
    }

    /// 將欄位資訊合併到既有的節點（去重）。
    pub fn add_columns(&mut self, id: &NodeId, columns: &[String]) {
        if let Some(&idx) = self.index.get(id) {
            let node = &mut self.graph[idx];
            for col in columns {
                let col_lower = col.to_lowercase();
                if !node.columns.iter().any(|c| c == &col_lower) {
                    node.columns.push(col_lower);
                }
            }
        }
    }

    /// 回傳 graph 中所有的節點。
    pub fn nodes(&self) -> Vec<&Node> {
        self.graph.node_weights().collect()
    }

    /// 回傳 graph 中所有的邊。
    pub fn edges(&self) -> Vec<&LineageEdge> {
        self.graph.edge_weights().collect()
    }

    /// 回傳節點總數。
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// 回傳邊的總數。
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// BFS 往下游走——沿 outgoing edges 可達的所有節點。
    pub fn downstream(&self, id: &NodeId, max_depth: Option<usize>) -> Result<Vec<&Node>> {
        self.bfs_collect(id, Direction::Outgoing, max_depth)
    }

    /// BFS 往上游走——沿 incoming edges 可達的所有節點。
    pub fn upstream(&self, id: &NodeId, max_depth: Option<usize>) -> Result<Vec<&Node>> {
        self.bfs_collect(id, Direction::Incoming, max_depth)
    }

    /// Impact analysis：所有下游節點（等同 downstream 不限深度）。
    pub fn impact(&self, id: &NodeId) -> Result<Vec<&Node>> {
        self.downstream(id, None)
    }

    fn bfs_collect(
        &self,
        id: &NodeId,
        direction: Direction,
        max_depth: Option<usize>,
    ) -> Result<Vec<&Node>> {
        let &start = self.index.get(id).ok_or_else(|| {
            LineageError::NodeNotFound(id.to_string())
        })?;

        let mut result = Vec::new();
        let mut visited = HashMap::new();
        visited.insert(start, 0usize);

        // 這裡不用 petgraph 內建 BFS iterator，而是手動維護 queue，
        // 因為 show/upstream/downstream 需要 depth-aware 查詢。
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((start, 0usize));

        while let Some((current, depth)) = queue.pop_front() {
            if current != start {
                result.push(&self.graph[current]);
            }

            if let Some(max) = max_depth {
                if depth >= max {
                    continue;
                }
            }

            let neighbors = self.graph.neighbors_directed(current, direction);
            for neighbor in neighbors {
                // visited 不只是避免重複，也是在保證圖上多條路徑匯流時，
                // 同一個節點不會被重複加入結果。
                if let std::collections::hash_map::Entry::Vacant(e) = visited.entry(neighbor) {
                    e.insert(depth + 1);
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }

        Ok(result)
    }
}

impl Default for LineageGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use node::{EdgeRelation, NodeKind};
    use std::path::PathBuf;

    fn make_node(name: &str) -> Node {
        Node {
            id: NodeId::from(name),
            kind: NodeKind::SqlTable,
            source_file: PathBuf::from("test.sql"),
            columns: Vec::new(),
        }
    }

    fn make_edge(source: &str, target: &str) -> LineageEdge {
        LineageEdge {
            source: NodeId::from(source),
            target: NodeId::from(target),
            relation: EdgeRelation::SelectFrom,
            source_file: PathBuf::from("test.sql"),
            line_number: None,
        }
    }

    #[test]
    fn test_add_node_and_get() {
        let mut g = LineageGraph::new();
        g.add_node(make_node("raw.orders"));
        assert!(g.get_node(&NodeId::from("raw.orders")).is_some());
        assert!(g.get_node(&NodeId::from("raw.payments")).is_none());
    }

    #[test]
    fn test_duplicate_node_returns_same_index() {
        let mut g = LineageGraph::new();
        let idx1 = g.add_node(make_node("raw.orders"));
        let idx2 = g.add_node(make_node("raw.orders"));
        assert_eq!(idx1, idx2);
        assert_eq!(g.node_count(), 1);
    }

    #[test]
    fn test_add_edge() {
        let mut g = LineageGraph::new();
        g.add_node(make_node("raw.orders"));
        g.add_node(make_node("stg.orders"));
        g.add_edge(make_edge("raw.orders", "stg.orders")).unwrap();
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn test_edge_missing_node() {
        let mut g = LineageGraph::new();
        g.add_node(make_node("raw.orders"));
        let result = g.add_edge(make_edge("raw.orders", "missing"));
        assert!(result.is_err());
    }

    // raw.orders -> stg.orders -> mart.orders
    //                          -> mart.payments
    #[test]
    fn test_downstream_bfs() {
        let mut g = LineageGraph::new();
        g.add_node(make_node("raw.orders"));
        g.add_node(make_node("stg.orders"));
        g.add_node(make_node("mart.orders"));
        g.add_node(make_node("mart.payments"));
        g.add_edge(make_edge("raw.orders", "stg.orders")).unwrap();
        g.add_edge(make_edge("stg.orders", "mart.orders")).unwrap();
        g.add_edge(make_edge("stg.orders", "mart.payments")).unwrap();

        let downstream = g.downstream(&NodeId::from("raw.orders"), None).unwrap();
        assert_eq!(downstream.len(), 3);

        let downstream_1 = g.downstream(&NodeId::from("raw.orders"), Some(1)).unwrap();
        assert_eq!(downstream_1.len(), 1);
        assert_eq!(downstream_1[0].id, NodeId::from("stg.orders"));
    }

    #[test]
    fn test_upstream_bfs() {
        let mut g = LineageGraph::new();
        g.add_node(make_node("raw.orders"));
        g.add_node(make_node("raw.payments"));
        g.add_node(make_node("mart.orders"));
        g.add_edge(make_edge("raw.orders", "mart.orders")).unwrap();
        g.add_edge(make_edge("raw.payments", "mart.orders")).unwrap();

        let upstream = g.upstream(&NodeId::from("mart.orders"), None).unwrap();
        assert_eq!(upstream.len(), 2);
    }

    #[test]
    fn test_impact_is_full_downstream() {
        let mut g = LineageGraph::new();
        g.add_node(make_node("a"));
        g.add_node(make_node("b"));
        g.add_node(make_node("c"));
        g.add_edge(make_edge("a", "b")).unwrap();
        g.add_edge(make_edge("b", "c")).unwrap();

        let impact = g.impact(&NodeId::from("a")).unwrap();
        assert_eq!(impact.len(), 2);
    }
}
