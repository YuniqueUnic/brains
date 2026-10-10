# Brains 本地工作、迁移与主库切换

本文对应 2026-10-10 完成的实现。命令以 `bsx --help` 为准；设计边界见 [ADR 0004](adr/0004-portable-workflow-and-editor.md)，实测与原始结果见 [首轮验收](evidence/2026-10-09-brains-improvements/README.md)、[分页/跨浏览器/规模续验](evidence/2026-10-10-continuation/README.md) 和 [发布包验收](evidence/2026-10-10-release-readiness/README.md)。真实 Cloudflare 账户尚未部署验收，本文中的 `cloud` 是需要自行配置的目标名称。

## 存储和三种传输语义

FS/R2 保存权威 HEAD、revision 和 blob；Turso/D1 保存可重建的搜索、列表和链接投影。local 模式直接调用应用层，remote 模式通过两种服务端 runtime 的共同 HTTP 契约访问。迁移复用同一套端点接口，不复制数据库文件，也不把索引当作正文。现有服务端 store traits 保持分层；客户端的 `FileEndpoint` 供工作树导入导出、同步和核验复用，`ArchiveEndpoint` 只承载完整历史维护能力。

| 操作 | 保留内容 | 写入语义 | 合适场景 |
| --- | --- | --- | --- |
| `import/export --format working-tree`（默认） | 当前 live 文件的正文、类型、可执行标志；导出 manifest 另记 tombstone | 导入会产生目标端新 revision，不应用目录缺失或 tombstone 为删除 | Markdown 目录导入、人工编辑、正文搬运 |
| `sync` | 当前状态与逐文件共同基线；可传递删除为 tombstone | 双向 CAS，同一文件无重叠文本修改可合并，冲突须显式处理；不传播永久 purge | 两个 profile 持续协作 |
| `import/export --format archive` | 每个 HEAD 的完整可达 parent 链、对应 blob、回收站和 metadata | 保留历史身份，仅改准确的 vault key 前缀；目标必须为空或同归档的精确历史前缀 | FS＋Turso ↔ R2＋D1 的完整迁移与恢复 |

工作树 manifest 为 `.brain/export-manifest.json`，格式 `brains-working-tree` v2；完整历史 manifest 为 `brains-archive` v1，两者不能混用。工作树导入不接受旧版缺失 metadata 的 manifest；保留旧原始库，使用新版重新导出。无 manifest 的普通目录也可以导入，文件类型按现有检测规则判定，Unix 可执行位来自文件模式。

## 构建身份和补全

```bash
mbx install --path apps/cli --locked --force
command -v bsx
bsx --version
# 示例：bsx 0.1.0 (git 9203518f7339-dirty)
```

版本由包 SemVer、最多 12 位 Git SHA 和 `-dirty` 组成；dirty 只统计 tracked 文件的 staged/unstaged 修改，不包含 untracked 文件。Cargo 监听 workspace tracked 输入、Git index、HEAD、当前 ref 和存在的 packed-refs。无 `.git` 的源码归档明确输出 `git unknown`，不会借用上级目录的 Git 身份。归档发布方可提供 `BRAINS_BUILD_GIT_SHA=<7..64位十六进制>` 和 `BRAINS_BUILD_GIT_DIRTY=true|false`；非法值会使构建失败，不伪装成有效版本。版本号没有构建时间，因此同样元数据可复现。

补全在加载 profile 前生成，无需配置或联网。本次所指的 shell 是 **Nushell（nu）**。

| Shell | 生成文件 |
| --- | --- |
| Bash | `bsx completion bash > bsx.bash` |
| Zsh | `bsx completion zsh > _bsx` |
| Fish | `bsx completion fish > bsx.fish` |
| PowerShell | `bsx completion powershell > bsx.ps1` |
| Nushell | `bsx completion nushell > bsx.nu`（`nu` 也是别名） |

将生成文件加入自己维护的补全目录或 shell profile；本次安装没有修改全局 shell 配置。Bash/Fish 可以 source 相应脚本，Zsh 在 `compinit` 后加载 `_bsx`，PowerShell 用 `. ./bsx.ps1`，Nushell 在文件已经生成后执行 `source ./bsx.nu`。五类产物均经过真实 shell 解析；PowerShell/Nushell 还验证了 `bsx import` 补全条目。

