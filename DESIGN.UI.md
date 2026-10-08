可以。**直接定这一套，不再保留 Radix，也不再做多套 UI primitive 并存。**

# Brains 新 UI 技术基线

目标非常明确：

> **shadcn/ui 负责组件代码与设计系统组织，Base UI 负责通用交互原语，React Aria 负责复杂可访问交互，CodeMirror 负责编辑器，cmdk 负责 Command Palette，Sonner 负责反馈，Lucide 负责图标。**

这套方向本身也是当前 shadcn 的主路线：2026 年 7 月起 shadcn 新项目默认使用 Base UI，同时 React Aria 也已经成为 shadcn 的一等 component base。([shadcn/ui][1])

---

# 1. 最终技术栈

```text
React 19
TypeScript
Tailwind CSS
        │
        ▼
shadcn/ui
        │
        ├── Base UI       → 通用 UI primitive
        ├── React Aria    → Tree / Selection / DnD / complex interaction
        ├── cmdk          → Command Palette
        ├── Sonner        → Toast / operation feedback
        ├── CodeMirror 6  → Markdown editor
        └── Lucide        → Icons
```

### 各自职责

| 技术             | Brains 中负责什么                                                   |
| -------------- | -------------------------------------------------------------- |
| **shadcn/ui**  | 本地 UI 组件、设计系统、组件源码                                             |
| **Base UI**    | Dialog / Popover / Dropdown / Menu / Tooltip / Tabs / Drawer 等 |
| **React Aria** | File Tree、Selection、Keyboard interaction、Drag & Drop           |
| **cmdk**       | `⌘K` Command Palette / 快速搜索                                    |
| **Sonner**     | Saved / Deleted / Uploaded / Restored 等 Toast                  |
| **CodeMirror** | Markdown 编辑器、语法高亮、Find/Replace、Undo/Redo                       |
| **Lucide**     | 全部图标                                                           |
| **Tailwind**   | 样式系统                                                           |
| **CVA**        | Button / Badge / TreeItem 等 variant                            |

Base UI 本身就是无样式、可组合、强调 accessibility 的 React primitive，因此非常适合让 **Brains 自己掌控视觉设计**。([Base UI][2])

---

# 2. 最核心的设计目标

Brains 不要再做成：

> 文件管理器 + Markdown 编辑器

而是：

> **AI-native Knowledge Workspace**

视觉方向：

```text
Notion
   +
Obsidian
   +
Linear
   +
VS Code 的键盘效率
```

但不要直接模仿任何一个。

最终关键词：

```text
Clean
Dense
Quiet
Fast
Keyboard-first
Content-first
Progressive disclosure
```

---

# 3. 总体 Layout

默认只有：

```text
┌──────────────────────────────────────────────────────────────┐
│ Brains        ⌘K Search             + New   Upload   ···    │
├───────────────┬──────────────────────────────────────────────┤
│               │                                              │
│  Workspace    │                                              │
│               │               Document                       │
│  🔍 Search    │                                              │
│               │               # AI 协作守则                  │
│  Documents    │                                              │
│  ▾ Projects   │               内容……                         │
│    ├─ A       │                                              │
│    ├─ B       │                                              │
│    └─ C       │                                              │
│               │                                              │
│  Recent       │                                              │
│  Trash        │                                              │
│               │                                              │
└───────────────┴──────────────────────────────────────────────┘
```

### 默认：

```text
Sidebar     248px
Editor      flex: 1
```

**没有永久右侧栏。**

Properties / History / Search / Command Palette 都是：

```text
按需出现
```

这就是整个设计的核心。

---

# 4. Component Architecture

建议直接建立自己的：

```text
src/components/ui/
```

而不是到处直接使用 Base UI。

结构：

