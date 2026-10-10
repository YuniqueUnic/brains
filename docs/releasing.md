# 统一版本与发布

Brains 的产品版本统一管理，Rust、Web、Worker、SDK 和验收工具使用同一个版本，以 [`VERSION.txt`](../VERSION.txt) 为准。源码版本不代表发布或部署已完成。HTTP 协议的 `v1`、存储 schema 和第三方依赖版本各有独立含义，不随产品版本改写。

## 日常发布

采用与 xidl 相同的 release-please 单组件方案：`release-please-config.json` 中只有根组件 `.`，使用 `simple` 策略及结构化 `extra-files` 同步 Cargo workspace、16 个本地锁定包、5 个 JavaScript package 和独立 npm lock。Rust 成员继续继承 workspace 版本。新增产品包时，需要同时加入 extra-files。

1. 使用 Conventional Commits 提交业务修改，例如 `fix(web): ...`、`feat(search): ...`。
2. `main` 更新后，Release Please 工作流创建或更新一个版本 PR，一次修改版本文件和 `CHANGELOG.md`。审核 PR 中的版本、日志和 CI。
3. 合并版本 PR 后，release-please 创建 `v<version>` tag 与 Release。它从默认分支启动 Release 工作流，工作流检出该 tag 的源码，构建原生、Web、Worker 产物。
4. Release 工作流校验产品版本及 tag 对应的提交，然后把带 SHA-256 的资产上传到该 Release；不再独立创建 tag 或覆盖发布说明。Release 页面出现不等于资产构建完成，以该工作流和资产清单为准。

工作流使用 GitHub 自动提供的 `GITHUB_TOKEN`，无需复制 xidl 的个人令牌配置。仓库需允许 Actions 创建 PR；由自动令牌创建的 PR，其工作流可能需要有写权限的成员批准运行。该令牌创建的 tag 不会自动触发 tag-push 工作流，因此这里使用 `workflow_dispatch`。

Release 已创建而构建或分发失败时，在仓库目录手动重跑资产工作流。`--ref main` 选择最新构建脚本，`version` 选择已有发布标签：

```bash
gh workflow run release.yml --ref main -f version=v0.3.0
```

构建脚本位于默认分支，产品源码始终来自输入标签，后续 job 使用首次解析出的同一提交。上传前会再次核对远端标签，标签发生变化就拒绝发布。修复构建脚本后可以重新构建已有版本，不需要移动标签。重跑 release-please 不保证再次分发资产构建。如果资产只上传了一部分，先核对已有文件，再手动上传缺失文件；同名资产不会被静默覆盖。

## 本地检查

```bash
python3 scripts/check-versions.py
mbx check --workspace --all-targets --all-features --locked
pnpm install --frozen-lockfile
```

`check-versions.py` 只校验，不负责改写版本。版本写入统一交给 release-please，避免两套版本更新器。pnpm lock v9 不存各工作区自身的版本，无需新增字段；不要对锁文件里的第三方版本做全局替换。

依据（2026-10-11 核对）：[xidl 的 release-please 配置](https://github.com/xidl/xidl/blob/master/release-please-config.json)、[release-please extra-files](https://github.com/googleapis/release-please/blob/main/docs/customizing.md#updating-arbitrary-files)、[GitHub 工作流触发规则](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)。
