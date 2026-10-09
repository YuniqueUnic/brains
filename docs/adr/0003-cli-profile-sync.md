# ADR 0003：CLI profile 双向同步

状态：已采纳。

## 决策

- `bsx --profile local sync --remote-profile cloud` 在两个 runtime profile 之间双向同步当前文件状态和软删除。首次同步会把单边存在的文件导入另一端；同路径内容不同则报告冲突。
- 同步范围按当前文件内容和 tombstone 收敛，不复制 revision 历史或 SQLite/D1 索引。已有 `bsx export` 继续导出可读工作树；首次 sync 已承担导入端点缺失文件的职责，因此不再增加一套并行的目录 importer。
- 客户端复用共享 HTTP API、不可变 revision 读取和 CAS。同步进度只保存在 `BRAIN_CONFIG` 所在目录下按 profile endpoint pair 区分的本地 JSON 文件；它记录两端的 revision、状态和内容摘要，不保存正文、token 或索引。
- 合并基线优先通过记录的 immutable revision 从任一端读取，不复制内容快照。文本修改互不重叠时自动合并；重叠文本、二进制、删除/修改冲突都保留两端内容并等待显式解决。所有写入继续使用 revision 前置条件，不自动重试覆盖。
- 同步按路径独立完成，不承诺整个 vault 原子提交。永久 purge 只作用于执行端，不会被同步成对端删除；若保留 tombstone 的一端之后被用户明确恢复，新 live revision 可作为普通单边变更重新导入已 purge 端。若 purge 与对端修改同时发生，按冲突策略保留双方内容并等待明确解决。缺少基线或基线 revision 不可读时转为人工冲突。

## 复杂度自省

### A 路线五问

1. **当下真实问题：** 本地 profile 与云端 profile 都能独立写入同一 vault 内容，但 CLI 没有跨 profile 的同步命令；现有 export 只生成单端工作树，无法双向收敛。
2. **完全不做的后果：** 用户只能手动导出、逐文件复制并猜测哪端更新；双端编辑容易覆盖或遗漏，且无法从 Web 修改批量拉回。冲突是用户可感知的数据分叉，现有条件写保护仍可防止单次盲覆盖。
3. **更便宜的替代：** 逐次选择 push 或 pull 可免本地基线，但不支持日常双向同步且无法判别两端先后修改。复用 export manifest 也不够：它描述一个导出端点，不记录两个 profile 各自的同步 revision。采用按 profile pair 保存的轻量状态文件。
4. **自然降级：** 逐路径 CAS；并发变化或缺失 merge base 会报告冲突并保留原内容。任何一步失败后再次运行可以重新检查；跨 vault 同步是 per-file，不声称全 vault 原子性。永久 purge 没有 tombstone 可传播，因此不自动镜像。
5. **删掉机制的影响：** 删除 pair 状态文件后，单端 CLI、Web、export、历史和 CAS 都继续可用；已有分叉文件会被当作首次同步冲突，要求用户显式解决，不能悄悄覆盖。

| 机制 | 换来什么 | 不做会怎样 | 更简单的替代 | 决定 |
|---|---|---|---|---|
| 每 profile pair 的本地基线文件 | 区分一端修改、双端修改和软删除 | 没有共同基线时无法安全判断覆盖方向，可能丢失用户内容 | 每次强制选 push/pull；但无法完成日常双向同步 | 保留轻量 JSON，删除即可安全重新初始化 |
| 服务端 sync 表、后台任务、完整历史镜像 | 增量扫描或远端 Git 图 | 首次扫描较慢；不影响当前内容的安全性 | 复用分页列表、revision read 和现有 CAS | 不增加；实测扫描瓶颈出现后再评估 |
| 本地内容快照 | 即使旧 revision 不可读也能找到 merge base | 不能自动三方合并 | 从两端已有 immutable revision 读取；两端都读不到时人工解决 | 不存快照，避免复制正文和额外生命周期 |
| 文本三方合并 | 无重叠修改可自动收敛 | 用户必须手动比较所有双端修改 | 全部进入人工解决 | 采用成熟 `diffy::merge`；冲突不写入任一端 |

### 共论记录

| 争议点 | 换来什么 | 不做会怎样 | 替代方案 | 决定 |
|---|---|---|---|---|
| 使用单端 export manifest 或 pair state | 识别两端各自已同步的 revision | 仅凭单端基线会把另一端已发生的更新误判为新文件或覆盖 | 每次强制用户选 push/pull | 采用每对 profile 的最小 JSON 状态，不存正文 |
| 新增 server-side sync mechanism | 远端增量扫描或后台调度 | 首次扫描全量分页，规模大时较慢但内容安全不受影响 | 现有分页 listing + CAS | 不新增；实测扫描瓶颈后再评估 |
| trait 抽象 | 将同步决策与 HTTP 传输调用分离，可对真实 endpoint 复用同一引擎 | 算法直接耦合 BrainClient，不便替换 transport | 直接把算法写成 BrainClient 方法 | 采用窄 `SyncEndpoint` trait，只暴露列表、revision read、条件写入、trash 与 restore |
| 冲突策略 | 无重叠文本自动合并，其他分叉保留两侧 | 全部手工解决会增加重复工作 | 自动接受单侧或写入 diffy 冲突标记 | 用 diffy 只接受无冲突结果；重叠/二进制/删除修改进入显式解决，不写冲突标记 |

裁决：新增机制限定为 profile pair 的本地 revision 基线与纯客户端合并；同步引擎先与 HTTP client 通过窄 trait 分离。放弃完整 Git 图、服务端同步状态、内容快照和隐式冲突覆盖，因为现有分页、不可变 revision、CAS 已满足当前安全边界。

新增复杂度服务于当前明确的双向同步需求和用户资产保护。删除该机制只会失去跨 profile 同步，现有单端 CLI、HTTP API、历史与 export 仍可独立工作。

## 结构审查与影响面

同步只依赖通用文件列表、revision 内容读取、条件 PUT/DELETE，不需要改存储 adapter、domain port 或数据库 schema。同步编排位于 `crates/client`，由窄 `SyncEndpoint` trait 与 HTTP client 分开；CLI 负责解析两个 profile、同步状态路径与输出。FS/SQLite 和 R2/D1 继续由各自 runtime 通过相同 API 提供能力，不新增存储特判或存储层适配器。

CLI 按参数路由、文件、查询、系统命令划分模块。export manifest 描述单端导出结果，无法表达两个服务端各自的同步基线，因此同步使用按 profile pair 区分的状态文件。

## 依据

- 仓库：`README.md` 的 local/cloud runtime profile、`docs/runbook.md` 的 profile 与 export 工作流。
- 仓库：`crates/client/src/export.rs` 当前导出每路径 current revision 和 tombstone，声明 per-file consistency 与 `history_verified: false`。
- 仓库：`crates/client/src/lib.rs` 的 `list_files`、`revision_raw`、`put_content`、`delete`；写入支持 Create/IfRevision。
- 合并库：[`diffy::merge`](https://docs.rs/diffy/latest/diffy/fn.merge.html) 以 ancestor/ours/theirs 进行三方文本合并，非冲突返回合并文本，冲突返回带标记的文本；[`diffy` 版本与 MSRV](https://crates.io/crates/diffy) 当前为 0.5.2、MIT/Apache-2.0、Rust 1.85，本项目工具链为 1.96。
