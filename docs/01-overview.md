# 01 — 專案導覽篇：這個 repo 到底長什麼樣

如果你已經摸過 Rust 一陣子，大致能認得 `struct`、`enum`、`impl`、`trait`、`Vec`、`HashMap`、`Result`，但打開一個真實的 repo 之後還是會有「每一行都看得懂，但不知道整體為什麼要這樣拆」的那種模糊感——這份就是為了把那種模糊感慢慢整理掉。

我自己從 hello world 走到能看懂中型專案，中間也卡了很久。看到 `mod`、`src/xxx/mod.rs`、trait object、一堆 `impl Scanner for ...`，語法都認得，但就是不知道為什麼要這樣組織、trait 為什麼值得花力氣定義。這份文件想把當時卡住的那些點整理出來，讓下一個走同一條路的人好走一點。

語法層面的複習會出現在 [02 — Rust 複習篇](./02-rust-notes.md)，讀到卡住時隨時翻。

## 1. 先講結論：這個專案到底在做什麼

一句話講完：**它會讀你的 `.sql`、dbt model、`.py` ETL 程式，不執行它們，只靠原始碼就推導出資料從哪裡流到哪裡。**

舉個例子。假設你的 repo 裡有這段 SQL：

```sql
CREATE VIEW reports.daily_revenue AS
SELECT *
FROM mart.mart_orders;
```

我們讀這段 SQL，很快會得到一個結論：這是在建一張新 view `reports.daily_revenue`，資料來自 `mart.mart_orders`。lineage-lite 做的事，就是把這個解析過程用程式重現一次——抽出來就是一條邊：

```text
mart.mart_orders -> reports.daily_revenue
```

如果旁邊還有一段 Python：

```python
events = pd.read_sql("raw.events", con=engine)
enriched.to_sql("staging.enriched_events", con=engine)
```

讀這段，直覺會看到：資料從 `raw.events` 進來，被 `sync_events_to_warehouse.py` 處理過，最後寫回 `staging.enriched_events`。對應到程式裡的解析結果：

```text
raw.events -> sync_events_to_warehouse.py
sync_events_to_warehouse.py -> staging.enriched_events
```

專案的核心工作就是把這些「我們一眼就能看懂的關係」從原始碼裡用程式抓出來，組成一張圖，之後做各種查詢。

## 2. 把整件事拆成四步

可以先把整個專案想成四步：

1. 讀檔案
2. 從檔案裡抓出 source → target 關係
3. 把關係組成 graph
4. 對 graph 做查詢與輸出

對應到 repo：

```text
scanner/  -> 負責第 1, 2 步
graph/    -> 負責第 3, 4 步的一部分
cli.rs    -> 把整個流程串起來
output/   -> 負責把結果印出來
storage/  -> 負責把結果存起來
```

如果要用最少的字來概括這個專案的核心資料，大概就是三個詞：**Node、Edge、Graph**。整個 repo 都圍繞著這三個東西轉。

## 3. 先看模組分工，不急著鑽語法

第一次打開這個 repo，與其急著點開某個 `.rs` 檔，不如先停在目錄層，看一下 `src/` 底下的結構：

```text
src/
  main.rs
  lib.rs
  cli.rs
  error.rs
  graph/
  scanner/
  output/
  storage/
```

一個個對應它在做什麼：

- `main.rs` — 真正的程式入口，但幾乎沒寫東西
- `lib.rs` — 告訴你這個 crate 對外有哪些主要模組
- `cli.rs` — 接使用者指令（`scan`、`impact`、`show`...）
- `error.rs` — 統一的錯誤型別
- `scanner/` — 真正去分析 `.sql`、`.py` 檔案的地方
- `graph/` — 把掃出來的關係存成圖，支援 upstream/downstream 查詢
- `output/` — 把結果輸出成表格、DOT、HTML
- `storage/` — 把 scan 結果存到 SQLite

### `main.rs` 幾乎沒做事

