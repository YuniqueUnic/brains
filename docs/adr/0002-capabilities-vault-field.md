# ADR 0002：Capabilities 增加可选 `vault` 字段

状态：已采纳（2026-10-07）

## 背景

最初定稿的 `capabilities` 载荷未包含实例管理的 vault 名。但单实例单 vault（`--vault`）的本地形态下，客户端（CLI `profile`、Web 顶栏）需要展示"我连的是哪个库"；多 vault 云形态（后续 M0）则可能一个入口代理多个 vault。

## 决策

在 `Capabilities` 上增加 `vault: Option<String>`（`crates/domain/src/capabilities.rs`）。local-rust profile 恒为 `Some(vault)`；序列化时 `None` 省略字段，因此与既有 openapi schema 向后兼容（旧客户端不感知）。

## 理由

- 单一数据源：vault 名由服务端声明，客户端不再自行猜测/配置；
- Option + 省略序列化使该扩展对契约无破坏，云形态下可自然回落为 `None` 或复用为当前 vault 指示；
- 替代方案（放 `format.json` 或独立端点）增加一次往返，且 capabilities 本就是"实例自述"的自然位置。

## 后果

- CLI `profile`/Web 顶栏 `token:default` 直接取自 capabilities；
- 若未来支持多 vault，该字段语义升级为"本次认证可见的当前 vault"，协议位不变。