```text
src/
├── components/
│
│   ├── ui/
│   │   ├── button.tsx
│   │   ├── icon-button.tsx
│   │   ├── input.tsx
│   │   ├── textarea.tsx
│   │   ├── dialog.tsx
│   │   ├── alert-dialog.tsx
│   │   ├── popover.tsx
│   │   ├── dropdown-menu.tsx
│   │   ├── context-menu.tsx
│   │   ├── command.tsx
│   │   ├── tooltip.tsx
│   │   ├── drawer.tsx
│   │   ├── scroll-area.tsx
│   │   ├── tabs.tsx
│   │   ├── separator.tsx
│   │   ├── checkbox.tsx
│   │   ├── select.tsx
│   │   ├── switch.tsx
│   │   ├── badge.tsx
│   │   └── ...
│   │
│   ├── shell/
│   │   ├── app-shell.tsx
│   │   ├── top-bar.tsx
│   │   └── sidebar.tsx
│   │
│   ├── tree/
│   │   ├── file-tree.tsx
│   │   ├── tree-item.tsx
│   │   ├── tree-selection.ts
│   │   ├── tree-context-menu.tsx
│   │   └── tree-drop-indicator.tsx
│   │
│   ├── search/
│   │   ├── command-palette.tsx
│   │   ├── search-results.tsx
│   │   └── search-filters.tsx
│   │
│   ├── editor/
│   │   ├── markdown-editor.tsx
│   │   ├── editor-toolbar.tsx
│   │   ├── editor-search.tsx
│   │   └── editor-status.tsx
│   │
│   ├── properties/
│   │   ├── properties-panel.tsx
│   │   ├── properties-general.tsx
│   │   ├── properties-links.tsx
│   │   └── properties-history.tsx
│   │
│   ├── upload/
│   │   ├── upload-dialog.tsx
│   │   └── upload-progress.tsx
│   │
│   └── trash/
│       ├── trash-view.tsx
│       ├── trash-item.tsx
│       └── empty-trash-dialog.tsx
│
├── editor/
│   ├── extensions/
│   ├── markdown/
│   └── state/
│
├── commands/
│   ├── command-registry.ts
│   ├── commands.ts
│   └── shortcuts.ts
│
├── stores/
│   ├── workspace-store.ts
│   ├── selection-store.ts
│   └── editor-store.ts
│
└── styles/
    ├── globals.css
    └── tokens.css
```

---

# 5. UI Primitive 原则

**所有普通 UI 都走 shadcn + Base UI。**

例如：

```text
Dialog
Popover
DropdownMenu
ContextMenu
Tooltip
Drawer
Tabs
Select
Checkbox
Switch
```

不要在业务组件里面直接堆 Base UI API。

统一：

```tsx
import { Dialog } from "@/components/ui/dialog"
```

而不是：

```tsx
import { Dialog } from "@base-ui/react/dialog"
```

这样以后 Brains 的视觉系统全部集中在：

```text
components/ui/
```

shadcn 本身就是 open-code 模式，组件代码归项目自己控制，非常适合这种产品型 UI。([shadcn/ui][3])

---

# 6. React Aria 专门管 Tree

这是整个项目里一个非常重要的边界：

```text
Base UI
    ↓
普通 UI

React Aria
    ↓
复杂交互组件
```

File Tree 使用 React Aria。

负责：

```text
Keyboard navigation
Focus
Selection
Multi-selection
Range selection
Tree semantics
Drag & Drop
Drop target
Accessibility
```

业务层只负责：

```text
FileId
ParentId
Children
Expanded
Selected
Renaming
Dragging
```

也就是说：

```text
React Aria = interaction engine

Brains Tree = visual + business layer
```

不要自己重新实现一套 tree keyboard/navigation。

---

# 7. Command Palette = 全局操作中心

使用：

```text
cmdk
```

作为底层。

快捷键：

```text
⌘K
```

打开：

```text
┌─────────────────────────────────────┐
│  Search or type a command...        │
├─────────────────────────────────────┤
│                                     │
│  Recent                             │
│  Open README.md                 ↵   │
│  Open Project A                ↵    │
│                                     │
│  Actions                            │
│  New document                  ⌘N   │
│  New folder                         │
│  Upload                             │
│  Rename                             │
│  Move                               │
│  Delete                             │
│                                     │
└─────────────────────────────────────┘
```