打開 [`../src/main.rs`](../src/main.rs)：

```rust
fn main() {
    if let Err(e) = lineage_lite::cli::run() {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}
```

就這樣。重點不是語法，是**設計**：入口很薄，真正的邏輯全部被推進 library 裡。這是中型 Rust 專案很常見的模式——`main.rs` 只負責「啟動」，其他東西都寫在 library 裡，這樣之後可以被其他 Rust 專案當 dependency 引用。

## 4. trait 在這裡不是語法題，是抽象邊界

教科書介紹 trait 的時候，範例通常長這樣：

```rust
trait Animal { fn speak(&self); }
impl Animal for Dog { ... }
impl Animal for Cat { ... }
```

看完難免會有個疑問：這跟直接寫 `Dog::speak()` 和 `Cat::speak()` 有什麼差別？為什麼要多繞一圈？答案在真實專案裡才會浮現。我舉三個這個 repo 的例子。

### 例子 1：Scanner trait

[`../src/scanner/mod.rs`](../src/scanner/mod.rs)：

```rust
pub trait Scanner: Send + Sync {
    fn extensions(&self) -> &[&str];
    fn scan_file(&self, path: &Path, content: &str) -> Result<Vec<LineageEdge>>;
}
```

這兩個方法定義了「能掃某種檔案的東西」長什麼樣：要能告訴別人「我負責哪幾種副檔名」，也要能「拿到一份 content，吐出一堆 lineage edge」。

目前有三個實作：

- `SqlScanner`
- `DbtScanner`
- `PythonScanner`

真正的價值在於：**orchestrator 不需要知道這三個具體是什麼，只要知道它們都是 `Scanner`**。明天要新增一個 `YamlScanner`、後天要新增 `TerraformScanner` 也一樣——trait 已經把那道抽象牆畫好了，實作端可以自由替換。

### 例子 2：Renderer trait

[`../src/output/mod.rs`](../src/output/mod.rs)：

```rust
pub trait Renderer {
    fn render_edges(&self, edges: &[&LineageEdge], writer: &mut dyn Write) -> Result<()>;
    fn render_nodes(&self, nodes: &[&Node], writer: &mut dyn Write) -> Result<()>;
}
```

Table renderer、Dot renderer、HTML renderer 都實作這組方法。所以 `cmd_scan()` 只要寫一次「把結果丟給 renderer」的邏輯，使用者選哪個輸出格式都走同一條路。

### 例子 3：StorageBackend trait

[`../src/storage/mod.rs`](../src/storage/mod.rs)：

```rust
pub trait StorageBackend {
    fn save(&self, graph: &LineageGraph, metadata: &ScanMetadata) -> Result<()>;
    fn load(&self) -> Result<(LineageGraph, ScanMetadata)>;
}
```

今天用 SQLite，明天要換 Parquet、PostgreSQL、或是直接寫 JSON 到 S3，都只要新增一個 impl，上層程式碼一行都不用改。

---

看完這三個例子，大概就能體會到：**trait 不是語法題，是「我不想讓上層程式碼知道下層用哪個實作」的那道牆。** 定義 trait 的那一刻，就是在畫出抽象邊界。

## 5. 為什麼看起來像一個小框架

讀到這裡可能會覺得這個 repo 的結構有點眼熟——像某種小框架。因為它真的有：

- 有抽象介面（`Scanner`、`Renderer`、`StorageBackend`）
- 有多個具體實作
- 有一個地方負責組裝它們

那個「組裝」的角色就是 `ScanOrchestrator`，在 [`../src/scanner/mod.rs`](../src/scanner/mod.rs)：

```rust
pub struct ScanOrchestrator {
    scanners: Vec<Box<dyn Scanner>>,
}
```

`Vec<Box<dyn Scanner>>` 這串記號第一次看容易眼花。可以先這樣理解：**一個清單，裡面裝著不同型別的值，但它們都保證實作了 `Scanner`**。更細的解釋在 02 篇第 5 節。

