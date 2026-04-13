use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// lineage graph 中的節點唯一識別碼（例如 "raw.payments"）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        Self(s.to_lowercase())
    }
}

/// 節點所代表的資料資產類型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    SqlTable,
    SqlView,
    DbtModel,
    DbtSource,
    PythonEtl,
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SqlTable => write!(f, "SQL Table"),
            Self::SqlView => write!(f, "SQL View"),
            Self::DbtModel => write!(f, "dbt Model"),
            Self::DbtSource => write!(f, "dbt Source"),
            Self::PythonEtl => write!(f, "Python ETL"),
        }
    }
}

/// lineage graph 中的節點。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub source_file: PathBuf,
    /// 已知的欄位名稱（從 SQL SELECT list 中 best-effort 提取）。
    #[serde(default)]
    pub columns: Vec<String>,
}

/// 兩個節點之間的關係類型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeRelation {
    SelectFrom,
    JoinOn,
    InsertInto,
    CreateTableAs,
    CteReference,
    DbtRef,
    DbtSource,
    PythonReadWrite,
}

impl fmt::Display for EdgeRelation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelectFrom => write!(f, "SELECT FROM"),
            Self::JoinOn => write!(f, "JOIN"),
            Self::InsertInto => write!(f, "INSERT INTO"),
            Self::CreateTableAs => write!(f, "CREATE TABLE AS"),
            Self::CteReference => write!(f, "CTE"),
            Self::DbtRef => write!(f, "dbt ref()"),
            Self::DbtSource => write!(f, "dbt source()"),
            Self::PythonReadWrite => write!(f, "Python read/write"),
        }
    }
}

/// lineage graph 中的有向邊：source（上游）→ target（下游）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageEdge {
    pub source: NodeId,
    pub target: NodeId,
    pub relation: EdgeRelation,
    pub source_file: PathBuf,
    pub line_number: Option<usize>,
}

// ===== Column-Level Lineage =====

/// 欄位的轉換類型：這個欄位是怎麼從上游產生的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransformKind {
    /// 直接 passthrough 或改名（e.g. `id AS order_id`）
    Direct,
    /// 聚合函數（e.g. `SUM(amount)`、`COUNT(DISTINCT order_id)`）
    Aggregation(String),
    /// 算式或 CASE（e.g. `a + b`、`CASE WHEN ... END`）
    Expression,
    /// Window function（e.g. `LAG(x, 12) OVER (...)`）
    Window(String),
    /// 來自 dbt macro 或 Jinja，無法追蹤細節
    Macro,
    /// 常數或不依賴任何上游欄位
    Constant,
    /// 無法判斷
    Unknown,
}

impl fmt::Display for TransformKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direct => write!(f, "direct"),
            Self::Aggregation(func) => write!(f, "{func}()"),
            Self::Expression => write!(f, "expression"),
            Self::Window(func) => write!(f, "{func}() OVER"),
            Self::Macro => write!(f, "macro"),
            Self::Constant => write!(f, "constant"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// 欄位級別的 lineage：一個 output column 是從哪些 source columns 經過什麼 transform 產生的。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnLineage {
    /// 目標表
    pub target_table: String,
    /// 目標欄位名稱
    pub target_column: String,
    /// 上游來源欄位（table.column 格式）
    pub source_columns: Vec<SourceColumn>,
    /// 轉換類型
    pub transform: TransformKind,
    /// 原始 SQL 表達式（best-effort）
    pub expression: String,
}

/// 上游來源欄位。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceColumn {
    /// 來源表（可能是 alias 或完整表名）
    pub table: Option<String>,
    /// 來源欄位名稱
    pub column: String,
}
