# 单独使用 bsx，再接入服务器

`bsx` 有两种连接模式。两种模式使用同一套文件、历史、搜索、归档和同步命令。

| 模式 | 数据在哪里 | 是否需要服务器和 token |
| --- | --- | --- |
| `local` | 当前电脑的数据目录 | 都不需要，bsx 直接读写 |
| `remote` | profile 指定 URL 的服务器 | 需要；服务器可以在本机，也可以在公网 |

profile 是一个有名字的连接配置。名字可以自选；例如 `laptop` 是离线库，`home` 是本机服务器，`cloud` 是远端服务器。名字里的 local/remote 不决定连接方式，保存的 `mode` 才决定。

## 1. 只有一台电脑、一个 bsx

安装后在任意目录执行：

```bash
bsx init "$HOME/.brains/my-library" --profile laptop
bsx import ./notes --dry-run
bsx import ./notes
bsx ls
bsx search "关键词"
bsx export ./notes-export
```

`init` 创建空库，并把该 profile 设为默认。`notes` 是用户已有的普通文件目录；`my-library` 是 Brains 管理的数据目录，两者不能混用。命令不会启动监听端口，也不会创建服务器认证文件。需要示例笔记时另行运行 `bsx seed`。已存在的库用 `bsx profile add laptop --data-dir /absolute/path --vault default` 登记，不要重复初始化。

保存已有文件需要明确的版本条件。先用 `bsx stat notes/a.md --json` 取得编辑依据中的 `revision`，编辑后执行：

```bash
bsx put notes/a.md --from ./a.md --if-revision "$BASE_REVISION"
```

版本变了就拒绝写入。`bsx edit notes/a.md --if-revision "$BASE_REVISION"` 可以调用 `$EDITOR` 完成同样的流程。不要临写前取一次最新版本来掩盖编辑期间的并发修改。

## 2. 将离线库交给服务器一起管理

搭好服务器后登记 URL 和 token：

```bash
bsx profile add cloud --url "$CLOUD_URL"
printf '%s' "$CLOUD_TOKEN" | bsx --profile cloud login --stdin-token
bsx --profile cloud whoami
```

需要把已有历史一起搬过去时，目标库应为空；原生服务器使用 `--no-seed` 启动新库：

```bash
bsx --profile laptop export ./first-archive --format archive
bsx --profile cloud import ./first-archive --format archive --dry-run
bsx --profile cloud import ./first-archive --format archive
bsx --profile laptop sync --remote-profile cloud
```

归档保留可达历史与修订身份，`sync` 建立日常协作基线。之后可以在 Web、远端 CLI 或离线 bsx 中修改，有网络时同步：

```bash
bsx --profile laptop sync --remote-profile cloud --dry-run
bsx --profile laptop sync --remote-profile cloud
bsx --profile laptop sync --remote-profile cloud verify
```

同步传递当前内容和软删除，不复制每一代历史。只需要当前文件时可以直接 sync，无需先归档；但不能把这种结果称作完整历史迁移。文本的不重叠修改可自动合并；无法合并的内容保留双方，使用 `sync resolve <路径> --keep local|remote` 或 `--merged-file` 明确解决。这里的 local/remote 指当前 profile 和另一端，与它们各自的连接模式无关。

## 3. 再加一个本机服务器

本机服务器与公网服务器没有特殊区别。让它使用独立目录，例如 `~/.brains/home-server`，监听 `127.0.0.1:45800`。登记后同步即可：

```bash
bsx profile add home --url http://127.0.0.1:45800
printf '%s' "$HOME_SERVER_TOKEN" | bsx --profile home login --stdin-token
bsx --profile laptop sync --remote-profile home
bsx --profile home sync --remote-profile cloud
bsx --profile laptop sync --remote-profile home
```

这样 laptop 可一直离线使用，home 提供本机 Web，cloud 提供远端访问。每对库各有同步基线；命令显式执行，没有后台自动同步。若首次也要保留历史，对空 home 重复上一节的 archive 导入，再开始 sync。

也可以让服务器直接接管原来的离线数据目录：

```bash
BRAINS_TOKEN="$HOME_SERVER_TOKEN" brains-server \
  --data-dir "$HOME/.brains/my-library" --vault default \
  --bind 127.0.0.1 --port 45800 --no-seed --web-dist /path/to/web/dist
```

这种方式不需要搬数据。**同一个数据目录一次只能由一个进程直接打开**：服务器运行期间，bsx 用 home 这个 remote profile 访问；laptop 直接打开会明确报“目录正在使用”，退出码为 6。停止服务器后 laptop 恢复可用。目录锁也覆盖该目录下的不同 vault。要同时使用离线库和 Web，请用两个独立目录，通过 sync 协作，不要删除 `.brains.lock` 绕过锁。

升级到这版时，应先停止尚未实现目录锁的旧服务器，再启动新版；新锁无法约束仍运行的旧二进制。

## 配置与备份

配置默认在 `~/.brains/config.json`，可用 `BRAIN_CONFIG` 指定另一个文件：

```json
{
  "active": "laptop",
  "profiles": {
    "laptop": {"mode": "local", "data_dir": "/absolute/path/my-library", "vault": "default"},
    "home": {"mode": "remote", "url": "http://127.0.0.1:45800", "token": "<用 login 保存>"}
  }
}
```

local profile 不接受 `BRAINS_URL` / `BRAINS_TOKEN` 覆盖，避免本地命令意外访问服务器。双 profile 同步要求两个具名配置，不能混用环境覆盖。配置文件权限为 `0600`；不要提交 token。

旧版只含 URL/token 的 profile 需要在备份配置后明确添加 `"mode": "remote"`，其余值保持原样。代码不猜测旧配置属于哪种模式，也不会静默转换。

数据正文和历史保存在 `vaults/<vault>/heads`、`revisions`、`blobs`；不要用编辑器直接改这些文件。每个 vault 的 `index.db` 只是派生索引。旧根目录索引不再读取，新位置不存在时会从权威文件完整重建；重建失败则报错，不把半成品当作可用索引。旧索引可在备份并确认重建成功后单独清理。

裸目录备份应停止写入后进行。日常可读导出和完整历史归档的边界、断点恢复与核验规则见[可移植工作流](portable-workflow.md)。
