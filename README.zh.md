<h1 align="left">
  <img src="apps/web/public/favicon.svg" width="64" height="64" align="middle" alt="" />
  <img src="docs/assets/brains-signature.svg" width="240" height="117" align="middle" alt="Brains" />
</h1>

**面向人类、可迁移的长期知识库——进来的是普通文件，出去的也是普通文件，且每一版都留着。**

[English](README.md) · [架构文档](docs/architecture.md) · [运行手册](docs/runbook.md) · [导入、同步与迁移](docs/portable-workflow.md)

[![CI](https://github.com/YuniqueUnic/brains/actions/workflows/ci.yml/badge.svg)](https://github.com/YuniqueUnic/brains/actions/workflows/ci.yml)

Brains **不是** AI memory framework：没有召回引擎、没有向量库、没有上下文装配。它只做一件事——为已经在写的笔记提供版本化存储，核心承诺是：**数据库可以消失，正文和历史不能。**

| 登录 | 工作台 |
|---|---|
| ![登录](docs/screenshots/01-login.png) | ![工作台](docs/screenshots/02-workspace.png) |
| *全库搜索* | *冲突工作台——分栏/内联 diff，绝不自动覆盖* |
| ![搜索](docs/screenshots/03-search.png) | ![冲突](docs/screenshots/04-conflict.png) |
| *暗色主题* | |
| ![暗色](docs/screenshots/05-dark.png) | |

Web 界面内置 **暗色 / 亮色 / 跟随系统** 三档主题与 **命名空间化 i18n（简体中文 / English，基于 i18next）**，均可在顶栏一键切换并按浏览器记忆。

## 在线演示

直接打开[浏览器演示](https://YuniqueUnic.github.io/brains/)，无需账号或服务器。演示文件仅保存在当前页面，刷新后重置；编辑器的本地草稿可能仍留在浏览器中。演示搜索使用简化的内存实现。

## 核心性质

- **开箱即有内容，不是空白屏。** 原生服务器首次启动空库自动播种五篇模板笔记——欢迎页、使用规则、**AI 协作守则**、**自包含记忆写作**与收件箱，人和 agent 都从可用的约定开始（纯本地 bsx 和云端实例用 `bsx seed`）。
- **对 AI 天然友好。** [docs/ai-integration.md](docs/ai-integration.md) 是面向机器的 API 参考（端点、CAS 礼仪、错误模型），`llms.txt` 为爬虫/agent 提供项目索引，且每个库内部自带精简守则（`docs/ai-guide.md`）。

- **不可变修订 + 唯一原子提交点。** 每次保存写入内容寻址的 blob 与修订记录，再以 CAS（compare-and-swap）原子替换该路径的 HEAD。两个客户端基于同一版本并发保存：恰好一个成功，另一个收到携带当前版本的 `412`——不存在静默覆盖。
- **冲突是一等公民。** 冲突工作台提供 VSCode 式 **分栏**（远端 vs 本地并排、差异块双侧同步高亮）与 **内联**（逐块 Accept/Reject）两种 diff 视图，均可编辑；"以本地保存"或"采用远端"必须由人决定；决策期间远端再变会再次冲突，绝不自动重试。编辑中还可一键开启 **与已保存基线的内联 diff**，未保存的改动一目了然，支持逐块回退。
- **真编辑器，不是文本框。** CodeMirror 提供查找替换（大小写/正则/全词、全部替换）与选区匹配高亮；侧栏为文件夹树，支持 **多选批量操作**（批量移入回收站 / 批量恢复）。
- **删除是墓碑，恢复是新版本。** generation 只前进不倒退；历史面板可把任意有内容的版本恢复为新修订。
- **本地与云端 profile 双向同步。** `bsx sync` 依据逐文件 revision 基线与 CAS 传递变更；无重叠文本修改自动合并，分歧文件保留两侧内容并交由用户明确解决。
- **普通目录与完整历史各有明确入口。** `bsx import/export` 搬运可读工作树；`--format archive` 在 FS/Turso 与 R2/D1 间保留完整可达 revision 和 blob。`profile use --verify-with` 实读正文、重建并核验目标索引后才切换日常主库；中断恢复、shell 补全及带 Git hash 的构建身份见[可移植工作流](docs/portable-workflow.md)。
- **磁盘布局即备份。** `vaults/{vault}/heads|revisions|blobs`——拷贝目录即完整裸备份；索引（Turso/D1）是可随时删除重建的投影。
- **一份 HTTP 契约，三类客户端。** Web、remote 模式 CLI、curl/agent 走同一套版本化 REST API；local 模式 CLI 直接调用同一应用层；两个运行时（原生 Axum 与 Cloudflare Worker）复用同一 axum 层。
- **一切有界。** 字面检索是有界窗口、绝不伪造"已搜完全库"；索引损坏时响亮降级（`503` / `building`）；超限文本只存储、不入索引。

[Markdown 元信息和预览用法](docs/markdown-preview.md)

## 快速开始：先用一个 bsx

免编译安装（直接取 GitHub Release 预编译产物，后续用 `bsx update --check` 跟进新版本），或用 cargo 从源码构建：

```bash
cargo binstall --git https://github.com/YuniqueUnic/brains brains-cli
bsx update            # 之后随时：自升级（SHA-256 校验、原子替换）
# 或者：cargo install --locked --git https://github.com/YuniqueUnic/brains brains-cli
```

```bash
bsx init "$HOME/.brains/my-library" --profile laptop
bsx import ./notes --dry-run
bsx import ./notes
bsx ls
bsx search "关键词" --mode hybrid
```

这是 `local` 模式：bsx 直接管理本地文件存储与索引，无需服务、端口或 token。`init` 创建空库并设为默认 profile；需要模板时执行 `bsx seed`。

正式发布的 `bsx` 自带完整本地能力（完整矩阵见 [docs/capabilities.md](docs/capabilities.md)）：

| 命令 | 作用 |
|---|---|
| `bsx search --mode hybrid` | 字面 + FTS 加权倒数排名融合；逐命中检索溯源；语义检索槽位已预留（尚未接入任何 provider，如实报告） |
| `bsx ls --status draft --type Decision --stale` | 基于索引投影的生命周期/信任过滤（`status`、`type`、`trust`、`stale_after`） |
| `bsx check` | 质量报告：过期笔记、未确认的 agent 结论、断链、脚注缺口（`--okf` 为 OKF v0.2 一致性检查） |
| `bsx checkpoint show/update <scope>` | 按 scope 的连续性检查点笔记，CAS 写入 |
| `bsx convert <attachment>` | PDF/DOCX/XLSX/PPTX/HTML/CSV/TXT → Markdown 伴随笔记（离线内嵌转换器） |
| `bsx import-url <url> <path>` | 网页 → Markdown，严格抓取阶梯（SSRF 防护、robots 遵循） |
| `bsx checkout/status/pull/push` | 受跟踪工作副本，三方合并与冲突上报 |
| `bsx seed --refresh` | 向老库补种模板笔记（内容比对、CAS 保护） |
| `bsx update` | 从 GitHub Release 自升级（SHA-256 校验、原子替换） |

需要 Web 或多人协作时，再启动服务器并添加 `remote` profile。服务器在本机还是公网，用法相同：

```bash
bsx profile add cloud --url "$CLOUD_URL"
printf '%s' "$CLOUD_TOKEN" | bsx --profile cloud login --stdin-token
bsx --profile laptop sync --remote-profile cloud
```

同步用于当前状态协作；保留每代历史用 `export/import --format archive`。完整的“离线首用 → 远端协作 → 本机 Web → 多服务器同步”步骤见[本地与远端模式](docs/local-remote.md)。同一个数据目录只允许一个进程直接打开，服务器接管期间通过 remote profile 访问它。

认证、服务器启动、备份与排障见[运行手册](docs/runbook.md)。

## Cloudflare 本地模拟（无需云账号）

同一 axum 层以 Worker 形态运行在 R2（内容）+ D1（可重建索引）之上。`wrangler dev` 在本地 workerd 里执行，R2/D1 均为本地模拟——**协议行为与生产一致**：

```bash
rustup target add wasm32-unknown-unknown
cargo install worker-build --locked
pnpm --dir apps/web build
cd apps/worker
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' worker-build --release
pnpm exec wrangler d1 migrations apply BRAINS_INDEX --local
pnpm exec wrangler dev --port 8799          # → http://localhost:8799
bash tests/worker-smoke.sh                  # CAS 竞争、搜索、回收站/恢复验收
```

## 架构

```
浏览器 ──/ (静态)──▶ Workers Static Assets  │  原生 Axum + web dist
remote CLI / curl ──/api/v1/*──┐
                        ▼
      共享 axum 层（crates/api-http）：路由 · 错误映射 · 鉴权
                        │
      ┌─────────────────┴──────────────────┐
      ▼ local-rust profile                ▼ cloudflare-rust profile
  文件系统 + 跨进程锁（HEAD CAS）       R2 binding（存储 ETag 作 CAS token）
  Turso + 可选 Tantivy                 D1 binding（SQLite FTS5）
      apps/server                          apps/worker（workers-rs）
```

| Crate | 职责 |
|---|---|
| `crates/domain` | 路径、HEAD/Revision、错误分类、能力报告 |
| `crates/format` | 有界 frontmatter/Markdown 解析、链接提取 |
| `crates/ports` | 存储 trait——CAS HEAD、create-only blob/revision、索引（刻意不强制 `Send`，Worker 侧桥接 `!Send` future） |
| `crates/application` | 全部业务编排：提交管线、搜索、恢复、patch、copy/move、reindex |
| `crates/api-http` | 两个运行时共享的 HTTP 层（路由、错误映射、鉴权） |
| `crates/adapter-fs` / `adapter-turso` | 本地 profile 适配器 |
| `crates/adapter-r2` / `adapter-d1` | Worker profile 适配器（同键布局、同 SQL） |
| `crates/local` | CLI 与原生服务器共用的本地存储装配，无 HTTP 与认证 |
| `crates/client` | 统一 Library 接口与 local/HTTP 实现，共用归档和同步 |
| `apps/web` | React + CodeMirror 编辑器（显式 load/clean/dirty/saving/conflict 状态机） |
| `apps/server` · `apps/worker` | 各 profile 的薄装配 |

## 测试

Rust 构建配置统一在 workspace 根目录：`dev` 保留源码位置与增量编译，`release` 使用体积优化、Thin LTO 和符号裁剪。CI 的原生端到端任务使用 release 二进制验证功能组合、CLI 操作和浏览器流程。构建命令及调试取舍见[开发与发布构建](docs/releasing.md#开发与发布构建)。原生检索基于 Turso，可选启用 Tantivy——见[构建开关与搜索规则](docs/turso.md)。

- **Rust 测试**——domain、format、适配器、应用层业务测试（真实 fs+Turso，注入 CAS/索引故障）、HTTP 与 CLI 契约测试。
- **浏览器 E2E（SerenityJS screenplay）**——对真实服务器与 workerd：登录、创作、CAS 冲突与再冲突、字面/FTS/混合检索、回收站/恢复、历史恢复。
- **独立 CLI 业务验收与快照**——离线导入、编辑、历史、分页、服务器接管与多端协作，见 `tests/local-workflows.py`；local/remote 两类来源分别与 Worker 验证归档和同步。
- **Worker 业务冒烟**——同一验收清单对 workerd 执行（`tests/worker-smoke.sh`）。

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
pnpm lint
pnpm --dir apps/web build && pnpm --dir tests/e2e exec playwright test   # 需 BRAINS_SERVER_BIN/BRAINS_WEB_DIST
bash tests/worker-smoke.sh                                               # 对运行中的 wrangler dev
```

## CI · 发布 · 部署

| Workflow | 触发 | 内容 |
|---|---|---|
| [ci.yml](.github/workflows/ci.yml) | push / PR | fmt · check · clippy `-D warnings` · Rust 测试 · SDK 代码生成漂移检查 · wasm 检查 · JS/TS lint · web 构建 · screenplay E2E（原生 **与** Worker）· workerd 业务冒烟 |
| [pages.yml](.github/workflows/pages.yml) | push / PR / 手动 | 构建演示并执行 6 项浏览器验收；只有 `main` 可以部署到 Pages。 |
| [release-please.yml](.github/workflows/release-please.yml) | `main` push / 手动 | 一个发布 PR 统一更新所有产品版本；合并后创建标签和发布说明，再启动产物构建。见[发版说明](docs/releasing.md)。 |
| [release.yml](.github/workflows/release.yml) | 从 `main` 触发，输入已有发布标签 | 构建 CLI+server（macOS arm64/x64、Linux musl）、web 产物和 Worker 包，校验标签后上传产物及 SHA-256。 |
| [deploy.yml](.github/workflows/deploy.yml) | 手动 dispatch | 构建 Worker 并部署到 Cloudflare（`wrangler deploy` + 远端 D1 迁移）。需要 `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` secrets 与文件头部记载的一次性资源开通 |
| dependabot | 每周 | cargo · npm · actions 依赖更新 |

`main` 只接受通过 PR 合入的变更。`Development gates` 和 `Build web demo` 必须在最新主分支基础上通过；禁止强推和删除主分支，管理员也不能绕过。优先使用 rebase 合并，合并前须解决审查讨论。个人仓库不强制第二位审核者批准。

## 状态与如实声明

- local-rust profile：**已完成并验证**（P0 全量 + patch/copy/move/diff/export、生命周期元数据、混合检索、文档转换、URL 导入、质量检查、工作副本）。
- cloudflare-rust profile：**已实现并经本地 workerd 验证**；真实账户部署、真实 R2/D1 行为与免费额度 CPU 预算待凭据后验证。
- Windows 与非 Chromium 浏览器未测试；尚未做长时间并发压测。

## 许可

[MIT](LICENSE)

## Docker 部署

使用 `docker compose up -d` 启动服务端和内置 Web，数据保存在持久化卷中。正式发布提供 Linux AMD64、ARM64 镜像；镜像默认携带完整能力构建，`BRAINS_DISABLE=document_conversion,semantic` 可在启动时停止声明所选能力。命名卷、宿主目录挂载、非 root 权限、文件 secret、HTTPS 与源码构建见 [Docker 部署说明](docs/docker.md)；能力控制模型见 [capabilities](docs/capabilities.md)。
