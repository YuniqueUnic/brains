# ADR 0005：逻辑路径与本机文件名解耦

状态：提案。逻辑路径与本机文件名存在不同的等价规则，需要在存储适配器内分离。该提案不改变现有同步模型。

## 意图与可检验的成功标准

同一逻辑路径必须始终对应同一个权威对象与写锁，不因 macOS/Windows 文件名折叠而读取、修改其它路径。R2 接受的合法逻辑路径组合应能完整恢复到 native FS；普通工作树无法表示的路径必须明确失败，不能报告成功后丢失正文。保留当前权威 HEAD 可达的正文、revision、generation、tombstone 和来源记录；原始旧库另行完整备份，保留可能已失去 HEAD 引用的对象，不承诺恢复既有损坏。

验收必须覆盖：`Case.md`/`case.md` 各自保存正文，受限 token 无法跨路径读取；Unicode `σ`/`ς`、长 s、连字，以及 `a`/`a.json/b` 在 native 中独立读写；并发 CAS 不串路径；完整 archive 往返保留身份；工作树导出拒绝无法表示的名字且不留成功目录；只有 live 文件参与工作树碰撞检查；原有业务门禁继续通过。

## 修正清单与事实

| 原有假设/实现 | 证据与修正 |
|---|---|
| 小写转换足以判断文件名冲突 | 当前 FS 实测 `σ.md`/`ς.md`、`ſ.md`/`s.md`、`ﬀ.md`/`ff.md` 是物理别名，而 `to_lowercase()` 不等；改用完整 Unicode case folding，并用 create-only 文件写入兜住真实 FS 冲突 |
| `heads/<path>.json` 能表示所有合法逻辑路径 | `prefix` 创建 201，`prefix.json/child.md` 创建 503；物理文件/目录位置相同，重试不能消除；应改为不含逻辑文件名的内部键 |
| 对请求路径鉴权即可安全读取 FS HEAD | `read:case.md` token 访问 `Case.md` 为 403，通过 `case.md` 却 200 返回相同正文；读取后必须核验记录与请求身份，物理键也应与逻辑路径一一对应 |
| 导出后的目录与 manifest 一致 | workerd 中 4 个逻辑文件，CLI exit 0，实际只剩 3 个物理文件；其中 `leaf/ς.md` 正文被覆盖。归档格式使用哈希 blob 文件名，仍保留四条完整历史 |

发现的关联边界：HEAD 使用原始路径而 lock 使用路径 SHA-256，因此物理别名还会使用不同锁；blob 文件名包含原始逻辑 basename，在 Windows 保留名等场景也不能直接作为物理名。已删除 HEAD 只进入工作树 manifest，不应与 live 文件一起阻止目录导出。SQLite/D1 是投影，修改 FS 对象布局不要求改变业务 schema、HTTP 契约或归档格式。

## 建议方案与范围

native FS 使用新 format version：`heads/<sha256(logical-path)>.json`、`blobs/<sha256(content-key)>`、原有 `revisions/<revision>.json` 与 `locks/<sha256(logical-path)>.lock`。保留原始 logical path、content key 和 revision metadata；哈希仅作为适配器内部地址，不替代业务身份。读取验证请求路径与 HEAD.path 相等，CAS 写入前也验证 new_head.path 与请求身份一致；listing 从 HEAD 内容取得路径并核验哈希文件名，再按现有逻辑排序/分页。blob 仍验证 vault 和 key 结构后再映射，权限、CAS、校验和、索引与服务 ports 均保留。

正常运行只支持新 FS 布局，打开旧布局时给出明确升级错误，不双写、不混读两套布局、不静默重建或删库。缺少 marker 但已有对象的目录也必须拒绝初始化，不能误标为新格式。迁移复用现有完整历史 archive：用旧二进制读取旧库，导出 archive；新二进制在新目录用 `--no-seed` 启动，恢复并验证全部历史、正文与索引。升级停写窗口覆盖导出至切换。archive 不携带 auth.json 与 secret.key；获准迁移时，在新服务首次启动前私密复制该专属实例的这两个文件并保留 0600 权限，不读取或回显内容，随后核验原 profile 与会话衔接。旧目录和旧二进制保留作为回退依据，不引入迁移表、后台迁移任务或长期兼容层。

工作树修复复用锁文件中已有的 Unicode folding 库，不手写字符特例；普通文件使用 create-only 写入。Archive 不套用工作树文件名限制，继续使用它自己的可移植内部文件名，因此仍能保存工作树无法表示的逻辑路径。

改动限于 `crates/adapter-fs`、`crates/client` 导入导出、对应依赖声明、存储/升级说明和必要验收。继续沿用真实业务验证和已有门禁，不新增正式测试体系。R2 key 格式、D1/SQLite schema、HTTP/SDK 契约、同步语义、编辑器布局及云资源均不因此改动。

## 关键取舍与假设

这是已有耦合的结构整理：API 路径不能同时承担物理文件地址。只增加别名拒绝虽然能挡住一部分访问，却仍让合法 R2 数据无法恢复到 FS，因此不作为最终方案。新内部键会让 raw FS 备份不再等同于 R2 原始 key 布局；迁移使用已验证的 HTTP archive，raw backup 只用于同格式恢复。完整历史的既有确认继续有效，不把“允许重构”解释为可以删除旧数据。
