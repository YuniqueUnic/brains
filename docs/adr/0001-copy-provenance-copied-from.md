# ADR 0001：copy 用 `copied_from` 在新修订上记录溯源

状态：已采纳（2026-10-07）

## 背景

copy（以及 move 的目标端）需要回答"这份内容从哪来"。两个可选位置：

1. 在**源路径**上记录"被拷贝到哪"（出向链接/反向登记）；
2. 在**新修订**上记录 `copied_from: {path, revision}`（入向溯源）。

## 决策

采用方案 2：`CopyProvenance { path, revision }` 作为新修订元数据的一部分（`crates/domain` 定义，`crates/application/src/commit.rs` 写入，`crates/api-types` 经 `RevisionDto.copied_from` 暴露；源修订零改动）。

## 理由

- **不可变性零破坏**：修订一旦写入不可变；在源上登记"被拷贝"必须回写历史修订或引入新的可变附属结构，两者都违反核心模型。
- **HEAD 单点语义不变**：溯源随新修订一起经 HEAD CAS 原子生效；任何失败路径下都不会出现"内容到了、溯源丢了"的中间态。
- **查询方向够用**：本仓库的 UI/CLI 只需要"这份从哪来"；"谁拷贝了我"可由索引投影按 `copied_from.path` 聚合得出（可重建），不必侵入 store。

## 后果

- `diff`/`show` 可直接展示溯源；move 的目标修订同样携带 `copied_from`（action=move 语义）。
- 若未来需要"反链式拷贝视图"，由索引层投影实现，不改 store 协议。
