use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::error::Result;
use crate::graph::node::{ColumnLineage, EdgeRelation, NodeId, NodeKind};
use crate::graph::LineageGraph;
use crate::output::{get_renderer, html::render_graph_html, OutputFormat};
use crate::scanner::{ScanOrchestrator, ScanResult};
use crate::storage::sqlite::SqliteStorage;
use crate::storage::{ScanMetadata, StorageBackend};

#[derive(Parser)]
#[command(name = "lineage-lite")]
#[command(about = "Static lineage analysis engine for data governance")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 掃描目錄中的 SQL/dbt/Python 檔案並建立 lineage graph
    Scan {
        /// 掃描的根目錄
        #[arg(default_value = ".")]
        path: PathBuf,

        /// 輸出格式
        #[arg(short, long, value_enum, default_value = "table")]
        format: OutputFormat,

        /// 輸出檔案路徑（未指定則輸出到 stdout）
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// 顯示某張表變更後所有受影響的下游節點
    Impact {
        /// 要分析的表名（例如 raw.payments）
        table: String,

        /// 掃描的根目錄
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// 輸出格式
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,

        /// 輸出檔案路徑（未指定則輸出到 stdout）
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// 顯示某張表的上游與下游鄰域
    Show {
        /// 要檢視的表名
        table: String,

        /// 顯示上游的層數
        #[arg(long, default_value = "2")]
        upstream: usize,

        /// 顯示下游的層數
        #[arg(long, default_value = "2")]
        downstream: usize,

        /// 掃描的根目錄
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// 輸出格式
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,

        /// 輸出檔案路徑（未指定則輸出到 stdout）
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// 顯示 lineage graph 的統計資訊，可依欄位名稱過濾
    Stats {
        /// 根目錄
        #[arg(default_value = ".")]
        path: PathBuf,

        /// 依欄位名稱過濾（例如 --where column=email）
        #[arg(long, value_name = "column=NAME")]
        r#where: Option<String>,
    },

    /// 追蹤某張表的 column-level lineage — 每個欄位從哪來、怎麼算的
    Trace {
        /// 表名（例如 reports.daily_revenue）
        table: String,

        /// 掃描的根目錄
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// dbt compiled 目錄（例如 target/compiled/）— 用來追蹤 dbt model 的 column lineage
        #[arg(long)]
        compiled: Option<PathBuf>,
    },

    /// 比較兩次 scan 結果的差異（CI 整合用）
    Diff {
        /// 基準 scan 結果（SQLite 檔案）
        base: PathBuf,

        /// 目前的 scan 根目錄
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// 驗證 data contract — 檢查 lineage graph 是否符合 policy 規則
    Check {
        /// 掃描的根目錄
        #[arg(default_value = ".")]
        path: PathBuf,

        /// 規則檔案路徑
        #[arg(short, long, default_value = ".lineage-rules.toml")]
        rules: PathBuf,
    },

    /// 合併多個 scan 結果（跨 repo lineage）
    Merge {
        /// 要合併的 SQLite 檔案
        files: Vec<PathBuf>,

        /// 輸出檔案
        #[arg(short, long, default_value = "merged-lineage.db")]
        out: PathBuf,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Scan { path, format, out } => cmd_scan(&path, &format, out.as_deref()),
        Commands::Impact {
            table,
            path,
            format,
            out,
        } => cmd_impact(&path, &table, &format, out.as_deref()),
        Commands::Show {
            table,
            upstream,
            downstream,
            path,
            format,
            out,
        } => cmd_show(&path, &table, upstream, downstream, &format, out.as_deref()),
        Commands::Stats { path, r#where } => cmd_stats(&path, r#where.as_deref()),
        Commands::Trace { table, path, compiled } => cmd_trace(&path, &table, compiled.as_deref()),
        Commands::Diff { base, path } => cmd_diff(&base, &path),
        Commands::Check { path, rules } => cmd_check(&path, &rules),
        Commands::Merge { files, out } => cmd_merge(&files, &out),
    }
}

/// 掃描結果：包含 graph、檔案數、column lineage。
struct BuildResult {
    graph: LineageGraph,
    files_scanned: u64,
    col_lineage: Vec<ColumnLineage>,
}

/// 掃描目錄，建立 graph，可選擇性地儲存到 SQLite。
fn build_graph(path: &Path) -> Result<BuildResult> {
    // 這是 CLI 和核心 library 之間最重要的組裝點：
    // scanner 負責提取 edge，graph 負責把 edge 變成可查詢的結構。
    let orchestrator = ScanOrchestrator::default_scanners();
    let ScanResult {
        edges,
        files_scanned,
        column_map,
        col_lineage,
        ..
    } = orchestrator.scan_directory(path)?;

    eprintln!(
        "Scanned {} files, found {} edges",
        files_scanned,
        edges.len()
    );

    let mut graph = LineageGraph::new();

    for edge in &edges {
        let (source_kind, target_kind) = infer_node_kinds(&edge.relation);
        // 先補節點再加邊，是因為 petgraph 內部真正連線時用的是 NodeIndex，
        // 所以 source / target 必須先存在於 graph 中。
        graph.ensure_node(&edge.source, source_kind, &edge.source_file);
        graph.ensure_node(&edge.target, target_kind, &edge.source_file);
    }

    for edge in edges {
        graph.add_edge(edge)?;
    }

    // 將 SQL 中提取的欄位資訊合併到對應的 node
    for (table, columns) in &column_map {
        let node_id = NodeId::from(table.as_str());
        graph.add_columns(&node_id, columns);
    }

    eprintln!(
        "Built graph: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    Ok(BuildResult { graph, files_scanned, col_lineage })
}

fn open_writer(out: Option<&Path>) -> Result<Box<dyn Write>> {
    match out {
        Some(path) => {
            let file = File::create(path)?;
            Ok(Box::new(file))
        }
        None => Ok(Box::new(std::io::stdout())),
    }
}

fn cmd_scan(path: &Path, format: &OutputFormat, out: Option<&Path>) -> Result<()> {
    let BuildResult { graph, files_scanned, col_lineage } = build_graph(path)?;

    // scan 指令有兩種出口：
    // 1. render 成 table / dot / html 給人看
    // 2. 存成 SQLite 快照，給 diff / merge / CI 再利用
    // 這兩條路都建立在同一份 graph 上。
    // 若輸出路徑副檔名為 .db 或 .sqlite，則儲存為 SQLite 資料庫
    if let Some(out_path) = out {
        if matches!(
            out_path.extension().and_then(|e| e.to_str()),
            Some("db" | "sqlite" | "sqlite3")
        ) {
            let storage = SqliteStorage::new(out_path);
            let metadata = ScanMetadata::new(path, files_scanned, &graph);
            storage.save(&graph, &metadata)?;
            eprintln!("Saved to {}", out_path.display());
            return Ok(());
        }
    }

    // HTML 格式需要完整 graph 資訊
    if matches!(format, OutputFormat::Html) {
        let mut writer = open_writer(out)?;
        render_graph_html(&graph, &col_lineage, &mut writer)?;
        if let Some(p) = out {
            eprintln!("HTML 已輸出至 {}", p.display());
        }
        return Ok(());
    }

    let renderer = get_renderer(format);
    let mut writer = open_writer(out)?;
    let edges: Vec<_> = graph.edges().into_iter().collect();
    renderer.render_edges(&edges, &mut writer)?;

    Ok(())
}

fn cmd_impact(path: &Path, table: &str, format: &OutputFormat, out: Option<&Path>) -> Result<()> {
    let BuildResult { graph, .. } = build_graph(path)?;
    let node_id = NodeId::from(table);

    let impacted = graph.impact(&node_id)?;
    eprintln!(
        "Impact analysis for '{}': {} downstream nodes",
        table,
        impacted.len()
    );

    let renderer = get_renderer(format);
    let mut writer = open_writer(out)?;
    renderer.render_nodes(&impacted, &mut writer)?;

    Ok(())
}

fn cmd_show(
    path: &Path,
    table: &str,
    up: usize,
    down: usize,
    format: &OutputFormat,
    out: Option<&Path>,
) -> Result<()> {
    let BuildResult { graph, .. } = build_graph(path)?;
    let node_id = NodeId::from(table);

    let upstream = graph.upstream(&node_id, Some(up))?;
    let downstream = graph.downstream(&node_id, Some(down))?;

    let renderer = get_renderer(format);
    let mut writer = open_writer(out)?;

    if !upstream.is_empty() {
        eprintln!("Upstream ({} levels, {} nodes):", up, upstream.len());
        renderer.render_nodes(&upstream, &mut writer)?;
    }

    if let Some(node) = graph.get_node(&node_id) {
        eprintln!("\nTarget: {} ({})", node.id, node.kind);
    }

    if !downstream.is_empty() {
        eprintln!("\nDownstream ({} levels, {} nodes):", down, downstream.len());
        renderer.render_nodes(&downstream, &mut writer)?;
    }

    Ok(())
}

fn cmd_stats(path: &Path, where_clause: Option<&str>) -> Result<()> {
    let BuildResult { graph, .. } = build_graph(path)?;

    // 解析 --where column=<name> 格式
    let column_filter = where_clause.and_then(|w| {
        w.strip_prefix("column=").or_else(|| w.strip_prefix("column:"))
    });

    let report = graph.stats(column_filter);

    let mut stdout = std::io::stdout();

    // 總覽
    writeln!(stdout, "\n=== Lineage Graph Stats ===")?;
    writeln!(stdout, "Nodes: {}", report.total_nodes)?;
    writeln!(stdout, "Edges: {}", report.total_edges)?;
    writeln!(stdout)?;

    // 依類型分組
    writeln!(stdout, "Nodes by kind:")?;
    let mut kinds: Vec<_> = report.nodes_by_kind.iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(a.1));
    for (kind, count) in &kinds {
        writeln!(stdout, "  {kind}: {count}")?;
    }

    // 欄位匹配結果
    if let Some(filter) = column_filter {
        writeln!(stdout)?;
        if report.column_matches.is_empty() {
            writeln!(stdout, "No nodes found with column matching '{filter}'.")?;
        } else {
            writeln!(
                stdout,
                "Nodes containing column '{}': {}",
                filter,
                report.column_matches.len()
            )?;

            let mut table = comfy_table::Table::new();
            table.load_preset(comfy_table::presets::UTF8_FULL_CONDENSED);
            table.set_header(vec!["Node", "Kind", "Matched Columns", "Source File"]);

            for m in &report.column_matches {
                table.add_row(vec![
                    m.node_id.to_string(),
                    format!("{}", m.kind),
                    m.matched_columns.join(", "),
                    m.source_file.clone(),
                ]);
            }
            writeln!(stdout, "{table}")?;
        }
    }

    Ok(())
}

fn cmd_trace(path: &Path, table: &str, compiled_dir: Option<&Path>) -> Result<()> {
    let BuildResult { mut col_lineage, .. } = build_graph(path)?;

    // 若有 --compiled 或自動偵測 target/compiled/，從 compiled SQL 額外提取 column lineage
    let compiled_path = compiled_dir
        .map(|p| p.to_path_buf())
        .or_else(|| {
            let auto = path.join("target/compiled");
            if auto.is_dir() { Some(auto) } else { None }
        });

    if let Some(cp) = &compiled_path {
        eprintln!("讀取 dbt compiled SQL: {}", cp.display());
        let extra = scan_compiled_dir(cp);
        eprintln!("從 compiled SQL 提取了 {} 筆 column lineage", extra.len());
        col_lineage.extend(extra);
    }

    let table_lower = table.to_lowercase();
    let matches: Vec<_> = col_lineage
        .iter()
        .filter(|cl| cl.target_table == table_lower)
        .collect();

    if matches.is_empty() {
        eprintln!("找不到表 '{}' 的 column-level lineage。", table);
        eprintln!("提示：使用 --compiled 指定 dbt compile 產出目錄，或確認 target/compiled/ 存在。");
        return Ok(());
    }

    let mut stdout = std::io::stdout();
    writeln!(stdout)?;
    writeln!(stdout, "=== Column Lineage: {} ===", table)?;
    writeln!(stdout)?;

    let mut tbl = comfy_table::Table::new();
    tbl.load_preset(comfy_table::presets::UTF8_FULL_CONDENSED);
    tbl.set_header(vec!["欄位", "轉換", "來源", "SQL 表達式"]);

    for cl in &matches {
        let sources_str = if cl.source_columns.is_empty() {
            "（無）".to_string()
        } else {
            cl.source_columns
                .iter()
                .map(|s| match &s.table {
                    Some(t) => format!("{}.{}", t, s.column),
                    None => s.column.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ")
        };

        let expr_display = if cl.expression.len() > 60 {
            format!("{}…", &cl.expression[..57])
        } else {
            cl.expression.clone()
        };

        tbl.add_row(vec![
            cl.target_column.clone(),
            cl.transform.to_string(),
            sources_str,
            expr_display,
        ]);
    }

    writeln!(stdout, "{tbl}")?;
    writeln!(stdout)?;
    writeln!(
        stdout,
        "共 {} 個欄位（{} 直接映射，{} 聚合，{} 表達式/其他）",
        matches.len(),
        matches.iter().filter(|c| matches!(c.transform, crate::graph::node::TransformKind::Direct)).count(),
        matches.iter().filter(|c| matches!(c.transform, crate::graph::node::TransformKind::Aggregation(_))).count(),
        matches.iter().filter(|c| !matches!(c.transform,
            crate::graph::node::TransformKind::Direct | crate::graph::node::TransformKind::Aggregation(_)
        )).count(),
    )?;

    Ok(())
}

/// 掃描 dbt compiled 目錄中的純 SQL 檔案，提取 column lineage。
fn scan_compiled_dir(dir: &Path) -> Vec<ColumnLineage> {
    use crate::scanner::sql::extract_column_lineage;
    let mut result = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("sql"))
    {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            result.extend(extract_column_lineage(&content));
        }
    }
    result
}

// ===== diff 指令 =====

fn cmd_diff(base_path: &Path, current_path: &Path) -> Result<()> {
    // 載入基準
    let storage = SqliteStorage::new(base_path);
    let (base_graph, _) = storage.load()?;

    // 建立目前的 graph
    let BuildResult { graph: current_graph, .. } = build_graph(current_path)?;

    let base_nodes: std::collections::HashSet<String> = base_graph.nodes().iter().map(|n| n.id.0.clone()).collect();
    let curr_nodes: std::collections::HashSet<String> = current_graph.nodes().iter().map(|n| n.id.0.clone()).collect();

    let added_nodes: Vec<_> = curr_nodes.difference(&base_nodes).collect();
    let removed_nodes: Vec<_> = base_nodes.difference(&curr_nodes).collect();

    let base_edges: std::collections::HashSet<String> = base_graph.edges().iter()
        .map(|e| format!("{} -> {}", e.source, e.target)).collect();
    let curr_edges: std::collections::HashSet<String> = current_graph.edges().iter()
        .map(|e| format!("{} -> {}", e.source, e.target)).collect();

    let added_edges: Vec<_> = curr_edges.difference(&base_edges).collect();
    let removed_edges: Vec<_> = base_edges.difference(&curr_edges).collect();

    let mut stdout = std::io::stdout();
    writeln!(stdout)?;
    writeln!(stdout, "=== Lineage Diff ===")?;
    writeln!(stdout, "Base: {} ({} nodes, {} edges)", base_path.display(), base_graph.node_count(), base_graph.edge_count())?;
    writeln!(stdout, "Current: {} ({} nodes, {} edges)", current_path.display(), current_graph.node_count(), current_graph.edge_count())?;
    writeln!(stdout)?;

    if added_nodes.is_empty() && removed_nodes.is_empty() && added_edges.is_empty() && removed_edges.is_empty() {
        writeln!(stdout, "沒有差異。")?;
        return Ok(());
    }

    if !added_nodes.is_empty() {
        writeln!(stdout, "新增 {} 個節點:", added_nodes.len())?;
        for n in &added_nodes { writeln!(stdout, "  + {n}")?; }
        writeln!(stdout)?;
    }
    if !removed_nodes.is_empty() {
        writeln!(stdout, "移除 {} 個節點:", removed_nodes.len())?;
        for n in &removed_nodes { writeln!(stdout, "  - {n}")?; }
        writeln!(stdout)?;
    }
    if !added_edges.is_empty() {
        writeln!(stdout, "新增 {} 條邊:", added_edges.len())?;
        for e in &added_edges { writeln!(stdout, "  + {e}")?; }
        writeln!(stdout)?;
    }
    if !removed_edges.is_empty() {
        writeln!(stdout, "移除 {} 條邊:", removed_edges.len())?;
        for e in &removed_edges { writeln!(stdout, "  - {e}")?; }
    }

    // 輸出 exit code: 有差異時回傳 1（方便 CI 判斷）
    if !added_nodes.is_empty() || !removed_nodes.is_empty() || !added_edges.is_empty() || !removed_edges.is_empty() {
        writeln!(stdout)?;
        writeln!(stdout, "⚠ 偵測到 lineage 變更，請確認影響範圍。")?;
    }

    Ok(())
}

// ===== check 指令（Data Contract 驗證） =====

fn cmd_check(path: &Path, rules_path: &Path) -> Result<()> {
    let BuildResult { graph, col_lineage, .. } = build_graph(path)?;

    // 讀取規則檔案
    let rules_content = match std::fs::read_to_string(rules_path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("找不到規則檔案: {}", rules_path.display());
            eprintln!("請建立 .lineage-rules.toml，範例：");
            eprintln!();
            eprintln!("  [[rules]]");
            eprintln!("  name = \"pii-isolation\"");
            eprintln!("  type = \"column-deny\"");
            eprintln!("  columns = [\"email\", \"phone\", \"id_number\"]");
            eprintln!("  denied_schemas = [\"reports\", \"analysis\", \"exports\"]");
            eprintln!("  message = \"PII 欄位不應出現在此 schema\"");
            return Ok(());
        }
    };

    let mut stdout = std::io::stdout();
    writeln!(stdout)?;
    writeln!(stdout, "=== Data Contract Check ===")?;
    writeln!(stdout)?;

    let mut violations = 0u32;

    // 簡易規則解析（支援 column-deny 類型）
    for line in rules_content.lines() {
        let line = line.trim();
        if line.starts_with("columns") {
            // 解析 columns = ["email", "phone"]
            if let Some(cols_str) = line.split('=').nth(1) {
                let cols: Vec<String> = cols_str
                    .replace(['[', ']', '"', '\'', ' '], "")
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_lowercase())
                    .collect();

                // 取得 denied_schemas
                let denied: Vec<String> = rules_content.lines()
                    .find(|l| l.trim().starts_with("denied_schemas"))
                    .and_then(|l| l.split('=').nth(1))
                    .map(|s| {
                        s.replace(['[', ']', '"', '\'', ' '], "")
                            .split(',')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_lowercase())
                            .collect()
                    })
                    .unwrap_or_default();

                if cols.is_empty() || denied.is_empty() { continue; }

                // 檢查：col_lineage 中的 target_table 是否在 denied schema 裡且包含 PII 欄位
                for cl in &col_lineage {
                    let schema = cl.target_table.split('.').next().unwrap_or("");
                    if denied.iter().any(|d| d == schema) && cols.contains(&cl.target_column) {
                        writeln!(stdout, "  ✗ VIOLATION: {}.{} — PII 欄位出現在禁止的 schema",
                            cl.target_table, cl.target_column)?;
                        violations += 1;
                    }
                }

                // 也檢查 graph nodes 的 columns
                for node in graph.nodes() {
                    let schema = node.id.0.split('.').next().unwrap_or("");
                    if denied.iter().any(|d| d == schema) {
                        for col in &node.columns {
                            if cols.contains(col) {
                                writeln!(stdout, "  ✗ VIOLATION: {}.{} — PII 欄位出現在禁止的 schema",
                                    node.id, col)?;
                                violations += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    writeln!(stdout)?;
    if violations == 0 {
        writeln!(stdout, "✓ 所有規則通過，共 0 個違規。")?;
    } else {
        writeln!(stdout, "✗ 發現 {} 個違規。", violations)?;
    }

    Ok(())
}

// ===== merge 指令（跨 repo lineage） =====

fn cmd_merge(files: &[PathBuf], out: &Path) -> Result<()> {
    let mut merged = LineageGraph::new();

    for file in files {
        eprintln!("載入 {}...", file.display());
        let storage = SqliteStorage::new(file);
        let (graph, _) = storage.load()?;

        for node in graph.nodes() {
            merged.add_node(node.clone());
        }
        for edge in graph.edges() {
            // 確保節點存在
            let (sk, tk) = infer_node_kinds(&edge.relation);
            merged.ensure_node(&edge.source, sk, &edge.source_file);
            merged.ensure_node(&edge.target, tk, &edge.source_file);
            let _ = merged.add_edge(edge.clone());
        }
    }

    let metadata = ScanMetadata {
        scanned_at: "merged".to_string(),
        root_path: "multiple".to_string(),
        file_count: files.len() as u64,
        node_count: merged.node_count(),
        edge_count: merged.edge_count(),
    };

    let out_storage = SqliteStorage::new(out);
    out_storage.save(&merged, &metadata)?;

    eprintln!(
        "合併完成: {} 個檔案 → {} nodes, {} edges → {}",
        files.len(), merged.node_count(), merged.edge_count(), out.display()
    );

    Ok(())
}

/// 根據 edge relation 推斷 node 類型。
fn infer_node_kinds(relation: &EdgeRelation) -> (NodeKind, NodeKind) {
    match relation {
        EdgeRelation::DbtRef => (NodeKind::DbtModel, NodeKind::DbtModel),
        EdgeRelation::DbtSource => (NodeKind::DbtSource, NodeKind::DbtModel),
        EdgeRelation::PythonReadWrite => (NodeKind::SqlTable, NodeKind::PythonEtl),
        EdgeRelation::CreateTableAs | EdgeRelation::InsertInto => {
            (NodeKind::SqlTable, NodeKind::SqlTable)
        }
        EdgeRelation::SelectFrom | EdgeRelation::JoinOn | EdgeRelation::CteReference => {
            (NodeKind::SqlTable, NodeKind::SqlTable)
        }
    }
}
