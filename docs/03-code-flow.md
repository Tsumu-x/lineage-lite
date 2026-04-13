# 03 — 流程與練習篇：跟著一條指令看整條 pipeline

前兩篇比較靜態：一個告訴你 repo 長什麼樣，一個補語法。這篇要你做一件更動態的事：**實際跑一個指令，然後一路追進 code 裡看它發生什麼**。

這是學中型專案最有效的方式。光看結構圖永遠會有一種「我好像懂了但又好像沒懂」的模糊感，只有跟著真實的呼叫鏈追一遍，那種感覺才會消失。

## 1. 先實際跑起來

在 repo 根目錄打開 terminal：

```bash
cargo run -- scan ./demo
```

第一次編譯要等一下。跑完之後你會看到一張終端機表格，列出從 demo 專案掃出來的所有 lineage edges，大概長這樣：

```text
source                    -> target                          (relation)
raw.orders                -> staging.stg_orders              (SelectFrom)
staging.stg_orders        -> intermediate.int_order_payments (SelectFrom)
...
```

記住這個輸出。接下來的每一步，我們都會回頭問：「所以剛剛那張表，是在哪一行 code 生出來的？」

## 2. 進入 `main()`

程式入口在 [`../src/main.rs`](../src/main.rs)：

```rust
fn main() {
    if let Err(e) = lineage_lite::cli::run() {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}
```

就四行。`main()` 唯一的工作是呼叫 `lineage_lite::cli::run()`，並把錯誤印出來。真正的邏輯都在 library 裡——這種「薄 `main.rs`」是 Rust 專案的標配。

下一步要追的是 `cli::run()`。

## 3. CLI 解析參數

打開 [`../src/cli.rs`](../src/cli.rs)：

```rust
let cli = Cli::parse();
```

這一行用 `clap` 把命令列參數 parse 成 `Cli` struct。`scan ./demo` 會對應到 `Commands::Scan { path, ... }` 這個 enum variant，接著進 `match`，落進 `cmd_scan(...)` 分支。

如果你在 `cli.rs` 找不到 `Cli::parse` 的來源不要慌——那是 `#[derive(Parser)]` 這個 clap derive macro 自動產生的。

## 4. `cmd_scan()` 的第一件事：建圖

`cmd_scan()` 裡最關鍵的一行：

```rust
let BuildResult { graph, files_scanned, col_lineage } = build_graph(path)?;
```

`build_graph()` 做三件事：

1. 把 repo 掃一遍
2. 把所有 source → target 關係收集起來
3. 組成一張 graph

這個函式是**幾乎所有指令都會先呼叫的前置**——不管你跑 `scan`、`impact`、`show` 還是 `stats`，都得先有 graph 才能做後面的事。所以追懂它就等於追懂半個 repo。

## 5. 裝好所有 scanner

`build_graph()` 裡會先：

```rust
let orchestrator = ScanOrchestrator::default_scanners();
```

這一行把目前內建的三個 scanner（dbt / SQL / Python）全部塞進 orchestrator 的 `Vec<Box<dyn Scanner>>` 裡。如果你之後要新增一個 scanner，這裡就是要改的地方之一。

## 6. 走訪目錄

接著：

```rust
let ScanResult {
    edges,
    files_scanned,
    column_map,
    col_lineage,
    ..
} = orchestrator.scan_directory(path)?;
```

`scan_directory()` 做的事可以拆成四步：

1. 用 `walkdir` 走訪整個目錄樹
2. 看每個檔案的副檔名
3. 把檔案內容讀出來，交給對應副檔名的 scanner
4. 收集所有 scanner 吐出來的 edges 和欄位資訊

每個 scanner 的共同介面都是：

```text
輸入：path + content
輸出：Vec<LineageEdge>
```

這就是為什麼 `Scanner` trait 要長那樣——它代表的就是「能掃某種檔案、能吐出 lineage edge 的東西」。

## 7. 為什麼要先 `ensure_node` 再 `add_edge`

拿到 edges 之後，`build_graph()` 不會直接 `graph.add_edge(edge)`，而是先做：

```rust
graph.ensure_node(&edge.source, source_kind, &edge.source_file);
graph.ensure_node(&edge.target, target_kind, &edge.source_file);
```

原因很簡單：**邊只能連到已經存在的節點**。你不能把一條 edge 指向一個還沒有被放進 graph 的節點。所以順序必須是：先確保兩端的 node 都在，再把 edge 連起來。

`ensure_node` 的名字取得很好——它的語義是「如果這個節點不在，就建一個；如果在，就直接用現有的」。不需要 caller 自己檢查，把這個邏輯封裝在一個方法裡很乾淨。

之後才會：

```rust
graph.add_edge(edge)?;
```

## 8. 補上欄位資訊（column-level）

除了 table-level 的 edge 之外，SQL scanner 還會在走訪 AST 的時候收集每個欄位的來源：

```rust
for (table, columns) in &column_map {
    let node_id = NodeId::from(table.as_str());
    graph.add_columns(&node_id, columns);
}
```

這就是為什麼你跑 `lineage-lite trace reports.daily_revenue` 時它能告訴你 `daily_revenue = SUM(mart_orders.amount)`——因為 column-level 資訊在 `build_graph()` 這一步已經被塞進 graph 了。

## 9. 選輸出格式

建好 graph 之後，`cmd_scan()` 會根據 `--format` 選 renderer：

```text
table -> TableRenderer
dot   -> DotRenderer
html  -> 特殊處理（因為需要 JSON 嵌入 + JS 模板）
```

每個 renderer 都實作 `Renderer` trait，所以 `cmd_scan` 只要寫一次「把 edges/nodes 丟給 renderer」的邏輯，三種格式都能走同一條路。

