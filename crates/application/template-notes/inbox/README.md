---
title: 收件箱
tags: [meta, inbox]
---

# 收件箱（inbox）

任何还没想清楚归属的念头，先丢在这里，定期整理归档到 `notes/`。

- 一条一个文件，文件名随手起（如 `inbox/2026-10-08-缓存想法.md`）
- 整理前提炼原始材料、必要短摘录与自己的解释；本地来源依赖要留在vault内，详见 [自包含记忆写作](../docs/memory-writing.md)。
- 移动用 `bsx mv <源路径> <目标路径> --if-revision <已读取的源revision>`：目标新建revision并记录copy来源，成功后对源路径条件删除为tombstone；目标不继承源parent链，源历史仍在回收站。若返回partial，要核对两条路径，不能假定全操作原子完成。