orchestrator 做的事其實很簡單：走訪目錄，看每個檔案的副檔名，交給對應的 scanner，把所有 scanner 吐出來的 edges 收集起來。整條流程不需要知道 `SqlScanner` 或 `DbtScanner` 具體是什麼——它只看 trait。

## 6. `crate::`、`super::` 在看什麼

這兩個東西在 Rust 專案裡滿街都是，不搞懂會一直覺得路徑很神祕。

### `crate::`

```rust
use crate::graph::node::{LineageEdge, NodeId};
```

意思是：**從整個 crate 的根開始算路徑**。不管這行寫在多深的子模組裡，`crate::graph::node` 都指向同一個地方。

### `super::`

```rust
use super::Scanner;
```

意思是：**從目前模組的上一層開始算**。例如這行寫在 `scanner/sql.rs` 裡，`super` 就是 `scanner`，所以 `super::Scanner` 就是「`scanner` 模組裡定義的 `Scanner` trait」。

你會在 `sql.rs`、`dbt.rs`、`python.rs` 裡都看到這行——因為它們都要引用上一層定義的 trait。

## 7. Node、Edge、Graph 是整個專案的共同語言

如果要挑第一個 `.rs` 檔來讀，[`../src/graph/node.rs`](../src/graph/node.rs) 會是一個很好的起點。

這個檔案不長，但整個 repo 都在操作這裡定義的型別：

- `NodeId` — 節點的唯一識別
- `NodeKind` — 節點類型（dbt model / source / SQL table / Python ETL...）
- `Node` — 節點本身
- `EdgeRelation` — 邊的種類
- `LineageEdge` — 邊本身
- `TransformKind` — 欄位層級的轉換類型（direct / SUM / expression...）
- `ColumnLineage` — 欄位層級的 lineage

特別值得看一下 `EdgeRelation`：

```rust
pub enum EdgeRelation {
    SelectFrom,
    JoinOn,
    InsertInto,
    CreateTableAs,
    DbtRef,
    DbtSource,
    PythonReadWrite,
}
```

這不是隨便取的名字——它把「SQL `FROM`」「dbt `ref()`」「Python `read_sql/to_sql`」這些概念直接編碼成 Rust 型別。用 enum 表達領域概念是 Rust 專案很常見的做法，而且 compiler 會幫你檢查：寫 `match edge.relation` 的時候忘了處理哪一種，它會直接給 warning。

## 8. SQL scanner 在做什麼

SQL scanner 的流程其實很簡單：

```text
把 SQL parse 成 AST
從 AST 找出 source tables
再找出 target table
最後產生 edges
```

它不用 regex，是因為真實 SQL 充滿 `JOIN`、`WITH`、子查詢、`UNION`、CTE，regex 永遠追不完。所以這裡用 `sqlparser-rs` 把 SQL parse 成 AST，然後在 AST 上走訪。

舉個例子：

```sql
CREATE VIEW reports.daily_revenue AS
SELECT *
FROM mart.mart_orders;
```

讀這段 SQL 是直覺的：「這是建一張 view，資料來自 `mart.mart_orders`」。parse 出來之後，程式看到的則是一棵 `Statement::CreateView` 節點，query 底下有 `from: [Table("mart.mart_orders")]`。scanner 看到 `CreateView` 就知道 target 是 `reports.daily_revenue`，看到 `from` 就知道 source 是 `mart.mart_orders`。一條 edge 就出來了。

`JOIN`、CTE、子查詢都是在這個 AST 走訪的過程裡處理的。細節可以之後看 [`../TECHNICAL.md`](../TECHNICAL.md)，現在先有這個概念就夠。

## 9. graph 查詢在做什麼

[`../src/graph/mod.rs`](../src/graph/mod.rs) 的核心結構：

```rust
pub struct LineageGraph {
    graph: DiGraph<Node, LineageEdge>,
    index: HashMap<NodeId, NodeIndex>,
}
```