## 10. 整條路線總結

濃縮成一張流程圖：

```text
main()
  └─ lineage_lite::cli::run()
       └─ Cli::parse()           // clap 解析參數
            └─ cmd_scan()
                 └─ build_graph()
                      ├─ ScanOrchestrator::default_scanners()
                      ├─ scan_directory()
                      │    └─ 每個 scanner 吐 edges
                      └─ 補 node、加 edge、補 column
                 └─ renderer.render_*()
                      └─ 寫到 stdout 或檔案
```

這條路線如能在心中大致描出來，讀其他指令會省很多力。`impact`、`show`、`stats` 其實都是同一個前半段：

```text
先 build_graph
再做不同的查詢
再選 renderer 輸出
```

差別只在中間那一步——`impact` 呼叫 `graph.downstream_unlimited()`、`show` 呼叫 `graph.neighborhood()`、`stats` 呼叫 `graph/query.rs` 裡的統計函式。掌握了 `build_graph()`，其他指令就都只是變體。

---

## 11. 練習題

這幾題都不需要動 code，只要看過前面的流程就答得出來。看完題目先在心裡回答，再翻到後面對答案。

### 題目 1

看到這段 SQL：

```sql
CREATE VIEW reports.user_orders AS
SELECT *
FROM mart.orders;
```

請回答：

1. source node 是誰？
2. target node 是誰？
3. 會產生哪一條 edge？

### 題目 2

為什麼 `Scanner` trait 的回傳型別是 `Result<Vec<LineageEdge>>`，而不是直接回一個 `LineageEdge`？

### 題目 3

`ScanOrchestrator` 為什麼存 `Vec<Box<dyn Scanner>>`，而不是 `Vec<SqlScanner>`？

### 題目 4

如果某個節點同時依賴兩個上游：

```text
raw.orders -> mart.order_summary
raw.users  -> mart.order_summary
```

那 `upstream("mart.order_summary")` 為什麼適合用 graph 來做？

### 題目 5

為什麼 `build_graph()` 會先 `ensure_node()`，再 `add_edge()`？

### 題目 6

`stats` 和 `impact` 最大的差別是什麼？

### 題目 7（動手題）

如果你要新增一個 `YamlScanner`，讓它能掃某種 `.yml` lineage 規則檔，你大概要做哪幾步？

### 題目 8（動手題）

如果你要新增一個輸出格式 `JsonRenderer`，大概要改哪些地方？

---

## 12. 參考答案

### 題目 1

1. source node 是 `mart.orders`
2. target node 是 `reports.user_orders`
3. edge：`mart.orders -> reports.user_orders`

### 題目 2

兩個原因：

- `Vec<LineageEdge>` — 一個檔案通常會產生**多條**邊（一份 SQL 有 `FROM` 又有 `JOIN`）
- `Result<...>` — 掃描過程可能失敗（檔案壞掉、SQL 語法不合法、regex 不匹配）

所以回傳型別得同時表達「一堆邊」和「可能失敗」，那就是 `Result<Vec<LineageEdge>>`。

### 題目 3

因為 orchestrator 要能把不同型別的 scanner 放在**同一個清單**裡統一處理。`SqlScanner`、`DbtScanner`、`PythonScanner` 是三個不同的型別，如果用 `Vec<SqlScanner>` 就只能放一種。用 `Vec<Box<dyn Scanner>>` 就能把它們都當成「某個實作了 `Scanner` 的東西」放在一起。

### 題目 4

因為 graph 天生就表達「多個上游匯流到同一個下游」：

```text
raw.orders ----\
                -> mart.order_summary
raw.users  ----/
```

`upstream("mart.order_summary")` 只要沿著 incoming edges 往回走，就能自動把兩個（或更多）上游都找出來。如果用 dict 或 list 存，你得自己處理「同一個下游對應多個上游」的資料結構——graph 把這件事內建了。

### 題目 5

因為 edge 必須指向已存在的節點。你沒辦法把一條邊連到一個還不存在的 node 上。所以順序必須是：

1. 先建節點
2. 再連邊

`ensure_node()` 的語義就是「不在就建、在就用」，讓 caller 不用自己檢查。

### 題目 6

- **`impact`** 是**結構走訪**——沿著 edges 往下走，找出所有（直接或間接）會被影響到的節點。本質上是 BFS/DFS。
- **`stats`** 是**屬性統計**——不走訪，只是掃 graph 裡的所有節點，統計 type、count、或過濾特定欄位。本質上是 filter + aggregate。

### 題目 7

大方向：

1. 在 `src/scanner/` 下新增 `yaml.rs`
2. 在 `yaml.rs` 裡定義 `pub struct YamlScanner;`
3. 寫 `impl Scanner for YamlScanner { ... }`，實作 `extensions()` 和 `scan_file()`
4. 在 `scanner/mod.rs` 加上 `pub mod yaml;`
5. 在 `ScanOrchestrator::default_scanners()` 裡把 `YamlScanner` 也塞進去
6. 寫幾個單元測試放在 `#[cfg(test)] mod tests { ... }` 裡

### 題目 8

大方向：

1. 在 `src/output/` 下新增 `json.rs`
2. 定義 `pub struct JsonRenderer;`
3. 寫 `impl Renderer for JsonRenderer { ... }`
4. 在 `output/mod.rs` 加上 `pub mod json;`
5. 在 `cli.rs` 裡的 `OutputFormat` enum 新增 `Json` variant
6. 在對應的 match 裡加上 `OutputFormat::Json => Box::new(JsonRenderer)`
7. 改一下 clap 的 `#[arg(value_enum)]`，讓 CLI 能接 `--format json`

---

做完這兩個動手題，也歡迎拿這個專案當你下一個學習專案的起點。
