# lineage-lite 技術解析

這份文件拆解 lineage-lite 背後的解析原理、用到的 Rust 手法、以及程式碼裡值得注意的設計決策。

如果你是第一次看 Rust 專案，先記住一件事：**這份文件不是要你一次學會所有 Rust 語法，而是幫你分辨哪些手法是理解專案運作的關鍵，哪些可以先略過。**

---

## 先讀這段：Rust idiom 哪些重要，哪些可以先跳過

這個專案確實用了不少 Rust 常見手法，但它們的重要性不一樣。

### 第一輪閱讀一定要懂的

下面這些是你理解整個專案怎麼運作時，最值得先掌握的：

| 手法 | 在這個專案裡的用途 | 為什麼重要 |
|------|-------------------|-----------|
| **module / `mod`** | 把 `scanner`、`graph`、`output`、`storage` 分層 | 幫你看懂專案責任邊界 |
| **`crate::` / `super::`** | 表示依賴方向 | 幫你看懂某個檔案是在用全域共用型別，還是在實作上層介面 |
| **enum + `match`** | 表示 `Statement`、`EdgeRelation`、`TransformKind` 等有限狀態 | 幫你看懂 parser 與 domain model 的核心控制流 |
| **trait** | 定義 `Scanner`、`Renderer`、`StorageBackend` 抽象 | 幫你看懂可替換的架構邊界 |
| **ownership / borrow** | `add_node(node)` 搬入所有權，`get_node(&self)` 借出 reference | 幫你理解 graph API 為什麼這樣設計 |
| **`Result<T, E>` + `?`** | 統一錯誤傳遞 | 幫你看懂流程怎麼在錯誤時提早返回 |

### 第一輪閱讀可以先跳過的

下面這些也有用，但第一次讀 code 不需要卡住：

| 手法 | 在這個專案裡的用途 | 第一次可否先略過 |
|------|-------------------|----------------|
| **derive macro** | 自動產生 `Debug`、`Clone`、`Serialize` 等實作 | 可以 |
| **newtype pattern** | `NodeId(pub String)` 把字串包成專用型別 | 可以先把它當成「比較安全的 String」 |
| **trait object / `Box<dyn Trait>`** | 執行期存放不同 scanner / renderer | 可以先把它當成「統一介面的多型容器」 |
| **`Send + Sync`** | 限制 trait object 可安全跨執行緒共享 | 這個專案裡目前不用深究 |
| **`Entry::Vacant`** | BFS 時避免重複 lookup | 屬於效能與 API 細節，先略過沒關係 |
| **serde 屬性像 `#[serde(default)]`** | 反序列化時補預設值 | 不影響主流程理解 |

### 最實際的讀法

如果你一看到 Rust 語法就想停下來查，會很容易迷路。比較有效的方式是：

1. 先看這行 code 在流程中扮演什麼角色
2. 再判斷這個語法是「理解邏輯所必需」，還是「Rust 的包裝細節」

例如：

- 看到 `match stmt`
  這通常很重要，因為它在分流不同 SQL statement

- 看到 `#[derive(...)]`
  這通常不重要，因為它只是自動產生 boilerplate

- 看到 `Box<dyn Scanner>`
  你至少要知道它表示「一組可替換 scanner」，但不用立刻深入 vtable 或 dynamic dispatch

### 一句話版本

第一次讀這個專案時，請把注意力優先放在：

- 資料流怎麼被提取
- graph 怎麼建立
- 模組如何分工
- trait 抽象怎麼把不同實作接起來

而不是把時間花在每一個 Rust 語法糖上。

---

## 一、解析原理：怎麼從原始碼裡「看出」資料流

lineage-lite 的核心工作是靜態分析（static analysis），不需要實際執行 SQL 或跑 Python，光讀原始碼就能推導出資料的流向。但不同類型的檔案，解析方式完全不同。

### SQL：AST 語法樹解析

SQL 檔案用的是正式的 AST（Abstract Syntax Tree）解析，靠 [sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs) 這個 crate。

