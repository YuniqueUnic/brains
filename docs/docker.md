# 用 Docker 运行 Brains

镜像包含 `brains-server` 和 Web 页面，正式发布提供 Linux AMD64、ARM64 两种架构。镜像名称为 `yunique001/brains`；版本标签与 GitHub Release 一致，例如 `<version>`，也可固定镜像 digest。`latest` 只在发布当前最新稳定版时更新，重建旧版本不会把它倒退。

这套部署方式从包含本文件的后续版本开始提供；旧 `v0.3.0` 没有这些容器功能。首次镜像发布前，可以按本文末尾从源码构建。

## 用命名卷启动

下载仓库中的 `compose.yaml`，在该文件所在目录运行：

```bash
docker compose up -d
docker compose ps
docker compose logs brains
```

默认使用 `yunique001/brains:latest`，只把宿主机 `127.0.0.1:45800` 映射到容器端口。可通过环境变量选择已发布版本、宿主地址和端口：

```bash
BRAINS_IMAGE=yunique001/brains:<version> BRAINS_HOST_PORT=45801 docker compose up -d
```

首次启动会生成 root token，并只在此次启动日志中显示。将它保存到密码管理器，CLI 使用 `bsx login --stdin-token` 登录。后续启动沿用卷内的认证信息；无需把 token 再写进 Compose。默认会创建引导笔记，准备恢复完整归档时，首次启动前设置 `BRAINS_NO_SEED=true`。

`brains-data` 命名卷挂载整个 `/data`，包含正文、历史、附件、索引、认证记录和会话签名密钥。`docker compose down` 后重新 `up` 会复用同一卷。**不要使用 `down -v` 或删除该卷，除非确实要删除这些数据。** Compose 项目名参与卷命名；更换目录或 `-p` 项目名时，应明确选择原来的卷。

已有外部卷可在 `compose.override.yaml` 中指定：

```yaml
volumes:
  brains-data:
    external: true
    name: my-existing-brains-data
```

原生 `docker run` 的等价入口：

```bash
docker run -d --name brains --restart unless-stopped \
  -p 127.0.0.1:45800:45800 \
  --mount type=volume,src=brains-data,dst=/data \
  --read-only --tmpfs /tmp:rw,noexec,nosuid,size=64m,mode=1777 \
  --cap-drop ALL --security-opt no-new-privileges \
  yunique001/brains:latest
```

镜像不自动创建匿名卷；运行者必须显式挂载 `/data`。只挂载 `/data/vaults` 会遗漏认证和签名密钥。

## 挂载宿主机目录与指定用户

镜像默认使用 UID/GID `65532:65532`，新建空命名卷会获得镜像内 `/data` 的目录权限。已有卷、宿主机目录或禁用卷填充的挂载，需要自行准备正确权限。服务不会以 root 启动，也不会递归修改宿主目录所有者。

下面以当前宿主用户运行，并使用当前用户拥有的目录：

```bash
mkdir -p ./brains-data
chmod 700 ./brains-data
export BRAINS_UID=$(id -u) BRAINS_GID=$(id -g)
```

保存为 `compose.override.yaml`：

```yaml
services:
  brains:
    user: "${BRAINS_UID:?set BRAINS_UID}:${BRAINS_GID:?set BRAINS_GID}"
    volumes:
      - type: bind
        source: ./brains-data
        target: /data
        bind:
          create_host_path: false
```

同一 `/data` 目标会替换基础 Compose 中的命名卷挂载。先运行 `docker compose config` 核对最终配置，再 `docker compose up -d`。设置 UID/GID 时应使用非 root 用户；改变已有数据的运行身份前，应停服并调整目录权限。

优先使用 `--mount` 或上面的长语法，源目录不存在时直接报错，避免拼错路径后悄悄创建另一个空目录。SELinux 主机可能还需为 bind 目录设置允许容器访问的标签；rootless Docker 的用户映射也要按宿主环境配置。不要用 `chmod -R 777` 解决权限问题。

## 能力开关

镜像内的 `brains-server` 是完整能力构建：全文检索、混合排序、文档转换声明、模板播种都默认开启。需要在启动时**停止声明**某项能力（例如不希望 agent 看到文档转换入口）：

```bash
BRAINS_DISABLE=document_conversion,semantic docker compose up -d
```

只接受 `document_conversion` 与 `semantic` 两个名字——它们能被诚实地收回（转换在 CLI 侧执行，服务器只做声明）；`fts` 这类编译期能力不允许伪装关闭，传入未知名字会启动失败。能力矩阵见 [capabilities.md](capabilities.md)。

## 参数、环境变量和文件 secret

命令行参数优先于对应环境变量，随后才使用默认值。镜像默认值和原生程序默认值有三处区别：`/data`、`0.0.0.0`、`/app/web`。

