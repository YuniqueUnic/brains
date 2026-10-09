# ADR 0004：可移植工作流与编辑器边界整理

状态：已采纳。复用 [ADR 0003](0003-cli-profile-sync.md) 的双向同步与冲突策略，完整可达历史另用 archive 格式迁移。

## 意图与成功标准

让同一套 bsx 命令把可读目录导入任一 runtime，再在 FS+SQLite 与 R2+D1 之间同步当前内容；提供可校验的迁移流程，在显式切换日常 profile 前验证远端。编辑器预览、Markdown 插入、批量选择和移动端 Info 应按实际上下文工作。版本输出可辨认源码提交与未提交修改，补全无须连接服务即可生成。

## 设计边界

- HeadStore、RevisionStore、BlobStore 保存权威数据；SQLite/D1 索引是可重建投影。CLI 通过共享文件端点访问两种运行时，不直接管理 Cloudflare 资源。
- 工作树导入和同步会产生目标端新 revision，不能保留历史身份；完整历史迁移使用独立 archive 契约。
- 同步按路径保存成功进度，失败路径单独报告。切库前比较正文与目标索引；索引重建失败或内容不一致时不能切换配置。配置损坏应报错，保存使用原子写入。
- 预览内容与定位分开：CodeMirror 和 Base UI 各自管理浮层坐标，内容组件不再额外平移。移动端 Info 的目标文件独立于桌面面板的可见性。

## 实施方案和影响面

1. 客户端：使用共享文件端点 trait，export/import/sync 复用分页、不可变 revision 读取和条件写。普通目录导入默认只创建缺失文件、相同内容跳过、不同内容报冲突；显式 --overwrite 才对读取到的目标 revision 使用 CAS，绝不覆盖并发修改。支持 --dry-run，不将目录缺失或 manifest tombstone 解释为删除；不跟随符号链接，不导入 .brain 元数据为用户文件。补齐当前内容的 media type/executable 传递。
2. 同步：保留已确认的双向/无重叠文本合并/显式冲突策略；保存已成功路径的 checkpoint，失败路径单独报告且非零退出。提供独立只读 verify，对权威列表、正文 SHA、类型和 executable 核验，明确不比较两端不同的 revision 身份；单边回收站和 purge 语义按 ADR 0003 报告。
3. 切库：保留显式 profile use；增加 --verify-with 来源 profile，实际读取比较并重建目标索引、核验后才原子保存 active。校验失败不切换；不自动部署、不新增同步 daemon/服务端同步表，不复制索引。环境覆盖不能绕过双 profile 的明确选择。
4. CLI：clap_complete + clap_complete_nushell 生成 Bash/Zsh/Fish/PowerShell/Nushell 补全；命令名统一 bsx；构建版本显示 SemVer、Git short SHA、dirty，源码归档用明确 unknown 或受控构建元数据，记录 Cargo 重跑依赖。--profile cloud login 只修改指定 profile；配置损坏必须报错，保存使用原子写。
5. UI：预览内容 renderer 不做定位；CM/Base UI 各自仅有一个定位 owner；completion 使用 CM 原生 Completion.info，删除自建全局 observer 与旁挂 host，body 定位坐标保持未缩放，内容使用 UI zoom。共享 bulk selection bar，保留外层业务动作与唯一 selection owner；批量恢复汇总刷新/失败。Markdown toolbar toggle 移 header，条件卸载整行；按 reference/wrap/prefix/fence 动作计算 selection 和建议。移动按钮只隐藏文字保留 aria-label；Info 目标与桌面面板可见性分开，清除切换文件时旧的 properties 数据。

完整可达历史迁移使用独立 archive 格式与维护契约；当前状态同步保留原语义。不提供全库原子快照，不镜像已永久删除的对象，不自动强制覆盖，不复制索引。

## 取舍

