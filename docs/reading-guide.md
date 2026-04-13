# lineage-lite 給 Rust 實作者的閱讀指南

我自己剛開始學 Rust 的時候，最卡的不是語法，而是看完教科書之後打開一個真實專案，整個不知道從哪裡讀起。`struct`、`enum`、`match` 都認得，但一遇到 `Vec<Box<dyn Scanner>>` 還是會愣住；知道什麼是 trait，卻不太清楚為什麼有人要特地定義一個。

後來我發現，這段「從 hello world 到真實專案」的斷層沒什麼捷徑可以跳，只能找一個自己有興趣的 repo 硬著頭皮讀。所以我把 lineage-lite 拆成一組閱讀材料，照我當時的心路走一遍，希望對下一個卡在這裡的人有幫助。

## 建議閱讀順序

1. **[`../BEGINNER_GUIDE.md`](../BEGINNER_GUIDE.md)**
   不管 Rust 熟不熟，這份都值得先翻一下。它會先讓你知道 lineage 是什麼、這個專案在做什麼，有了領域背景，後面的 Rust code 才有意義。

2. **[01 — 專案導覽篇](./01-overview.md)**
   適合已經認得 `struct`、`enum`、`impl`、`trait`、`Vec`、`HashMap`、`Result` 這些 Rust 基本語法的讀者。這篇帶你看一個中型 Rust 專案怎麼拆模組、trait 在真實情境下長什麼樣、為什麼 code 會長得像個小框架。

3. **[02 — Rust 複習篇](./02-rust-notes.md)**
   針對本專案實際用到的寫法做一次整理：`mod`、`crate::`、`super::`、trait object、`Result<T>` 與 `?`、borrow。讀 01 或 03 時遇到卡住的語法可以翻來對照。

4. **[03 — 流程與練習篇](./03-code-flow.md)**
   跟著 `cargo run -- scan ./demo` 這行指令，一路走進 code 看它怎麼跑完整條 pipeline，最後附八題練習與參考答案。看完這篇，動手改這個 repo 會順很多。

5. **[`../TECHNICAL.md`](../TECHNICAL.md)**
   深入設計、演算法、SQL AST 處理、column-level lineage 的實作細節。適合想知道「這些選擇背後的 trade-off 是什麼」的人。

## 不同狀態的讀者該從哪裡開始

| 你現在的狀態 | 從哪裡讀 |
|---|---|
| 完全不會 Rust、也不知道 lineage 是什麼 | 先讀 [`../BEGINNER_GUIDE.md`](../BEGINNER_GUIDE.md)，docs/ 底下先放著 |
| 認得 Rust 基本語法，想看中型專案怎麼拆 | `BEGINNER_GUIDE` → `01` → `02`（卡住時翻）→ `03` |
| 已經熟 Rust，只想看設計 | 直接跳 [`../TECHNICAL.md`](../TECHNICAL.md) |
| 想動手貢獻或練手 | `03` 的練習題 → [`../WALKTHROUGH.md`](../WALKTHROUGH.md) |
