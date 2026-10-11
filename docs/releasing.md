# 统一版本与发布

Brains 的产品版本统一管理，Rust、Web、Worker、SDK 和验收工具使用同一个版本，以 [`VERSION.txt`](../VERSION.txt) 为准。源码版本不代表发布或部署已完成。HTTP 协议的 `v1`、存储 schema 和第三方依赖版本各有独立含义，不随产品版本改写。

## 日常发布

采用与 xidl 相同的 release-please 单组件方案：`release-please-config.json` 中只有根组件 `.`，使用 `simple` 策略及结构化 `extra-files` 同步 Cargo workspace、全部本地锁定包、5 个 JavaScript package 和独立 npm lock。Rust 成员继续继承 workspace 版本。新增产品包时，需要同时加入 extra-files。

1. 使用 Conventional Commits 提交业务修改，例如 `fix(web): ...`、`feat(search): ...`。
2. `main` 更新后，Release Please 工作流创建或更新一个版本 PR，一次修改版本文件和 `CHANGELOG.md`。审核 PR 中的版本、日志和 CI。
3. 合并版本 PR 后，release-please 创建 `v<version>` tag 与 Release。它从默认分支启动 Release 工作流，工作流检出该 tag 的源码，构建原生、Web、Worker 产物。
4. Release 工作流校验产品版本及 tag 对应的提交，然后把带 SHA-256 的资产上传到该 Release；不再独立创建 tag 或覆盖发布说明。Release 页面出现不等于资产构建完成，以该工作流和资产清单为准。

工作流使用 GitHub 自动提供的 `GITHUB_TOKEN`，无需复制 xidl 的个人令牌配置。仓库需允许 Actions 创建 PR；由自动令牌创建的 PR，其工作流可能需要有写权限的成员批准运行。该令牌创建的 tag 不会自动触发 tag-push 工作流，因此这里使用 `workflow_dispatch`。

Release 已创建而构建或分发失败时，在仓库目录手动重跑资产工作流。`--ref main` 选择最新构建脚本，`version` 选择已有发布标签：

```bash
gh workflow run release.yml --ref main -f version=v0.3.0 -f publish_docker=false
```

构建脚本位于默认分支，产品源码始终来自输入标签，后续 job 使用首次解析出的同一提交。上传前会再次核对远端标签，标签发生变化就拒绝发布。修复构建脚本后可以重新构建已有版本，不需要移动标签。重跑 release-please 不保证再次分发资产构建。如果资产只上传了一部分，先核对已有文件，再手动上传缺失文件；同名资产不会被静默覆盖。

## 本地检查

```bash
python3 scripts/check-versions.py
mbx check --workspace --all-targets --all-features --locked
pnpm install --frozen-lockfile
```

`check-versions.py` 只校验，不负责改写版本。版本写入统一交给 release-please，避免两套版本更新器。pnpm lock v9 不存各工作区自身的版本，无需新增字段；不要对锁文件里的第三方版本做全局替换。

## 构建身份

`brains-build-info` 集中采集并格式化整个 workspace 的构建身份，CLI、原生服务器和 Worker 读取同一个版本常量。`bsx --version` 与 `brains-server --version` 在版本前加各自的程序名；实例的 `capabilities.build_version` 返回版本正文，例如 `0.3.0 (git 123456789abc-dirty)`。该字段由被查询的实例提供，`api_version` 仍单独表示协议版本。

默认规则：

- 包版本继承根 `Cargo.toml` 的 workspace 版本；Git 提交取最多前 12 位，转为小写。
- `-dirty` 只统计整个 workspace 已跟踪文件的暂存和未暂存修改，包括文档；不包含未跟踪文件，也不说明具体改了什么。
- workspace 根没有 `.git` 时，不借用上级目录的 Git 仓库。未提供覆盖值时显示 `git unknown`，且不加 `-dirty`。
- Cargo 监听已跟踪的 workspace 文件、存在的 Git index、HEAD、packed-refs、当前分支引用以及以下两个环境变量，以便常规编辑、提交和切换分支后刷新身份。不同目标或构建模式可能分别执行采集。

构建者可显式提供以下环境变量，源码归档和 Git checkout 都适用；两个变量分别覆盖自动检测结果：

| 变量 | 合法值 | 未提供时 |
| --- | --- | --- |
| `BRAINS_BUILD_GIT_SHA` | 7–64 位十六进制；显示时最多取前 12 位并转小写 | 从 workspace 自身的 Git 读取；没有可用值则为 `unknown` |
| `BRAINS_BUILD_GIT_DIRTY` | 严格为 `true` 或 `false` | 有 Git 时检查已跟踪文件，否则为 `false` |

非法值使构建失败。合法格式不表示已经核实它对应当前源码；例如显式的 `false` 会覆盖实际工作区的修改状态。发布方应从可信的源码来源提供元数据，不应把覆盖变量用于掩盖修改。

版本字符串不含构建时间。同一组选定元数据会生成相同文字，但不同修改也可能共享同一个 `-dirty` 版本字符串。缺少 `-dirty` 不等于源码未改动，版本相同也不等于二进制相同。若要验证下载文件，应检查发布资产的 SHA-256；二进制可复现还需固定工具链、依赖及其它构建输入，本文没有承诺已实现完整的可复现构建。

## 开发与发布构建

Rust 编译配置统一放在根 `Cargo.toml`。CLI、原生服务和 Worker 共享这份配置，CI 和发布工作流只选择构建模式，不再复制优化参数。

