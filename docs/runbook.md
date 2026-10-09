# Brains 运行手册（local-rust profile）

面向本机运行与排障。协议与架构见 [architecture.md](architecture.md)；本文只讲"怎么跑、怎么备份、怎么修索引"。

## 构建

```bash
cargo build -p brains-server -p brains-cli   # 服务端 + CLI（二进制名 bsx）
pnpm --dir apps/web build                            # Web 前端（产物 apps/web/dist）
```

## 启动服务

```bash
./target/debug/brains-server \
  --data-dir ./brain-data \
  --vault default \
  --bind 127.0.0.1 --port 45800 \
  --token "$BRAINS_TOKEN" \
  --web-dist apps/web/dist
```

- `--token`：注册（或替换）全库 root token；也可用环境变量 `BRAINS_TOKEN`。省略时**生成一个并打印一次**，落在 `--data-dir` 下（见下）。
- `--web-dist`：同源托管 Web 应用；缺省时纯 API 服务。
- `--origin`：额外允许的 cookie 流 Origin；默认仅同源。
- 服务启动时自动应用 `migrations/` 内嵌 SQL（索引库）；无需手工迁移。

认证模型：root token 存 `auth.json`（0600）；Web 登录后发 HMAC 签名会话 cookie `brains_session`（HttpOnly, SameSite=Lax），cookie 变更请求要求 CSRF token + Origin 校验；API 直连用 `Authorization: Bearer`。详见 [architecture.md](architecture.md)。

## 数据布局（即裸备份布局）

```
<data-dir>/
  vaults/<vault>/
    format.json            # 库格式与能力
    heads/<path>.json      # 每路径 HEAD（唯一提交点，CAS）
    revisions/<hex>.json   # 不可变修订元数据
    blobs/<hex>/<name>     # 不可变内容（hard_link 去重）
    locks/                 # 路径级排他锁
  index.sqlite*            # 可重建投影（搜索/列表/链接）
  auth.json  secret.key    # token 注册表 / 会话签名密钥
```

**备份 = 拷贝 `<data-dir>`（或其下 `vaults/<vault>/`）**。索引库可随时删除重建，不在备份关键路径上。

## CLI

`bsx --help`。常用链路：

```bash
bsx login --base-url http://127.0.0.1:45800 --token "$BRAINS_TOKEN"  # 写 ~/.brains/config.json
bsx capabilities && bsx whoami
bsx ls ""                              # `ls /` 同义；LsArgs 支持 --prefix/--limit 等
bsx put note.md --file local.md       # 文本自动判型；二进制自动携带 X-Content-SHA256
bsx cat note.md / stat note.md / history note.md / show <revision>
bsx search "关键词" --mode literal|fts
bsx links note.md --kind outbound|backlinks
bsx patch note.md --set-kind note ...
bsx cp a.md b.md / mv a.md b.md / diff a.md b.md
bsx restore note.md --revision <hex>
bsx reindex --all-pages                # 重建索引投影（不碰 store）
bsx seed                              # 推送内置模板笔记（已存在的跳过，幂等）
bsx export ./worktree                 # 导出可读工作树（两阶段 revision 锚定 + manifest + casefold/便携名检查）
bsx edit note.md / add note.md        # $EDITOR 编辑后落库
```

冲突语义与 HTTP 一致：预置条件失败返回 412（携带当前 revision），CLI 报错并提示远端当前版本；不自动重试。

### 本地与云端双向同步

每个 endpoint 先登记为具名 profile，并通过 stdin 保存 token：

```bash
bsx profile add local --url http://127.0.0.1:45800
bsx --profile local login --stdin-token < local-token.txt
bsx profile add cloud --url https://<worker>.workers.dev
bsx --profile cloud login --stdin-token < cloud-token.txt
```

先预览再同步。第一次运行会把只存在于一端的 live 文件导入另一端；之后的运行会双向传递新版本和软删除：

```bash
bsx --profile local sync --remote-profile cloud --dry-run
bsx --profile local sync --remote-profile cloud
```