cmdk 本身就是针对这种 command menu / accessible combobox 场景设计的，并提供过滤、排序和键盘导航。([GitHub][4])

以后所有重要操作都注册成：

```ts
Command
```

例如：

```ts
{
  id: "document.new",
  title: "New document",
  shortcut: "⌘N",
  keywords: ["create", "document", "new"],
  execute: () => {}
}
```

这样：

```text
⌘K
```

不是单纯搜索框，而是 **Brains 的操作系统**。

---

# 8. CodeMirror = 唯一编辑器

继续：

```text
CodeMirror 6
```

不换 Monaco。

编辑器职责：

```text
Markdown
Syntax highlighting
Undo
Redo
Find
Replace
Selection
Keyboard shortcuts
Code blocks
Document search
```

UI：

````text
                    Saved
                      ↓
┌──────────────────────────────────────┐
│                                      │
│ # AI 协作守则                         │
│                                      │
│ 内容……                               │
│                                      │
│ ```rust                              │
│ fn main() {}                         │
│ ```                                  │
│                                      │
└──────────────────────────────────────┘
````

工具栏默认隐藏：

```text
Hover / selection
        ↓
Undo | Redo | Find | Format | ···
```

---

# 9. Sonner = 所有非阻塞反馈

统一使用：

```text
Sonner
```

例如：

```text
Saved
Uploaded 3 files
Moved to Trash
Restored
Copied path
Renamed
```

不要：

```text
alert()
```

不要：

```text
页面顶部永久 Banner
```

危险操作才：

```text
AlertDialog
```

例如：

```text
Delete permanently?

This action cannot be undone.

Cancel        Delete
```

---

# 10. Sidebar

Sidebar 是：

```text
Workspace
├── Search
├── Recent
│
├── Documents
│   ├── Project A
│   │   ├── README
│   │   └── Design
│   └── Project B
│
├── Favorites
│
└── Trash
```

尺寸：

```text
248px default
220px minimum
320px maximum
```

行高：

```text
28–30px
```

操作全部渐进出现：

```text
hover
   ↓
···
```

右键：

```text
Open
Open in New Pane
Rename
Duplicate
Move
Copy Path
New Document
New Folder
Properties
Move to Trash
```

---

# 11. Properties

默认：

```text
关闭
```

快捷键：

```text
⌘I
```

打开：

```text
                    ┌──────────────────┐
                    │ Properties    ×  │
                    ├──────────────────┤
                    │ General           │
                    │                  │
                    │ Type   Markdown  │
                    │ Size   12 KB      │
                    │                  │
                    │ Organization      │
                    │ Tags              │
                    │                  │
                    │ Links             │
                    │                  │
                    │ History           │
                    └──────────────────┘
```

所以：

```text
Properties = transient context
```

不是 Layout 的第三列。

---

# 12. Trash 是独立 Workspace

不要再：

```text
普通 FileTree + trash=true
```

而是：

```ts
type WorkspaceMode =
  | "workspace"
  | "trash"
```

Trash 自己有自己的页面：

```text
Trash

Items deleted from your workspace
are kept for 30 days.

────────────────────────────

Today

README.md
Deleted 2 hours ago          Restore

Project A
Deleted yesterday            Restore

────────────────────────────

Older

...
```

顶部：

```text
Empty Trash
```

必须确认。

---

# 13. 状态架构

避免：

```ts
isSearchOpen
isTrashOpen
isPropertiesOpen
isHistoryOpen
isUploadOpen
isRenameOpen
isDeleteOpen
...
```

改成状态模型。

```ts
type WorkspaceMode =
  | "workspace"
  | "trash"
  | "search"

type SidePanel =
  | null
  | "properties"
  | "history"

type Dialog =
  | null
  | "new"
  | "upload"
  | "rename"
  | "delete"
  | "move"
  | "empty-trash"

type SelectionState = {
  ids: Set<FileId>
  anchor?: FileId
}
```

