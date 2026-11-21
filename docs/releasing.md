# GitHub Releases 发布

## 工作流

[CI](https://github.com/KaguyaJs/mihomo-web-panel/actions/workflows/ci.yml) 在 main 推送、Pull Request 和手动运行时验证源码。它也作为可复用工作流供 [Release](https://github.com/KaguyaJs/mihomo-web-panel/actions/workflows/release.yml) 调用。

Release 由 `v*` 标签推送或手动选择已有版本标签触发，依次执行：

1. 校验标签、Cargo.toml、前端 package / lockfile 和 CHANGELOG.md 版本一致，并确认 checkout 对应标签提交。
2. 执行与 CI 相同的 Linux / Windows / macOS、最低 Rust、浏览器、独立启动和真实 systemd 检查。
3. 在五个原生目标构建程序并运行目标测试：Linux x86_64 / ARM64、Windows x64、macOS Intel / Apple Silicon。Linux 使用 musl，Windows 静态链接 C 运行库，macOS 使用系统库并执行 ad-hoc 签名。
4. 收集许可证、制作五个二进制包和完整源码归档，验证 ELF / PE / Mach-O、依赖库、归档内容、权限、路径隐私和 SHA256。在原生目标实际解压发行包，验证默认 / 便携启动、初始化、数据权限、API、会话持久化和正常关闭；Linux 另执行 systemd 验证。
5. 汇总发行版文件，创建或更新草稿，全部上传成功后将草稿发布。

普通检查和构建任务只获得 `contents: read`。仅最终发布任务获得 `contents: write`，使用 GitHub 自动提供的 GITHUB_TOKEN，不需要保存个人访问令牌。外部 Actions 固定到提交 SHA，Dependabot 负责更新。

## 准备版本

同步以下文件中的语义化版本：

- Cargo.toml 和 Cargo.lock。
- web/package.json 和 web/package-lock.json（包括根 package 记录）。
- CHANGELOG.md 中的 `## [X.Y.Z]` 章节。

依赖变化时，在已安装前端依赖的环境执行 `python3 scripts/generate-notices.py`，检查 THIRD_PARTY_NOTICES.md 和 third_party/licenses。发布工作流会在构建环境再次收集依赖许可；缺失许可文本会阻止发布。

完成 Pull Request 和 CI 验证后，在对应提交创建版本标签：

```bash
git switch main
git pull --ff-only
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

`v0.2.0` 是示例，已发布版本必须使用新版本号。预发行可使用 `v0.2.0-rc.1`，并同步相应版本字段；包含预发行后缀的标签会发布为 prerelease。

仓库需要启用 GitHub Actions 并允许 GITHUB_TOKEN 创建发行版。原生 runner 使用 `ubuntu-24.04`、`ubuntu-24.04-arm`、`windows-latest`、`macos-15-intel` 和 `macos-15`；仓库 runner 策略需要允许这些标签。五个目标的唯一清单位于 scripts/package-release.py，标签元数据生成构建矩阵，完整验证使用同一清单，避免遗漏平台。工作流不会申请自定义服务器或部署生产服务。

macOS 的 ad-hoc 签名不等于 Developer ID 签名或 Apple 公证；当前不需要 Apple 证书或账户凭据。首次下载可能需要用户按系统提示允许程序运行。

## 发行版文件

| 文件 | 内容 |
| --- | --- |
| `mihomo-web-panel-X.Y.Z-linux-x86_64.tar.gz` | x86_64 主程序、helper、安装脚本、部署配置和文档 |
| `mihomo-web-panel-X.Y.Z-linux-aarch64.tar.gz` | ARM64 主程序及相同部署文件 |
| `mihomo-web-panel-X.Y.Z-windows-x86_64.zip` | Windows x64 主程序、配置示例、文档和许可证 |
| `mihomo-web-panel-X.Y.Z-macos-x86_64.tar.gz` | Intel Mac 主程序与 Start.command |
| `mihomo-web-panel-X.Y.Z-macos-aarch64.tar.gz` | Apple Silicon 主程序与 Start.command |
| `mihomo-web-panel-X.Y.Z-source.tar.gz` | Rust、Vue、测试、迁移、锁文件、文档和许可证 |
| `SHA256SUMS` | 以上归档的 SHA256 校验值 |

主程序位于二进制归档根目录；Linux helper 也在根目录，安装器继续兼容早期 bin/ 布局。每份归档内部另有 SHA256SUMS，供解压后逐文件校验。归档时间使用标签提交的时间；tar 用户与组固定为 root，ZIP 不保存用户账户。Rust 发布构建映射源码和工具链路径，归档和可执行内容检查禁止私有路径。源码归档排除编译缓存、临时工具、开发配置、数据库、日志和凭据。

## 手动运行与重试

在 Actions → Release → Run workflow 中输入**已存在**的版本标签。工作流 checkout 和构建该标签，不会自动移动标签或创建不匹配的发行版。

测试、构建或上传失败时，查看失败步骤和 CI artifacts，修复后发布新标签；纯网络或 runner 临时失败可重新运行相同工作流。上传中断留下的草稿可由重试继续完成。已正式发布的发行版不会被覆盖，版本代码变化必须使用新版本标签。

不要强制移动已发布标签，也不要上传未经对应标签 CI 验证的可执行文件。

## 独立打包

原生或交叉编译目标程序之后：

```bash
python3 scripts/generate-notices.py --target x86_64-unknown-linux-musl
python3 scripts/package-release.py --target x86_64-unknown-linux-musl --binary-only
python3 scripts/package-release.py --source-only
python3 scripts/verify-archives.py --architecture x86_64 --require-source
```

Windows 和 macOS 使用 `--target` 选择各自目标；实际构建使用 `python3 scripts/build-release.py --target TARGET`。完整发行版验证使用 `python3 scripts/verify-archives.py --complete`，要求五个二进制包与源码都存在。发布前检查 `python3 scripts/check-repository.py`。
