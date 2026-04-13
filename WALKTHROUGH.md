# lineage-lite 程式碼導覽

給第一次看這個專案的人，建議按這個順序走一遍。

## 先跑起來再看 code

先在終端機跑一次，知道這個東西在幹嘛，再回頭看 code 會比較有方向感：

```bash
cargo run -- scan ./demo
cargo run -- impact raw.payments --path ./demo
cargo run -- trace marts.mart_customers --path ./demo
```

## 專案結構

```
src/
├── main.rs              # 程式進入點，8 行，呼叫 cli::run()
├── lib.rs               # 模組 re-export，6 行
├── error.rs             # 統一錯誤型別（thiserror）
├── cli.rs               # CLI 指令入口，把下面四個模組串起來
├── graph/
│   ├── node.rs          # 核心 domain types（Node、Edge、ColumnLineage）
│   ├── mod.rs           # LineageGraph（petgraph wrapper + BFS）
│   └── query.rs         # stats 查詢
├── scanner/
│   ├── mod.rs           # Scanner trait + ScanOrchestrator
│   ├── sql.rs           # SQL 解析（sqlparser-rs AST）
│   ├── dbt.rs           # dbt ref()/source() 解析（regex）
│   └── python.rs        # Python ETL 解析（regex）
├── output/
│   ├── mod.rs           # Renderer trait + OutputFormat enum
│   ├── table.rs         # 終端機表格（comfy-table）
│   ├── dot.rs           # Graphviz DOT
│   └── html.rs          # 互動式 HTML（vanilla JS，零外部依賴）
└── storage/
    ├── mod.rs           # StorageBackend trait
    └── sqlite.rs        # SQLite 持久化
```

一句話版本：**scanner 產 edges → graph 建圖 → cli 查詢 → output 輸出**。

## 導覽順序

### 1. `main.rs` → `lib.rs`（30 秒）

`main.rs` 只做一件事：呼叫 `cli::run()`。`lib.rs` 列出五個模組，讓你知道東西放在哪。

### 2. `graph/node.rs`（149 行）— 整個專案的語言

這是最重要的一站，所有 domain type 都定義在這裡：

- `NodeId` / `NodeKind` — 一個節點是什麼（SqlTable、DbtModel、PythonEtl…）
- `LineageEdge` / `EdgeRelation` — 兩個節點的關係（SelectFrom、DbtRef、InsertInto…）
- `ColumnLineage` / `TransformKind` — column-level 追蹤（Direct、Aggregation、Expression…）

後面所有模組都在操作這些型別，先把名字混熟。

### 3. `scanner/mod.rs`（110 行）— 掃描的骨架

`Scanner` trait 只有兩個方法：

```rust
fn extensions(&self) -> &[&str];                              // 負責哪些副檔名
fn scan_file(&self, path, content) -> Result<Vec<LineageEdge>>;  // 解析一個檔案
```

`ScanOrchestrator` 走整個目錄，按副檔名 dispatch 給對應的 scanner。

### 4. 挑一個 scanner 看

建議從 **`dbt.rs`（146 行）** 開始，最短、最好懂：用 regex 抓 `{{ ref('...') }}` 跟 `{{ source('...', '...') }}`，抓到就產一條 edge。看完就會理解 Scanner trait 在做什麼。

**`python.rs`（188 行）** 也是 regex，邏輯類似，可以快速掃過。

**`sql.rs`（910 行）** 是最大的模組，用 sqlparser-rs 做 AST 解析，處理 FROM、JOIN、CTE、子查詢、UNION 等。建議等其他模組都熟了再來看，不然一開始就陷進去容易迷路。

### 5. `graph/mod.rs`（268 行）— 圖的操作

`LineageGraph` 包了一個 petgraph 的 `DiGraph`，重點方法：

- `add_node` / `ensure_node` / `add_edge` — 建圖
- `upstream(id, depth)` / `downstream(id, depth)` — BFS 走訪
- `impact(id)` — 其實就是 `downstream(id, None)`，不限深度全部走完

scanner 負責產 edges，這裡負責把 edges 變成一張可以查詢的圖。

### 6. `cli.rs`（688 行）— 串起來的地方

挑 **`cmd_impact`** 從頭到尾追一次，它最短也最完整：

1. `build_graph(path)` → 呼叫 ScanOrchestrator 掃描，建圖
2. `graph.impact(&node_id)` → BFS 找所有下游
3. `renderer.render_nodes()` → 輸出結果

追完這條路，就知道一個使用者指令從進來到出去經過了哪些模組。

八個指令的邏輯都在這個檔案裡：scan、impact、show、stats、trace、diff、check、merge。

### 7. `output/` 和 `storage/`（選讀）

這些是末端模組，理解了前面的核心之後看不會有障礙：

- `output/table.rs`（57 行）— comfy-table 印表格
- `output/dot.rs`（87 行）— Graphviz DOT 格式
- `output/html.rs`（408 行）— 互動式 HTML，內嵌 vanilla JS
- `storage/sqlite.rs`（317 行）— 建表、存圖、讀圖，用 WAL mode

## 資料流全貌

```
使用者下指令
  ↓
cli.rs → 解析參數，選擇 command
  ↓
ScanOrchestrator → 走目錄，按副檔名 dispatch
  ├→ dbt.rs    (regex 抓 ref/source)
  ├→ sql.rs    (AST 解析 FROM/JOIN/CTE)
  └→ python.rs (regex 抓 read_sql/to_sql)
  ↓
LineageGraph → add_node + add_edge 建圖
  ↓
指令邏輯 → impact / show / trace / stats / diff / check
  ↓
Renderer → 表格 / DOT / HTML
Storage  → SQLite（可選）
```

## 各模組行數

| 檔案 | 行數 | 角色 |
|------|------|------|
| `scanner/sql.rs` | 910 | SQL AST 解析（最複雜的模組） |
| `cli.rs` | 688 | 指令入口，串接所有模組 |
| `output/html.rs` | 408 | 互動式 HTML 視覺化 |
| `storage/sqlite.rs` | 317 | SQLite 持久化 |
| `graph/mod.rs` | 268 | DAG 資料結構 + BFS |
| `scanner/python.rs` | 188 | Python ETL 掃描 |
| `graph/node.rs` | 149 | Domain types |
| `scanner/dbt.rs` | 146 | dbt 掃描 |
| `graph/query.rs` | 132 | Stats 查詢 |
| `scanner/mod.rs` | 110 | Scanner trait + orchestrator |
| `output/dot.rs` | 87 | Graphviz DOT |
| `output/table.rs` | 57 | 終端機表格 |
| `storage/mod.rs` | 43 | StorageBackend trait |
| `output/mod.rs` | 31 | Renderer trait |
| `error.rs` | 21 | 錯誤型別 |
| `main.rs` | 8 | 進入點 |
| `lib.rs` | 6 | 模組 re-export |
