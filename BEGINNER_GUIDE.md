# lineage-lite 初學者導覽

這份文件是給第一次接觸這個專案、也還不太熟 Rust 的人。

目標不是一次講完所有細節，而是先讓你知道三件事：

1. 這個專案到底在做什麼
2. 它怎麼從原始碼推導出資料流
3. 如果你要開始讀 Rust 程式碼，應該先看哪裡

如果你之後想看更深入的設計與 Rust 手法，再去看 [TECHNICAL.md](./TECHNICAL.md)。

---

## 一句話先講完

`lineage-lite` 會讀你的 `.sql`、dbt model、`.py` ETL 程式，**不執行它們，只靠原始碼內容**，就推導出：

- 哪些表是上游 source
- 哪些表是下游 target
- 哪個 Python job 或 dbt model 在中間做了轉換
- 某個欄位是直接傳遞、聚合、還是用算式算出來的

也就是說，它做的是 **靜態分析（static analysis）**，不是跑 SQL、不是連資料庫查 metadata。

---

## 先用一個很小的例子理解

假設有一個 SQL 檔案：

```sql
CREATE VIEW reports.daily_revenue AS
SELECT
    order_date,
    COUNT(DISTINCT order_id) AS order_count,
    SUM(amount) AS daily_revenue
FROM mart.mart_orders
GROUP BY order_date;
```

我們讀這段 SQL，很容易知道：

- 它建立了一張新 view：`reports.daily_revenue`
- 它的資料來自 `mart.mart_orders`

所以最基本的 lineage 就是一條邊：

```text
mart.mart_orders -> reports.daily_revenue
```

如果再細一點看欄位：

- `order_date` 是直接從來源表帶過來
- `order_count` 是 `COUNT(DISTINCT order_id)` 算出來的
- `daily_revenue` 是 `SUM(amount)` 算出來的

所以 column-level lineage 大概會長成：

```text
reports.daily_revenue.order_date      <- mart.mart_orders.order_date   (Direct)
reports.daily_revenue.order_count     <- mart.mart_orders.order_id     (Aggregation: COUNT)
reports.daily_revenue.daily_revenue   <- mart.mart_orders.amount       (Aggregation: SUM)
```

這就是這個專案做的事。

---

## 用這個 repo 裡的真實例子看

這個專案有一個 `demo/` 目錄，裡面放了 dbt、SQL report、Python ETL 的範例。

### 例子 1：dbt model

[`demo/models/staging/stg_orders.sql`](./demo/models/staging/stg_orders.sql) 裡有：

```sql
WITH source AS (
    SELECT * FROM {{ source('raw', 'orders') }}
)
```

這段不是一般 SQL，而是 dbt 的 Jinja 語法。我們看得出它表示：

```text
raw.orders -> stg_orders
```

也就是說，`stg_orders` 這個 model 依賴 `raw.orders`。

### 例子 2：dbt mart

[`demo/models/marts/mart_orders.sql`](./demo/models/marts/mart_orders.sql) 裡有：

```sql
SELECT * FROM {{ ref('int_order_payments') }}
```

還有：

```sql
SELECT * FROM {{ ref('stg_users') }}
```

所以可以推導：

```text
int_order_payments -> mart_orders
stg_users -> mart_orders
```

### 例子 3：Python ETL

[`demo/etl/sync_events_to_warehouse.py`](./demo/etl/sync_events_to_warehouse.py) 裡有：

```python
events = pd.read_sql("raw.events", con=engine)
users = pd.read_sql("raw.users", con=engine)
enriched.to_sql("staging.enriched_events", con=engine, if_exists="replace")
```

我們讀這段 Python，會知道：

- 這個 job 讀了 `raw.events`
- 也讀了 `raw.users`
- 最後寫進 `staging.enriched_events`

所以圖上會出現：

```text
raw.events -> sync_events_to_warehouse.py
raw.users -> sync_events_to_warehouse.py
sync_events_to_warehouse.py -> staging.enriched_events
```

這也是 lineage-lite 想還原出來的資料流。

---

## 它的核心原理是什麼？

先講最重要的觀念：

**lineage-lite 不是用一種方法處理所有檔案，而是針對不同檔案類型，用不同 scanner。**

你可以把它想成：

```text
不同檔案
  -> 不同 scanner
  -> 產生一批 edges
  -> 合併成一張圖
  -> 再做 impact / trace / show
```

