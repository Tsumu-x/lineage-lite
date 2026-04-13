# 02 — Rust 複習篇：本專案實際用到的寫法

這份不是教學，而是一本隨手可查的對照表——把本專案**真的有用到**的 Rust 寫法整理出來。讀 01 或 03 的時候如果對某個語法卡住了，翻來對照一下就好，不用逐節讀完。

## 1. `mod` 到底在做什麼

`mod` 是 Rust 學習路上第一個常常讓人稍微卡一下的東西。它的角色可以先這樣理解：

> 把程式切成模組，建立命名空間，也決定檔案之間怎麼組織。

打開 [`../src/lib.rs`](../src/lib.rs)：

```rust
pub mod cli;
pub mod error;
pub mod graph;
pub mod output;
pub mod scanner;
pub mod storage;
```

這六行在說：「這個 crate 的根下面有六個子模組。」

### `mod` 和檔案怎麼對應

當你寫 `pub mod scanner;`，Rust 會去找兩個地方之一：

- `src/scanner.rs`（單檔寫法）
- `src/scanner/mod.rs`（資料夾寫法）

本專案都用後者，因為每個模組內容都不小，分成多檔比較清爽。所以你會看到：

- [`../src/scanner/mod.rs`](../src/scanner/mod.rs) — `scanner` 模組的入口
- [`../src/scanner/sql.rs`](../src/scanner/sql.rs) — `scanner::sql` 子模組
- [`../src/scanner/dbt.rs`](../src/scanner/dbt.rs) — `scanner::dbt` 子模組

`scanner/mod.rs` 裡面會用 `pub mod sql;`、`pub mod dbt;` 把底下的檔案一一宣告出來。

### 最小範例

```rust
// lib.rs
pub mod animals;

// animals/mod.rs
pub mod dog;
pub fn count() -> usize { 1 }

// animals/dog.rs
pub fn bark() { println!("woof"); }
```

使用的時候：`animals::dog::bark()`。

## 2. `crate::`、`self::`、`super::` 差在哪

讀 `use` 語句最容易卡的就是這三個。它們的差別其實只是「從哪裡開始算路徑」。

### `crate::` — 從整個 crate 的根開始

```rust
use crate::graph::node::NodeId;
```

不管這行寫在多深的子模組裡，`crate::graph::node` 永遠指向同一個地方。適合用在「我要引用另一條路徑上的東西」。

### `self::` — 從目前模組開始

```rust
use self::sql::SqlScanner;
```

寫在 `scanner/mod.rs` 裡，`self` 就是 `scanner` 本身。不太常用，但看到了不要慌。

### `super::` — 從目前模組的上一層開始

```rust
// 寫在 scanner/sql.rs 裡
use super::Scanner;
```

`sql.rs` 的 `self` 是 `scanner::sql`，那 `super` 就是 `scanner`，所以 `super::Scanner` = `scanner::Scanner`——也就是 [`../src/scanner/mod.rs`](../src/scanner/mod.rs) 裡定義的那個 trait。

### 一個心智模型

每次看到 path 先問自己：「這個檔案在哪一層？」然後：

- `self` = 這一層
- `super` = 再上一層
- `crate` = 整個 crate 的最頂層

例如在假想的 `scanner/sql/helpers.rs` 裡：

- `self` = `scanner::sql::helpers`
- `super` = `scanner::sql`
- `super::super` = `scanner`
- `crate` = crate root

## 3. `pub` 為什麼到處都是

Rust 預設所有東西都是**私有的**。沒寫 `pub` 就只有同一個模組看得到。這跟 Python 之類的語言反過來，剛開始寫會覺得很煩，但它強迫你想清楚「這個型別到底要不要對外公開」。

例如：

```rust
pub struct NodeId(pub String);
```

`pub struct` 表示型別本身公開，第二個 `pub` 表示裡面的第 0 個欄位也公開。如果沒有第二個 `pub`，別人就算拿到 `NodeId`，也不能直接存取裡面的字串——必須透過你定義的方法。

## 4. `trait` 和 `impl` 在真實專案裡怎麼搭

教科書介紹 trait 的時候，常見的範例長這樣：

```rust
trait Animal { fn speak(&self); }
struct Dog;
impl Animal for Dog { fn speak(&self) { } }
```

本專案的寫法一模一樣，只是換成有意義的名字：

```rust
impl Scanner for SqlScanner { ... }
impl Scanner for DbtScanner { ... }
impl Scanner for PythonScanner { ... }
```

可以把 trait + impl 記成兩件事：

- `trait` — 定義「能做到什麼事」
- `impl ... for ...` — 把能力套到某個具體型別上

一個型別可以實作多個 trait，一個 trait 可以被多個型別實作。這就是 Rust 做 polymorphism 的方式。

## 5. `Box<dyn Trait>` 和 `Vec<Box<dyn Trait>>` 怎麼讀

本專案你會看到：

```rust
Vec<Box<dyn Scanner>>
```

這串記號密度很高，可以拆開來讀：

- `dyn Scanner` — 「某個實作了 `Scanner` 的型別，但編譯期不知道具體是哪個」
- `Box<dyn Scanner>` — 把那個「未知型別」裝在 heap 上
- `Vec<Box<dyn Scanner>>` — 一堆這種未知型別的清單

整句就是：**「一個清單，裡面可以裝不同型別的值，但它們都保證實作了 `Scanner`。」**

為什麼不能直接 `Vec<dyn Scanner>`？因為 Rust 要求 `Vec` 裡的每個元素都必須大小一樣。`SqlScanner`、`DbtScanner`、`PythonScanner` 是不同結構、大小可能不一樣，所以要用 `Box` 把它們放在 heap 上——heap pointer 大小固定，這樣 `Vec` 才裝得下。

### 最小範例

