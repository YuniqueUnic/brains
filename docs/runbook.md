# Brains 运行手册（native 与 Cloudflare Worker）

面向本机运行、隔离 Worker 验收、真实云部署准备与排障。协议与架构见 [architecture.md](architecture.md)；本文只讲"怎么跑、怎么备份、怎么修索引"。

可读目录导入导出、完整历史 archive、profile 同步与验收后切库、Git 构建身份和五种 shell 补全见 [可移植工作流](portable-workflow.md)。

## 构建

```bash
mbx build -p brains-server -p brains-cli   # 服务端 + CLI（二进制名 bsx）
pnpm --dir apps/web build                            # Web 前端（产物 apps/web/dist）
```

## 只用 bsx

```bash
bsx init "$HOME/.brains/my-library" --profile laptop
bsx import ./notes
bsx ls
```

以上 local 模式无需服务器和 token。Web 是可选入口；离线库和服务器之间的接管、同步、完整历史搬迁见[本地与远端模式](local-remote.md)。

## 启动服务

```bash
./target/debug/brains-server \
  --data-dir ./brain-data \
  --vault default \
  --bind 127.0.0.1 --port 45800 \
  --web-dist apps/web/dist
```

- 推荐通过环境变量 `BRAINS_TOKEN` 提供全库 root token，避免值进入进程参数。`--token` 同样注册或替换 root token；首次启动且没有配置时生成并打印一次，之后使用数据目录内的注册表。
- `--web-dist`：同源托管 Web 应用；缺省时纯 API 服务。
- `--origin`：额外允许的 cookie 流 Origin；默认仅同源。
- 服务启动时自动应用 `migrations/` 内嵌 SQL（索引库）；无需手工迁移。

认证模型：token 注册表存 `auth.json`（0600）。显式提供的 token 只存 SHA-256 摘要；首次自动生成的 root token 另存可恢复明文。CLI 的原始 token 存私有配置；Web 登录后发 HMAC 签名会话 cookie `brains_session`（HttpOnly, SameSite=Lax），cookie 变更请求要求 CSRF token + Origin 校验；API 直连用 `Authorization: Bearer`。native 仅在实际配置绑定 loopback IP，且请求为同源 loopback HTTP 时省略 `Secure`，使 WebKit 可在本机登录；HTTPS、公共 Host、跨源 allowlist、非 loopback 绑定和 Worker 始终保留 `Secure`。转发头不会放宽策略。IPv6 本机绑定使用 `--bind ::1`，浏览器使用 `http://[::1]:<port>`。详见 [architecture.md](architecture.md)。

## 数据布局（即裸备份布局）

```
<data-dir>/
  vaults/<vault>/
    format.json            # 库格式与能力
    heads/<path>.json      # 每路径 HEAD（唯一提交点，CAS）
    revisions/<hex>.json   # 不可变修订元数据
    blobs/<hex>/<name>     # 不可变内容（hard_link 去重）
    locks/                 # 路径级排他锁
    index.db*              # 每个 vault 独立的可重建索引
  .brains.lock             # 数据目录所有权锁
  auth.json  secret.key    # token 注册表 / 会话签名密钥
```

**备份 = 拷贝 `<data-dir>`（或其下 `vaults/<vault>/`）**。备份前停止写入；索引是派生数据，不是正文备份。新位置没有索引时自动从权威 HEAD 完整重建，失败不发布半成品。停止进程后才能维护其数据库和目录锁文件。

## CLI

`bsx --help`。先登记 profile；后面的片段是独立命令示例，不是顺序执行的脚本。`BASE_REVISION` 从当前 `stat --json` 读取，`OLD_REVISION` / `NEW_REVISION` 从 `history --json` 选择；每次成功写入后重新读取基线。