## 配置本地和远端

没有服务器时，先创建一个离线库：

```bash
bsx init "$HOME/.brains/my-library" --profile local
bsx --profile local capabilities --json
bsx --profile local whoami --json

# cloud URL 和 token 来自实际部署，不是示例常量。
bsx profile add cloud --url "$CLOUD_URL"
printf '%s' "$CLOUD_TOKEN" | bsx --profile cloud login --stdin-token
bsx --profile cloud whoami --json
```

local 直接读写数据目录，不需要 token。remote 使用 URL 和 token，服务器也可以在 `127.0.0.1`。同目录服务器接管、多个服务器以及新配置的 `mode` 字段见[本地与远端模式](local-remote.md)。

默认配置为 `~/.brains/config.json`，可用 `BRAIN_CONFIG` 指向独立文件。双 profile 同步和受验证切库要求使用已存储的命名 profile，执行前取消 `BRAINS_URL` / `BRAINS_TOKEN` 环境覆盖。配置不存在才按空配置处理，坏 JSON 和权限错误会明确失败；配置与 sync 状态使用 0600 临时文件、fsync 和原子 rename，避免截断上一次有效记录。`--profile cloud login` 只更新 cloud 的 token。

## 从可读目录开始

```bash
bsx --profile local import ./notes --dry-run
bsx --profile local import ./notes
bsx --profile local export ./notes-export
```

导入默认 create-only：缺失文件创建、相同正文和 metadata 跳过、不同内容列为冲突并非零退出。已核对目标内容、确需更新时再使用 `import ./notes --overwrite`；它仍以读取到的目标 revision 作为 CAS 条件，并发变化不会被静默覆盖。导入拒绝符号链接、特殊文件、重复或不具可移植性的路径；`.brain/` 元数据不会变为用户文件。它不会因目录少了文件而删除库里的文件。

导出要求输出目录尚不存在；先列出权威 HEAD，再按不可变 revision 下载并检查身份、SHA-256 和大小，经 staging 成功后发布目录。工作树导出提供逐文件一致性，不承诺全库同一时刻的快照。仅 live 文件参与物理路径检查；tombstone 保留在 manifest，不阻止同名目录下的 live 文件导出。导出使用完整 Unicode case folding 检查冲突，并由实际文件系统的 create-only 文件和精确拼写目录创建阻止覆盖或别名合并；失败不发布输出目录。需要保留这些合法但不可作为普通工作树表示的路径时，使用 archive 格式。Unix 导出恢复可执行位；其它平台保留 manifest 中的标志，平台文件模式行为仍需实机验证。

## 日常同步与冲突

```bash
bsx --profile local sync --remote-profile cloud --dry-run
bsx --profile local sync --remote-profile cloud
bsx --profile local sync --remote-profile cloud verify

# local/remote 相对于当前 --profile，而不是固定地理位置。
bsx --profile local sync --remote-profile cloud resolve notes/topic.md --keep local
bsx --profile local sync --remote-profile cloud resolve notes/topic.md --merged-file ./resolved.md
```

共同基线位于配置文件旁的 `sync/<profile-pair-hash>.json`。每个处理成功的路径立即保存 checkpoint。后续网络/CAS 失败会报告出错路径与已完成计数，保留此前检查点，并非零退出；出错路径可能只写成了一部分，修复原因后重跑。若远端写入已完成但本地 checkpoint 保存失败，报告明确指出“文件可能已提交、该路径 checkpoint 未保存”，不能把它当成全部成功或据此切换主库。

`sync verify` 不读取 sync 基线，也不写远端；它实读两端 live 正文和 metadata 并在读取后重查 HEAD。两边新生成的 revision 身份可以不同，双方均非 live 时 absent 和 tombstone 可以等价。因此 verify 通过只能证明当前 live 内容一致，不能证明历史相同。发生并发写入时停止写入后重新核验。

## 完整可达历史迁移

准备原始库备份，并在最终导出、迁移与切换窗口停止日常写入。本地全新目标使用 `--no-seed`，避免默认模板成为归档之外的目标路径；已有真实数据不能为迁移直接删除。