### 1. SQL：用 AST 解析

對純 SQL 來說，專案不是用 regex 硬抓，而是用 `sqlparser` 先把 SQL 轉成 AST（抽象語法樹）。

概念上像這樣：

```text
SQL 字串
  -> Parser::parse_sql(...)
  -> Statement
  -> 從 Statement 裡找 target table 與 source tables
```

例如：

```sql
CREATE VIEW reports.daily_revenue AS
SELECT *
FROM mart.mart_orders;
```

解析後，程式會知道：

- target 是 `reports.daily_revenue`
- source 是 `mart.mart_orders`

於是產生：

```text
mart.mart_orders -> reports.daily_revenue
```

為什麼 SQL 要用 AST？

- 因為 SQL 有巢狀結構
- 有 `JOIN`
- 有子查詢
- 有 `WITH` CTE
- 有 `UNION`

這些都很難只靠 regex 正確處理。

### 補充：人眼看到的 SQL，和 AST 看到的 SQL

初學者最容易卡住的點是：人是照畫面順序理解 SQL，但 parser 看到的是巢狀語法結構。

我們通常這樣理解：

```sql
CREATE VIEW reports.daily_revenue AS
SELECT
    order_date,
    SUM(amount) AS daily_revenue
FROM mart.mart_orders
GROUP BY order_date;
```

人眼會自然拆成：

1. 這是一個 `CREATE VIEW`
2. view 名字叫 `reports.daily_revenue`
3. 裡面包了一個 `SELECT`
4. `FROM mart.mart_orders` 是來源
5. `SUM(amount)` 表示某個欄位是聚合算出來的

AST 會把同一件事表示成樹狀結構，概念上像這樣：

```text
Statement::CreateView
  name = reports.daily_revenue
  query =
    Query
      body =
        SetExpr::Select
          projection =
            SelectItem::UnnamedExpr(order_date)
            SelectItem::ExprWithAlias(SUM(amount), daily_revenue)
          from =
            TableWithJoins
              relation = TableFactor::Table(mart.mart_orders)
```

這個對照非常重要，因為它直接解釋了 [`src/scanner/sql.rs`](./src/scanner/sql.rs) 為什麼會寫成現在這樣：

- 先 `match Statement`
  因為最外層先分 `CreateView`、`CreateTable`、`Insert`

- 再進 `Query`
  因為要進到查詢本體找來源

- 再進 `SetExpr`
  因為 query body 可能是普通 `SELECT`，也可能是 `UNION`

- 再進 `Select`
  因為 `FROM`、`JOIN`、`projection` 都掛在這裡

- 再進 `TableFactor`
  因為真正的來源表、子查詢、巢狀 join 都是在這一層分辨

你可以把這條路想成：

```text
extract_from_statement()
  -> extract_sources_from_query()
  -> extract_sources_from_set_expr()
  -> extract_sources_from_select()
  -> extract_source_from_table_factor()
```

這不是 Rust 寫法故意繞，而是 SQL AST 本來就是一層包一層，所以解析函式自然也會一層包一層。

### 再看一個 CTE 例子

```sql
WITH paid_orders AS (
    SELECT * FROM raw.orders
),
final AS (
    SELECT * FROM paid_orders
)
SELECT * FROM final;
```

人眼會知道：

- `paid_orders` 和 `final` 是查詢內部的暫時名字
- 真正的外部來源其實是 `raw.orders`

所以 scanner 做的事情不是只看最後一層 `FROM final`，而是：

1. 先記下 CTE 名稱，例如 `paid_orders`、`final`
2. 再遞迴進 CTE 本體，找真正的外部表

因此最後留下的來源應該是：

```text
raw.orders
```

而不是把 `final` 當成外部來源。這也是 AST 解析可靠的關鍵原因之一。

### 一個完整對照表：SQL 原文 -> AST -> lineage 結果

如果你想把整件事一次對起來，可以看下面這個例子。

#### SQL 原文

```sql
CREATE VIEW reports.daily_revenue AS
SELECT
    order_date,
    SUM(amount) AS daily_revenue
FROM mart.mart_orders
GROUP BY order_date;
```

#### 人眼怎麼理解