兩個東西：

- `graph` — 底層是 `petgraph` 的 `DiGraph`，真正的圖結構
- `index` — 從 `NodeId`（字串 ID）快速查到對應的 `NodeIndex`

為什麼要 index？因為 `DiGraph` 內部用 `NodeIndex` 標示節點，但使用者下指令時用的是名字（`mart.orders`），所以每次查詢都要有一層「名字 → 內部索引」的轉換。用 `HashMap` 做這層 O(1) 查找是最自然的寫法。

有了這兩個，常見查詢就變得很直接：

- `downstream(node)` — 沿著 outgoing edges 走，找出這個節點會影響誰
- `upstream(node)` — 沿著 incoming edges 走，找出這個節點依賴誰
- `impact(node)` — 本質上就是不限深度的 downstream

## 10. `build_graph()` 是最值得追的一條流程

如果要挑一條 code path 從頭追到尾，追 [`../src/cli.rs`](../src/cli.rs) 裡的 `build_graph()` 最划算。它是所有指令的共同前置——不管跑 `scan`、`impact`、`show`、`stats`，開頭都會先呼叫它。

大致流程：

1. 建立 `ScanOrchestrator`（裝好所有 scanner）
2. 掃描目錄，拿到 `ScanResult`
3. 對每一條 edge，先 `ensure_node()`（確保 source 和 target 都已經在 graph 裡）
4. 再 `add_edge()`
5. 最後把欄位資訊補進 graph（for column-level lineage）

用比較白話的方式講：

```text
scanner 負責找關係
graph 負責存結構
cli 負責把兩者接起來
```

第 3 步為什麼要先 `ensure_node`？因為 edge 必須連兩個已存在的 node，不能連到一個還沒建立的節點。所以順序一定是：先建節點，再連邊。這是 03 篇會再展開的細節。

## 11. 一條建議的閱讀順序

我自己第一次讀這個 repo 時，走得最順的路線是：

1. [`../BEGINNER_GUIDE.md`](../BEGINNER_GUIDE.md)
2. [`../src/graph/node.rs`](../src/graph/node.rs) — 先理解共同語言（型別）
3. [`../src/scanner/mod.rs`](../src/scanner/mod.rs) — 看 trait 和 orchestrator
4. [`../src/scanner/dbt.rs`](../src/scanner/dbt.rs) — 最簡單的 scanner 實作（regex）
5. [`../src/scanner/python.rs`](../src/scanner/python.rs) — 還是 regex，但難一點
6. [`../src/scanner/sql.rs`](../src/scanner/sql.rs) — 這個最硬，留到最後
7. [`../src/graph/mod.rs`](../src/graph/mod.rs) — graph 怎麼存、怎麼查
8. [`../src/cli.rs`](../src/cli.rs) — 把上面全部串起來

為什麼 `scanner/sql.rs` 要留到最後？因為它用 AST、有 CTE 穿透、有 column-level lineage，是整個 repo 最複雜的一個檔案。先看過其他 scanner、建立「scanner 長什麼樣子」的直覺之後，再來看它會好讀很多。

## 12. 哪些 Rust 細節第一次先不用卡住

第一次讀時，這些東西看不懂**沒關係**，之後再回頭看：

- `#[derive(...)]` — 先當作「自動幫型別補常見能力」
- `Send + Sync` — 多執行緒相關，目前暫時用不到
- trait object 的底層實作（vtable 那些）
- `Entry::Vacant` / `Entry::Occupied` — `HashMap` 的細節
- serde 的各種 attribute

相對值得先搞懂的是：

- 模組怎麼分工
- trait 怎麼定義抽象邊界
- scanner 怎麼產 edge
- graph 怎麼支援查詢
- CLI 怎麼把整個流程組起來

把這五件事理解清楚，大概就已經能開始動手改這個 repo 了。其他細節會在實際動手的過程裡慢慢長出來。