```bash
# 仅为新建目标的启动示例，使用独立数据目录和端口。
BRAINS_TOKEN="$TARGET_TOKEN" ./target/debug/brains-server \
  --data-dir ./brain-target --vault migrated --port 45801 --no-seed

# 完整历史在目录中传输；输出目录必须是新的。
bsx --profile local export ./full-archive --format archive
bsx --profile cloud import ./full-archive --format archive --dry-run
bsx --profile cloud import ./full-archive --format archive
```

`manifest.json` 保存完整 HEAD 和每路径从旧到新的 revision；`blobs/<sha256(original_content_key)>` 保存原始字节。归档保留 revision ID、generation、actor、operation ID、时间以及 restore/copy provenance。`restores_revision` / `copied_from` 是出处记录，不扩展 parent 链的可达闭包；已 purge 的来源不凭空恢复，不可达垃圾、用户 token、会话密钥与数据库索引不进入归档。

导入先完整校验离线 archive，再检查目标附件上限和全部目标路径。目标不能包含归档外路径或不同历史；同一路径已有 HEAD 必须是严格相同的历史前缀，已存在不可变对象也必须相同。dry-run 执行相同预检，不写入。archive 不允许 `--overwrite`。

恢复按每一代的 parent 与 generation 使用 CAS 发布，目标 HEAD 本身就是续传位置，不引入恢复 journal 或全局事务。中断时目标会保留已经发布的历史前缀，CLI 非零退出并报告具体路径/revision和完成计数；保持目标不用于日常写入，重跑同一个 archive。最后逐 revision/blob/HEAD 实读核对、分页 reindex 并比较投影，成功输出 `history_verified: true` 和 `index_verified: true`。两项都通过前不把该目标当成已验收主库。

归档维护契约集中在 [api/brains.idl](../api/brains.idl) 与 [OpenAPI](../sdk/openapi/brains-openapi.json)，请求以 `/api/v1` 为前缀：读取/导入 `/maintenance/archive/revision`、上传 `/maintenance/archive/blob`、发布 `/maintenance/archive/head`。这些接口需要 admin；cookie 写请求保留 CSRF 与 Origin 校验。使用统一 HTTP 端点可在两个 runtime 间双向迁移，CLI 无需 Cloudflare 账户管理权限。

## 验证后显式切换日常主库

```bash
bsx --profile local sync --remote-profile cloud verify
bsx profile use cloud --verify-with local
bsx profile list
```

受验证切换执行“实读当前内容比较 → 目标完整 reindex → 索引与权威对象对照 → 再次内容比较 → 原子保存 active”。任何一步失败都不切换；reindex 有 failed 项也不能返回成功。切换的是 CLI 的日常 profile，不会自动改变部署、关闭旧库或修改其它客户端配置。旧本地库与原始归档保留到用户完成真实环境验收。

## 已验证与待实测

代码可证明：local 与 remote 共用应用规则、FileEndpoint 和 ArchiveEndpoint，索引可重建；普通传输与历史归档的语义不同；写入保留 CAS、不可变对象和权限边界。业务实测：native FS＋Turso 与本地 workerd R2＋D1 往返、完整历史及 metadata、两种 SDK 的原始字节/null 往返、故障续传、冲突解决和受验证切库均已通过，具体输入和结果见验收记录。

续验已覆盖 261 个路径/504 个 revision、130 代单路径历史、分页重建与并发 CAS；native Firefox/WebKit 及 Worker Chromium/HTTPS WebKit 的完整浏览器用例也已通过。这些是明确规模和本地 runtime 下的结果，不代表生产容量或系统 Safari/iOS 实机验收。

必须实测：真实 Cloudflare 的 R2/D1 延迟、权限和配额、生产规模负载与网络中断、Windows 文件模式、系统 Safari 原生交互及 iOS Safari。系统 Safari 26.6.2 已加载页面并执行 JS，但原生输入命令超时；现有自动化会话待释放后重验，不能以 Playwright WebKit 结果替代。当前实现没有全库原子快照、在线多写者无缝切库或同步 daemon；归档维护请求的用户输入仍受现有 body/attachment 限制约束。不要把本地 workerd 的结果写成真实云部署成功。
