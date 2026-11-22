# 直接运行二进制程序

从 [GitHub Releases](https://github.com/KaguyaJs/mihomo-web-panel/releases/latest) 直接下载对应平台的可执行文件和 `SHA256SUMS`。程序内嵌网页、SQLite、项目许可和第三方许可文本，运行时不需要 Rust、Node.js、npm、Python 或额外的 Web 服务器。已有 Mihomo 的 Controller 必须由原有方式保持运行。

| 系统 | CPU / 最低系统 | 可执行文件后缀 | 启动方式 |
| --- | --- | --- | --- |
| Windows | x64，Windows 10 / Server 2016 或更新版本 | `windows-x86_64.exe` | 双击 `.exe` |
| macOS | Apple Silicon，macOS 13 或更新版本 | `macos-aarch64` | 设置执行权限后在 Terminal 启动 |
| macOS | Intel，macOS 13 或更新版本 | `macos-x86_64` | 设置执行权限后在 Terminal 启动 |
| Linux | x64 | `linux-x86_64` | 设置执行权限后运行 |
| Linux | ARM64 | `linux-aarch64` | 设置执行权限后运行 |

Windows 包静态链接 Microsoft C 运行库；无需单独安装 Visual C++ Redistributable。macOS 使用系统自带库，Linux 使用 musl 静态程序。当前不提供 Windows ARM64 专用版本。

私有仓库需要相应访问权限，可使用已登录的 GitHub CLI。以下以 Windows v0.3.0 为例：

```bash
gh release download v0.3.0 --repo KaguyaJs/mihomo-web-panel \
  --pattern 'mihomo-web-panel-0.3.0-windows-x86_64.exe' --pattern SHA256SUMS
```

## Windows

下载后双击 `mihomo-web-panel-0.3.0-windows-x86_64.exe`。程序显示本地控制台，首次运行会显示一次性初始化令牌，并在交互式启动时打开浏览器。也可以在 PowerShell 校验并启动：

```powershell
Get-FileHash .\mihomo-web-panel-0.3.0-windows-x86_64.exe -Algorithm SHA256
.\mihomo-web-panel-0.3.0-windows-x86_64.exe
```

将 `Get-FileHash` 的结果与 `SHA256SUMS` 中同名文件的值比较。正常运行无需管理员权限；使用支持 ACL 的文件系统存放私密数据。

## macOS

Apple Silicon 示例：

```bash
shasum -a 256 mihomo-web-panel-0.3.0-macos-aarch64
chmod +x mihomo-web-panel-0.3.0-macos-aarch64
./mihomo-web-panel-0.3.0-macos-aarch64 --open
```

将校验值与 `SHA256SUMS` 中同名文件比较。Intel Mac 选择后缀为 `macos-x86_64` 的程序。通过 Terminal 运行并保留启动控制台；不要使用 sudo 启动主程序。

macOS 程序使用 ad-hoc 签名，尚未使用 Apple Developer ID 签名和公证。首次启动可能被 Gatekeeper 拦截；核对下载来源和校验值后，按 macOS 提示在“系统设置 → 隐私与安全性”中允许该程序。自动化测试验证的是实际程序运行，不代表通过 Apple 公证。

## Linux

```bash
sha256sum --ignore-missing -c SHA256SUMS
chmod +x mihomo-web-panel-0.3.0-linux-x86_64
./mihomo-web-panel-0.3.0-linux-x86_64
```

使用普通用户运行。带桌面环境时程序可以打开默认浏览器；没有桌面的机器可通过 `--no-open` 保持控制台运行。完整 systemd 服务及专用 helper 从对应源码标签构建，详见 [Linux 安装](installation.md)。

核对校验值后，可以把文件重命名为 `mihomo-web-panel`，Windows 为 `mihomo-web-panel.exe`。以下通用命令使用这个简短名称；重命名不改变默认用户数据目录。

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

升级便携版本时保留 `config.toml`、`config.lock` 和整个 `data` 目录，只替换可执行文件。数据库和备份包含 Controller Secret，保护其访问权限。用户模式下，程序可以移动或改名，配置和数据继续存放在原来的应用目录。

`--config` 加载**已存在**的配置文件。配置中的相对 `data_dir` 相对配置文件所在目录解析。`--portable` 和 `--config` 互斥；指定文件缺失或无效时程序会报错，不自动替换它。

```bash
./mihomo-web-panel --config config.example.toml --no-open serve
./mihomo-web-panel --open
./mihomo-web-panel --no-open
./mihomo-web-panel --help
./mihomo-web-panel licenses
```

无参数交互式启动自动打开浏览器；`serve`、重定向输出或 `--no-open` 适合脚本运行，`--open` 可以显式打开浏览器。

`licenses` 输出内嵌的项目许可、第三方清单及完整许可原文，不读取配置、不初始化数据，也不连接 Controller。文档与配置示例在对应版本的 GitHub 源码中查看。

## 平台功能范围

三个系统都支持认证、初始化、节点选择与测速、代理模式、连接、流量、日志和设置。面板始终管理已有 Mihomo，不安装或启动内核。

网页中的开机自启控制目前只支持已安装专用 helper 的 Linux systemd 服务。Windows 和 macOS 直接运行模式会显示该操作不可用，其他管理功能正常；此版本不提供 Windows 服务或 launchd 安装器。