```bash
unset BRAINS_URL BRAINS_TOKEN
bsx profile add home --url http://127.0.0.1:45800
bsx --profile home login --stdin-token < /absolute/private/local-token.txt
bsx --profile home capabilities
bsx --profile home whoami
bsx --profile home ls ""                       # prefix 是位置参数；自动遍历所有页
bsx --profile home put note.md --from local.md --create
bsx --profile home cat note.md
bsx --profile home stat note.md --json
bsx --profile home history note.md --json
bsx --profile home show note.md --revision "$OLD_REVISION"
bsx --profile home search "关键词" --mode literal
bsx --profile home links note.md               # 默认出链；反链加 --backlinks
bsx --profile home patch note.md --if-revision "$BASE_REVISION" --old "old text" --new "new text"
bsx --profile home cp note.md copy.md --if-revision "$BASE_REVISION"
bsx --profile home mv note.md moved.md --if-revision "$BASE_REVISION"
bsx --profile home diff note.md --from "$OLD_REVISION" --to "$NEW_REVISION"
bsx --profile home restore note.md --revision "$OLD_REVISION" --if-revision "$BASE_REVISION"
bsx --profile home reindex --all-pages         # 重建索引投影，不改权威历史
bsx --profile home seed                        # 已有路径或墓碑跳过
bsx --profile home export ./worktree           # 可读工作树；输出目录必须不存在
bsx --profile home edit note.md --if-revision "$BASE_REVISION"  # 用 $EDITOR 编辑已有文件
```

冲突语义与 HTTP 一致：预置条件失败返回 412（携带当前 revision），CLI 报错并提示远端当前版本；不自动重试。

### 本地与云端双向同步

每个 endpoint 先登记为具名 profile，并通过 stdin 保存 token。下面是当前状态同步；如果要迁移完整历史，直接使用后面的 archive 流程，不能先对空目标执行 sync：

```bash
unset BRAINS_URL BRAINS_TOKEN
bsx profile add home --url http://127.0.0.1:45800
bsx --profile home login --stdin-token < local-token.txt
bsx profile add cloud --url https://<worker>.workers.dev
bsx --profile cloud login --stdin-token < cloud-token.txt
```

先预览再同步。第一次运行会把只存在于一端的 live 文件导入另一端；之后的运行会双向传递新版本和软删除：

```bash
bsx --profile home sync --remote-profile cloud --dry-run
bsx --profile home sync --remote-profile cloud
```

两端对同一文本的修改没有重叠时会自动三方合并。首次同步时同路径内容不同、重叠修改、二进制双改以及删除/修改都会列为冲突，保持两边当前内容不变并以退出码 `5` 返回。选择一侧，或准备好人工合并文件后写回两端：

```bash
bsx --profile home sync --remote-profile cloud resolve 'docs/guide.md' --keep local
bsx --profile home sync --remote-profile cloud resolve 'docs/guide.md' --keep remote
bsx --profile home sync --remote-profile cloud resolve 'docs/guide.md' --merged-file /tmp/guide.md
```

执行 `resolve` 前会重新读取两端 HEAD 并确认路径仍处于冲突；如果内容已经收敛或可自动合并，命令会拒绝过期决策并要求先重新运行 `sync`。

同步使用每个 profile pair 独立的基线文件，默认位于 `~/.brains/sync/`，也跟随 `BRAIN_CONFIG` 的父目录。基线只记录 endpoint 指纹、revision、状态和摘要，不包含正文或 token。profile pair 同步要求配置中保存具名 profile；执行时需 unset `BRAINS_URL` 与 `BRAINS_TOKEN`，避免一个环境变量覆盖两个 endpoint。同步覆盖当前文件与软删除，不复制旧 revision 历史。永久 purge 只作用于执行端，不会被同步成另一端的删除；若 purge 与另一端的并发修改同时被发现，CLI 会跳过或报告冲突并保留内容。若保留 tombstone 的一端之后被用户明确恢复，新的 live revision 可在后续 sync 中重新导入已 purge 端。写入逐路径使用 CAS，整库操作不是原子事务。

## 模板内容（首启播种）