| 看到的 SQL 片段 | 人會怎麼理解 |
|----------------|-------------|
| `CREATE VIEW reports.daily_revenue AS` | 目標表是 `reports.daily_revenue` |
| `FROM mart.mart_orders` | 來源表是 `mart.mart_orders` |
| `order_date` | 這個輸出欄位直接來自來源欄位 |
| `SUM(amount) AS daily_revenue` | 這個輸出欄位是聚合計算出來的 |

#### AST 大概長什麼樣

| SQL 概念 | AST 節點 |
|---------|---------|
| 整句 `CREATE VIEW` | `Statement::CreateView` |
| `reports.daily_revenue` | `name` |
| `SELECT ...` 主體 | `query.body` |
| 一般 `SELECT` | `SetExpr::Select` |
| `order_date` | `SelectItem::UnnamedExpr(...)` |
| `SUM(amount) AS daily_revenue` | `SelectItem::ExprWithAlias(...)` |
| `FROM mart.mart_orders` | `TableFactor::Table(...)` |

#### 程式會怎麼走

| 程式步驟 | 對應函式 | 做的事 |
|---------|---------|-------|
| 先看最外層 statement | `extract_from_statement()` | 判斷這是 `CreateView` |
| 進 query | `extract_sources_from_query()` | 準備找來源 |
| 進 query body | `extract_sources_from_set_expr()` | 判斷這是 `Select` 不是 `UNION` |
| 進 `FROM` | `extract_sources_from_select()` | 找出 `from` 裡的來源 |
| 碰到真正表名 | `extract_source_from_table_factor()` | 取得 `mart.mart_orders` |
| 進 `SELECT` list | `extract_column_lineage()` / `analyze_expr()` | 分析輸出欄位的來源與轉換類型 |

#### 最後會產生什麼結果

**Table-level edge：**

```text
mart.mart_orders -> reports.daily_revenue
```

**Column-level lineage：**

```text
reports.daily_revenue.order_date
  <- mart.mart_orders.order_date
  (Direct)

reports.daily_revenue.daily_revenue
  <- mart.mart_orders.amount
  (Aggregation: SUM)
```

這個表的重點是讓你看到：

- SQL 原文
- AST 節點
- Rust 函式
- 最終 lineage 結果

其實是一條連續的鏈，不是四件分開的事。

### 再看一個進階版：`JOIN` + `CTE`

前一個例子只有單一來源，還看不出 AST 和 graph 為什麼真的有必要。下面這個例子比較接近真實 SQL：

#### SQL 原文

```sql
CREATE TABLE mart.order_summary AS
WITH paid_orders AS (
    SELECT
        order_id,
        user_id,
        amount
    FROM raw.orders
),
users AS (
    SELECT
        id,
        email
    FROM raw.users
)
SELECT
    p.order_id,
    u.email,
    p.amount
FROM paid_orders p
JOIN users u ON p.user_id = u.id;
```

#### 人眼怎麼理解

我們通常會這樣讀：

1. 目標表是 `mart.order_summary`
2. `paid_orders` 是內部 CTE，它背後來自 `raw.orders`
3. `users` 是內部 CTE，它背後來自 `raw.users`
4. 最外層 `SELECT` 是把兩個 CTE join 起來
5. 所以真正的外部來源其實是兩張表：
   `raw.orders` 和 `raw.users`

也就是：

```text
raw.orders -> mart.order_summary
raw.users  -> mart.order_summary
```

#### AST 大概長什麼樣

概念上可以想成：

```text
Statement::CreateTable
  name = mart.order_summary
  query =
    Query
      with =
        CTE paid_orders
          Query
            SetExpr::Select
              from = raw.orders
        CTE users
          Query
            SetExpr::Select
              from = raw.users
      body =
        SetExpr::Select
          from =
            TableWithJoins
              relation = TableFactor::Table(paid_orders)
              joins =
                TableFactor::Table(users)
```

這裡最重要的觀察是：

- 最外層 `FROM` 看到的是 `paid_orders` 和 `users`
- 但它們不是外部表，只是 CTE 名字
- 真正的來源要再往 CTE 本體裡遞迴

這就是為什麼 scanner 不能只靠最後一層字串。

#### 程式會怎麼走