| 争议点 | 换来什么 | 不做会怎样 | 替代方案 | 决定 |
| --- | --- | --- | --- | --- |
| 四后端平级抽象 | 表面接口统一 | 会误把索引当内容并丢历史 | 复用文件端点和权威存储 ports | 使用 runtime 组合，不新增四向索引转换框架 |
| 完整历史归档 | 保留源 revision 身份 | 当前状态同步本就不保历史 | 保留原始 vault backup，沿用已确认 current_state | 使用独立 archive 契约 |
| 目录 importer 与 sync 分叉 | 直接实现各自循环 | SHA/CAS/metadata 会重复漂移 | 共用窄文件端点与内容校验 | 收敛共享能力，流程各自清晰 |
| 每路径进度与失败报告 | 异常仍可知道已完成范围 | 成功写入与状态文件脱节 | 整库事务或服务端任务表 | 客户端路径级 checkpoint；不建新服务端状态 |
| preview 多处偏移修补 | 单场景看似可用 | 缩放/异步内容继续争夺坐标 | 唯一定位 owner + 纯内容 renderer | 重构定位边界，删除 inverse zoom 和 self clamp |
| mobile Info 触发 desktop open | 少量行数可显示内容 | 目标继续被可见性和空 editor 绑住 | 独立派生 selection context | 分开目标与布局，保持单一当前文件 owner |
| bulk/toolbar 逐按钮处理 | 局部追加快捷行为 | 选择条重复、插入游标不一致 | 共享选择 UI 和语义动作 | 共享组件，业务 mutation 保留外层 |

端点、定位和选择状态各有明确所有者。新增机制限定为导入、核验和路径级进度，不增加服务端同步表、内容快照或后台任务。

## 完整历史迁移

归档按每个 HEAD 的 parent 链保存所有可达 revision、这些 revision 引用的全部 blob、回收站及 metadata。revision ID、generation、actor、时间、operation ID、恢复/复制来源均保留；跨 vault 只替换 content.key 的准确 vault 前缀。provenance 指向的已 purge 来源不扩展可达闭包；被可达 revision 引用的旧 blob 必须保留。

普通导入只保留当前内容；原始 vault 备份不能直接在两种运行时之间恢复。archive 因此保留完整历史，中断后以目标端已发布的历史前缀继续核验和重放，不另存恢复日志。

| 争议点 | 换来什么 | 不做会怎样 | 替代方案 | 决定 |
| --- | --- | --- | --- | --- |
| 全链一次验证后发布最终 HEAD | 单文件恢复期间不暴露历史前缀 | 单请求成本随历史无界，Worker资源可能耗尽 | 每代仅检查 parent/current/blob 后 CAS | 逐代重放，验收前目标不作日常主库 |
| 独立不可变对象导入 | ID和blob key可保持不变 | 普通PUT改变身份和时间 | raw全库对象存储工具 | 窄 ArchiveEndpoint + 现有 Head/Revision/Blob ports |
| 新恢复journal/进度表 | 单独保存任务进度 | 会多一个生命周期和一致性源 | 目标HEAD就是可验证历史前缀 | 不新增表/日志，CLI先完整预检再顺序发布 |
| 目标已有不同历史 | 自动融合更方便 | 可能覆盖历史或产生虚假parent | 明确拒绝，并用空目标或精确同归档前缀 | 全目标预检，不覆盖或改ID；本地增加 --no-seed |
| FS revision直接写最终文件 | 当前代码简单 | 崩溃会留半JSON并永久collision | 复用现blob的staging+create-only hard_link+fsync | 整理发布边界，禁止盲覆盖损坏对象 |

读写归档端点要求admin，写请求保留既有cookie/CSRF校验。导入开始前检查所有目标路径是归档的精确历史前缀；恢复后逐revision/blob及最终HEAD实读核验，再完整reindex并比较投影。无全库原子性声明，无新后台任务。

## 操作与实现参考

- [可移植工作流](../portable-workflow.md)：普通目录、同步、完整历史迁移及安全切库。
- [CLI profile 双向同步](0003-cli-profile-sync.md)：基线与冲突策略。
- [运行手册](../runbook.md)：配置、备份、发布和部署。
- Shell 补全使用 [`clap_complete`](https://docs.rs/clap_complete/latest/clap_complete/) 与 [`clap_complete_nushell`](https://docs.rs/clap_complete_nushell/latest/clap_complete_nushell/)。