- **本地**：`brains-server` 启动时若 vault 完全为空（无任何 HEAD，含墓碑），自动写入 5 篇模板笔记：`README.md`、`docs/brains-guide.md`（使用规则）、`docs/ai-guide.md`（AI 协作守则）、`docs/memory-writing.md`（自包含记忆写法）、`inbox/README.md`。有任何内容即跳过，绝不覆盖。
- **新建独立云库**：Worker 不自动播种；明确要新库模板时运行 `bsx --profile <cloud> seed`，已有路径和墓碑都会跳过。**用于完整历史迁移的目标保持空库**，先执行 archive restore，不能先 seed、sync 或工作树 import。native 迁移目标用 `--no-seed`。

## GitHub 自动 Release / 部署清单（真实运行所需）

仓库提供 Release、Deploy 与 CI 工作流；工作流文件存在不代表已有 GitHub remote、云账号或生产资源。本机当前部署条件见 [2026-10-10 续验记录](evidence/2026-10-10-continuation/README.md)。

**1. 自动发布（`.github/workflows/release.yml`）** — 使用 GitHub 自动提供的 `GITHUB_TOKEN`；只有 publish job 需要 `contents: write`，不需要 Cloudflare 凭据：
```bash
git tag v0.1.0 && git push origin v0.1.0     # 或 Actions 页手动 dispatch 填版本号
```
先更新根 `Cargo.toml` 的 `[workspace.package].version`，并确保同一提交已通过 CI（含 SDK codegen drift）。tag 与手动输入都必须是 `v<workspace version>`；例如当前 `0.1.0` 只接受 `v0.1.0`。prepare 统一输出 tag、version、commit SHA，所有构建 job 固定检出该 SHA。Web/Worker 的 package.json 是私有构建工具配置，不承担公开 Release 版本。

产物包括 macOS arm64/x64 与 Linux musl 的 server+CLI tar.gz、web dist、完整 Worker bundle 和已提交的 OpenAPI；每个 tar.gz 都带 SHA-256 文件。native job 会实际运行 `bsx --version`，精确核对 SemVer、Git SHA 前 12 位和 clean 状态，再打包 `VERSION` 与自包含的 [native 使用说明](native-release.md)。macOS 使用 `macos-15`/`macos-15-intel`，当前 workflow 不执行代码签名或公证。[GitHub runner 说明](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)

