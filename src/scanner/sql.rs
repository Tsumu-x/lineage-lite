use std::collections::HashMap;
use std::path::Path;

use sqlparser::ast::{
    Query, Select, SelectItem, SetExpr, Statement, TableFactor, TableWithJoins, With,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use crate::error::Result;
use crate::graph::node::{ColumnLineage, EdgeRelation, LineageEdge, NodeId, SourceColumn, TransformKind};

use super::Scanner;

/// 使用 sqlparser-rs AST 分析從 SQL 檔案中提取 lineage 的掃描器。
pub struct SqlScanner;

impl Scanner for SqlScanner {
    fn extensions(&self) -> &[&str] {
        &["sql"]
    }

    fn scan_file(&self, path: &Path, content: &str) -> Result<Vec<LineageEdge>> {
        // 跳過含有 Jinja 語法的檔案——由 dbt scanner 處理
        if content.contains("{{") {
            return Ok(Vec::new());
        }

        let dialect = GenericDialect {};
        let statements = match Parser::parse_sql(&dialect, content) {
            Ok(stmts) => stmts,
            Err(_) => return Ok(Vec::new()), // 優雅地跳過無法解析的檔案
        };

        let mut edges = Vec::new();
        for stmt in &statements {
            extract_from_statement(stmt, path, &mut edges);
        }
        Ok(edges)
    }
}

/// 從單一 SQL 陳述式中提取 lineage edges。
fn extract_from_statement(stmt: &Statement, path: &Path, edges: &mut Vec<LineageEdge>) {
    // 第一層先按 statement 類型分流：目標表通常在這一層最容易確定。
    match stmt {
        // CREATE TABLE ... AS SELECT ...
        Statement::CreateTable(create) => {
            let target = table_name_to_string(&create.name);
            if let Some(query) = &create.query {
                let sources = extract_sources_from_query(query);
                for (source, relation) in sources {
                    edges.push(LineageEdge {
                        source: NodeId::from(source.as_str()),
                        target: NodeId::from(target.as_str()),
                        relation: if matches!(relation, EdgeRelation::SelectFrom) {
                            EdgeRelation::CreateTableAs
                        } else {
                            relation
                        },
                        source_file: path.to_path_buf(),
                        line_number: None,
                    });
                }
            }
        }

        // CREATE VIEW ... AS SELECT ...
        Statement::CreateView { name, query, .. } => {
            let target = table_name_to_string(name);
            let sources = extract_sources_from_query(query);
            for (source, relation) in sources {
                edges.push(LineageEdge {
                    source: NodeId::from(source.as_str()),
                    target: NodeId::from(target.as_str()),
                    relation,
                    source_file: path.to_path_buf(),
                    line_number: None,
                });
            }
        }

        // INSERT INTO ... SELECT ...
        Statement::Insert(insert) => {
            let target = match &insert.table {
                sqlparser::ast::TableObject::TableName(name) => table_name_to_string(name),
                _ => return,
            };
            if let Some(source) = &insert.source {
                let sources = extract_sources_from_query(source);
                for (src, _) in sources {
                    edges.push(LineageEdge {
                        source: NodeId::from(src.as_str()),
                        target: NodeId::from(target.as_str()),
                        relation: EdgeRelation::InsertInto,
                        source_file: path.to_path_buf(),
                        line_number: None,
                    });
                }
            }
        }

        _ => {}
    }
}

/// 從 Query 中提取所有來源表（處理 CTE、子查詢、集合運算）。
fn extract_sources_from_query(query: &Query) -> Vec<(String, EdgeRelation)> {
    let mut sources = Vec::new();
    let mut cte_names: Vec<String> = Vec::new();

    // 先處理 WITH，是因為最外層 SELECT 看到的可能只是 CTE 名字；
    // 真正的外部來源往往藏在 CTE 本體裡。
    if let Some(with) = &query.with {
        extract_cte_sources(with, &mut sources, &mut cte_names);
    }

    // 再處理主 query body，並帶著 cte_names 往下傳，避免把內部 CTE 誤判成外部表。
    extract_sources_from_set_expr(&query.body, &cte_names, &mut sources);

    sources
}

/// 從 CTE (WITH) 子句中提取來源。
fn extract_cte_sources(with: &With, sources: &mut Vec<(String, EdgeRelation)>, cte_names: &mut Vec<String>) {
    for cte in &with.cte_tables {
        let cte_name = cte.alias.name.value.clone();
        cte_names.push(cte_name);

        // CTE 本體可能參照其他表
        extract_sources_from_set_expr(&cte.query.body, &[], sources);
    }
}

/// 遞迴地從 SetExpr 中提取來源表（處理 UNION、INTERSECT 等）。
fn extract_sources_from_set_expr(
    body: &SetExpr,
    cte_names: &[String],
    sources: &mut Vec<(String, EdgeRelation)>,
) {
    // 這一層對應 AST 的 Query.body：
    // 普通 SELECT、子查詢、UNION/INTERSECT 都在這裡分開處理。
    match body {
        SetExpr::Select(select) => {
            extract_sources_from_select(select, cte_names, sources);
        }
        SetExpr::Query(query) => {
            let inner_sources = extract_sources_from_query(query);
            sources.extend(inner_sources);
        }
        SetExpr::SetOperation { left, right, .. } => {
            extract_sources_from_set_expr(left, cte_names, sources);
            extract_sources_from_set_expr(right, cte_names, sources);
        }
        _ => {}
    }
}

/// 從 SELECT 子句中提取來源表（FROM + JOIN）。
fn extract_sources_from_select(
    select: &Select,
    cte_names: &[String],
    sources: &mut Vec<(String, EdgeRelation)>,
) {
    // 對 lineage 來說，SELECT 最重要的不是 projection，而是 from/join 鏈上有哪些來源表。
    for table_with_joins in &select.from {
        extract_sources_from_table_with_joins(table_with_joins, cte_names, sources, false);
    }
}

/// 從 FROM 子句項目中提取來源（表 + 其 JOIN）。
fn extract_sources_from_table_with_joins(
    twj: &TableWithJoins,
    cte_names: &[String],
    sources: &mut Vec<(String, EdgeRelation)>,
    _is_join: bool,
) {
    extract_source_from_table_factor(&twj.relation, cte_names, sources, false);

    for join in &twj.joins {
        extract_source_from_table_factor(&join.relation, cte_names, sources, true);
    }
}

/// 從 TableFactor 中提取來源表名稱。
fn extract_source_from_table_factor(
    factor: &TableFactor,
    cte_names: &[String],
    sources: &mut Vec<(String, EdgeRelation)>,
    is_join: bool,
) {
    match factor {
        TableFactor::Table { name, .. } => {
            let table_name = table_name_to_string(name);
            // 跳過 CTE——它們是查詢內部的，不是外部來源
            if !cte_names.iter().any(|c| c.eq_ignore_ascii_case(&table_name)) {
                let relation = if is_join {
                    EdgeRelation::JoinOn
                } else {
                    EdgeRelation::SelectFrom
                };
                sources.push((table_name, relation));
            }
        }
        TableFactor::Derived { subquery, .. } => {
            // FROM (SELECT ...) 這種 derived table 對人類來說是子查詢；
            // 對 parser 來說它不是終點，所以要再遞迴進內層 query。
            let inner = extract_sources_from_query(subquery);
            sources.extend(inner);
        }
        TableFactor::NestedJoin { table_with_joins, .. } => {
            extract_sources_from_table_with_joins(table_with_joins, cte_names, sources, is_join);
        }
        _ => {}
    }
}

/// 將 sqlparser ObjectName 轉換為以點分隔的字串。
fn table_name_to_string(name: &sqlparser::ast::ObjectName) -> String {
    name.0
        .iter()
        .map(|part| part.to_string())
        .collect::<Vec<_>>()
        .join(".")
}

/// 從 SQL 檔案中提取目標表的欄位名稱（best-effort）。
/// 回傳 target_table_name → Vec<column_name> 的對應。
pub fn extract_columns_from_sql(content: &str) -> HashMap<String, Vec<String>> {
    let mut result = HashMap::new();

    if content.contains("{{") {
        return result;
    }

    let dialect = GenericDialect {};
    let statements = match Parser::parse_sql(&dialect, content) {
        Ok(stmts) => stmts,
        Err(_) => return result,
    };

    for stmt in &statements {
        match stmt {
            Statement::CreateTable(create) => {
                let target = table_name_to_string(&create.name).to_lowercase();
                if let Some(query) = &create.query {
                    let cols = extract_columns_from_query(query);
                    if !cols.is_empty() {
                        result.insert(target.clone(), cols);
                    }
                }
                // 也處理 column definitions (非 CTAS)
                if !create.columns.is_empty() {
                    let cols: Vec<String> = create
                        .columns
                        .iter()
                        .map(|c| c.name.value.to_lowercase())
                        .collect();
                    result.insert(target, cols);
                }
            }
            Statement::CreateView { name, query, .. } => {
                let target = table_name_to_string(name).to_lowercase();
                let cols = extract_columns_from_query(query);
                if !cols.is_empty() {
                    result.insert(target, cols);
                }
            }
            _ => {}
        }
    }

    result
}

/// 從 Query 的 SELECT list 提取欄位名稱。
fn extract_columns_from_query(query: &Query) -> Vec<String> {
    extract_columns_from_set_expr(&query.body)
}

fn extract_columns_from_set_expr(body: &SetExpr) -> Vec<String> {
    match body {
        SetExpr::Select(select) => extract_columns_from_select(select),
        SetExpr::Query(query) => extract_columns_from_query(query),
        SetExpr::SetOperation { left, .. } => extract_columns_from_set_expr(left),
        _ => Vec::new(),
    }
}

fn extract_columns_from_select(select: &Select) -> Vec<String> {
    let mut columns = Vec::new();
    for item in &select.projection {
        match item {
            SelectItem::UnnamedExpr(expr) => {
                if let Some(name) = expr_to_column_name(expr) {
                    columns.push(name.to_lowercase());
                }
            }
            SelectItem::ExprWithAlias { alias, .. } => {
                columns.push(alias.value.to_lowercase());
            }
            SelectItem::Wildcard(_) => {
                // SELECT * — 無法確定欄位，跳過
            }
            SelectItem::QualifiedWildcard(_, _) => {
                // SELECT t.* — 無法確定欄位，跳過
            }
        }
    }
    columns
}

/// 嘗試從 expression 中取得欄位名稱。
fn expr_to_column_name(expr: &sqlparser::ast::Expr) -> Option<String> {
    match expr {
        sqlparser::ast::Expr::Identifier(ident) => Some(ident.value.clone()),
        sqlparser::ast::Expr::CompoundIdentifier(parts) => {
            // 取最後一個部分作為欄位名（例如 t.column_name → column_name）
            parts.last().map(|p| p.value.clone())
        }
        _ => None,
    }
}

// ===== Column-Level Lineage =====

/// 從 SQL 檔案中提取 column-level lineage（best-effort）。
/// 回傳每個 output column 的來源欄位與轉換類型。
pub fn extract_column_lineage(content: &str) -> Vec<ColumnLineage> {
    if content.contains("{{") {
        return Vec::new();
    }

    let dialect = GenericDialect {};
    let statements = match Parser::parse_sql(&dialect, content) {
        Ok(stmts) => stmts,
        Err(_) => return Vec::new(),
    };

    let mut result = Vec::new();

    for stmt in &statements {
        match stmt {
            Statement::CreateTable(create) => {
                let target = table_name_to_string(&create.name).to_lowercase();
                if let Some(query) = &create.query {
                    let alias_map = build_alias_map_from_query(query);
                    result.extend(extract_col_lineage_from_query(query, &target, &alias_map));
                }
            }
            Statement::CreateView { name, query, .. } => {
                let target = table_name_to_string(name).to_lowercase();
                let alias_map = build_alias_map_from_query(query);
                result.extend(extract_col_lineage_from_query(query, &target, &alias_map));
            }
            _ => {}
        }
    }

    result
}

/// 表 alias → 真實表名的對照表。
type AliasMap = HashMap<String, String>;

/// 從 Query 的 FROM 子句建立 alias map。
fn build_alias_map_from_query(query: &Query) -> AliasMap {
    let mut map = AliasMap::new();
    if let SetExpr::Select(select) = &*query.body {
        for twj in &select.from {
            collect_aliases_from_table_factor(&twj.relation, &mut map);
            for join in &twj.joins {
                collect_aliases_from_table_factor(&join.relation, &mut map);
            }
        }
    }
    map
}

fn collect_aliases_from_table_factor(factor: &TableFactor, map: &mut AliasMap) {
    if let TableFactor::Table { name, alias, .. } = factor {
        let table_name = table_name_to_string(name).to_lowercase();
        if let Some(alias) = alias {
            map.insert(alias.name.value.to_lowercase(), table_name.clone());
        }
        // 也把自己映射到自己（方便查找）
        map.insert(table_name.clone(), table_name);
    }
}

/// 從 Query 中提取各 output column 的 lineage。
/// 如果最外層是 SELECT * FROM <cte>，會穿透 CTE 取得真正的欄位定義。
fn extract_col_lineage_from_query(query: &Query, target_table: &str, alias_map: &AliasMap) -> Vec<ColumnLineage> {
    match &*query.body {
        SetExpr::Select(select) => {
            let result = extract_col_lineage_from_select(select, target_table, alias_map);

            // 如果結果只有 SELECT *，嘗試穿透 CTE
            let is_only_star = result.len() == 1 && result[0].target_column == "*";
            if is_only_star {
                if let Some(with) = &query.with {
                    // 找到最外層 SELECT 引用的 CTE 名稱
                    let from_table = select.from.first()
                        .and_then(|twj| match &twj.relation {
                            TableFactor::Table { name, .. } => Some(table_name_to_string(name).to_lowercase()),
                            _ => None,
                        });

                    if let Some(ref from_name) = from_table {
                        // 找到對應的 CTE 定義
                        for cte in &with.cte_tables {
                            if cte.alias.name.value.to_lowercase() == *from_name {
                                let cte_alias_map = build_alias_map_from_query(&cte.query);
                                let mut merged_alias = alias_map.clone();
                                merged_alias.extend(cte_alias_map);
                                let cte_result = extract_col_lineage_from_query(&cte.query, target_table, &merged_alias);
                                if !(cte_result.is_empty() || cte_result.len() == 1 && cte_result[0].target_column == "*") {
                                    return cte_result;
                                }
                            }
                        }
                    }
                }
            }

            result
        }
        _ => Vec::new(),
    }
}

/// 從 SELECT 子句提取 column lineage。
fn extract_col_lineage_from_select(select: &Select, target_table: &str, alias_map: &AliasMap) -> Vec<ColumnLineage> {
    let mut result = Vec::new();

    for item in &select.projection {
        match item {
            SelectItem::UnnamedExpr(expr) => {
                let col_name = expr_to_column_name(expr)
                    .unwrap_or_else(|| expr.to_string())
                    .to_lowercase();
                let (sources, transform) = analyze_expr(expr, alias_map);
                result.push(ColumnLineage {
                    target_table: target_table.to_string(),
                    target_column: col_name,
                    source_columns: sources,
                    transform,
                    expression: expr.to_string(),
                });
            }
            SelectItem::ExprWithAlias { expr, alias } => {
                let (sources, transform) = analyze_expr(expr, alias_map);
                result.push(ColumnLineage {
                    target_table: target_table.to_string(),
                    target_column: alias.value.to_lowercase(),
                    source_columns: sources,
                    transform,
                    expression: expr.to_string(),
                });
            }
            SelectItem::Wildcard(_) => {
                result.push(ColumnLineage {
                    target_table: target_table.to_string(),
                    target_column: "*".to_string(),
                    source_columns: vec![],
                    transform: TransformKind::Direct,
                    expression: "*".to_string(),
                });
            }
            SelectItem::QualifiedWildcard(kind, _) => {
                let prefix = kind.to_string().to_lowercase();
                let resolved = alias_map.get(&prefix).cloned().unwrap_or(prefix.clone());
                result.push(ColumnLineage {
                    target_table: target_table.to_string(),
                    target_column: format!("{prefix}.*"),
                    source_columns: vec![SourceColumn {
                        table: Some(resolved),
                        column: "*".to_string(),
                    }],
                    transform: TransformKind::Direct,
                    expression: format!("{prefix}.*"),
                });
            }
        }
    }

    result
}

/// 分析一個 SQL expression，提取它引用的來源欄位和轉換類型。
fn analyze_expr(expr: &sqlparser::ast::Expr, alias_map: &AliasMap) -> (Vec<SourceColumn>, TransformKind) {
    use sqlparser::ast::Expr;

    match expr {
        // 單一欄位引用：`amount` 或 `o.amount`
        Expr::Identifier(ident) => {
            let col = SourceColumn {
                table: None,
                column: ident.value.to_lowercase(),
            };
            (vec![col], TransformKind::Direct)
        }
        Expr::CompoundIdentifier(parts) => {
            if parts.len() >= 2 {
                let table_alias = parts[parts.len() - 2].value.to_lowercase();
                let column = parts.last().unwrap().value.to_lowercase();
                let resolved_table = alias_map.get(&table_alias).cloned().unwrap_or(table_alias);
                let col = SourceColumn {
                    table: Some(resolved_table),
                    column,
                };
                (vec![col], TransformKind::Direct)
            } else {
                (vec![], TransformKind::Unknown)
            }
        }

        // 聚合函數：SUM(amount), COUNT(DISTINCT order_id), AVG(x), etc.
        Expr::Function(func) => {
            let func_name = func.name.to_string().to_uppercase();
            let agg_funcs = ["SUM", "COUNT", "AVG", "MIN", "MAX", "ARRAY_AGG", "LISTAGG", "MEDIAN"];
            let window_funcs = ["LAG", "LEAD", "ROW_NUMBER", "RANK", "DENSE_RANK", "NTILE",
                                "FIRST_VALUE", "LAST_VALUE", "NTH_VALUE"];

            // 收集函數參數中的所有欄位引用
            let mut sources = Vec::new();
            collect_sources_from_func_args(func, alias_map, &mut sources);

            let transform = if window_funcs.iter().any(|w| func_name.contains(w)) || func.over.is_some() {
                TransformKind::Window(func_name)
            } else if agg_funcs.iter().any(|a| func_name.contains(a)) {
                TransformKind::Aggregation(func_name)
            } else {
                TransformKind::Expression
            };

            (sources, transform)
        }

        // CASE WHEN ... END
        Expr::Case { operand, conditions, else_result, .. } => {
            let mut sources = Vec::new();
            if let Some(op) = operand {
                let (s, _) = analyze_expr(op, alias_map);
                sources.extend(s);
            }
            for case_when in conditions {
                let (s1, _) = analyze_expr(&case_when.condition, alias_map);
                sources.extend(s1);
                let (s2, _) = analyze_expr(&case_when.result, alias_map);
                sources.extend(s2);
            }
            if let Some(el) = else_result {
                let (s, _) = analyze_expr(el, alias_map);
                sources.extend(s);
            }
            dedup_sources(&mut sources);
            (sources, TransformKind::Expression)
        }

        // 二元運算：a + b, a / b, CONCAT(a, b)
        Expr::BinaryOp { left, right, .. } => {
            let (mut s1, _) = analyze_expr(left, alias_map);
            let (s2, _) = analyze_expr(right, alias_map);
            s1.extend(s2);
            dedup_sources(&mut s1);
            (s1, TransformKind::Expression)
        }

        // 一元運算：-x, NOT x
        Expr::UnaryOp { expr: inner, .. } => {
            let (s, _) = analyze_expr(inner, alias_map);
            (s, TransformKind::Expression)
        }

        // CAST(x AS type)
        Expr::Cast { expr: inner, .. } => {
            let (s, t) = analyze_expr(inner, alias_map);
            (s, t) // CAST 不改變語義上的轉換類型
        }

        // 巢狀括號
        Expr::Nested(inner) => analyze_expr(inner, alias_map),

        // 常數值
        Expr::Value(_) => (vec![], TransformKind::Constant),

        // 子查詢
        Expr::Subquery(_) => (vec![], TransformKind::Expression),

        // 其他不認識的 expression
        _ => {
            // 嘗試遞迴收集所有欄位引用
            let mut sources = Vec::new();
            collect_all_column_refs(expr, alias_map, &mut sources);
            let transform = if sources.is_empty() {
                TransformKind::Constant
            } else {
                TransformKind::Expression
            };
            (sources, transform)
        }
    }
}

/// 從函數參數中收集所有來源欄位引用。
fn collect_sources_from_func_args(
    func: &sqlparser::ast::Function,
    alias_map: &AliasMap,
    sources: &mut Vec<SourceColumn>,
) {
    use sqlparser::ast::FunctionArguments;
    match &func.args {
        FunctionArguments::List(arg_list) => {
            for arg in &arg_list.args {
                use sqlparser::ast::FunctionArg;
                match arg {
                    FunctionArg::Unnamed(arg_expr)
                    | FunctionArg::Named { arg: arg_expr, .. }
                    | FunctionArg::ExprNamed { arg: arg_expr, .. } => {
                        use sqlparser::ast::FunctionArgExpr;
                        match arg_expr {
                            FunctionArgExpr::Expr(e) => {
                                let (s, _) = analyze_expr(e, alias_map);
                                sources.extend(s);
                            }
                            FunctionArgExpr::QualifiedWildcard(name) => {
                                let prefix = table_name_to_string(name).to_lowercase();
                                let resolved = alias_map.get(&prefix).cloned().unwrap_or(prefix);
                                sources.push(SourceColumn {
                                    table: Some(resolved),
                                    column: "*".to_string(),
                                });
                            }
                            FunctionArgExpr::Wildcard => {}
                        }
                    }
                }
            }
        }
        FunctionArguments::None | FunctionArguments::Subquery(_) => {}
    }
}

/// 遞迴收集 expression 中的所有欄位引用（fallback 用）。
fn collect_all_column_refs(
    expr: &sqlparser::ast::Expr,
    alias_map: &AliasMap,
    sources: &mut Vec<SourceColumn>,
) {
    use sqlparser::ast::Expr;
    match expr {
        Expr::Identifier(ident) => {
            sources.push(SourceColumn {
                table: None,
                column: ident.value.to_lowercase(),
            });
        }
        Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
            let table_alias = parts[parts.len() - 2].value.to_lowercase();
            let column = parts.last().unwrap().value.to_lowercase();
            let resolved = alias_map.get(&table_alias).cloned().unwrap_or(table_alias);
            sources.push(SourceColumn {
                table: Some(resolved),
                column,
            });
        }
        _ => {
            // 對其他類型不做進一步遞迴（避免無限迴圈）
        }
    }
}

