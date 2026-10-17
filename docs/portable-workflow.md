# Brains 本地工作、迁移与主库切换

本文说明怎样用 `bsx` 管理离线库、同步两端内容，以及把完整历史迁到新库。profile 是有名字的连接配置；示例中的 `local`、`cloud` 和 `migration-target` 都是配置名，不代表固定的部署位置。命令示例使用 Bash/Zsh，参数以当前版本的 `bsx --help` 为准。

## 先选择要搬哪些内容

| 需求 | 使用方式 | 保留内容与限制 |
| --- | --- | --- |
| 导入 Markdown 目录，或导出后用普通编辑器处理 | `import` / `export`，默认 `--format working-tree` | 搬运当前未删除文件、内容类型和可执行标志。导入产生目标库自己的修订版本，不复制历史，也不会因目录缺少文件而删除库中内容。 |
| 在两个库之间持续协作 | `sync` | 双向同步当前内容和移入回收站的状态；不复制每一代历史，也不传播永久清除。 |
| 迁移或备份完整历史 | `import` / `export --format archive` | 保留当前各文件及其历史链、回收站记录和版本元数据。目标必须为空，或只包含同一归档中各文件历史的准确前半段，便于中断后续传。 |

**需要完整历史时，先向新目标导入归档，再开始日常同步。** 不要先用工作树导入或 `sync` 填充目标，再向它导入历史归档；这些操作可能已经产生不同历史，归档导入会拒绝继续。

正文和历史是原始数据，搜索、列表和链接索引可以重建。迁移使用上述命令，不靠复制索引数据库完成。更多设计说明见 [ADR 0004](adr/0004-portable-workflow-and-editor.md)。

## 安装与查看版本