publish 会把远端 tag 解引用到 commit 后与构建 SHA 比较，支持 annotated tag。已有同 SHA tag 可用于失败后的重跑；指向其它提交时拒绝发布，绝不强推。仅手动 dispatch 可以创建缺失的 tag；tag push 触发的运行若发现标签被删除，会失败而不是自动重建。`gh release create --verify-tag` 阻止 CLI 自动创建错误标签；已经正式发布的 Release 不会被自动覆盖，再次 create 将明确失败，需先检查既有发布状态。[GitHub CLI 说明](https://cli.github.com/manual/gh_release_create)

Worker 包含 `build/`（含 shim、JS、Wasm）、`web/dist/`、`migrations/d1/`、`wrangler.json`、`release.json` 和独立部署 README。`scripts/package-worker.py` 从开发配置生成包内相对路径，移除源码 `[build]`，保留 binding、路由、限制和 required secrets；Wrangler 精确版本取自 frozen-lockfile 安装结果并写入 README。解包后不需要 Rust 源码或另一个 Web 包。首次部署仍需填写真实 D1 ID、R2 目标和实例 secrets；包内不附凭据。[Wrangler 配置](https://developers.cloudflare.com/workers/wrangler/configuration/)

本地包验收必须在源码目录外解压，再从该目录使用包内 `wrangler.json` 做 dry-run、本地 D1 migration、静态资源和 Worker 业务 smoke；不能改回源码目录的 assets/migrations 路径来得到通过。当前本地实测、Git tag 失败分支和系统 Safari 限制见 [Release 验收记录](evidence/2026-10-10-release-readiness/README.md)。GitHub 托管矩阵及实际发布尚需配置 remote 后触发验证。

**2. Cloudflare Worker 部署（`.github/workflows/deploy.yml`）**：首次部署与后续工作流部署分开准备。

| 前置 | 要求 |
|---|---|
| 账号权限 | Cloudflare API token 具有目标账号 Workers/R2/D1 编辑权限，已确认 account ID |
| 独立资源 | 创建 `brains-data` R2 bucket 与 `brains-index` D1，把实际 D1 ID 写入 `apps/worker/wrangler.toml`，不能带着 `database_id = "local"` 上云 |
| 目标隔离 | 一个实例管理一个 vault；D1 表的主键是 path，没有 vault 列，因此换 `VAULT_ID` 不能把复用的 D1 变成空目标。新目标使用独立 D1；独立 R2 bucket 也便于验收和回滚 |
| 三个实例 secret | `TOKEN_REGISTRY_JSON`、`SESSION_SIGNING_KEY`、`CURSOR_SIGNING_KEY`，配置的 `secrets.required` 会让缺失 secret 的部署失败 |
| GitHub 部署 | 配置 repository remote 和 Actions secrets `CLOUDFLARE_API_TOKEN`、`CLOUDFLARE_ACCOUNT_ID`，再运行 Deploy workflow；此前应完成首次 secret 初始化 |

`TOKEN_REGISTRY_JSON` 的值是 JSON 字符串，内容为 `{"tokens":[{"id":"owner","name":"owner","token_sha256":"<sha256 of bearer token>","scopes":["read:/","write:/","admin:/"],"enabled":true}]}`；两个签名 key 各为 32 字节随机数的 64 位 hex 表示。保管原始 Bearer token，以便 CLI 登录；不要把它写入 Git 或命令参数。将三项值写在仓库外、权限 0600 的 JSON secret 文件中，使用绝对路径。

首次部署时将三项 secret 与代码一起上传；这是实际部署，会修改云资源。`wrangler secret put` 同样会创建并立即部署版本，不能作为无副作用的准备步骤。[Cloudflare secrets 官方说明](https://developers.cloudflare.com/workers/configuration/secrets/)

```bash
pnpm --dir apps/worker exec wrangler whoami
pnpm --dir apps/worker exec wrangler r2 bucket create brains-data
pnpm --dir apps/worker exec wrangler d1 create brains-index
# 将返回的 D1 ID 填入配置，并准备上述私有 secret 文件后：
pnpm --dir apps/web build
mbx install worker-build --locked
pnpm --dir apps/worker exec wrangler d1 migrations apply BRAINS_INDEX --remote
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' \
  pnpm --dir apps/worker exec wrangler deploy --secrets-file /absolute/private/brains-secrets.json
```

Wrangler 的 `[build]` 会在 dev/deploy 时执行 `worker-build --release`。Deploy workflow 只预构建 Web 并安装工具，由 Wrangler 构建一次 wasm，避免重复编译。[Cloudflare custom builds](https://developers.cloudflare.com/workers/wrangler/custom-builds/)

完整历史迁移先确认两端 URL、vault，源端与目标端的 `whoami` 均拥有全库 `admin:/`，且目标真实索引查询成功；`/healthz` 不会验证 D1，空库 dry-run 也不能代替 admin 权限检查。操作窗口暂停源库写入，保留 archive 后再导入：

```bash
unset BRAINS_URL BRAINS_TOKEN
bsx --profile home whoami
bsx profile add cloud --url https://<worker>.workers.dev
bsx --profile cloud login --stdin-token < /absolute/private/cloud-token.txt
bsx --profile cloud capabilities
bsx --profile cloud whoami
bsx --profile cloud ls "" --deleted --json
bsx --profile cloud ls "" --source index --json
bsx --profile home export ./before-cloud.archive --format archive
bsx --profile cloud import ./before-cloud.archive --format archive --dry-run
bsx --profile cloud import ./before-cloud.archive --format archive
bsx --profile home sync --remote-profile cloud verify
bsx profile use cloud --verify-with local
```

归档导入会验证全部历史、blob、HEAD 与重建后的索引；目标必须为空或是同一归档的精确历史前缀。`sync verify` 与受验证切主关注当前状态，不能替代 archive 的完整历史验收。切主只在命令与人工读回验收均通过后执行；保留本地源库与迁移归档。更完整的语义和故障恢复见 [可移植工作流](portable-workflow.md)。

**3. CI（`.github/workflows/ci.yml`）**：push/PR 自动跑 Rust 门禁、Web build、两种运行时 E2E 和 Worker smoke。两个 Worker job 复用下述隔离 runner，自行生成一次性测试凭据，不依赖 checkout 中的真实 `.dev.vars` 或云凭据。

## E2E（screenplay pattern）

```bash
pnpm --dir tests/e2e exec playwright test
```

`tests/e2e/playwright.config.ts` 自动拉起独立服务实例（端口 18799、独立数据目录 `/tmp/brains-e2e-data`），需环境变量 `BRAINS_SERVER_BIN`（服务端二进制）与 `BRAINS_WEB_DIST`（前端产物）。本机注意：若 build-cache 守护进程会清理 `target/` 内二进制，把构建与测试放在同一条命令内执行。

## Cloudflare 本地验收（无需云凭据）

Worker 与 native 共享 axum HTTP 层，存储装配为本地 workerd 模拟的 R2/D1。预先构建 Web 与 Worker，再由 Python 3.11+ runner 负责迁移、认证就绪、测试和回收。该 runner 面向 macOS/Linux；CI 显式安装 Python 3.12。

```bash
rustup target add wasm32-unknown-unknown
mbx install worker-build --locked
pnpm --dir apps/web build
(cd apps/worker && RUSTFLAGS='--cfg getrandom_backend="wasm_js"' mbx exec --project-root ../.. worker-build --release)
python3 scripts/run-workerd.py -- bash tests/worker-smoke.sh
pnpm --dir tests/e2e exec playwright install chromium
tests/e2e/run-workerd.sh
```

每次运行生成随机 token 与签名 key，使用独立 config、cwd、日志、D1/R2 state 和两个动态端口。先应用 D1 migration，再以 Bearer 验证 capabilities、session 和索引查询，最后把 `BRAINS_BASE_URL`、`BRAINS_E2E_TOKEN` 传给子命令。结束时无论成功、非零退出或中断，都只回收自己启动的进程组和临时目录；不会按端口清理已有实例，也不改真实 `.dev.vars`。

`tests/worker-smoke.sh` 逐项断言既有业务：条件创建与更新、正文/ETag、CAS 竞争、字面/FTS/prefix 过滤、回收站恢复与完整历史链。指定已运行实例时必须显式提供 URL/token 环境变量；失败不会被打印日志或 `|| true` 掩盖。E2E 入口接受 Playwright 参数，例如 `tests/e2e/run-workerd.sh --grep 'trash'`。

Worker 始终设置 Secure Cookie。WebKit 浏览器验证需 HTTPS，可用 `python3 scripts/run-workerd.py --protocol https -- <验证命令>` 启动隔离入口；runner 仅对本机 readiness 接受 Wrangler 自签证书，验收客户端需要明确自己的本机证书策略，不能将证书校验全局关闭。

手动长期开发实例可继续用 `wrangler dev`，需要自己准备开发凭据、应用 D1 migration 并管理其进程。重建 `apps/web/dist` 后重启实例，因为资产清单在启动时快照。本地模拟通过不能替代真实 Cloudflare 上的权限、R2 条件写、D1 配额和延迟验收。

## 排障

- **搜索结果落后/缺失**：索引是有界投影，可能短暂落后于 HEAD（`complete=false` 或 503 Pending）。运行 `bsx reindex --all-pages` 重建；无需重启。
- **412 VersionConflict**：另一客户端已推进 HEAD。重新读取后基于新基线重做；Web 端冲突视图提供三栏比较。
- **token 丢失**：`--token` 重新注册即替换 root token；`secret.key` 删除会使所有 Web 会话失效（重登录即可），不影响数据。
- **误删文件**：删除是墓碑（tombstone），历史保留；`bsx restore` 或 Web 回收站恢复，恢复产生新修订（generation 只前进）。
