# 原生 Turso 索引与可选全文检索

原生 bsx 和 Rust server 使用 Turso 0.8.3 派生索引；Cloudflare Worker 继续使用 D1 的 SQLite。正文、文件当前版本和历史仍保存在原来的 FS/R2 中，导入、导出与同步不搬运数据库文件。数据库不是第二份正文来源。

## 按构建选择

```bash
# 默认包含 Tantivy 全文检索
mbx build -p brains-cli -p brains-server

# 不编译 Tantivy；仍可进行文件管理、字面搜索、标签、链接、历史、归档与同步
mbx build -p brains-cli -p brains-server --no-default-features

# 显式开启
mbx build -p brains-cli -p brains-server --no-default-features --features fts
```

`fts` 从 CLI/server 经 client/local 传到 native adapter，再传给 `turso/fts`；它实际裁剪 Tantivy 依赖。不要用 workspace `--all-features` 来验证关闭能力，该命令会合并并开启所有成员功能。开发门禁仍需要跑全功能构建。

`bsx capabilities` 和服务端 capabilities 的 `fts_search` 反映本实例能力，`fts_tantivy` 区分原生 Tantivy 方言。不带 FTS 时，显式请求 `--mode fts` 返回 CAPABILITY_UNSUPPORTED，HTTP 为 501；字面搜索保持可用，不悄悄把全文请求改为字面搜索。

本地目录始终有 OS 排他锁：一个 server 持有时，bsx 应用 remote profile 访问它。更换数据库引擎不会放开 HEAD/revision/blob 的并发边界。

## 搜索行为

`--mode literal` 按原文做字面查找，保留原来的有界扫描和游标语义。`--mode fts` 是关键词全文检索，不是语义检索。

- 原生使用 Tantivy 查询语法与 BM25 排名；相同分数按路径排序。返回最多 240 个字符的正文开头预览，不返回未经限制的全文高亮。
- D1 继续使用 SQLite FTS5 查询语法与 snippet。不要假设两种引擎的高级查询、分词和排序相同；没有 FTS5→Tantivy 字符串兼容层。
- 两种引擎都保留权限、路径范围、标签、删除状态、可检索文件类型及分页过滤。`--prefix public` 与 `public/` 均表示路径 `public` 及其子目录，不包括 `public-secret.md` 或 `public2/`；权限使用相同边界，Unicode 路径同样适用。
- `fts` 只控制运行该二进制的原生索引；remote bsx 能请求什么由目标 server 的能力决定，不由 CLI 自己是否编译了 Tantivy 决定。

例如 `bsx search 'rust AND cache' --mode fts --prefix notes/`；中文原文子串优先使用默认 literal。默认 Tantivy tokenizer 不是中文语义模型。

## 索引重建与旧数据

新的派生索引放在各 vault 目录，以 `index.turso-fts.db` / `index.turso-basic.db` 区分 schema 能力。旧 SQLite `index.db` 保留但不再打开；首次使用从所有当前 HEAD 完整重建。切换带/不带 FTS 的程序也重建，成功后只保留当前 Turso 变体，避免另一变体落后于正文。

重建先写暂存数据库，检查每条结果，再显式 checkpoint、释放连接和数据库 owners、清理旧派生 sidecars，最后发布。任一步失败均报告错误；不可变 revision/blob 和 HEAD 不因此修改。`reindex --all-pages` 仍可修复既有投影。

## 其余 Turso 能力为何没有空开关

| 能力 | 已验证范围 | brains 当前边界 |
|---|---|---|
| vector | 精确余弦距离计算 | 没有嵌入模型和向量生命周期，不能称为 semantic search；近似向量索引仍属上游路线图 |
| 加密 | 测试库创建、正确密钥重开、错钥拒绝 | 只加密索引保护不到正文、历史和归档；需先确定完整库加密与密钥管理方案 |
| `.tshm` | 子进程写入被已打开的父进程连接读到 | 仅协调数据库 WAL；现有整个目录只有一个所有者，没有开启它的实际消费者 |
| DBSP | 写入后物化 count 自动更新 | 尚无增量视图或查询订阅的产品消费者，不新增常驻状态或空 feature |

这些能力的部分开关属于运行时 Builder 设置，不是同名 Cargo feature。`semantic` 是正确拼写；它需要明确模型身份、维度、分块、正文 revision、过期向量处理和查询权限。模型下载与缓存应由统一资源管理边界负责，不能在搜索模块另建一套下载逻辑。

官方依据：[0.8.3 Rust features](https://github.com/tursodatabase/turso/blob/v0.8.3/bindings/rust/Cargo.toml)、[Builder API](https://github.com/tursodatabase/turso/blob/v0.8.3/bindings/rust/src/lib.rs)、[FTS兼容范围](https://github.com/tursodatabase/turso/blob/v0.8.3/COMPAT.md#full-text-search-fts)、[功能与路线图](https://github.com/tursodatabase/turso/blob/v0.8.3/README.md#features-and-roadmap)。Turso这些扩展仍含实验能力，独立探针成功不能替代完整库业务验收。
