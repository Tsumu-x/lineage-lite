**English** | [中文](README.zh-TW.md)

# lineage-lite

A static lineage analysis engine for data governance.

lineage-lite statically analyzes SQL, dbt models, and Python ETL source code to extract the real source → transform → sink data flow, building a complete DAG (directed acyclic graph) for impact analysis, onboarding, and governance policy verification.

## What problems does it solve?

These are long-standing pain points in the data engineering community:

| Pain point | Status quo | lineage-lite's approach |
|---|---|---|
| **dbt lineage only covers dbt models** — Python ETL and hand-written SQL reports are invisible | dbt lineage graph stops at sources | `scan` processes `.sql` + `.py` together, stitching all data flows into one graph |
| **No way to know blast radius before schema changes** — relying on gut feeling and grep, then reports break for a week | "It probably won't break much" | `impact raw.payments` → lists all downstream nodes, including legacy jobs nobody knew about |
| **Missing column-level lineage** — you know table A feeds table B, but not where `lifetime_value` gets SUMmed from | dbt-core [Discussion #4458](https://github.com/dbt-labs/dbt-core/discussions/4458) has been open for years | `trace marts.mart_customers` → shows source columns, transform types, and SQL expressions for each output column |
| **PII scattered everywhere** — email and phone columns showing up in schemas they shouldn't be in | Manual inventory | `stats --where column=email` → instantly find all tables containing PII columns |
| **Data contracts can't be auto-verified** — policies live in a wiki nobody follows | Manual review | `check --rules .lineage-rules.toml` → automated violation checks |
| **New hires can't understand the pipeline** — wiki is outdated, architecture diagrams have drifted from actual code | Two weeks to figure out the data flow | `show mart.orders --upstream 4` → see the real upstream/downstream chain |
| **Jinja macros make dbt code unreadable** — 500 lines of Jinja, 47 broken models, 1 engineer who already left | No good tooling to extract structure from Jinja SQL | Reads `dbt compile` output from `target/compiled/` for column-level analysis |

## Screenshots

`lineage-lite scan ./demo --format html --out lineage.html` generates a zero-dependency interactive HTML file you can browse in any browser.

### Full lineage graph

![Full lineage graph](assets/overview.png)

See dbt models, SQL tables/views, and Python ETL all in one graph with source → transform → sink relationships. The legend in the top-right corner color-codes node types.

### Click a node → see which tables it depends on

![Upstream trace on click](assets/upstream-trace.png)

Click `mart_orders` and the upstream chain is immediately highlighted. The bottom-left panel shows node type, file path, and all upstream/downstream tables. No more guessing blast radius with grep before schema changes.

### Click a node → see which columns it's computed from

![Column-level lineage](assets/column-lineage.png)

Select `intermediate.int_order_payments` and the panel expands a **Column Lineage** section, listing each output column's source columns and transform type (direct / SUM / expression / window) — the column-level lineage that dbt-core still hasn't built in.

![Full upstream/downstream chain + column panel](assets/node-detail.png)

Zoom to fit a node's neighborhood, and the same panel shows both "which tables compose it" and "where each column comes from."

## Why Rust?

- **Blazing Fast**: Built with Rust for high-performance static analysis, even on large dbt projects with hundreds of models.
- **Single Binary**: Easy to integrate into CI/CD pipelines without managing Python environments or dependencies.

## Installation

```bash
cargo install --path .
```

## Quick start

```bash
# Scan the repo and build the lineage graph
lineage-lite scan .

# Check blast radius before a schema change
lineage-lite impact raw.payments --path .

# Generate an interactive HTML visualization
lineage-lite scan . --format html --out lineage.html
```

## Documentation

- `README.md` — Quick start, commands, and feature overview
- `BEGINNER_GUIDE.md` — Project intro for Rust beginners (zh-TW)
- `docs/reading-guide.md` — Reading guide index for Rust practitioners (zh-TW)
- `docs/01-overview.md` — Project walkthrough: how a mid-size Rust project is structured (zh-TW)
- `docs/02-rust-notes.md` — Rust review: `mod`, `crate::`, `super::`, traits, borrowing in practice (zh-TW)
- `docs/03-code-flow.md` — Code flow & exercises: follow the `scan` command through the code (zh-TW)
- `WALKTHROUGH.md` — Code reading order and module guide (zh-TW)
- `TECHNICAL.md` — Deep dive into design, Rust patterns, and algorithms (zh-TW)

## All commands

### `scan` — Scan and build the lineage graph

```bash
lineage-lite scan ./your-repo

# Output formats
lineage-lite scan . --format table    # Terminal table (default)
lineage-lite scan . --format dot      # Graphviz DOT
lineage-lite scan . --format html --out lineage.html  # Interactive HTML

# Export to SQLite
lineage-lite scan . --out lineage.db
```

### `impact` — Schema change impact analysis

```bash
lineage-lite impact raw.payments --path ./your-repo
```

Lists all downstream nodes (dbt models, SQL reports, Python jobs) for a given table, so you know the blast radius before opening a PR.

### `show` — Display upstream/downstream neighborhood

```bash
lineage-lite show mart.orders --upstream 4 --downstream 2 --path .
```

### `trace` — Column-level lineage tracing

```bash
# Trace column origins for a plain SQL table
lineage-lite trace reports.daily_revenue --path .

# Trace a dbt model (requires dbt compile output in target/compiled/)
lineage-lite trace marts.mart_customers --path .
lineage-lite trace marts.mart_customers --path . --compiled target/compiled/
```

Outputs each column's source columns, transform type (direct / SUM / COUNT / expression), and the original SQL expression.

### `stats` — Statistics and column search

```bash
# Overview
lineage-lite stats .

# Find all tables containing an email column
lineage-lite stats . --where column=email
```

### `check` — Data contract verification

```bash
lineage-lite check . --rules .lineage-rules.toml
```

Example rules file (`.lineage-rules.toml`):

```toml
[[rules]]
name = "pii-isolation"
type = "column-deny"
columns = ["email", "phone", "id_number"]
denied_schemas = ["reports", "analysis", "exports"]
message = "PII columns should not appear in this schema"
```

### `diff` — Compare two scans (CI integration)

```bash
# Save a baseline
lineage-lite scan . --out baseline.db

# After code changes, compare
lineage-lite diff baseline.db .
```

Outputs added/removed nodes and edges — useful for PR reviews and CI pipelines.

### `merge` — Merge lineage from multiple repos

```bash
lineage-lite merge repo-a.db repo-b.db --out merged.db
```

## Supported source types

| Type | Detection | Extracted relationships |
|---|---|---|
| **SQL** | `.sql` files (no Jinja) | `FROM`, `JOIN`, `INSERT INTO`, `CREATE TABLE AS`, `CREATE VIEW`, CTE |
| **dbt** | `.sql` files containing `{{ ref() }}` / `{{ source() }}` | Model dependencies |
| **dbt compiled** | `target/compiled/` directory | Column-level lineage (pure SQL after Jinja expansion) |
| **Python ETL** | `.py` files | `read_sql`, `to_sql`, `saveAsTable`, `insertInto`, `spark.table()` |

## Architecture

```
src/
├── cli.rs               # CLI entry point (clap derive) — scan/impact/show/trace/stats/check/diff/merge
├── error.rs             # Unified error type (thiserror)
├── graph/
│   ├── mod.rs           # LineageGraph (petgraph wrapper + BFS)
│   ├── node.rs          # Node, NodeKind, LineageEdge, EdgeRelation, ColumnLineage
│   └── query.rs         # Stats queries
├── scanner/
│   ├── mod.rs           # Scanner trait + ScanOrchestrator
│   ├── sql.rs           # SQL scanner (sqlparser-rs AST + column-level lineage)
│   ├── dbt.rs           # dbt scanner (regex)
│   └── python.rs        # Python ETL scanner (regex)
├── storage/
│   ├── mod.rs           # StorageBackend trait
│   └── sqlite.rs        # SQLite persistence
└── output/
    ├── mod.rs           # Renderer trait
    ├── table.rs         # Terminal table
    ├── dot.rs           # Graphviz DOT
    └── html.rs          # Interactive HTML (vanilla JS + SVG, zero dependencies)
```

### Design highlights

- **Scanner trait** — Adding a new file type only requires implementing one trait; ScanOrchestrator auto-dispatches by file extension
- **Column-level lineage** — Extracts each column's source and transform type from the SQL AST (direct / aggregation / expression / window)
- **CTE pass-through** — `SELECT * FROM final_cte` automatically traces into CTE definitions to resolve actual columns
- **Domain enums** — `NodeKind` / `EdgeRelation` / `TransformKind` encode domain knowledge in the Rust type system
- **thiserror error handling** — Structured errors, no `.unwrap()` in library code
- **lib.rs re-exports** — Works as both a CLI tool and a library for other Rust projects

## Demo

The `demo/` directory contains a full XYZMart scenario simulating a real e-commerce data team:

- dbt project (staging → intermediate → marts, with Jinja macros, `{{ config() }}`, `{% for %}`)
- SQL reports (hand-written CREATE VIEW / CREATE TABLE AS by the BI team)
- Python ETL (legacy pandas read_sql → to_sql)
- dbt compiled SQL (`target/compiled/`, pure SQL after Jinja expansion)
- Data contract rules (`.lineage-rules.toml`)

```bash
# Try it out
lineage-lite scan ./demo
lineage-lite impact raw.payments --path ./demo
lineage-lite trace marts.mart_customers --path ./demo
lineage-lite check ./demo --rules ./demo/.lineage-rules.toml
lineage-lite scan ./demo --format html --out lineage.html
```

## Development

```bash
cargo build     # Build
cargo test      # 47 tests
cargo clippy    # Lint
```

## License

Apache License 2.0 — See [LICENSE](LICENSE)
