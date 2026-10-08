---
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
B="http://127.0.0.1:45800/api/v1"

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
