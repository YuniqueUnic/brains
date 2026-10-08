//! First-boot template (design §19, cold-start experience): every fresh
//! vault wakes up with guide content instead of a blank screen. The notes
//! double as living documentation — including the conventions an AI agent
//! needs to operate the vault safely (§15.2 scopes, §8.2 CAS etiquette).

use brainstore_domain::{HexId, Principal, Scope, VaultPath};

use crate::commit::CommitInput;
use crate::{Precondition, Vault};

/// The template: `(vault-relative path, markdown body)`. Keep every note well
/// under the text limit; content ships inside the binary, no network needed.
pub fn template_notes() -> &'static [(&'static str, &'static str)] {
    &[
        (
            "README.md",
            r##"---
title: BrainStore
tags: [meta]
---

# BrainStore

**可读、可版本化、可恢复的知识库 —— 纯文件进、纯文件出，每个修订都被保留。**

这是一个刚初始化的空库，里面已经放好了几篇入门文档：

- [docs/brainstore-guide.md](docs/brainstore-guide.md) —— 日常使用规则（路径、保存/冲突、回收站、搜索、备份）
- [docs/ai-guide.md](docs/ai-guide.md) —— AI 协作守则（给 AI agent 的操作约定）
- [inbox/README.md](inbox/README.md) —— 收件箱用法

## 三分钟上手

```bash
# Web 界面：直接用浏览器打开本服务地址，粘贴 token 登录
# CLI：
brainstore-cli ls ""
echo "# 第一条笔记" | brainstore-cli put notes/first.md
brainstore-cli search 缓存
```

## 备份

磁盘上的目录就是备份本身：`vaults/{vault}/heads|revisions|blobs`。
复制这个目录即得完整原始备份；索引（SQLite/D1）只是可随时删除重建的投影（`brainstore-cli reindex`）。
"##,
        ),
        (
            "docs/brainstore-guide.md",
            r##"---
title: BrainStore 使用规则
tags: [meta, guide]
---

# BrainStore 使用规则

## 路径与文件

- 路径是 vault 内相对路径，如 `notes/cache.md`、`inbox/idea.md`；按主题分目录，文件名即主题。
- 文本文件（markdown 等）在网页端直接编辑；二进制用 CLI `put`/`get`（上传走顶栏「上传」按钮，落到 `attachments/`）。
- 在正文里用 `[[wiki-link]]` 互链，信息面板会统计出链与反链。

## 保存与冲突（CAS）

- 每次保存都基于某个基线修订；两个人同时基于同一版本修改，**恰好一人成功，另一人收到 412 版本冲突**。
- 冲突不会自动覆盖：冲突工作台提供分栏 / 内联 diff，由你决定「以本地保存」或「采用远端」。
- 网页端 Ctrl/Cmd+S 保存；工具条上的 diff 按钮可以随时查看未保存的改动。

## 回收站与历史

- 删除是**墓碑**（tombstone）：文件进回收站，历史完整保留，随时恢复；恢复产生**新**修订（generation 只前进）。
- 侧栏「回收站」切换批量恢复；信息面板可从历史链恢复任意有内容的版本。

## 搜索

- **字面**模式：大小写敏感的 UTF-8 子串匹配，永不漏报（结果标注是否翻完全库）。
- **FTS** 模式：英文按词元、中文按字的显式分词搜索，速度快但依赖索引。
- 索引只是投影：删掉索引文件后 `brainstore-cli reindex` 即可完整重建。

## 备份与恢复

- 目录即备份：`vaults/{vault}/heads|revisions|blobs` 三个目录复制走就是完整原始备份。
- 恢复：把目录放回去（或新机器同位置），启动即用；索引按需 reindex。
"##,
        ),
        (
            "docs/ai-guide.md",
            r##"---
title: AI 协作守则
tags: [meta, ai, guide]
---

# AI 协作守则（给 AI agent 的操作约定）

如果你是 AI agent（Claude / Codex / 自研 agent），本库就是你的长期记忆层。
以下约定让人类与 AI 可以安全共写。

## 读写礼仪

1. **先搜索再写入**：写新笔记前先 `GET /api/v1/search?q=关键词`，避免重复主题。
2. **一篇文章一个主题**：文件名即主题（如 `notes/cas-conflicts.md`），过长就拆分并用 `[[链接]]` 互链。
3. **条件写入**：更新已有内容必须带 `If-Match: "<revision>"`；新建用 `If-None-Match: *`。收到 `412` 就重新读取、合并、再试，**绝不覆盖他人修改**。
4. **幂等重试**：写操作带 `X-Operation-ID`（随机 32 hex），同一操作重试用同一个 id，服务端去重。
5. **不碰历史**：修订与墓碑是不可变事实；"撤销" = 用 restore 产生新修订，而非改写。
6. **声明式 frontmatter**：每篇笔记开头写 `title` 与 `tags`，索引与搜索依赖它们。
7. **留痕**：批量操作前在横幅/回复中列出将影响的路径清单；失败如实上报。

## 最小 API 速查

```bash
TOKEN="..."   # Bearer token，或先 POST /api/v1/session 换 cookie
B="http://127.0.0.1:8787/api/v1"

# 读
curl -s -H "Authorization: Bearer $TOKEN" "$B/file?path=docs/ai-guide.md"
curl -s -H "Authorization: Bearer $TOKEN" "$B/search?q=CAS&mode=fts"
# 写（新建）
curl -s -X PUT -H "Authorization: Bearer $TOKEN" -H "If-None-Match: *" \
  -H "Content-Type: text/markdown" --data-binary "# 标题" \
  "$B/file?path=notes/topic.md"
# 更新（条件写）
curl -s -X PUT -H "Authorization: Bearer $TOKEN" -H 'If-Match: "<revision>"' \
  -H "Content-Type: text/markdown" --data-binary "新内容" \
  "$B/file?path=notes/topic.md"
```

完整端点、错误码与分页游标规则见仓库 `docs/ai-integration.md`。
"##,
        ),
        (
            "inbox/README.md",
            r##"---
title: 收件箱
tags: [meta, inbox]
---

# 收件箱（inbox）

任何还没想清楚归属的念头，先丢在这里，定期整理归档到 `notes/`。

- 一条一个文件，文件名随手起（如 `inbox/2026-10-08-缓存想法.md`）
- 整理时移动文件（CLI `move` 或复制后删除），历史链完整保留
"##,
        ),
    ]
}