做法是把 SQL 字串丟進 parser，拿回一棵語法樹，然後遞迴地往下走，找出我們要的結構：

```
SQL 字串
  → Parser::parse_sql()
  → Vec<Statement>
  → 對每個 Statement 做 pattern match
```

以 `CREATE TABLE mart.orders AS SELECT * FROM raw.orders o JOIN raw.payments p ON ...` 為例，parser 會產出一個 `Statement::CreateTable`，裡面包含：

- `name`: `mart.orders`（目標表）
- `query`: 一個 `Query` 結構，裡面有 `FROM` 子句和 `JOIN` 子句

我們從 `name` 拿到 target，從 `query` 裡面遞迴地提取所有 source table，就能產出兩條 edge：

```
raw.orders → mart.orders (CreateTableAs)
raw.payments → mart.orders (JoinOn)
```

需要處理的 SQL 結構比想像中多：

| SQL 結構 | 怎麼處理 |
|---------|---------|
| `CREATE TABLE AS SELECT` | 從 query body 提取所有 FROM/JOIN 的表 |
| `CREATE VIEW AS SELECT` | 同上 |
| `INSERT INTO ... SELECT` | target 從 INSERT 拿，source 從 SELECT 拿 |
| CTE（`WITH ... AS`） | 記錄 CTE 名稱，遞迴提取 CTE body 裡的來源，但排除 CTE 名稱本身（它不是外部表） |
| 子查詢（`FROM (SELECT ...)`） | 遞迴進入 `Derived` 節點，繼續提取 |
| UNION / INTERSECT | 左右兩邊都遞迴提取 |

整個提取邏輯是一組互相呼叫的函式，每個處理一層結構：

```
extract_sources_from_query()         ← 入口：處理 CTE + body
  → extract_cte_sources()            ← 處理 WITH 子句
  → extract_sources_from_set_expr()  ← 處理 UNION / SELECT / 子查詢
    → extract_sources_from_select()  ← 處理 FROM + JOIN
      → extract_source_from_table_factor()  ← 處理單一表、子查詢、巢狀 JOIN
```

這種層層遞迴的結構，正好對應 SQL 語法本身的巢狀特性。

### Column-Level Lineage：從 SELECT list 往回追

table-level 只告訴你「A 表流到 B 表」，column-level 要追到「B 表的 `total` 欄位是 A 表的 `amount` 經過 `SUM()` 算出來的」。

做法是對 SELECT list 裡的每個 item 做 expression 分析：

```rust
match item {
    SelectItem::UnnamedExpr(expr) => analyze_expr(expr, alias_map),
    SelectItem::ExprWithAlias { expr, alias } => analyze_expr(expr, alias_map),
    SelectItem::Wildcard(_) => /* 無法追蹤，跳過 */,
}
```

`analyze_expr()` 遞迴地拆解 expression，判斷轉換類型：

| Expression 類型 | 判定結果 | 範例 |
|----------------|---------|------|
| 單一欄位引用 | `Direct` | `id`、`o.amount` |
| `SUM()` / `COUNT()` / `AVG()` | `Aggregation("SUM")` | `SUM(amount)` |
| `LAG()` / `ROW_NUMBER()` + `OVER` | `Window("LAG")` | `LAG(x, 12) OVER (...)` |
| 二元運算 / CASE WHEN | `Expression` | `a + b`、`CASE WHEN ...` |
| 常數值 | `Constant` | `'order'`、`42` |

這裡有一個重要的細節：**alias 解析**。SQL 裡面的 `o.amount` 裡的 `o` 是 alias，不是真正的表名。所以在分析之前，先從 FROM 子句建了一個 `AliasMap`（`HashMap<String, String>`），把 alias 對應回真正的表名。

另一個細節是 **CTE 穿透**。如果最外層是 `SELECT * FROM final_cte`，只會拿到一個 wildcard，追不到東西。所以程式碼會偵測這個情況，找到對應的 CTE 定義，再往裡面追一層。

### dbt：Regex 抓 Jinja 語法