| 环境变量 | 命令行参数 | 含义 |
| --- | --- | --- |
| `BRAINS_DATA_DIR` | `--data-dir` | 整个持久化目录；改路径时同步修改挂载目标 |
| `BRAINS_VAULT` | `--vault` | vault 名称，默认 `default`；每个实例管理一个 vault |
| `BRAINS_BIND` | `--bind` | 容器内监听地址，默认 `0.0.0.0` |
| `BRAINS_PORT` | `--port` | 容器内端口，默认 `45800`；改动后同步修改端口映射 |
| `BRAINS_TOKEN` | `--token` | 启动时注册或替换 root token |
| `BRAINS_TOKEN_FILE` | `--token-file` | 启动时从 UTF-8 文件读取 root token，支持文件末尾换行 |
| `BRAINS_NO_SEED` | `--no-seed` | `true` 时不创建引导笔记，默认 `false` |
| `BRAINS_WEB_DIST` | `--web-dist` | Web 静态文件目录，可挂载另一个只读目录 |
| `BRAINS_ALLOWED_ORIGINS` | `--origin` | 额外允许的浏览器来源，以逗号分隔；CLI 可重复传入 |
| `RUST_LOG` | — | 日志过滤，例如 `brains_server=debug,tower_http=info` |

`BRAINS_TOKEN` 与 `BRAINS_TOKEN_FILE` 互斥；空值、含空白的 token、不可读的文件都会在打开数据目录之前报错退出。不要把“未配置”写成 `BRAINS_TOKEN: ""`。文件只在启动时读取，修改后需要重建或重启容器。

文件 secret 可以追加到上一节的 **bind mount + 当前用户** 配置中。先把 token 保存在当前用户可读、其他用户不可读的 `./secrets/root-token` 文件，目录权限 `0700`、文件权限 `0400`，然后添加：

```yaml
services:
  brains:
    environment:
      BRAINS_TOKEN_FILE: /run/secrets/brains_token
    secrets:
      - brains_token
secrets:
  brains_token:
    file: ./secrets/root-token
```

Compose 的文件型 secret 实际使用只读 bind mount，不会按 `uid/gid/mode` 设置自动改写宿主文件所有者。上面的运行 UID 与文件所有者一致，才能读取 `0400` 文件。若沿用默认 UID `65532`，则要在宿主准备该 UID 可读的文件或通过组权限授权，不能只在 Compose 写一个 `uid` 就认为生效。不要把密钥文件放进 Web 目录或 Git。

## 网络、HTTPS、健康检查和停止

CLI 的 bearer token 请求可通过本机 HTTP 端口使用。浏览器登录使用 Secure cookie：容器监听 `0.0.0.0` 时，宿主仅映射到回环地址也不会降低 cookie 安全策略。正式 Web 使用应通过 HTTPS 反向代理，尤其不要假定 Safari 在本机 HTTP 下可以保持登录。

反向代理应转发原始 `Host`，页面与 `/api/` 使用同一来源。只有跨来源访问时才配置 `BRAINS_ALLOWED_ORIGINS`，例如 `https://notes.example.com`。如果代理也运行在 Compose 网络中，可让它访问 `brains:45800`，删除 Brains 的 `ports` 映射；仅代理对外开放 TLS 端口。

镜像通过 `/healthz` 检查 HTTP 进程是否可响应，并使用 `BRAINS_PORT` 选择端口。该检查不证明磁盘可写、鉴权或搜索结果正确。若用命令行覆盖端口、只监听其他地址，需同时覆盖 `healthcheck`；通过环境变量改端口会自动同步探测端口。

日志写入标准输出/错误，Compose 默认轮转日志。容器接收 SIGTERM/SIGINT 后停止接受新连接，等待已接收请求完成；默认 Compose 给予 30 秒，超时 Docker 仍会强制停止。需要更长的上传收尾时间时可增大 `stop_grace_period`。CPU、内存和进程数可按负载使用 Compose `cpus`、`mem_limit`、`pids_limit` 设置，不需要服务端再实现一套资源配置。

同一个数据目录只能由一个 Brains 进程打开；不要把同一卷用于多个写入副本。这里验证的是本机 bind mount 和 Docker 本地卷，没有承诺 NFS、SMB 等远程文件系统具备所需的锁和同步语义。

## 备份、升级与本地构建

内容迁移可用 `bsx export --format archive`，参见[迁移指南](portable-workflow.md)。需要连认证、签名密钥一起备份时，停止容器后复制整个持久化目录，再重新启动；不要只复制运行中的索引数据库。升级前保留备份，使用已发布的固定版本或 digest，拉取镜像并重建容器，保留原卷。

从源码构建（Docker BuildKit）：

```bash
docker build --target source -t brains-local:dev .
BRAINS_IMAGE=brains-local:dev docker compose up -d
```

构建上下文排除 `.git`、本地数据和凭据；默认版本显示 `git unknown`。如果需要声明源码身份，可传入 `--build-arg BRAINS_BUILD_GIT_SHA=<sha>` 和 `--build-arg BRAINS_BUILD_GIT_DIRTY=true|false`，含义与[构建身份](releasing.md#构建身份)相同；元数据由构建者负责，不是源码真实性证明。

`source` 目标在容器中编译 Rust 与 Web，BuildKit 缓存 Cargo 下载和编译目录。CI/发布选择 `release` 目标，从 `docker-artifacts/brains-server` 和 `docker-artifacts/web/` 组装已经构建、验收过的 musl 二进制和 Web，避免重复编译。两个目标共用同一个运行层。

依据：[Docker volumes](https://docs.docker.com/engine/storage/volumes/)、[bind mounts](https://docs.docker.com/engine/storage/bind-mounts/)、[Compose 文件 secret 的权限限制](https://docs.docker.com/reference/compose-file/services/#secrets)、[多阶段构建](https://docs.docker.com/build/building/multi-stage/)。