核心：

```text
Workspace
 ├── mode
 ├── selection
 ├── sidebar
 ├── panel
 ├── dialog
 └── editor
```

---

# 14. 视觉设计 Token

整体不要花哨。

```css
--background: #ffffff;
--surface: #fafafa;
--surface-hover: #f5f5f5;

--border: #e5e7eb;
--border-subtle: #eeeeee;

--text: #1f2328;
--text-secondary: #6b7280;
--text-muted: #9ca3af;

--accent: ...;
--danger: ...;
--success: ...;

--radius-sm: 6px;
--radius-md: 8px;
--radius-lg: 10px;

--control-height: 32px;
--row-height: 30px;
```

核心原则：

```text
无渐变
无大阴影
无玻璃拟态
无巨大圆角
无粗边框
无五颜六色 icon
```

---

# 15. Icon 统一 Lucide

统一：

```tsx
import {
  Search,
  Plus,
  Upload,
  MoreHorizontal,
  Folder,
  FileText,
  Trash2,
  Settings,
  Command,
} from "lucide-react"
```

规格：

```text
16px   normal UI
18px   primary action
20px   major navigation
```

不要出现：

```text
24 / 28 / 32px 大图标满天飞
```

---

# 16. 最终页面结构

最终 Brains 应该非常简单：

```text
AppShell
│
├── TopBar
│
├── Sidebar
│   └── FileTree
│
└── MainContent
    │
    ├── DocumentHeader
    │
    └── MarkdownEditor
```

然后全部是 **overlay / contextual UI**：

```text
CommandPalette
ContextMenu
PropertiesPanel
HistoryPanel
UploadDialog
ConfirmDialog
Sonner
```

也就是：

```text
                    ┌───────────────┐
                    │ Command       │
                    │ Palette       │
                    └───────────────┘
                           ↑
                           │ ⌘K
                           │

┌──────────┐     ┌─────────────────────────┐
│ Sidebar  │     │                         │
│          │     │       Editor            │
│ Tree     │     │                         │
│          │     │                         │
└──────────┘     └─────────────────────────┘
                          │
                          │ ⌘I
                          ▼
                   ┌──────────────┐
                   │ Properties   │
                   └──────────────┘
```

**界面默认保持安静，需要操作时才出现工具。**

---

# 17. 一句话定调

以后 Brains 的 UI 就遵循这一条：

> **shadcn 定义组件体系，Base UI 提供基础交互，React Aria 解决复杂交互，CodeMirror 负责内容，cmdk 负责命令，Sonner 负责反馈，Lucide 负责图标；Brains 自己掌控最终视觉。**

技术路线可以直接锁死为：

```text
React 19
TypeScript
Tailwind
shadcn/ui
@base-ui/react
React Aria
cmdk
CodeMirror 6
Sonner
Lucide
CVA
```

而且现在 shadcn 已经直接提供 **Base UI 版本的 Sidebar、Dialog、Drawer 等组件**，因此这一套组合可以比较自然地落地，不需要再自己搭一层“第三方 UI 大杂烩”。([shadcn/ui][5])

**后续开发就以这套为唯一 UI 基线。**

[1]: https://ui.shadcn.com/docs/changelog/2026-07-base-ui-default?utm_source=chatgpt.com "July 2026 - Base UI as the Default - shadcn/ui"
[2]: https://base-ui.com/react/overview/about?utm_source=chatgpt.com "About Base UI · Base UI"
[3]: https://ui.shadcn.com/docs?utm_source=chatgpt.com "Introduction - shadcn/ui"
[4]: https://github.com/dip/cmdk?utm_source=chatgpt.com "GitHub - dip/cmdk: Fast, unstyled command menu React component. · GitHub"
[5]: https://ui.shadcn.com/docs/components/base/sidebar?utm_source=chatgpt.com "Sidebar - shadcn/ui"