dbt model 的 `.sql` 檔案裡面充滿 Jinja template 語法，sqlparser 完全沒辦法解析。但我們不需要理解整個 Jinja，只需要抓兩個 pattern：

```
{{ ref('stg_orders') }}         → DbtRef edge
{{ source('raw', 'orders') }}   → DbtSource edge
```

所以 dbt scanner 的做法非常直接：用 regex 掃一遍，抓到就產 edge。

判斷一個 `.sql` 檔案要不要交給 dbt scanner 處理，只看一個條件：**檔案內容裡有沒有 `{{`**。有就是 dbt，沒有就是純 SQL。這個判斷很粗暴但實務上非常有效，因為一般 SQL 檔案不會出現雙大括號。

同時，SQL scanner 也用同樣的條件反向排除：看到 `{{` 就跳過，避免 sqlparser 去解析 Jinja 然後噴錯。

### Python：Regex 抓 read/write pattern

Python ETL 的解析更粗糙，因為 Python 不像 SQL 有明確的語法結構可以 parse。做法是用 regex 找常見的資料讀寫 pattern：

**讀取 pattern（source）：**
- `pd.read_sql("raw.events", ...)`
- `spark.table("raw.events")`
- `.sql("SELECT ... FROM raw.events")`

**寫入 pattern（sink）：**
- `.to_sql("staging.events", ...)`
- `.saveAsTable("mart.orders")`
- `.insertInto("staging.events")`
- `.write.save("...")`

每抓到一個讀取，就產一條「表 → Python job」的 edge；每抓到一個寫入，就產一條「Python job → 表」的 edge。

這裡有一個小技巧：`looks_like_table_name()` 函式會過濾掉不像表名的字串（包含空格、斜線、太長），避免把原始 SQL 查詢誤判成表名。

---

## 二、Rust 手法

### Newtype Pattern：`NodeId`

```rust
pub struct NodeId(pub String);
```

`NodeId` 就是一個包著 `String` 的 newtype。為什麼不直接用 `String`？因為型別系統會幫你擋住「把隨便一個字串當成 node id 傳進去」的錯誤。

比如 `add_edge` 的參數是 `NodeId` 不是 `String`，你不可能不小心把檔案路徑傳進去。

同時 `NodeId` 實作了 `From<&str>`，轉換時會自動 `.to_lowercase()`，確保所有 id 一致：

```rust
impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        Self(s.to_lowercase())
    }
}
```

### Enum 編碼領域知識

這個專案的 enum 不只是拿來分類，而是在型別裡直接編碼了領域知識：

```rust
pub enum EdgeRelation {
    SelectFrom,       // SQL SELECT FROM
    JoinOn,           // SQL JOIN
    InsertInto,       // SQL INSERT INTO
    CreateTableAs,    // SQL CTAS
    CteReference,     // CTE 引用
    DbtRef,           // dbt ref()
    DbtSource,        // dbt source()
    PythonReadWrite,  // Python ETL
}
```

```rust
pub enum TransformKind {
    Direct,                 // 直接 passthrough
    Aggregation(String),    // SUM / COUNT（帶函式名）
    Expression,             // 算式
    Window(String),         // 窗函式（帶函式名）
    Macro,                  // Jinja macro
    Constant,               // 常數
    Unknown,                // 無法判斷
}
```

注意 `Aggregation(String)` 和 `Window(String)` 帶了 payload，存的是函式名稱（像 `"SUM"` 或 `"LAG"`）。這樣輸出的時候可以直接印出 `SUM()` 而不只是「某種聚合」。

Rust 的 enum 本質上是 tagged union（又叫 sum type），每個 variant 可以攜帶不同形狀的資料。這在其他語言裡通常要用 class hierarchy 或 interface + multiple implementations 來做。

### Trait Object 實現 Strategy Pattern

三個 scanner（SQL、dbt、Python）都實作同一個 trait：

```rust
pub trait Scanner: Send + Sync {
    fn extensions(&self) -> &[&str];
    fn scan_file(&self, path: &Path, content: &str) -> Result<Vec<LineageEdge>>;
}
```