| 程式步驟 | 對應函式 | 做的事 |
|---------|---------|-------|
| 看最外層 statement | `extract_from_statement()` | 判斷這是 `CreateTable` |
| 進 query | `extract_sources_from_query()` | 同時處理 `WITH` 與 main body |
| 先看 CTE | `extract_cte_sources()` | 收集 `paid_orders`、`users`，並遞迴分析它們的 query |
| 進 CTE 的 `SELECT` | `extract_sources_from_set_expr()` / `extract_sources_from_select()` | 找出 `raw.orders`、`raw.users` |
| 回到最外層 `SELECT` | `extract_sources_from_select()` | 看到 `paid_orders` 與 `users`，但它們已在 CTE 名單裡 |
| 過濾 CTE 名稱 | `extract_source_from_table_factor()` | 避免把 CTE 當成外部來源 |

#### 最後會產生什麼 table-level edge？

```text
raw.orders -> mart.order_summary
raw.users  -> mart.order_summary
```

如果其中一個來源是 join 進來的，edge relation 也可能不同，例如 `SelectFrom` 和 `JoinOn`。

這也是為什麼這個專案不只記「有沒有依賴」，還會記 `EdgeRelation`。

#### Column-level lineage 又會怎麼看？

最外層欄位是：

```sql
SELECT
    p.order_id,
    u.email,
    p.amount
```

我們會知道：

- `p.order_id` 最終來自 `raw.orders.order_id`
- `u.email` 最終來自 `raw.users.email`
- `p.amount` 最終來自 `raw.orders.amount`

也就是：

```text
mart.order_summary.order_id <- raw.orders.order_id
mart.order_summary.email    <- raw.users.email
mart.order_summary.amount   <- raw.orders.amount
```

這就是為什麼 column-level lineage 不能只停在 alias `p` 或 CTE 名稱 `paid_orders`，而要繼續往回展開。

#### 這和 graph 查詢有什麼關係？

當這些 edge 建好之後，圖上大概會像這樣：

```text
raw.orders ----\
                -> mart.order_summary
raw.users  ----/
```

這時候查詢就很自然：

- `upstream mart.order_summary`
  會得到 `raw.orders`、`raw.users`

- `impact raw.orders`
  會把 `mart.order_summary` 算進去

這也是 graph 模型有效的原因之一：

**當一個節點同時有多個上游來源時，圖可以自然表示「匯流」這件事。**

如果不用 graph，這種多對一依賴在查詢和推導上會麻煩很多。

### 2. dbt：用 regex 抓 `ref()` / `source()`

dbt model 的 `.sql` 不是純 SQL，裡面會混入 Jinja：

```sql
SELECT * FROM {{ ref('stg_orders') }}
SELECT * FROM {{ source('raw', 'orders') }}
```

這種檔案如果直接丟進 SQL parser，通常會失敗。

但 lineage-lite 的目標不是完整理解 Jinja，而是先抓出依賴關係，所以做法比較務實：

- 看到 `{{ ref('...') }}` 就記一條 dbt model 依賴
- 看到 `{{ source('...', '...') }}` 就記一條 source 依賴

也就是：

```text
{{ ref('stg_orders') }}       -> stg_orders -> 目前這個 model
{{ source('raw', 'orders') }} -> raw.orders -> 目前這個 model
```

這樣雖然沒有完整解析 Jinja，但足夠回答「資料從哪裡來」這個問題。

### 3. Python：用 regex 抓常見讀寫模式

Python 更難做完整靜態分析，因為語法太自由：

- 字串可能先組起來
- 表名可能存在變數裡
- 可能有動態函式呼叫

所以這個專案沒有試圖完全理解所有 Python，而是抓常見 ETL pattern，例如：

- `pd.read_sql("raw.events", ...)`
- `spark.table("raw.events")`
- `.to_sql("staging.events", ...)`
- `.saveAsTable("mart.orders")`

抓到讀取就代表 source，抓到寫入就代表 sink。

這是典型的工程取捨：

- 優點：簡單、快、實用
- 缺點：太動態的寫法抓不到

---

## 什麼是 edge？什麼是 graph？

這個專案的核心資料其實很單純。

### Node

Node 是圖上的節點，例如：

- `raw.orders`
- `marts.mart_customers`
- `reports.daily_revenue`
- `sync_events_to_warehouse.py`

### Edge

Edge 是「誰流到誰」的關係，例如：