/// Seed the template into a COMPLETELY empty vault (no live heads, no
/// tombstones): idempotent by construction — any prior content skips it.
/// Returns the number of notes written (0 = vault not empty, nothing done).
pub async fn seed_if_empty<H, R, B, I>(
    vault: &Vault<H, R, B, I>,
) -> brainstore_domain::DomainResult<usize>
where
    H: brainstore_ports::HeadStore + Send + Sync,
    R: brainstore_ports::RevisionStore + Send + Sync,
    B: brainstore_ports::BlobStore + Send + Sync,
    I: brainstore_ports::IndexStore + Send + Sync,
{
    // The raw store listing includes tombstones: any head at all means the
    // vault has been used before and must not be touched.
    if !vault.heads.list(None, None, 1).await?.is_empty() {
        return Ok(0);
    }
    let principal = Principal {
        actor: "system:seed".into(),
        scopes: vec![
            Scope::parse("read:/").expect("valid scope"),
            Scope::parse("write:/").expect("valid scope"),
        ],
    };
    let mut seeded = 0;
    for (raw_path, body) in template_notes() {
        let path = VaultPath::new(raw_path)?;
        vault
            .put_file(
                &principal,
                CommitInput {
                    path: &path,
                    content: body.as_bytes(),
                    media_type: None,
                    actor: principal.actor.clone(),
                    operation_id: HexId::generate(),
                    precondition: Precondition::Create,
                    executable: false,
                    copied_from: None,
                },
            )
            .await?;
        seeded += 1;
    }
    Ok(seeded)
}