两端对同一文本的修改没有重叠时会自动三方合并。首次同步时同路径内容不同、重叠修改、二进制双改以及删除/修改都会列为冲突，保持两边当前内容不变并以退出码 `5` 返回。选择一侧，或准备好人工合并文件后写回两端：

```bash
bsx --profile local sync --remote-profile cloud resolve 'docs/guide.md' --keep local
bsx --profile local sync --remote-profile cloud resolve 'docs/guide.md' --keep remote
bsx --profile local sync --remote-profile cloud resolve 'docs/guide.md' --merged-file /tmp/guide.md
```

执行 `resolve` 前会重新读取两端 HEAD 并确认路径仍处于冲突；如果内容已经收敛或可自动合并，命令会拒绝过期决策并要求先重新运行 `sync`。

同步使用每个 profile pair 独立的基线文件，默认位于 `~/.brains/sync/`，也跟随 `BRAIN_CONFIG` 的父目录。基线只记录 endpoint 指纹、revision、状态和摘要，不包含正文或 token。profile pair 同步要求配置中保存具名 profile；执行时需 unset `BRAINS_URL` 与 `BRAINS_TOKEN`，避免一个环境变量覆盖两个 endpoint。同步覆盖当前文件与软删除，不复制旧 revision 历史。永久 purge 只作用于执行端，不会被同步成另一端的删除；若 purge 与另一端的并发修改同时被发现，CLI 会跳过或报告冲突并保留内容。若保留 tombstone 的一端之后被用户明确恢复，新的 live revision 可在后续 sync 中重新导入已 purge 端。写入逐路径使用 CAS，整库操作不是原子事务。

## 模板内容（首启播种）

- **本地**：`brains-server` 启动时若 vault 完全为空（无任何 HEAD，含墓碑），自动写入 4 篇模板笔记：`README.md`、`docs/brains-guide.md`（使用规则）、`docs/ai-guide.md`（AI 协作守则）、`inbox/README.md`。有任何内容即跳过，绝不覆盖。
- **云端**（Worker 首次部署后是空库）：`bsx --profile <cloud> seed` 推送同一套模板；已存在的路径自动跳过，可重复执行。

## GitHub 自动 Release / 部署清单（真实运行所需）

仓库内三个工作流已就绪，真实运行只需一次性配置：

**1. 自动发布（`.github/workflows/release.yml`）** — 无需任何 secret：
```bash
git tag v0.1.0 && git push origin v0.1.0     # 或 Actions 页手动 dispatch 填版本号
```
产物：macOS arm64/x64 与 Linux musl 的 server+CLI tar.gz（含 sha256）、web dist、worker bundle，自动创建 GitHub Release。注意 `Cargo.toml` / `package.json` 版本号需先手动 bump。

**2. 自动部署 Cloudflare Worker（`.github/workflows/deploy.yml`）** — 需要一次性准备：

| 步骤 | 命令 / 位置 |
|---|---|
| repo secrets | `CLOUDFLARE_API_TOKEN`（Workers/R2/D1 编辑权限）、`CLOUDFLARE_ACCOUNT_ID`（GitHub → Settings → Secrets and variables → Actions） |
| 资源开通（一次） | `pnpm --dir apps/worker exec wrangler r2 bucket create brains-data` |
| | `pnpm --dir apps/worker exec wrangler d1 create brains-index` → 把输出的 `database_id` 填进 `apps/worker/wrangler.toml` |
| 实例密钥（一次，永不入库） | `cd apps/worker && pnpm exec wrangler secret put TOKEN_REGISTRY_JSON`（内容形如 `{"tokens":[{"id":"default","name":"owner","token_sha256":"<sha256(token)>","scopes":["read:/","write:/","admin:/"],"enabled":true}]}`） |
| | `wrangler secret put SESSION_SIGNING_KEY`（64 位 hex）、`wrangler secret put CURSOR_SIGNING_KEY`（64 位 hex） |
| 可选 var | `wrangler.toml` 的 `[vars]` 追加 `ALLOWED_ORIGINS = "https://a.example,https://b.example"`（跨域 cookie 白名单，缺省仅同源） |
| 触发部署 | Actions → Deploy → Run workflow（可选先应用远程 D1 迁移），或本地 `pnpm --dir apps/worker exec wrangler deploy` |
| 部署后 | 给空库灌内容：`bsx profile add cloud --url https://<worker>.workers.dev --token <token> && bsx --profile cloud seed` |