| 模式 | 命令 | 用途与取舍 |
| --- | --- | --- |
| 开发（`dev`） | `mbx build -p brains-cli -p brains-server` | 不优化机器码，保留增量编译、调试断言和整数溢出检查；`debug = 1` 保留源码位置，减少完整类型与变量调试数据。产物在 `target/debug/`。 |
| 发布（`release`） | `mbx build --locked --release -p brains-cli -p brains-server` | `opt-level = "s"` 面向体积优化，保留循环向量化；Thin LTO 与 `codegen-units = 1` 配合跨模块优化，`strip = "symbols"` 裁剪符号。产物在 `target/release/`。 |

发布构建保留默认全文检索功能，以及原生平台的 panic 展开行为；没有通过关闭全文检索或改成进程直接退出缩小体积。代价是优化编译耗时更长，且裁剪符号后，部分平台的崩溃堆栈和性能分析不能完整显示函数名。需要用调试器检查变量时，可临时构建完整开发调试信息；需要排查优化构建时，关闭符号裁剪并启用完整调试信息：

```bash
CARGO_PROFILE_DEV_DEBUG=2 mbx build -p brains-cli -p brains-server
CARGO_PROFILE_RELEASE_DEBUG=2 CARGO_PROFILE_RELEASE_STRIP=none \
  mbx build --locked --release -p brains-cli -p brains-server
```

`cargo test` 默认继承开发配置。CI 的原生端到端任务使用发布二进制验证功能组合、CLI 操作和浏览器流程；本地可以复用同一组业务用例：

```bash
BRAINS_CLI_BIN="$PWD/target/release/bsx" \
BRAINS_SERVER_BIN="$PWD/target/release/brains-server" \
  python3 tests/local-workflows.py
```

Worker 通过 `worker-build --release` 使用根发布配置，其中 `brains-worker` 的 `strip` 单独设为 `"debuginfo"`：生成 JavaScript 接口前，wasm-bindgen 还要读取 Wasm 的 `target_features`，不能提前裁剪全部符号与模块元数据（[上游问题](https://github.com/cloudflare/workers-rs/issues/1014)）。`apps/worker/Cargo.toml` 中的 `wasm-opt` 是另一项 Wasm 后处理开关，本次没有引入额外优化工具链。已发布标签和资产保持不变，新配置从后续源码构建与正式版本开始生效。

配置依据：[Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)、[Rust 编译配置与体积权衡](https://nnethercote.github.io/perf-book/build-configuration.html)、[符号裁剪的诊断影响](https://doc.rust-lang.org/rustc/codegen-options/index.html#strip)。

依据（2026-10-11 核对）：[xidl 的 release-please 配置](https://github.com/xidl/xidl/blob/master/release-please-config.json)、[release-please extra-files](https://github.com/googleapis/release-please/blob/main/docs/customizing.md#updating-arbitrary-files)、[GitHub 工作流触发规则](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)。


## Docker 分发与 CI 产物复用

新版本发布默认同时推送 `yunique001/brains`，需要仓库 Actions Secrets `DOCKERHUB_USERNAME`、`DOCKERHUB_TOKEN`。这是 Docker Hub 用户名与访问令牌，不是 GitHub App 私钥。旧标签缺少 Dockerfile 或容器配置能力时，显式使用 `publish_docker=false`，只重建该标签原有的原生/Web/Worker 资产；不拿新源码冒充旧版本。

Linux x64/ARM64 使用对应架构的 Ubuntu 24.04 runner 原生编译 musl 二进制。Web 与 Worker 编译并行，Worker 打包再取同次 Web 产物。Docker 直接取同次 Linux 和 Web 资产，在两个架构各运行一次持久化与停机验收，通过后推送带运行编号的构建镜像。GitHub 资产上传成功后才合并这两个镜像 digest，发布版本号及 `sha-<12位提交>` 标签。只有该标签仍是 GitHub 最新稳定 Release 时才更新 `latest`；预发布和旧版本重建不覆盖它。镜像最终标签发布使用仓库级并发组排队，避免两个版本竞争覆盖 `latest`。Docker Hub 无法与 GitHub 资产上传组成原子事务；镜像发布失败会明确使 workflow 失败，不能只凭 Release 页面认为镜像已可用。

开发 CI 保留完整 Rust 检查，并与代码生成、Web、原生 release 和 Worker 构建并行。每种原生功能/架构组合只编译一次，浏览器、SDK、CLI 和 Docker 验收下载同次产物；最终 `Development gates` 汇总全部结果。代码生成器使用包含工具链、依赖清单及安装脚本哈希的独立缓存，命中缓存仍做生成差异检查。Linux runner 固定 `ubuntu-24.04` / `ubuntu-24.04-arm`，不跟随 `ubuntu-latest` 的系统版本切换；固定标签仍会接收该系统镜像的维护更新。

部署命令、挂载、权限和 secret 配置见 [Docker 部署](docker.md)。

如果 GitHub 资产已上传、后续 Docker job 失败，使用 `gh run rerun <run-id> --failed` 重跑失败及受影响任务，复用已成功的构建和 GitHub 发布结果。在 artifact 保留期（7 天）内执行；不要重新 dispatch 整套资产上传，否则同名资产保护会阻止再次上传。修改了构建配方时，需先核对失败位置及已发布内容，再选择新 dispatch 或手动补交缺失资产，不能通过覆盖已有资产掩盖不同构建输入。
