---
title: Brains
tags: [meta]
---

# Brains

**可读、可版本化、可恢复的知识库 —— 纯文件进、纯文件出，每个修订都被保留。**

这是一个刚初始化的空库，里面已经放好了几篇入门文档：

- [docs/brains-guide.md](docs/brains-guide.md) —— 日常使用规则（路径、保存/冲突、回收站、搜索、备份）
- [docs/ai-guide.md](docs/ai-guide.md) —— AI 协作守则（给 AI agent 的操作约定）
- [inbox/README.md](inbox/README.md) —— 收件箱用法

## 三分钟上手

```bash
# Web 界面：直接用浏览器打开本服务地址，粘贴 token 登录
# CLI：
bsx ls ""
echo "# 第一条笔记" | bsx put notes/first.md
bsx search 缓存
```

## 备份

磁盘上的目录就是备份本身：`vaults/{vault}/heads|revisions|blobs`。
复制这个目录即得完整原始备份；索引（SQLite/D1）只是可随时删除重建的投影（`bsx reindex`）。
