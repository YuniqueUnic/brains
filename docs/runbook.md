# BrainStore 运行手册（local-rust profile）

面向本机运行与排障。协议与架构见 `brainstore-design/BrainStore-Architecture-v1.md`；本文只讲"怎么跑、怎么备份、怎么修索引"。

## 构建

```bash
cargo build -p brainstore-server -p brainstore-cli   # 服务端 + CLI
pnpm --dir apps/web build                            # Web 前端（产物 apps/web/dist）
```

## 启动服务

```bash
./target/debug/brainstore-server \
  --data-dir ./brain-data \
  --vault default \
  --bind 127.0.0.1 --port 8787 \
  --token "$BRAIN_TOKEN" \
  --web-dist apps/web/dist
```

- `--token`：注册（或替换）全库 root token；也可用环境变量 `BRAIN_TOKEN`。省略时**生成一个并打印一次**，落在 `--data-dir` 下（见下）。
- `--web-dist`：同源托管 Web 应用；缺省时纯 API 服务。
- `--origin`：额外允许的 cookie 流 Origin；默认仅同源。
- 服务启动时自动应用 `migrations/` 内嵌 SQL（索引库）；无需手工迁移。

认证模型（设计 §15）：root token 存 `auth.json`（0600）；Web 登录后发 HMAC 签名会话 cookie `brain_session`（HttpOnly, SameSite=Lax），cookie 变更请求要求 CSRF token + Origin 校验；API 直连用 `Authorization: Bearer`。

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

`brainstore-cli --help`。常用链路：

```bash
brainstore-cli login --base-url http://127.0.0.1:8787 --token "$BRAIN_TOKEN"  # 写 ~/.brainstore/config.json
brainstore-cli capabilities && brainstore-cli whoami
brainstore-cli ls ""                              # `ls /` 同义；LsArgs 支持 --prefix/--limit 等
brainstore-cli put note.md --file local.md       # 文本自动判型；二进制自动携带 X-Content-SHA256
brainstore-cli cat note.md / stat note.md / history note.md / show <revision>
brainstore-cli search "关键词" --mode literal|fts
brainstore-cli links note.md --kind outbound|backlinks
brainstore-cli patch note.md --set-kind note ...
brainstore-cli cp a.md b.md / mv a.md b.md / diff a.md b.md
brainstore-cli restore note.md --revision <hex>
brainstore-cli reindex --all-pages                # 重建索引投影（不碰 store）
brainstore-cli export ./worktree                 # 导出可读工作树（两阶段 revision 锚定 + manifest + casefold/便携名检查）
brainstore-cli edit note.md / add note.md        # $EDITOR 编辑后落库
```

冲突语义与 HTTP 一致：预置条件失败返回 412（携带当前 revision），CLI 报错并提示远端当前版本；不自动重试。

## E2E（screenplay pattern）

```bash
pnpm --dir tests/e2e exec playwright test
```

`tests/e2e/playwright.config.ts` 自动拉起独立服务实例（端口 18799、独立数据目录 `/tmp/brainstore-e2e-data`），需环境变量 `BRAIN_SERVER_BIN`（服务端二进制）与 `BRAIN_WEB_DIST`（前端产物）。本机注意：若 build-cache 守护进程会清理 `target/` 内二进制，把构建与测试放在同一条命令内执行。

## Cloudflare 本地模拟（wrangler dev，无需云凭据）

Worker（cloudflare-rust profile）与 native 共享同一 axum HTTP 层，仅换装配：R2（HEAD/revision/blob）+ D1（索引）+ 环境变量密钥。本地模拟由 workerd 提供 R2/D1，真实模拟协议行为：

```bash
rustup target add wasm32-unknown-unknown
cargo install worker-build --locked
pnpm --dir apps/web build
cd apps/worker
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' worker-build --release   # 产物 build/worker/shim.mjs
pnpm exec wrangler d1 migrations apply BRAIN_INDEX --local            # 幂等 schema（migrations/d1/）
pnpm exec wrangler dev --port 8799                                    # 本地 workerd，静态资源 + /api 同域
```

- 本地密钥在 `apps/worker/.dev.vars`（TOKEN_REGISTRY_JSON 存 token 的 SHA-256 哈希；SESSION/CURSOR_SIGNING_KEY 为 64 位 hex）。生产用 `wrangler secret put` 同名注入，禁止明文入库。
- **`wrangler dev` 不自动应用 D1 迁移**：首次运行或清空 `.wrangler/state` 后，必须先执行上面的 `migrations apply`，否则索引为空、搜索恒 0 命中（服务端不伪装成功）。
- 验收速查：`bash tests/worker-smoke.sh`（workerd 运行中执行）覆盖 CAS 竞争、条件写、字面/FTS+prefix、trash→restore、历史链。
- CLI 对接：`brain profile add worker --url http://127.0.0.1:8799 --token <token>` 后加 `--profile worker`，导出与 reindex 语义一致。

## 排障

- **搜索结果落后/缺失**：索引是有界投影，可能短暂落后于 HEAD（设计 §10，`complete=false` 或 503 Pending）。运行 `brainstore-cli reindex` 重建；无需重启。
- **412 VersionConflict**：另一客户端已推进 HEAD。重新读取后基于新基线重做；Web 端冲突视图提供三栏比较。
- **token 丢失**：`--token` 重新注册即替换 root token；`secret.key` 删除会使所有 Web 会话失效（重登录即可），不影响数据。
- **误删文件**：删除是墓碑（tombstone），历史保留；`brainstore-cli restore` 或 Web 回收站恢复，恢复产生新修订（generation 只前进）。
