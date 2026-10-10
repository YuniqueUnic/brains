# 编辑源码、搜索命令与统一版本

状态：已采纳。编辑布局、元信息解析和搜索模式各由现有模块管理，不新增配置持久层。

| 争议点 | 换来什么 | 不做会怎样 | 替代方案 | 决定 |
| --- | --- | --- | --- | --- |
| 编辑器行宽 | 收起侧栏后使用释放的空间 | 860px 上限造成宽度看似未更新 | 根据侧栏布尔值计算宽度 | 删除源码行宽上限，由容器与 CodeMirror 原生测量负责 |
| YAML 标签识别 | block/flow 两种语法使用相同标签语义 | 源码中的列表、方括号仍按 Markdown 理解 | 另写 tags 正则 | 复用现有 YAML AST 的值与范围，用 mark 保持源码可编辑；悬浮按钮复用标签搜索 |
| 搜索模式命令 | 键盘切换 literal/fts，保留标签组合 | 必须用鼠标切换 | 给每个键盘事件加分支 | 在统一纯解析函数中提取标签与模式，沿用 Workspace 的 mode 状态 |
| 账号展示 | 明确当前访问权限 | 把审计用 actor 当用户名 | 改认证与存储身份 | 仅在展示边界派生权限文案，不展示 token 内部编号 |
| 版本管理 | 一个 release-please 发布单元更新所有包 | Cargo 与前端版本漂移 | 自制版本写入脚本 | 采用 xidl 的 manifest/extra-files；校验只读取版本，发布构建复用现有 workflow |

源码标签不替换文本、不拦截普通点击；悬浮搜索与键盘快捷方式调用同一入口。

CodeMirror 的 `yamlFrontmatter` 只接受 `---`，会将 `...` 后的正文也交给 YAML 解析。使用 Lezer 的 Markdown 扩展划分前置区，YAML 语法仍由现有库处理；元信息的值与标签位置统一来自原有 YAML 抽象语法树。没有新增 YAML 解析器或兼容路径。[官方围栏定义](https://github.com/codemirror/lang-yaml/blob/main/src/frontmatter.grammar)

搜索命令只有在空格、Enter 或 Tab 确认后才变成 chip。尚未确认的命令和输入法中间文字不发起搜索；关闭搜索时结束该输入会话。选中结果的 Enter 交给 cmdk，发布的 Mod+Enter 由 EditorPane 负责，移除编辑器自带的同键插入空行行为。

使用方式见 [编辑与搜索](../editor-search.md)，版本边界与首次发布操作见 [统一版本与发布](../releasing.md)。