`ScanOrchestrator` 持有一個 `Vec<Box<dyn Scanner>>`，遍歷目錄時，根據副檔名動態 dispatch：

```rust
pub struct ScanOrchestrator {
    scanners: Vec<Box<dyn Scanner>>,
}
```

同樣的模式也出現在 output 層：

```rust
pub trait Renderer {
    fn render_edges(&self, edges: &[&LineageEdge], writer: &mut dyn Write) -> Result<()>;
    fn render_nodes(&self, nodes: &[&Node], writer: &mut dyn Write) -> Result<()>;
}
```

以及 storage 層：

```rust
pub trait StorageBackend {
    fn save(&self, graph: &LineageGraph, metadata: &ScanMetadata) -> Result<()>;
    fn load(&self) -> Result<(LineageGraph, ScanMetadata)>;
}
```

**三層都用了同一個手法**：定義 trait → 多個 struct 各自實作 → 呼叫端持有 `Box<dyn Trait>` 做動態分派。好處是加新的 scanner / renderer / storage backend 不需要改既有程式碼，只需要加一個新的 struct 然後實作 trait。

### `#[derive]` 和 thiserror

幾乎所有 domain type 都用了 derive macro 批量產生實作：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);
```

一行 derive 就得到：debug 印出、複製、比較、hash（可以塞 HashMap）、JSON 序列化/反序列化。這是 Rust 裡面減少 boilerplate 最主要的手段。

錯誤處理用 [thiserror](https://github.com/dtolnay/thiserror)，把整個專案的錯誤統一成一個 enum：

```rust
#[derive(Debug, thiserror::Error)]
pub enum LineageError {
    #[error("IO 錯誤: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQL 解析錯誤於 {file}: {message}")]
    SqlParse { file: PathBuf, message: String },

