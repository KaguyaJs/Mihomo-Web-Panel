# 直接运行二进制程序

从 [GitHub Releases](https://github.com/KaguyaJs/mihomo-web-panel/releases/latest) 下载对应平台的发行包和 `SHA256SUMS`。程序已内嵌网页、SQLite 和必要的应用依赖，运行时不需要 Rust、Node.js、npm、Python 或额外的 Web 服务器。已有 Mihomo 的 Controller 必须由原有方式保持运行。

| 系统 | CPU / 最低系统 | 发行包后缀 | 启动入口 |
| --- | --- | --- | --- |
| Windows | x64，Windows 10 / Server 2016 或更新版本 | `windows-x86_64.zip` | `mihomo-web-panel.exe` |
| macOS | Apple Silicon，macOS 13 或更新版本 | `macos-aarch64.tar.gz` | `Start.command` 或 `mihomo-web-panel` |
| macOS | Intel，macOS 13 或更新版本 | `macos-x86_64.tar.gz` | `Start.command` 或 `mihomo-web-panel` |
| Linux | x64 | `linux-x86_64.tar.gz` | `mihomo-web-panel` |
| Linux | ARM64 | `linux-aarch64.tar.gz` | `mihomo-web-panel` |

Windows 包静态链接 Microsoft C 运行库；无需单独安装 Visual C++ Redistributable。macOS 使用系统自带库，Linux 使用 musl 静态程序。当前不提供 Windows ARM64 专用版本。

私有仓库需要相应访问权限，可使用已登录的 GitHub CLI。以下以 Windows v0.2.0 为例：

```bash
gh release download v0.2.0 --repo KaguyaJs/mihomo-web-panel \
  --pattern 'mihomo-web-panel-0.2.0-windows-x86_64.zip' --pattern SHA256SUMS
```

## Windows

解压 ZIP，双击 `mihomo-web-panel.exe`。程序显示本地控制台，首次运行会显示一次性初始化令牌，并在交互式启动时打开浏览器。也可以在 PowerShell 运行：

```powershell
Get-FileHash .\mihomo-web-panel-0.2.0-windows-x86_64.zip -Algorithm SHA256
Expand-Archive .\mihomo-web-panel-0.2.0-windows-x86_64.zip -DestinationPath .\panel
cd .\panel\mihomo-web-panel-0.2.0-windows-x86_64
.\mihomo-web-panel.exe
```

将 `Get-FileHash` 的结果与 `SHA256SUMS` 中同名文件的值比较。正常运行无需管理员权限；使用支持 ACL 的文件系统存放私密数据。

## macOS

Apple Silicon 示例：

```bash
shasum -a 256 mihomo-web-panel-0.2.0-macos-aarch64.tar.gz
tar -xzf mihomo-web-panel-0.2.0-macos-aarch64.tar.gz
cd mihomo-web-panel-0.2.0-macos-aarch64
./mihomo-web-panel
```

将校验值与 `SHA256SUMS` 中同名文件比较。Finder 用户可双击 `Start.command`，它在 Terminal 中保留启动控制台并打开浏览器。Intel Mac 选择 `macos-x86_64` 包。不要使用 sudo 启动主程序。

macOS 程序使用 ad-hoc 签名，尚未使用 Apple Developer ID 签名和公证。首次启动可能被 Gatekeeper 拦截；核对下载来源和校验值后，按 macOS 提示在“系统设置 → 隐私与安全性”中允许该程序。自动化测试验证的是实际程序运行，不代表通过 Apple 公证。

## Linux

```bash
sha256sum --ignore-missing -c SHA256SUMS
tar -xzf mihomo-web-panel-0.2.0-linux-x86_64.tar.gz
cd mihomo-web-panel-0.2.0-linux-x86_64
./mihomo-web-panel
```

使用普通用户运行。带桌面环境时程序可以打开默认浏览器；没有桌面的机器可通过 `--no-open` 保持控制台运行。长期服务器部署可使用发行包内的 systemd 安装器，详见 [Linux 部署](deployment-guide.md)。

## 初始化与日常启动

默认入口为 `http://127.0.0.1:8787`，仅本机可访问。将控制台显示的一次性令牌填入浏览器初始化向导，再设置管理员、已有 Controller 地址与 Secret。Controller 测试成功后完成初始化。以后启动保留管理员、后端连接、会话和网页设置。

关闭浏览器不会停止面板。按启动终端的 Ctrl+C 停止程序；更新时先停止旧进程，再替换可执行文件。一个数据目录只能运行一个实例，8787 被占用时程序会报告错误。

若启动时的输出被重定向，程序不会自动打开浏览器或向日志写入令牌。可在另一个终端通过相同的参数读取令牌：

```bash
./mihomo-web-panel bootstrap-token
```

Windows 将可执行名替换为 `.\mihomo-web-panel.exe`。初始化完成后，此命令不会再返回已消费的令牌。

## 配置与数据位置

默认配置和数据库存放在当前用户的应用目录，独立于程序位置和当前工作目录：

| 平台 | 配置文件 | 数据目录 |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\mihomo-web-panel\config.toml` | `%LOCALAPPDATA%\mihomo-web-panel\data` |
| macOS | `$HOME/Library/Application Support/mihomo-web-panel/config.toml` | 同目录的 `data` |
| Linux | `$XDG_CONFIG_HOME/mihomo-web-panel/config.toml`，默认 `$HOME/.config/...` | `$XDG_DATA_HOME/mihomo-web-panel`，默认 `$HOME/.local/share/...` |

使用 `paths` 查看实际配置和数据位置。默认生成的文件使用回环 HTTP 安全配置。公网访问需要另外配置 HTTPS 反向代理及 Secure Cookie，不能把默认 HTTP 配置直接改成公开监听；见 [配置](configuration.md)。

便携模式将 `config.toml` 和 `data` 放在可执行文件旁边：

```bash
./mihomo-web-panel --portable
./mihomo-web-panel --portable paths
./mihomo-web-panel --portable bootstrap-token
```

升级便携版本时保留 `config.toml`、`config.lock` 和整个 `data` 目录，只替换程序与发行文档。数据库和备份包含 Controller Secret，保护其访问权限。macOS 的启动脚本默认使用用户模式，便携模式使用上述终端命令。

`--config` 加载**已存在**的配置文件。配置中的相对 `data_dir` 相对配置文件所在目录解析。`--portable` 和 `--config` 互斥；指定文件缺失或无效时程序会报错，不自动替换它。

```bash
./mihomo-web-panel --config config.example.toml --no-open serve
./mihomo-web-panel --open
./mihomo-web-panel --no-open
./mihomo-web-panel --help
```

无参数交互式启动自动打开浏览器；`serve`、重定向输出或 `--no-open` 适合脚本运行，`--open` 可以显式打开浏览器。

## 平台功能范围

三个系统都支持认证、初始化、节点选择与测速、代理模式、连接、流量、日志和设置。面板始终管理已有 Mihomo，不安装或启动内核。

网页中的开机自启控制目前只支持已安装专用 helper 的 Linux systemd 服务。Windows 和 macOS 直接运行模式会显示该操作不可用，其他管理功能正常；此版本不提供 Windows 服务或 launchd 安装器。