从 [Releases](https://github.com/YuniqueUnic/brains/releases) 下载适合系统与处理器的原生包，按[原生包说明](native-release.md)核对校验和、解压，并将程序所在目录加入 `PATH`。开发者也可以在源码仓库根目录安装 CLI 和服务器：

```bash
mbx install --path apps/cli --locked --force
mbx install --path apps/server --locked --force
bsx --version
brains-server --version
```

两者使用相同的版本格式，例如 `bsx 0.6.0 (git 123456789abc)`、`brains-server 0.6.0 (git 123456789abc)`。`-dirty` 表示构建时检测到已跟踪文件的修改；未跟踪文件不计入。`git unknown` 表示构建没有提供可用的提交标识。

Worker 没有命令行入口。连接服务器或 Worker 后，用 `bsx --profile cloud capabilities --json` 查看 `build_version`，得到的是服务端的构建版本，不是当前 CLI 的版本。`api_version` 则是协议版本，含义不同。

版本文字是排查问题的线索，不能单独证明源码相同、源码未修改或二进制可复现；构建者也可以显式提供这些元数据。具体规则见[构建身份说明](releasing.md#构建身份)。CLI、服务器与 Worker 应配套升级：旧版服务端不返回新版 CLI 所需的 `build_version` 字段，重新构建旧标签也不会补上。

## 创建离线库并导入文件

开始前取消可能遗留的远端环境覆盖；本地命令不接受这两个变量：

```bash
unset BRAINS_URL BRAINS_TOKEN
bsx init "$HOME/.brains/my-library" --profile local
bsx --profile local capabilities --json
bsx --profile local whoami --json
```

`init` 创建空库，并把该 profile 设为默认。已有库应使用 `bsx profile add local --data-dir /absolute/path/to/library --vault default` 登记；这与 `init` 是两种选择，不要对同一个名字重复添加。库的实际 vault 名称不是 `default` 时应相应调整。更多连接与目录锁规则见[本地与远端模式](local-remote.md)。

假设已有普通目录 `./notes`，其中包含 `topic.md`：

```bash
bsx --profile local import ./notes --dry-run
bsx --profile local import ./notes
bsx --profile local export ./notes-export
```

库中路径是 `topic.md`，不会自动加上 `notes/` 前缀。普通文件目录与 Brains 数据目录用途不同，不要把 `my-library` 当作笔记目录导入。

导入默认只创建缺失文件：内容和元数据相同则跳过，不同则报告冲突并以非零状态退出。确认要更新已有文件后，才使用 `import ./notes --overwrite`；若该文件在读取后又被修改，写入仍会被拒绝。`--overwrite` 不会恢复回收站中的路径，这类路径仍报告冲突，需要先单独处理回收站记录。

导入可能部分成功。检查报告中的 `conflicts`、`failures`、`index_pending` 及退出码，处理问题后再继续。目录缺项、导出清单中的删除记录都不会触发工作树导入删除。符号链接、特殊文件和不适合跨平台使用的路径会被拒绝；`.brain/` 中的导出清单不会变成用户文件。

导出目录必须尚不存在。导出成功后才发布完整目录；它逐文件校验读取内容，但不保证全库在同一时刻的快照。回收站记录只写入清单。若库中路径在目标文件系统上无法同时表示，例如大小写冲突，工作树导出会失败；需要保留这些路径时使用历史归档。

## 登记远端

以下示例假设已经部署服务器，并取得其地址和访问 token。先把真实值放入 `CLOUD_URL`、`CLOUD_TOKEN` 变量，再执行：

```bash
bsx profile add cloud --url "$CLOUD_URL"
printf '%s' "$CLOUD_TOKEN" | bsx --profile cloud login --stdin-token
bsx --profile cloud whoami --json
bsx --profile cloud capabilities --json
```

`login` 只保存 token，后两条命令才会访问服务器。确认地址、身份、权限和目标库都正确后再写入；完整历史迁移及重建索引需要管理员权限。remote 配置既可以指向公网，也可以指向本机服务器。

配置默认保存在 `~/.brains/config.json`，可用 `BRAIN_CONFIG` 指定独立配置文件。配置含 token，不要公开。双端同步及受验证切换使用已保存的命名配置，同样要求取消 `BRAINS_URL` / `BRAINS_TOKEN` 覆盖。

## 路线一：日常同步当前内容

只需当前内容时可以直接执行本节；需要完整历史时，先完成下一节的归档迁移。

```bash
bsx --profile local sync --remote-profile cloud --dry-run
bsx --profile local sync --remote-profile cloud
bsx --profile local sync --remote-profile cloud verify
```

不重叠的文本修改可以自动合并。发生冲突时，先查看双方内容，再按报告中的实际路径选择一种处理方式。例如保留当前 `--profile local` 一端的 `topic.md`：

```bash
bsx --profile local sync --remote-profile cloud resolve topic.md --keep local
```

或者在外部编辑器合并为 `./resolved.md`，然后使用合并文件；这是上一条命令的替代方案：

```bash
bsx --profile local sync --remote-profile cloud resolve topic.md --merged-file ./resolved.md
```

`--keep remote` 指 `--remote-profile cloud` 一端，与服务器是否在本机无关。`resolve` 只适用于仍存在冲突的路径。处理完成后重新执行 `sync` 和 `sync verify`。

同步会在配置文件旁保存进度。失败时此前完成的路径可能已经写入；检查报告、修复原因后重跑，不要把非零退出当作全部回滚。若文件已提交但进度保存失败，报告会明确指出，也不能据此切换主库。

`sync verify` 比较两端当前未删除文件的正文、内容类型和可执行标志，并检查核验期间文件版本是否变化。它不比较完整历史；两端都没有当前文件时，“不存在”和“已移入回收站”也可视为一致。它不会同步正文或更新同步进度。

## 路线二：把完整历史迁到新目标

先备份原始库，在最终导出、导入和切换期间停止两端日常写入。以下使用独立目录和新配置 `migration-target`，不复用已进行日常同步的 `cloud`。不要删除已有真实数据来凑出空目标。

**终端 A：** 确认已安装 `brains-server`，在新的数据目录启动服务，并保持运行。`--no-seed` 避免生成示例笔记；首次启动输出的访问 token 需妥善保存。

```bash
unset BRAINS_URL BRAINS_TOKEN
brains-server --data-dir "$HOME/.brains/migration-target" \
  --vault migrated --bind 127.0.0.1 --port 45801 --no-seed
```

**终端 B：** 将刚才的 token 保存到自己可读的私有文件，下面用 `/absolute/private/migration-token.txt` 代指该文件。登记的地址必须对应终端 A 的服务：

```bash
unset BRAINS_URL BRAINS_TOKEN
bsx profile add migration-target --url http://127.0.0.1:45801
bsx --profile migration-target login --stdin-token < /absolute/private/migration-token.txt
bsx --profile migration-target whoami --json
bsx --profile migration-target capabilities --json

# full-archive 必须是尚不存在的新目录。
bsx --profile local export ./full-archive --format archive
bsx --profile migration-target import ./full-archive --format archive --dry-run
bsx --profile migration-target import ./full-archive --format archive
```

先检查每条命令的结果再执行下一条。远端 Worker 也可作为目标，但须先按其部署文档准备空库、地址和管理员凭据，再用该地址登记目标配置。

归档保留每个当前文件记录及其历史链，包括版本标识、操作人、操作时间、回收站状态和恢复／复制的来源记录。来源记录不会使已永久清除的文件重新出现。不可达的废弃对象、用户 token、会话密钥和数据库索引不在归档中；备份部署配置需另行安排。

导入会校验归档、目标附件大小限制及所有目标路径。目标不能有归档外的路径或不同历史，`--overwrite` 也不适用于归档。如果中断，目标可能已恢复了部分历史：保持目标不用于日常写入，重跑**同一份归档**即可继续。

成功报告必须包含 `history_verified: true` 和 `index_verified: true`：前者表示已读回核对历史对象与当前文件记录；后者表示已重建目标索引，并核对索引列表的路径、版本、代数及删除状态。后者不等于逐项验收标签、链接或全文搜索结果。失败时不要把目标当作已完成迁移的主库。

各类 `--dry-run` 不提交正文或历史变更；不过它们与 `sync verify` 都可能需要打开本地库。索引缺失或构建的全文检索能力变化时，打开库会重建派生索引，因此这些命令不保证对整个文件系统零写入。

## 验收后切换默认库

保持两端停止日常写入，确认迁移结果与目标身份后执行：

```bash
bsx profile use migration-target --verify-with local
bsx profile list
bsx whoami --json
bsx capabilities --json
```

切换前依次比较当前内容、重建目标索引、核对索引列表，再比较一次当前内容。全部通过后才保存默认配置；失败不会切换。这个检查不能替代归档导入的完整历史核验。

切换只影响当前配置文件中的 CLI 默认库。显式 `--profile local` 仍访问旧库，其它客户端和服务器配置不会自动改变。保留旧库与原始归档，直到实际使用验收完成。如果要继续两端协作，用 `migration-target` 作为前述同步命令的另一端。

## 可选：命令补全

生成补全无需配置或联网。选择当前 shell 对应的一行，生成后按该 shell 的补全机制加载：

| Shell | 生成命令 |
| --- | --- |
| Bash | `bsx completion bash > bsx.bash` |
| Zsh | `bsx completion zsh > _bsx` |
| Fish | `bsx completion fish > bsx.fish` |
| PowerShell | `bsx completion powershell > bsx.ps1` |
| Nushell | `bsx completion nushell > bsx.nu`（`nu` 也是别名） |

Bash/Fish 可以 `source` 相应文件；Zsh 将 `_bsx` 放入 `$fpath` 中的目录后运行 `compinit`；PowerShell 使用 `. ./bsx.ps1`；Nushell 在文件生成后使用 `source ./bsx.nu`。这些命令不会自动修改 shell 启动配置。

## 能力边界与验证方式

当前没有全库原子快照、在线多写者无缝切库或后台同步服务。归档和请求仍受目标的正文、附件大小及部署限制约束。上线前需验证实际服务的权限、容量、网络中断与平台文件行为；本地 workerd 验收不能代替真实 Cloudflare 部署验收，浏览器 WebKit 自动化也不能代替系统 Safari 或 iOS 实机验收。

可重复执行的业务用例见[本地工作流](../tests/local-workflows.py)、[归档与同步](../tests/cli-workflows.py)和 [Worker 冒烟检查](../tests/worker-smoke.sh)。当前提交的执行结果以 [CI](https://github.com/YuniqueUnic/brains/actions/workflows/ci.yml) 为准；构建与本地验收入口见[开发与发布构建](releasing.md#开发与发布构建)。