```text
raw.orders -> stg_orders
stg_orders -> mart_orders
mart_orders -> reports.daily_revenue
raw.events -> sync_events_to_warehouse.py
sync_events_to_warehouse.py -> staging.enriched_events
```

只要把很多這種 edge 收集起來，就得到一張有向圖（directed graph）。

### 為什麼要建成圖？

因為一旦有圖，就能做這些查詢：

- `impact raw.payments`
  看 `raw.payments` 壞掉時，哪些東西會一起壞

- `show mart.orders --upstream 3`
  看 `mart.orders` 上游三層有哪些表與 model

- `trace marts.mart_customers`
  看某個 output column 是從哪些 source columns 推導出來

### 為什麼用 graph 來表示是有效的？

因為 lineage 的本質就是方向性關係：

```text
來源 -> 產物
上游 -> 下游
依賴方 -> 被依賴方
```

只要關係有方向，graph 就是很自然的模型。

例如：

```text
raw.orders -> stg_orders -> mart_orders -> reports.daily_revenue
```

你很自然就會問：

- 往右走，哪些東西會被影響？
- 往左走，這個節點依賴誰？
- 只看一層或兩層鄰居時，附近有哪些節點？

這些問題本質上都是圖走訪問題，所以用 graph 不是為了炫技，而是因為資料結構剛好對應問題本身。

### 這個專案的 graph 實際上存了什麼？

[`src/graph/mod.rs`](./src/graph/mod.rs) 裡的 `LineageGraph` 可以先簡單理解成兩部分：

- `DiGraph<Node, LineageEdge>`
  真正存圖的結構

- `HashMap<NodeId, NodeIndex>`
  讓你可以用 `raw.orders` 這種名字快速找到圖裡的節點

也就是說：

- graph library 負責處理節點和邊
- 專案自己的 `NodeId` 負責對外提供好懂的名字

這樣查詢時就不用直接操作內部 index。

### 這裡其實有多種 graph 查詢

雖然都用同一張圖，但它們回答的問題不一樣。

#### 1. `downstream`

問題是：

```text
從某個節點出發，沿著箭頭往後走，可以到哪些節點？
```

這適合回答「如果上游 source 壞掉，哪些東西會被波及」。

#### 2. `upstream`

問題是：

```text
某個節點往回追，依賴了哪些上游？
```

這適合回答「這張表從哪裡來」。

#### 3. `impact`

`impact` 本質上其實就是不限深度的 `downstream`。

演算法沒有變，只是產品語意不同：

- `downstream` 是一般圖查詢
- `impact` 是面向使用者場景的名字

#### 4. `stats`

`stats` 跟前面三個不同。它不是沿著邊走，而是統計整張圖的節點資料。

[`src/graph/query.rs`](./src/graph/query.rs) 目前做的是：

- 總節點數
- 總邊數
- 各種 `NodeKind` 的數量
- 某個欄位名稱出現在哪些節點

所以 graph 查詢大致可以分成兩類：

```text
結構走訪型：upstream / downstream / impact
屬性統計型：stats
```

#### 5. `trace`

`trace` 又是另一類。它會利用 graph 中的節點資訊，但核心更偏向 column-level lineage，而不是單純 node-to-node 走訪。

它回答的是：

```text
這個欄位從哪個欄位來？中間經過什麼轉換？
```

所以不要把所有查詢都想成只是 BFS。

### 這些查詢背後為什麼用 BFS？

對 `upstream`、`downstream`、`impact` 來說，核心都是 BFS（廣度優先搜尋）。

這樣做合理，因為：

- BFS 天然就是一層一層往外擴散
- 很容易加上 `max_depth`
- 很適合「只看附近幾層依賴」這種需求

例如 `show mart.orders --upstream 2`，意思其實就是：

```text
先找第一層上游
再找第二層上游
超過兩層就停
```

這正是 BFS 最自然的工作方式。

[`src/graph/mod.rs`](./src/graph/mod.rs) 裡的 `bfs_collect()` 也因此做了三件事：

1. 用 queue 一層一層走
2. 用 `visited` 避免重複走到同一個節點
3. 記錄深度，讓 `max_depth` 可以生效

---

## Column-level lineage 是怎麼追的？

table-level lineage 只回答：

```text
A 表 -> B 表
```

但很多時候你真正想知道的是：

```text
B.total_amount 是不是從 A.amount 加總出來的？
B.email 是不是從 raw.users.email 傳下來的？
```