    #[error("儲存錯誤: {0}")]
    Storage(#[from] rusqlite::Error),

    #[error("找不到節點: {0}")]
    NodeNotFound(String),
}
```

`#[from]` 讓 `std::io::Error` 和 `rusqlite::Error` 可以用 `?` 運算子自動轉型成 `LineageError`。所以整個 library 裡面不需要手動寫 `.map_err()`（除了 rusqlite 那邊因為型別推導的關係還是需要）。

搭配 `type Result<T> = std::result::Result<T, LineageError>;` 這個 type alias，所有函式簽名都很乾淨。

### 所有權與借用

`LineageGraph` 的 API 設計很注意所有權：

- `add_node(node: Node)` — 接收所有權，node 搬進 graph
- `add_edge(edge: LineageEdge)` — 同上
- `get_node(&self, id: &NodeId) -> Option<&Node>` — 借出 reference，不轉移所有權
- `nodes(&self) -> Vec<&Node>` — 回傳 reference 的 vector
- `impact(&self, id: &NodeId) -> Result<Vec<&Node>>` — BFS 結果也是 reference

寫入的時候交出所有權，讀取的時候只借不拿。這樣 graph 始終是唯一 owner，不需要 `Rc` 或 `Arc`。

### Graceful Degradation：解析失敗不中斷

SQL scanner 遇到解析失敗的檔案，不會噴 error 中斷整個 scan，而是靜靜跳過：

```rust
let statements = match Parser::parse_sql(&dialect, content) {
    Ok(stmts) => stmts,
    Err(_) => return Ok(Vec::new()),  // 跳過，繼續下一個檔案
};
```

column-level lineage 和欄位提取也是同樣的態度：做到多少算多少，做不到就回空。這在 `extract_columns_from_sql`、`extract_column_lineage` 裡都是一樣的做法。

這是實務上很重要的設計決定，因為真實 repo 裡一定有各種奇怪的 SQL dialect、不完整的檔案、或帶有 vendor-specific 語法的東西，不可能每個都能解析。

---

## 三、Rust 模組與架構手法

前一節講的是 Rust 語言層面的技巧；這一節講的是 **這個專案怎麼用 Rust 的模組系統和 trait，把整體架構撐起來**。

很多人第一次看這個 repo，會有一種「怎麼有點像 framework」的感覺。這不是錯覺，因為它確實用了幾個常見的架構手法：

- 用模組切出清楚的責任邊界
- 用 trait 定義抽象介面
- 用 `Box<dyn Trait>` 做執行期替換
- 讓高層模組依賴抽象，不直接依賴具體實作

只是這個專案規模很小，所以這些手法看起來不會像大型 Java / C# 專案那麼重。

### 模組系統：先切邊界，再談抽象

Rust 的 module system 在這個專案裡不是拿來「分資料夾而已」，而是直接對應架構分層：

```text
src/
  main.rs      → 程式入口
  lib.rs       → crate 對外暴露哪些模組
  cli.rs       → orchestration / command handling
  graph/       → 核心資料模型與查詢
  scanner/     → 各種原始碼解析器
  output/      → 各種輸出 renderer
  storage/     → 持久化 backend
  error.rs     → 統一錯誤型別
```

`src/lib.rs` 很短：

```rust
pub mod cli;
pub mod error;
pub mod graph;
pub mod output;
pub mod scanner;
pub mod storage;
```

這行為上的意思不是「方便 import」而已，而是：

- 這六塊被視為這個 crate 的正式對外模組
- `main.rs` 不直接碰各個內部檔案，而是透過 crate API 進入

所以 `main.rs` 才能很乾淨：

```rust
fn main() {
    if let Err(e) = lineage_lite::cli::run() {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}
```

入口只負責：

- 呼叫 `cli::run()`
- 印錯誤
- 設 exit code

真正的邏輯都留在 library 內部。這種做法的好處是：**CLI 只是外殼，核心邏輯是 library**。

### `crate::` 與 `super::`：依賴方向的視覺提示

這個 repo 裡很常看到兩種 import：

```rust
use crate::error::Result;
use crate::graph::node::{LineageEdge, Node};
```

以及：

```rust
use super::Scanner;
use super::Renderer;
use super::{ScanMetadata, StorageBackend};
```

它們代表的依賴方向不同。

#### `crate::`

`crate::...` 表示「從整個 crate 的根往下拿某個模組」。

例如 `src/scanner/sql.rs` 裡：

```rust
use crate::graph::node::{ColumnLineage, EdgeRelation, LineageEdge, NodeId, SourceColumn, TransformKind};
```

意思是 SQL scanner 依賴的是整個系統共用的 domain type，而不是自己私有的型別。

這讓你一眼就能看出：

- `graph::node` 是核心 domain model
- scanner / output / storage 都圍繞這批 type 在工作

#### `super::`

`super::...` 表示「從目前模組的上一層拿東西」。

例如 `src/scanner/sql.rs` 裡：

```rust
use super::Scanner;
```

意思不是語法花招，而是很具體的架構訊號：

- `Scanner` trait 定義在 `scanner/mod.rs`
- `sql.rs`、`dbt.rs`、`python.rs` 都是 scanner family 的子模組
- 子模組不是自己發明一套介面，而是實作上層已定義好的契約

同樣的模式也出現在：

- `output/table.rs` / `output/dot.rs` 用 `super::Renderer`
- `storage/sqlite.rs` 用 `super::StorageBackend`

所以 `super::` 在這個專案裡可以理解成：

**「我不是獨立模組，我是某個 family 底下的一個具體實作。」**

### Trait 當作架構邊界

這個專案最像 framework 的地方，不是 enum 或 match，而是這三個 trait：

```rust
pub trait Scanner { ... }
pub trait Renderer { ... }
pub trait StorageBackend { ... }
```

它們分別對應三種可替換能力：

- Scanner：怎麼從某種檔案提取 lineage
- Renderer：怎麼把結果輸出
- StorageBackend：怎麼存取 graph

重點不是「Rust 有 trait」，而是 **這個專案把 trait 放在抽象層，具體實作放在子模組**。

例如 scanner：

```rust
pub trait Scanner: Send + Sync {
    fn extensions(&self) -> &[&str];
    fn scan_file(&self, path: &Path, content: &str) -> Result<Vec<LineageEdge>>;
}
```

這個介面完全不在乎你是：

- SQL AST parser
- dbt regex scanner
- Python regex scanner

只在乎兩件事：

1. 你處理哪些副檔名
2. 你能不能把一個檔案變成 `Vec<LineageEdge>`

這就是典型的「面向介面、不是面向實作」。

### 這其實很接近依賴反轉（Dependency Inversion）

如果用比較接近架構設計的話來說，這個專案的高層模組通常依賴抽象，不依賴具體實作。

最明顯的例子是 `ScanOrchestrator`：

```rust
pub struct ScanOrchestrator {
    scanners: Vec<Box<dyn Scanner>>,
}
```

它不知道裡面放的是：

- `SqlScanner`
- `DbtScanner`
- `PythonScanner`

它只知道每個元素都滿足 `Scanner` 介面。

同樣地：

```rust
pub fn get_renderer(format: &OutputFormat) -> Box<dyn Renderer>
```

CLI 在大多數情況下也不需要知道它拿到的是 `TableRenderer` 還是 `DotRenderer`；只要它會 `render_*` 就夠了。

Storage 也是一樣：

```rust
use crate::storage::{ScanMetadata, StorageBackend};
```

`cli.rs` 使用 storage 時，依賴的是 `StorageBackend` 這個抽象介面；`SqliteStorage` 只是目前的其中一個實作。

嚴格說這個 repo 不是 textbook 等級、完全純粹的 dependency inversion，因為：

- `cli.rs` 還是直接 `use crate::storage::sqlite::SqliteStorage;`
- `ScanOrchestrator::default_scanners()` 也直接 new 了三個 scanner

所以它不是「容器式注入」那種很重的 DI 架構。

但概念上已經非常接近：

- 抽象（trait）定義穩定邊界
- 高層流程大多透過抽象工作
- 新實作可以在不改主流程的情況下被插進來

這是一種 **輕量級、Rust 風格的依賴反轉**。

### `Box<dyn Trait>`：執行期多型，不把 enum 寫死

為什麼 scanner / renderer / storage 沒有寫成一個超大 enum？

因為這個專案希望新增能力時，不要一直回頭改中央 dispatch code。

例如：

```rust
Vec<Box<dyn Scanner>>
```

表示一個 scanner 清單，裡面的每個元素型別都可能不同，但都能用 `Scanner` 介面被呼叫。

這和 enum 的差別在於：

- enum：所有 variant 必須在中央先定義好
- trait object：只要實作 trait，就能被裝進來

所以這個設計比較像 plugin point，而不是固定 switch statement。

當然，這不是免費的：

- 有 dynamic dispatch 成本
- 型別資訊在執行期才決定

但這裡的 workload 主要是 I/O、字串解析、AST 走訪，不是數值熱路徑，所以這個 tradeoff 很合理。

### 不是「為了框架而框架」，而是把變動點抽出來

這個 repo 看起來有 framework 感，是因為它把幾個**未來最可能變動**的點先抽掉了：

- 掃描來源類型會增加
- 輸出格式會增加
- 儲存 backend 可能會增加

反過來說，有些地方就沒有被過度抽象，例如：

- graph 本身沒有再包一層 trait
- SQL scanner 內部 helper function 直接寫死在 `sql.rs`
- CLI command handler 沒有拆成一堆 command object

**只在真正有替換價值的地方做抽象。**

### 模組內聚：domain model 集中在 `graph/node.rs`

一個很關鍵的結構是：核心 domain types 大多集中在 `src/graph/node.rs`。

例如：

- `NodeId`
- `NodeKind`
- `Node`
- `EdgeRelation`
- `LineageEdge`
- `TransformKind`
- `ColumnLineage`

這個安排背後的設計想法是：

- scanner 需要產 `LineageEdge`
- graph 需要存 `Node` 和 `LineageEdge`
- output 需要 render `Node` / `LineageEdge`
- storage 需要 serialize / deserialize 它們

所以這些 type 不屬於某個單一模組，而是整個系統共用的語言。

把它們放在中心位置，可以避免：

- scanner 自己定一套 edge
- storage 又定另一套 row model
- output 再定第三套 DTO

這讓系統的語言非常一致。

### `build_graph()`：組裝點，而不是邏輯點

`cli.rs` 裡的 `build_graph()` 很值得看，因為它展示了這個架構怎麼被串起來：

1. `ScanOrchestrator::default_scanners()` 建立 scanner 集合
2. `scan_directory()` 回傳 `ScanResult`
3. 迭代 edges，`ensure_node()` 補齊節點
4. `add_edge()` 正式建圖
5. `add_columns()` 把欄位資訊合併進節點

它的角色比較像 assembly / composition root：

- scanner 負責解析
- graph 負責存結構
- CLI 只負責把這些零件接起來

這也是 framework 感的來源之一：有一個明確的「組裝點」把抽象與實作接起來。

### 小而明確的 public surface

這個專案也有一個 Rust 上很健康的特徵：public API 沒有失控。

例如：

- `scanner/mod.rs` 公開 `Scanner`、`ScanOrchestrator`、`ScanResult`
- `output/mod.rs` 公開 `Renderer`、`OutputFormat`、`get_renderer()`
- `storage/mod.rs` 公開 `StorageBackend`、`ScanMetadata`

而各子模組大多只暴露必要 struct：

- `SqlScanner`
- `DbtScanner`
- `PythonScanner`
- `TableRenderer`
- `DotRenderer`
- `SqliteStorage`

這讓 module boundary 很清楚：你不需要知道每個檔案內部所有 helper function，才能用這個系統。

### 測試模組的 `use super::*`

你還會看到很多：

```rust
#[cfg(test)]
mod tests {
    use super::*;
}
```

這是 Rust 很常見的寫法，意思是測試模組直接吃同一個檔案上層模組的所有內容。

好處是：

- 測試可以直接碰 private helper
- 測試和實作放在一起，改 code 時比較不容易 drift

所以這不只是省 import，而是「模組內測試」這種 Rust 慣例的一部分。

### 這個專案的 Rust 風格總結

如果把這些手法濃縮成一句話，這個專案的風格是：

**用 Rust 的 module + trait + enum，把一個小型工具寫成有邊界、有可替換點、但不過度企業化的架構。**

所以你感覺到「有點像為了做到設計框架」，某種程度上是對的；但更準確的說法是：

- 它確實有在做架構設計
- 但抽象只放在少數高價值位置
- 整體仍然維持小工具該有的直接性

---

## 四、圖的演算法

### 資料結構：petgraph DiGraph + HashMap 索引

```rust
pub struct LineageGraph {
    graph: DiGraph<Node, LineageEdge>,
    index: HashMap<NodeId, NodeIndex>,
}
```

[petgraph](https://github.com/petgraph/petgraph) 的 `DiGraph` 用 adjacency list 實作有向圖，node 和 edge 各自帶 payload。但 petgraph 的 API 是用 `NodeIndex`（一個整數 index）來操作的，不方便用名字查節點。所以額外維護一個 `HashMap<NodeId, NodeIndex>` 做 O(1) 的名字到 index 查找。

這是 petgraph 很常見的用法：**graph 負責結構和走訪，HashMap 負責命名查找**。

### BFS 走訪

`upstream()` 和 `downstream()` 都是 BFS（廣度優先搜尋），差別只在走的方向：

```rust
fn bfs_collect(&self, id: &NodeId, direction: Direction, max_depth: Option<usize>) -> Result<Vec<&Node>> {
    let mut queue = VecDeque::new();
    let mut visited = HashMap::new();
    queue.push_back((start, 0usize));
    visited.insert(start, 0);

    while let Some((current, depth)) = queue.pop_front() {
        if let Some(max) = max_depth {
            if depth >= max { continue; }
        }
        for neighbor in self.graph.neighbors_directed(current, direction) {
            if let Entry::Vacant(e) = visited.entry(neighbor) {
                e.insert(depth + 1);
                queue.push_back((neighbor, depth + 1));
            }
        }
    }
}
```

用 `Direction::Outgoing` 就是往下游走（downstream），`Direction::Incoming` 就是往上游走（upstream）。`impact()` 就是 `downstream(id, None)` 不限深度的版本。

手動實作 BFS 而不是用 petgraph 內建的 `Bfs` iterator，是因為需要追蹤深度（`max_depth`），petgraph 的內建版本沒有這個功能。

`visited` 用 `HashMap` 而不是 `HashSet`，是因為同時存了到達深度，方便 depth 控制。用了 `Entry::Vacant` 這個 API 來做「已經看過就跳過、沒看過就插入」的操作，比先 `contains` 再 `insert` 少一次 hash lookup。

---

## 五、設計決策

### Scanner 的優先序與互斥

`.sql` 檔案會被兩個 scanner 搶：`DbtScanner` 和 `SqlScanner`。它們的解法是用**內容特徵**互斥：

- `DbtScanner`：看到 `{{` 才處理，否則回空
- `SqlScanner`：看到 `{{` 就跳過，否則正常解析

`ScanOrchestrator` 對同一個副檔名會跑所有匹配的 scanner，但因為上面的互斥邏輯，實際上每個 `.sql` 檔案只會被其中一個處理。

### CLI 是 orchestration 層，不是邏輯層

`cli.rs` 裡面的每個 `cmd_*` 函式都是同一個結構：

1. 呼叫 `build_graph()` 掃描 + 建圖
2. 對 graph 執行查詢
3. 用 renderer 輸出

它不包含任何解析邏輯或圖的操作邏輯，純粹是把各個模組接起來。這讓每個模組都可以獨立測試。

### SQLite 做快照，不做 live store

SQLite 在這裡的定位是 scan 結果的「快照」，不是 live 的 graph store。每次 save 都會清空重寫：

```rust
conn.execute_batch("DELETE FROM edges; DELETE FROM nodes; DELETE FROM scan_metadata;")
```

這是刻意的簡化。因為 lineage-lite 的使用模式是「scan 完存起來，之後拿去 diff 或 merge」，不需要 incremental update。

SQLite 設定了 `WAL` mode（Write-Ahead Logging），讀寫不互斥，適合「一個 process 寫入，另一個 process 讀取」的場景（例如 CI pipeline 裡面）。

### 沒有用的依賴就不加

整個專案的依賴很精簡：

| crate | 用途 |
|-------|------|
| `clap` + `derive` | CLI 參數解析 |
| `sqlparser` | SQL AST 解析 |
| `petgraph` | 圖的資料結構和走訪 |
| `walkdir` | 遞迴遍歷目錄 |
| `thiserror` | 錯誤型別的 derive macro |
| `serde` + `serde_json` | 序列化（存 SQLite 的 columns_json） |
| `comfy-table` | 終端機表格輸出 |
| `rusqlite` + `bundled` | SQLite（bundled 表示自帶 SQLite，不依賴系統） |
| `regex` | dbt 和 Python 的 pattern matching |

沒有 async runtime、沒有 web framework、沒有 ORM。夠用就好。

---

## 六、測試策略

每個模組都有 `#[cfg(test)] mod tests`，測試直接寫在同一個檔案裡。這是 Rust 社群的慣例，好處是測試可以存取 module 內部的 private 函式。

測試的風格很一致：

- **Scanner 測試**：餵一段 SQL / Python 字串進去，檢查產出的 edges 是否正確
- **Graph 測試**：手動建一個小 graph，檢查 BFS / impact 的結果
- **SQLite 測試**：存進去再讀出來，確認 round-trip 正確（用 `tempfile` 建暫時檔案）
- **Column lineage 測試**：餵 SQL 進去，檢查每個欄位的 `TransformKind` 和 `source_columns`

沒有用 mock，所有測試都是對真實邏輯的端對端驗證。因為各模組之間的介面就是 `Vec<LineageEdge>`，沒有需要 mock 的外部 I/O。
