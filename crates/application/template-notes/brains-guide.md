---
title: Brains 使用规则
tags: [meta, guide]
---

# Brains 使用规则

## 路径与文件

- 路径是 vault 内相对路径，如 `notes/cache.md`、`inbox/idea.md`；按主题分目录，文件名即主题。
- 文本文件（markdown 等）在网页端直接编辑；二进制用 CLI `put --from`/`cat`，如 `bsx cat attachments/image.png > image.png`；上传也可走顶栏「上传」。
- 在正文里用 `[[wiki-link]]` 互链，信息面板会统计出链与反链。

## 保存与冲突（CAS）

- 每次保存都基于某个基线修订；两个人同时基于同一版本修改，**恰好一人成功，另一人收到 412 版本冲突**。
- 冲突不会自动覆盖：冲突工作台提供分栏 / 内联 diff，由你决定「以本地保存」或「采用远端」。
- 网页端明确区分本地草稿与发布到服务器；发布按钮或 Ctrl/Cmd+Enter 才执行远端条件写。未发布的内容不能当成已同步；编辑器工具栏或更多菜单可查看未发布改动。

## 回收站与历史

- 删除是**墓碑**（tombstone）：文件进回收站，历史完整保留，随时恢复；恢复产生**新**修订（generation 只前进）。
- 侧栏「回收站」切换批量恢复；信息面板可从历史链恢复任意有内容的版本。

## 搜索

- **字面**模式：大小写敏感的 UTF-8 子串匹配，结果标注是否完整；扫描预算耗尽时必须继续处理明确的不完整结果，不能声称全库无匹配。
- **FTS** 模式：英文按词元、中文按字的显式分词搜索，速度快但依赖索引。
- **hybrid** 模式：字面 + FTS 加权倒数排名融合（`bsx search --mode hybrid`），每条命中带检索溯源（`literal#1 · fts#2`）；接入语义检索后自动参与，未接入时如实声明不可用。
- **元数据过滤**：`bsx ls --status draft --type Decision --stale` 与搜索同参数可过滤 `status`/`type`/`trust`/`stale_after`（配合 frontmatter 生命周期字段，见 memory-writing）。
- 索引只是投影：需要重建时使用 `bsx reindex --all-pages`，检查失败项；不要为日常操作主动删除真实索引。

## 质量与检查点

- `bsx check`：只读质量报告——过期（stale_after）、机器生成未确认、推断缺出处、断链、脚注无来源；`--json` 供 agent 消费，发现问题时退出码 10。
- `bsx checkpoint show/update <scope>`：按目录约定维护 `<scope>/checkpoint.md`（目标/进行中/已交付/下一步），普通版本化文件、CAS 写入；任务开始先 show，里程碑后 update。

## 备份与恢复

- 原始备份包含完整 `vaults/{vault}/`（含format.json、HEAD、revision、blob），停止写入后复制，保留源库。跨runtime使用 `export/import --format archive`，不会复制认证秘密或数据库索引。
- archive恢复要求空目标或精确同历史前缀，本地空目标以 `--no-seed` 启动；不能覆盖另一条历史。恢复失败后目标可能有合法前缀，重跑同一archive，最终实读全部历史及索引通过后再使用。
- `bsx import ./notes` 默认只创建或跳过相同文件，冲突不自动覆盖；普通import/export与sync只处理当前内容，不保留两端相同revision身份。
- 本地写入后：`bsx --profile local sync --remote-profile cloud`，再 `bsx --profile local sync --remote-profile cloud verify`；确认正文一致后用 `bsx profile use cloud --verify-with local` 验证索引并显式切换。先配置两端profile，不要把示例名称当成已有远端。
- 内容组织和来源保存见 [自包含记忆写作](memory-writing.md)，条件写与失败处理见 [AI协作守则](ai-guide.md)。