這就是 column-level lineage。

例如這段 SQL：

```sql
SELECT
    user_id,
    SUM(amount) AS total_amount,
    COUNT(*) AS order_count
FROM raw.orders
GROUP BY user_id;
```

lineage-lite 會逐一分析 `SELECT` 裡的每個欄位：

- `user_id`
  這是直接引用欄位，所以是 `Direct`

- `SUM(amount) AS total_amount`
  這是聚合，所以是 `Aggregation("SUM")`

- `COUNT(*) AS order_count`
  這也是聚合，所以是 `Aggregation("COUNT")`

結果可以理解成：

```text
output.user_id      <- raw.orders.user_id   (Direct)
output.total_amount <- raw.orders.amount    (Aggregation: SUM)
output.order_count  <- raw.orders.*         (Aggregation: COUNT)
```

它不是把 SQL 當成單純文字，而是會遞迴往 expression 裡面看。

這也是為什麼在 [`TECHNICAL.md`](./TECHNICAL.md) 裡會一直提到 `analyze_expr()` 之類的函式。

### 人眼看欄位，程式看 `SelectItem`

對我們來說，下面這段 SQL 很直觀：

```sql
SELECT
    user_id,
    SUM(amount) AS total_amount
FROM raw.orders
GROUP BY user_id;
```

但對 parser 來說，`SELECT` list 不是兩行字串，而是兩個 AST 節點：

```text
SelectItem::UnnamedExpr(user_id)
SelectItem::ExprWithAlias(SUM(amount), total_amount)
```

這就是為什麼程式會先 `match SelectItem`：

- `UnnamedExpr`
  代表像 `user_id` 這種沒有 alias 的欄位

- `ExprWithAlias`
  代表像 `SUM(amount) AS total_amount`

- `Wildcard`
  代表 `SELECT *`

再把 expression 丟給 `analyze_expr()` 做下一層分析。

例如：

```text
SUM(amount)
  -> 這是一個函式呼叫
  -> 函式名是 SUM
  -> 參數裡引用到 amount
  -> 所以 TransformKind = Aggregation("SUM")
```

這樣你就能把「人眼看到的 SELECT list」和「程式裡為什麼要 match `SelectItem`、再遞迴 analyze expression」對起來。

---

## 為什麼 SQL、dbt、Python 要分開處理？

因為這三者的「可分析程度」不同。

| 類型 | 可分析程度 | 專案採用的方法 |
|------|------------|----------------|
| 純 SQL | 高 | AST 解析 |
| dbt SQL | 中 | regex 抓 `ref()` / `source()`；compiled SQL 再做 deeper analysis |
| Python ETL | 低到中 | regex 抓常見 read/write pattern |

這裡的思路很重要：

**不要追求理論上最完整的解析，而是用成本合理的方法，先得到夠有用的結果。**

這也是這個專案最值得學的工程觀念之一。

---

## Rust 初學者應該怎麼理解這個專案？

你不需要一開始就懂所有 Rust 細節。

先把這個專案想成四層：

1. `scanner`
   讀檔案，產生 lineage edges

2. `graph`
   把 edges 組成圖，支援 upstream/downstream 查詢

3. `cli`
   接使用者命令，呼叫 graph 查詢

4. `output` / `storage`
   把結果印出來或存進 SQLite

也就是：

```text
scanner -> graph -> cli query -> output/storage
```

---

## 建議的讀 code 順序

如果你是 Rust 初學者，建議照這個順序。

### 1. 先看 domain type

先看 [`src/graph/node.rs`](./src/graph/node.rs)。

這裡定義了整個專案最重要的名詞：

- `Node`
- `NodeId`
- `NodeKind`
- `LineageEdge`
- `EdgeRelation`
- `ColumnLineage`
- `TransformKind`

先把這些名詞搞懂，後面讀其他檔案會輕鬆很多。

### 2. 再看 scanner 的骨架

接著看 [`src/scanner/mod.rs`](./src/scanner/mod.rs)。

你只要先理解兩件事：

- `Scanner` trait 代表「能掃某種檔案的東西」
- `ScanOrchestrator` 代表「走目錄，把檔案交給對應 scanner」

這裡先不用深究 trait object，先把它理解成「統一介面」就夠了。

