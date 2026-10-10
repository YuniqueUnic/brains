---
title: Brains
tags: [meta]
---

# Brains

**可读、可版本化、可恢复的知识库 —— 纯文件进、纯文件出，每个修订都被保留。**

这是一个刚初始化的空库，里面已经放好了几篇入门文档：

- [docs/brains-guide.md](docs/brains-guide.md) —— 日常使用规则（路径、保存/冲突、回收站、搜索、备份）
- [docs/ai-guide.md](docs/ai-guide.md) —— AI 协作守则（给 AI agent 的操作约定）
- [docs/memory-writing.md](docs/memory-writing.md) —— 自包含记忆、来源摘录、自己的解释与可复用整理提示词
- [inbox/README.md](inbox/README.md) —— 收件箱用法

## 三分钟上手

```bash
# Web 界面：直接用浏览器打开本服务地址，粘贴 token 登录
# CLI使用独立配置；把地址换为当前服务，token从安全来源读入环境。
bsx profile add local --url http://127.0.0.1:45800
printf '%s' "$LOCAL_TOKEN" | bsx --profile local login --stdin-token
bsx --profile local ls ""
echo "# 第一条笔记" | bsx --profile local put notes/first.md --create
bsx --profile local search 缓存
```

## 备份

原始备份要保存完整 `vaults/{vault}/`，包含format.json、HEAD、revision与blob；备份窗口应停止写入。跨runtime完整迁移使用 `bsx export ./archive --format archive` 与目标profile的 `import ./archive --format archive`。索引（SQLite/D1）只是投影，可用 `bsx reindex --all-pages` 分页重建；原始备份与CLI archive的目录格式不同，不能混用。
