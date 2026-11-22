# 更新记录

版本采用语义化版本号；Git 标签使用 `vX.Y.Z`。

## [0.3.0]

- Release 直接提供五个平台的主程序可执行文件和 SHA256SUMS，下载后即可运行。
- 重写发行文件准备、完整性检查和发布流程；验证实际上传的单个可执行文件。
- 项目许可、第三方清单和完整许可文本内嵌到程序，使用 `licenses` 命令查看，无需初始化或配置文件。
- 许可证在编译前按目标平台收集；发行校验拒绝错误架构、额外运行库、私有路径和非预期附件。
- 更新 Windows / macOS / Linux 的直接下载、执行权限、持久化与升级说明；Linux 完整 systemd/helper 部署从源码构建。

## [0.2.0]

- 增加解压后直接运行的 Windows x64、macOS Intel / Apple Silicon 和 Linux x64 / ARM64 可执行版本。
- 首次启动自动创建用户配置和数据目录，支持 `--portable`、`paths`、`--open` 和 `--no-open`。
- 交互式本地启动显示一次性初始化令牌并打开浏览器；重定向输出及服务日志不包含令牌。
- Windows 静态链接 C 运行库，数据目录和私密文件使用当前用户、Administrators 和 SYSTEM 的受限 ACL。
- macOS 包含 Finder / Terminal 启动入口；程序仅依赖系统自带库。
- 相对数据目录按配置文件位置解析，切换工作目录或升级程序保留配置和数据。
- CI 增加 macOS 和真实独立启动、初始化及持久化检查；发布验证五个平台的可执行格式、归档和 SHA256。

## [0.1.0]

- Rust 服务端认证、一次性初始化令牌、Session、CSRF 与登录限流。
- Mihomo 代理组、节点选择、延迟测试、代理模式、连接和实时流量 / 日志。
- Vue 桌面及移动界面，主题切换和账户 / 服务 / 后端设置。
- 非 root Linux 服务、固定权限 helper、systemd 自启控制和安装 / 卸载工具。
- Linux x86_64 / ARM64 静态发布包，源码归档、SHA256 及自动 GitHub Releases 工作流。
- Linux / Windows CI、最低 Rust 编译检查、浏览器及真实 systemd 验证。

[0.1.0]: https://github.com/KaguyaJs/mihomo-web-panel/releases/tag/v0.1.0
[0.2.0]: https://github.com/KaguyaJs/mihomo-web-panel/releases/tag/v0.2.0
[0.3.0]: https://github.com/KaguyaJs/mihomo-web-panel/releases/tag/v0.3.0