**3. CI（`.github/workflows/ci.yml`）**：push/PR 自动跑 Rust 全门禁 + web build + 双运行时 e2e + worker smoke，无 secret 依赖。

## E2E（screenplay pattern）

```bash
pnpm --dir tests/e2e exec playwright test
```

`tests/e2e/playwright.config.ts` 自动拉起独立服务实例（端口 18799、独立数据目录 `/tmp/brains-e2e-data`），需环境变量 `BRAINS_SERVER_BIN`（服务端二进制）与 `BRAINS_WEB_DIST`（前端产物）。本机注意：若 build-cache 守护进程会清理 `target/` 内二进制，把构建与测试放在同一条命令内执行。

## Cloudflare 本地模拟（wrangler dev，无需云凭据）

Worker（cloudflare-rust profile）与 native 共享同一 axum HTTP 层，仅换装配：R2（HEAD/revision/blob）+ D1（索引）+ 环境变量密钥。本地模拟由 workerd 提供 R2/D1，真实模拟协议行为：

```bash
rustup target add wasm32-unknown-unknown
cargo install worker-build --locked
pnpm --dir apps/web build
cd apps/worker
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' worker-build --release   # 产物 build/worker/shim.mjs
pnpm exec wrangler d1 migrations apply BRAINS_INDEX --local            # 幂等 schema（migrations/d1/）
pnpm exec wrangler dev --port 8799                                    # 本地 workerd，静态资源 + /api 同域
```

- 本地密钥在 `apps/worker/.dev.vars`（TOKEN_REGISTRY_JSON 存 token 的 SHA-256 哈希；SESSION/CURSOR_SIGNING_KEY 为 64 位 hex）。生产用 `wrangler secret put` 同名注入，禁止明文入库。
- **`wrangler dev` 不自动应用 D1 迁移**：首次运行或清空 `.wrangler/state` 后，必须先执行上面的 `migrations apply`，否则索引为空、搜索恒 0 命中（服务端不伪装成功）。
- 验收速查：`bash tests/worker-smoke.sh`（workerd 运行中执行）覆盖 CAS 竞争、条件写、字面/FTS+prefix、trash→restore、历史链。
- 浏览器 e2e 对 workerd：`tests/e2e/run-workerd.sh`（自动重启 wrangler → 就绪探测 → playwright）。**重建 `apps/web/dist` 后必须重启 wrangler dev**：资产清单在启动时快照，在跑实例对新 hash 会落 SPA fallback 返回 text/html（浏览器报 MIME module script 错）。脚本会幂等确保 `.dev.vars` 注册表包含 e2e token 的 SHA-256（id: e2e）。
- CLI 对接：`bsx profile add worker --url http://127.0.0.1:8799 --token <token>` 后加 `--profile worker`，导出与 reindex 语义一致。

## 排障

- **搜索结果落后/缺失**：索引是有界投影，可能短暂落后于 HEAD（`complete=false` 或 503 Pending）。运行 `bsx reindex` 重建；无需重启。
- **412 VersionConflict**：另一客户端已推进 HEAD。重新读取后基于新基线重做；Web 端冲突视图提供三栏比较。
- **token 丢失**：`--token` 重新注册即替换 root token；`secret.key` 删除会使所有 Web 会话失效（重登录即可），不影响数据。
- **误删文件**：删除是墓碑（tombstone），历史保留；`bsx restore` 或 Web 回收站恢复，恢复产生新修订（generation 只前进）。