### 3. 從最簡單的 scanner 開始

然後去看：

- [`src/scanner/dbt.rs`](./src/scanner/dbt.rs)
- [`src/scanner/python.rs`](./src/scanner/python.rs)

這兩個比較短，也比較直觀，容易建立信心。

### 4. 最後再看 SQL scanner

最後才看 [`src/scanner/sql.rs`](./src/scanner/sql.rs)。

這個檔案最大，也最複雜，但你前面概念先建立好之後，會知道它其實在做同一件事：

```text
讀檔 -> 找來源與目標 -> 產生 edges
```

只是 SQL 的內部結構比較複雜，所以需要 AST 與遞迴。

### 5. 再回頭看 graph 與 cli

最後看：

- [`src/graph/mod.rs`](./src/graph/mod.rs)
- [`src/cli.rs`](./src/cli.rs)

這時候你會比較容易理解：

- 圖怎麼建立
- `impact()` 為什麼本質上只是往下游走 BFS
- `show` / `trace` / `stats` 是怎麼把資料接起來的

---

## 一個完整的心智模型

你可以把一次 `scan` 想成下面這個流程：

```text
使用者執行 lineage-lite scan ./demo
  ->
cli 收到指令
  ->
ScanOrchestrator 走過所有檔案
  ->
不同 scanner 各自解析
  ->
收集出很多 edges
  ->
LineageGraph 把 edges 組成一張圖
  ->
Renderer 把圖輸出成 table / dot / html
```

如果是 `impact raw.payments`，那就只是多了一步：

```text
先建圖
  ->
從 raw.payments 往下游做 BFS
  ->
把所有會受到影響的節點列出來
```

---

## 初學者最容易卡住的點

### 1. 為什麼不用同一種 parser 全部解析？

因為檔案類型不同，結構也不同。純 SQL 很適合 AST，但 dbt/Jinja 和 Python 並不適合用同一套做。

### 2. 為什麼 Python 只用 regex？

不是因為 parser 做不到，而是因為完整分析 Python 成本很高，而且實務上常見 ETL pattern 已經能抓出很多有價值的 lineage。

### 3. 為什麼有些東西抓不到？

因為這是靜態分析，不執行程式，所以以下情況天生比較難：

- 動態組字串
- 執行期才決定表名
- 很重的 macro 展開
- vendor-specific SQL 語法

這不是 bug，而是靜態分析工具的常見限制。

### 4. 為什麼要特別提 CTE、子查詢、UNION？

因為真實 SQL 不會只有單層 `SELECT ... FROM table`。如果不處理這些結構，lineage 很快就會失真。

---

## 可以先忽略哪些 Rust 細節？

如果你是初學者，第一次讀時可以先不要卡在下面這些詞：

- newtype
- trait object
- derive macro
- `Send + Sync`
- `Entry::Vacant`

它們都重要，但不是你理解「這個專案怎麼運作」的第一步。

第一次讀，先抓住下面這些就好：

- 專案在收集 `source -> target` 關係
- 不同檔案類型用不同 scanner
- scanner 產 edge，graph 負責查詢
- SQL 比較精準，dbt/Python 比較偏實務取向

---

## 建議你實際跑一次

先跑這幾個指令，再回來看文件會更清楚：

```bash
cargo run -- scan ./demo
cargo run -- impact raw.payments --path ./demo
cargo run -- trace marts.mart_customers --path ./demo --compiled ./demo/target/compiled
```

跑完後再對照這些檔案看：

- [`demo/models/staging/stg_orders.sql`](./demo/models/staging/stg_orders.sql)
- [`demo/models/marts/mart_orders.sql`](./demo/models/marts/mart_orders.sql)
- [`demo/sql/reports/daily_revenue_report.sql`](./demo/sql/reports/daily_revenue_report.sql)
- [`demo/etl/sync_events_to_warehouse.py`](./demo/etl/sync_events_to_warehouse.py)

你會很快把「原始碼長相」和「lineage 圖上的節點/邊」對起來。

---

## 下一步看哪份文件？

- 想知道程式碼從哪裡開始讀：看 [WALKTHROUGH.md](./WALKTHROUGH.md)
- 想深入理解 Rust 設計與演算法：看 [TECHNICAL.md](./TECHNICAL.md)
- 想快速知道指令怎麼用：看 [README.md](./README.md)
