# GitHub Releases 发布

## 工作流

[CI](https://github.com/KaguyaJs/mihomo-web-panel/actions/workflows/ci.yml) 在 main 推送、Pull Request 和手动运行时验证源码。它也作为可复用工作流供 [Release](https://github.com/KaguyaJs/mihomo-web-panel/actions/workflows/release.yml) 调用。

Release 由 `v*` 标签推送或手动选择已有版本标签触发，依次执行：

1. 校验标签、Cargo.toml、前端 package / lockfile 和 CHANGELOG.md 版本一致，并确认 checkout 对应标签提交。
2. 执行与 CI 相同的 Linux / Windows / macOS、最低 Rust、浏览器、独立启动和真实 systemd 检查。
3. 在五个原生目标收集并内嵌许可证，构建程序并运行目标测试：Linux x86_64 / ARM64、Windows x64、macOS Intel / Apple Silicon。Linux 使用 musl，Windows 静态链接 C 运行库，macOS 使用系统库并执行 ad-hoc 签名。
4. 将主程序逐字节复制为对应平台的下载文件，验证 ELF / PE / Mach-O、运行库、路径隐私和 SHA256。在原生目标只复制这个程序到临时目录，验证内嵌网页、完整许可、默认 / 便携启动、初始化、数据权限、API、会话持久化和正常关闭；Linux 另执行 systemd 验证。
5. 汇总五个可执行文件并生成 SHA256SUMS，完整校验要求只有这六个附件。创建或更新草稿，全部上传成功后将草稿发布。

Release 附件只提供可执行文件和校验文件。开发者通过对应 Git 标签获取源码；GitHub 自带的 Source code 入口由 GitHub 提供，工作流不再额外上传源码附件。使用说明在对应标签的 docs/standalone.md 中，完整许可随程序内嵌。

普通检查和构建任务只获得 `contents: read`。仅最终发布任务获得 `contents: write`，使用 GitHub 自动提供的 GITHUB_TOKEN，不需要保存个人访问令牌。外部 Actions 固定到提交 SHA，Dependabot 负责更新。

## 准备版本

同步以下文件中的语义化版本：

- Cargo.toml 和 Cargo.lock。
- web/package.json 和 web/package-lock.json（包括根 package 记录）。
- CHANGELOG.md 中的 `## [X.Y.Z]` 章节。

依赖变化时，在已安装前端依赖的环境执行 `python3 scripts/generate-notices.py`，检查 THIRD_PARTY_NOTICES.md 和 third_party/licenses。默认收集当前 Rust 主机平台，交叉目标使用 `--target TARGET`。正式构建脚本在编译前再次按目标收集许可，并将清单和完整原文内嵌；缺失许可文本会阻止发布。

完成 Pull Request 和 CI 验证后，在对应提交创建版本标签：

```bash
git switch main
git pull --ff-only
git tag -a v0.3.0 -m "Release v0.3.0"
git push origin v0.3.0
```

`v0.3.0` 是示例，已发布版本必须使用新版本号。预发行可使用 `v0.4.0-rc.1`，并同步相应版本字段；包含预发行后缀的标签会发布为 prerelease。

仓库需要启用 GitHub Actions 并允许 GITHUB_TOKEN 创建发行版。原生 runner 使用 `ubuntu-24.04`、`ubuntu-24.04-arm`、`windows-latest`、`macos-15-intel` 和 `macos-15`；仓库 runner 策略需要允许这些标签。五个目标的唯一清单位于 scripts/prepare-release.py，标签元数据生成构建矩阵和准确文件名，完整验证使用同一清单，避免遗漏平台。工作流不会申请自定义服务器或部署生产服务。

macOS 的 ad-hoc 签名不等于 Developer ID 签名或 Apple 公证；当前不需要 Apple 证书或账户凭据。首次下载可能需要用户按系统提示允许程序运行。

## 发行版文件

| 文件 | 内容 |
| --- | --- |
| `mihomo-web-panel-X.Y.Z-linux-x86_64` | x86_64 Linux 主程序 |
| `mihomo-web-panel-X.Y.Z-linux-aarch64` | ARM64 Linux 主程序 |
| `mihomo-web-panel-X.Y.Z-windows-x86_64.exe` | Windows x64 主程序 |
| `mihomo-web-panel-X.Y.Z-macos-x86_64` | Intel Mac 主程序 |
| `mihomo-web-panel-X.Y.Z-macos-aarch64` | Apple Silicon 主程序 |
| `SHA256SUMS` | 以上五个可执行文件的 SHA256 校验值 |

Windows 下载后可直接双击。HTTP 下载的 macOS / Linux 文件需要设置执行权限；这是文件系统权限操作，不改变程序内容或 SHA256。文件名含版本、系统和 CPU，升级可只替换主程序，默认用户目录与便携数据继续保留。

项目 MIT 许可、第三方清单和完整许可原文内嵌到程序；运行 `文件名 licenses` 可查看，不读取配置、不创建数据目录。Rust 发布构建映射源码和工具链路径，发行内容检查禁止私有路径。编译缓存、开发配置、数据库、日志、凭据和源码不作为程序附件发布。Linux 专用 helper 与安装器保留在源码中，完整部署步骤见 [安装说明](installation.md)。

## 手动运行与重试

在 Actions → Release → Run workflow 中输入**已存在**的版本标签。工作流 checkout 和构建该标签，不会自动移动标签或创建不匹配的发行版。

测试、构建或上传失败时，查看失败步骤和 CI artifacts，修复后发布新标签；纯网络或 runner 临时失败可重新运行相同工作流。上传中断留下的草稿可由重试继续完成。已正式发布的发行版不会被覆盖，版本代码变化必须使用新版本标签。

不要强制移动已发布标签，也不要上传未经对应标签 CI 验证的可执行文件。

## 独立准备发行文件

安装前端依赖并构建网页后，在目标原生系统执行：

```bash
python3 scripts/build-release.py --target x86_64-unknown-linux-musl
python3 scripts/prepare-release.py --target x86_64-unknown-linux-musl
python3 scripts/verify-release.py --target x86_64-unknown-linux-musl --write-checksums
python3 tests/standalone_smoke.py --target x86_64-unknown-linux-musl
```

Windows 和 macOS 使用 `--target` 选择各自目标。macOS 构建后、准备发行文件前，需要在原始主程序上执行 `codesign --force --sign -` 和 `codesign --verify --strict`，与工作流一致。

完整发行版校验使用 `python3 scripts/verify-release.py --complete --write-checksums`，要求 dist 中恰好包含五个平台的主程序，并生成统一校验文件。下载后的检查使用 `python3 scripts/verify-release.py --complete`，只验证已有 SHA256SUMS，不覆盖它。发布前检查 `python3 scripts/check-repository.py`；将其他版本或非发行附件存放在 dist 之外。
