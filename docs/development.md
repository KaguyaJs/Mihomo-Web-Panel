# 开发与构建

## 环境

- Rust 1.92+；`rust-toolchain.toml` 默认选择 Stable 并包含 rustfmt、Clippy。
- Node.js 24 LTS 的最新补丁版本，npm 11+。
- Python 3.11+，用于仓库校验、发布打包和 systemd 测试。
- Linux 的 C 编译器、Windows 的 MSVC Build Tools，或 macOS 的 Xcode Command Line Tools。

Cargo.lock 和 web/package-lock.json 必须提交。安装依赖使用 `npm ci`，Cargo 构建使用 `--locked`。发布的可执行文件不需要 Rust、Node.js 或外部 SQLite 服务。

## 目录

```text
src/                    Rust API、认证、数据存储、流式转发及 helper
migrations/             SQLite 编号迁移
web/src/                Vue 页面、组件和 API 客户端
web/e2e/                Playwright 浏览器流程
tests/                  核心、集成、安全、打包及 systemd 测试
examples/mock_mihomo.rs  独立测试 Mock
deploy/                 配置、systemd、Caddy 与 Nginx 示例
scripts/                安装、卸载、校验、许可证和打包工具
docs/                   中文文档和界面截图
.github/                CI、Release、依赖更新与协作模板
```

构建产物、开发配置、数据库、凭据和临时测试数据已在 .gitignore 中排除。

`src/runtime.rs` 管理独立启动、用户数据目录、便携模式和浏览器启动；`src/windows.rs` 通过 Win32 API 保护私密文件 ACL。`.cargo/config.toml` 为 Windows x64 配置静态 C 运行库。

## 启动服务

在仓库根目录执行：

```bash
npm --prefix web ci
npm --prefix web run build
cp config.example.toml config.toml
cargo run --locked -- --config config.toml serve
```

另开终端执行 `cargo run --locked -- --config config.toml bootstrap-token`。打开 `http://127.0.0.1:8787`，连接已有 Controller。开发 HTTP 配置只用于回环访问。

PowerShell 使用 `Copy-Item config.example.toml config.toml`；必要时显式调用 `npm.cmd`。Linux 生产主服务拒绝 root，systemd 测试单独以 root 执行安装器和 helper。

## Vite 热更新

开发 TOML 中设置：

```toml
listen = "127.0.0.1:8787"
public_origin = "http://127.0.0.1:5173"
secure_cookies = false
```

启动 Rust 服务和 `npm --prefix web run dev`，通过 5173 访问。Vite 代理保留同源 Host / Origin，管理 API 仍进行认证和 CSRF 校验。

生产先构建前端，再编译 Rust release。前端资源由 RustEmbed 编译进主程序，更新界面后需要重建可执行文件。

## 测试

命令和覆盖范围见 [测试与验收](test-report.md)。浏览器测试由 `scripts/e2e-server.mjs` 启动独立 Mock 和 Web 服务，每次使用新的临时数据目录。

测试夹具是公开、固定的模拟数据，不是生产凭据。测试不会启动、安装或修改真实 Mihomo。systemd 测试会安装并卸载平台，只能在专用测试 VM 或 CI runner 执行。

## Linux 静态构建

Debian / Ubuntu 的原生 x86_64 构建：

```bash
sudo apt-get update
sudo apt-get install -y musl-tools
rustup target add x86_64-unknown-linux-musl
npm --prefix web ci
npm --prefix web run build
CC=musl-gcc CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc \
  cargo build --release --locked --target x86_64-unknown-linux-musl --bins
```

ARM64 主机使用 `aarch64-unknown-linux-musl` 和 `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER`。Release 工作流分别在对应的原生 Linux runner 构建并运行测试。

其他平台交叉编译可使用 cargo-zigbuild 和 Zig；编译结果仍需在目标架构运行验证。发布包中的主程序和 helper 都必须为静态 ELF。

## Windows 与 macOS 原生构建

安装前端依赖并构建网页后，在相应系统运行：

```bash
# Windows x64
python scripts/build-release.py --target x86_64-pc-windows-msvc
python scripts/generate-notices.py --target x86_64-pc-windows-msvc
python scripts/package-release.py --target x86_64-pc-windows-msvc --binary-only
python tests/standalone_smoke.py --target x86_64-pc-windows-msvc
```

macOS Intel 使用 `x86_64-apple-darwin`，Apple Silicon 使用 `aarch64-apple-darwin`。macOS 的最低部署版本为 13.0，发布工作流执行 ad-hoc 签名和验证。构建脚本重新映射源码及工具链路径，运行原生目标测试；Windows 包拒绝依赖额外 C/C++ 运行库，macOS 包拒绝非系统动态库。

独立启动的终端输出包含实际配置和数据路径，属于运行时信息；这些生成文件不能提交或打入发行包。无参数启动、`--portable` 与命令行使用见 [直接运行说明](standalone.md)。

## 打包与维护

许可证收集、源码和部署归档及版本标签流程见 [发布文档](releasing.md)。所有归档内部包含 SHA256SUMS，发行版另提供归档级校验值。

新增管理功能必须使用明确 API 路由、Session、CSRF、上游白名单和数据投影。数据库变更新增迁移，不修改已执行的迁移、不清空已有数据。Controller Secret、Cookie、密码和完整上游错误不得写入响应或日志。