fn dedup_sources(sources: &mut Vec<SourceColumn>) {
    let mut seen = std::collections::HashSet::new();
    sources.retain(|s| {
        let key = format!("{}.{}", s.table.as_deref().unwrap_or(""), s.column);
        seen.insert(key)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(sql: &str) -> Vec<LineageEdge> {
        let scanner = SqlScanner;
        scanner
            .scan_file(Path::new("test.sql"), sql)
            .expect("SQL parse failed")
    }

    #[test]
    fn test_simple_select() {
        let edges = scan("SELECT * FROM raw.orders;");
        // Standalone SELECT without INSERT/CREATE doesn't produce edges
        // because there's no target table
        assert!(edges.is_empty());
    }

    #[test]
    fn test_create_table_as_select() {
        let edges = scan("CREATE TABLE stg.orders AS SELECT * FROM raw.orders;");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, NodeId::from("raw.orders"));
        assert_eq!(edges[0].target, NodeId::from("stg.orders"));
        assert_eq!(edges[0].relation, EdgeRelation::CreateTableAs);
    }

    #[test]
    fn test_create_table_as_with_join() {
        let edges = scan(
            "CREATE TABLE mart.orders AS \
             SELECT o.*, p.amount \
             FROM raw.orders o \
             JOIN raw.payments p ON o.id = p.order_id;",
        );
        assert_eq!(edges.len(), 2);
        let sources: Vec<_> = edges.iter().map(|e| e.source.0.as_str()).collect();
        assert!(sources.contains(&"raw.orders"));
        assert!(sources.contains(&"raw.payments"));
    }

    #[test]
    fn test_insert_into_select() {
        let edges = scan(
            "INSERT INTO staging.events SELECT * FROM raw.events;",
        );
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, NodeId::from("raw.events"));
        assert_eq!(edges[0].target, NodeId::from("staging.events"));
        assert_eq!(edges[0].relation, EdgeRelation::InsertInto);
    }

    #[test]
    fn test_create_view() {
        let edges = scan(
            "CREATE VIEW analytics.daily_orders AS \
             SELECT date, count(*) FROM raw.orders GROUP BY date;",
        );
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, NodeId::from("raw.orders"));
        assert_eq!(edges[0].target, NodeId::from("analytics.daily_orders"));
    }

    #[test]
    fn test_cte_not_treated_as_source() {
        let edges = scan(
            "CREATE TABLE mart.summary AS \
             WITH recent AS (SELECT * FROM raw.orders WHERE date > '2024-01-01') \
             SELECT * FROM recent JOIN raw.users ON recent.user_id = raw.users.id;",
        );
        // Should have raw.orders (from CTE body) and raw.users (from main query)
        // but NOT "recent" (the CTE name itself)
        let sources: Vec<_> = edges.iter().map(|e| e.source.0.as_str()).collect();
        assert!(sources.contains(&"raw.orders"));
        assert!(sources.contains(&"raw.users"));
        assert!(!sources.contains(&"recent"));
    }

    #[test]
    fn test_subquery() {
        let edges = scan(
            "CREATE TABLE mart.high_value AS \
             SELECT * FROM (SELECT * FROM raw.orders WHERE amount > 100) sub;",
        );
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, NodeId::from("raw.orders"));
    }

    #[test]
    fn test_union() {
        let edges = scan(
            "CREATE TABLE combined.events AS \
             SELECT * FROM raw.web_events \
             UNION ALL \
             SELECT * FROM raw.mobile_events;",
        );
        assert_eq!(edges.len(), 2);
        let sources: Vec<_> = edges.iter().map(|e| e.source.0.as_str()).collect();
        assert!(sources.contains(&"raw.web_events"));
        assert!(sources.contains(&"raw.mobile_events"));
    }

    #[test]
    fn test_multiple_statements() {
        let edges = scan(
            "CREATE TABLE stg.a AS SELECT * FROM raw.x;\n\
             INSERT INTO stg.b SELECT * FROM raw.y;",
        );
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn test_column_extraction_ctas() {
        let cols = super::extract_columns_from_sql(
            "CREATE TABLE mart.orders AS SELECT id, amount, user_name FROM raw.orders;",
        );
        let mart_cols = cols.get("mart.orders").unwrap();
        assert_eq!(mart_cols, &["id", "amount", "user_name"]);
    }

    #[test]
    fn test_column_extraction_with_alias() {
        let cols = super::extract_columns_from_sql(
            "CREATE VIEW v AS SELECT o.id, sum(amount) AS total FROM orders o GROUP BY o.id;",
        );
        let v_cols = cols.get("v").unwrap();
        assert!(v_cols.contains(&"id".to_string()));
        assert!(v_cols.contains(&"total".to_string()));
    }

    #[test]
    fn test_column_extraction_star_returns_empty() {
        let cols = super::extract_columns_from_sql(
            "CREATE TABLE t AS SELECT * FROM raw.orders;",
        );
        // SELECT * 無法確定具體欄位
        assert!(cols.get("t").is_none() || cols.get("t").unwrap().is_empty());
    }

    #[test]
    fn test_insert_with_join() {
        let edges = scan(
            "INSERT INTO mart.orders \
             SELECT o.*, u.name \
             FROM stg.orders o \
             JOIN stg.users u ON o.user_id = u.id;",
        );
        assert_eq!(edges.len(), 2);
        assert!(edges.iter().all(|e| e.target == NodeId::from("mart.orders")));
    }

    // ===== Column-Level Lineage Tests =====

    #[test]
    fn test_col_lineage_direct() {
        let lineage = super::extract_column_lineage(
            "CREATE TABLE stg.orders AS SELECT id AS order_id, amount FROM raw.orders;",
        );
        assert_eq!(lineage.len(), 2);

        let order_id = lineage.iter().find(|c| c.target_column == "order_id").unwrap();
        assert_eq!(order_id.transform, TransformKind::Direct);
        assert_eq!(order_id.source_columns.len(), 1);
        assert_eq!(order_id.source_columns[0].column, "id");

        let amount = lineage.iter().find(|c| c.target_column == "amount").unwrap();
        assert_eq!(amount.transform, TransformKind::Direct);
    }

    #[test]
    fn test_col_lineage_aggregation() {
        let lineage = super::extract_column_lineage(
            "CREATE TABLE mart.summary AS \
             SELECT user_id, SUM(amount) AS total, COUNT(DISTINCT order_id) AS cnt \
             FROM raw.orders GROUP BY user_id;",
        );
        assert_eq!(lineage.len(), 3);

        let total = lineage.iter().find(|c| c.target_column == "total").unwrap();
        assert!(matches!(total.transform, TransformKind::Aggregation(ref f) if f == "SUM"));
        assert_eq!(total.source_columns[0].column, "amount");

        let cnt = lineage.iter().find(|c| c.target_column == "cnt").unwrap();
        assert!(matches!(cnt.transform, TransformKind::Aggregation(ref f) if f == "COUNT"));
    }

    #[test]
    fn test_col_lineage_expression() {
        let lineage = super::extract_column_lineage(
            "CREATE TABLE t AS SELECT a + b AS total FROM raw.x;",
        );
        let total = lineage.iter().find(|c| c.target_column == "total").unwrap();
        assert_eq!(total.transform, TransformKind::Expression);
        assert_eq!(total.source_columns.len(), 2);
    }

    #[test]
    fn test_col_lineage_case_when() {
        let lineage = super::extract_column_lineage(
            "CREATE TABLE t AS \
             SELECT CASE WHEN status = 'active' THEN amount ELSE 0 END AS val \
             FROM raw.orders;",
        );
        let val = lineage.iter().find(|c| c.target_column == "val").unwrap();
        assert_eq!(val.transform, TransformKind::Expression);
        assert!(val.source_columns.iter().any(|s| s.column == "status"));
        assert!(val.source_columns.iter().any(|s| s.column == "amount"));
    }

    #[test]
    fn test_col_lineage_alias_resolution() {
        let lineage = super::extract_column_lineage(
            "CREATE TABLE mart.orders AS \
             SELECT o.id AS order_id, p.amount \
             FROM raw.orders o \
             JOIN raw.payments p ON o.id = p.order_id;",
        );
        let order_id = lineage.iter().find(|c| c.target_column == "order_id").unwrap();
        assert_eq!(order_id.source_columns[0].table.as_deref(), Some("raw.orders"));
        assert_eq!(order_id.source_columns[0].column, "id");

        let amount = lineage.iter().find(|c| c.target_column == "amount").unwrap();
        assert_eq!(amount.source_columns[0].table.as_deref(), Some("raw.payments"));
    }

    #[test]
    fn test_col_lineage_constant() {
        let lineage = super::extract_column_lineage(
            "CREATE TABLE t AS SELECT 'order' AS revenue_type FROM raw.x;",
        );
        let rt = lineage.iter().find(|c| c.target_column == "revenue_type").unwrap();
        assert_eq!(rt.transform, TransformKind::Constant);
        assert!(rt.source_columns.is_empty());
    }
}