```rust
trait Animal { fn speak(&self); }
struct Dog;
struct Cat;
impl Animal for Dog { fn speak(&self) {} }
impl Animal for Cat { fn speak(&self) {} }

let animals: Vec<Box<dyn Animal>> = vec![
    Box::new(Dog),
    Box::new(Cat),
];

for a in &animals {
    a.speak();
}
```

這就是本專案 `ScanOrchestrator` 在做的事。

## 6. `enum` + `match` 為什麼到處都是

因為 Rust 很愛用 enum 表達「只有這幾種可能」，而 `match` 是處理 enum 最自然的方式。

本專案用 enum 的地方很多：

- `NodeKind` — 節點是 dbt model、source、SQL table、還是 Python job
- `EdgeRelation` — 邊是 `SelectFrom`、`JoinOn`、`InsertInto`、還是 `DbtRef`
- `TransformKind` — 欄位轉換是 direct、SUM、expression、還是 window
- sqlparser 丟回來的 `Statement`、`SetExpr`、`SelectItem`——整個 AST 都是 enum

典型的 match 長這樣：

```rust
match stmt {
    Statement::CreateTable(create) => { ... }
    Statement::CreateView { name, query, .. } => { ... }
    Statement::Insert(insert) => { ... }
    _ => {}
}
```

Rust compiler 會檢查有沒有涵蓋所有 variant（或至少寫個 `_` 兜底）。這點對維護大型 AST 很重要——未來 sqlparser 新增一個 statement type 時，所有沒處理到的 `match` 都會直接變 warning，不會悄悄漏掉。

## 7. `Result` 和 `?` 怎麼讀

本專案很多函式長這樣：

```rust
fn build_graph(path: &Path) -> Result<BuildResult>
```

`Result<T>` 意思是「成功回傳 `T`，失敗回傳錯誤」。那個錯誤型別是專案自己定義的 `LineageError`（在 `error.rs`）。

`?` 運算子可以理解成：

> 如果這一行成功，把值拿出來繼續；如果失敗，就把錯誤直接往上丟給 caller。

### 最小範例

```rust
fn read_name() -> Result<String, std::io::Error> {
    let text = std::fs::read_to_string("name.txt")?;  // 失敗就直接 return Err
    Ok(text)
}
```

沒有 `?` 的話，每次都要寫 `match result { Ok(v) => ..., Err(e) => return Err(e) }`，很快就會讓 code 長一倍。

## 8. borrow 在本專案最常見的樣子

借用規則是 Rust 最有名的東西，但本專案用到的模式其實很固定。可以對照這張表：

| 寫法 | 意思 |
|---|---|
| `&self` | 「我只讀這個物件，不修改」 |
| `&mut self` | 「我會修改這個物件」 |
| `&NodeId` | 「借用這個 id，不要把所有權拿走」 |
| `Option<&Node>` | 「可能有、可能沒有，有的話借出一個參考」 |

所以當你看到：

```rust
fn add_node(&mut self, node: Node) -> NodeIndex
fn get_node(&self, id: &NodeId) -> Option<&Node>
```

可以直接讀成：

- `add_node`：我要改 graph（`&mut self`），把一個 `Node` 放進來（所有權移交）
- `get_node`：我只看 graph（`&self`），給我一個 id（借用），回傳一個節點的參考（或沒有）

掌握這個對照表大概能應付 80% 的 borrow 情況。

## 9. `#[derive(...)]` 第一次怎麼看

第一次看到：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);
```

可以先這樣理解：

> 這是在請 compiler 自動幫這個型別補上常見能力。

每個 derive 代表一個能力：

- `Debug` — 可以用 `{:?}` 印出來
- `Clone` — 可以 `.clone()` 複製
- `PartialEq` / `Eq` — 可以用 `==` 比較
- `Hash` — 可以拿來當 `HashMap` 的 key
- `Serialize` / `Deserialize` — 可以被 serde 轉 JSON / TOML

如果想自己實作 `Clone`，也可以不 derive 改寫 `impl Clone for NodeId`——但大部分情況 derive 就夠。

## 10. 其他值得認識的 Rust 習慣

### inherent `impl`

跟 trait impl 不一樣，inherent impl 是「這個型別自己的方法」。例如 [`../src/graph/mod.rs`](../src/graph/mod.rs)：

```rust
impl LineageGraph {
    pub fn new() -> Self { ... }
    pub fn add_node(&mut self, node: Node) -> NodeIndex { ... }
}
```

這些方法不屬於任何 trait，純粹是 `LineageGraph` 自己的方法。可以用 `graph.new()`、`graph.add_node(...)` 呼叫。

### `Default` trait

```rust
impl Default for LineageGraph {
    fn default() -> Self { Self::new() }
}
```

讓型別可以用 `LineageGraph::default()` 建一個預設值。這樣其他地方寫 `#[derive(Default)]` 的時候就能自動銜接。

### type alias

本專案在錯誤處理用了：

```rust
type Result<T> = std::result::Result<T, LineageError>;
```

意思是：「本 crate 裡只要寫 `Result<T>`，都等於 `std::result::Result<T, LineageError>`。」這樣每個函式的簽名可以少寫一次錯誤型別，看起來更清爽。是很多 Rust library 都會用的慣用手法。

### `Option`

```rust
line_number: Option<usize>
```

意思是：「這個欄位可能有值，也可能沒有。」

為什麼不能直接用 `usize` 配 `0` 當「沒有」？因為 `0` 是合法的行號，沒辦法區分「第 0 行」和「沒記錄」。`Option` 強制在編譯期就想清楚這兩種情況怎麼處理，避免 null-pointer 那一類的問題。

---

這份不用一次全部記起來——讀 01 或 03 的時候卡到哪個語法，能翻回來對照就夠了。
