# 测试与验收

测试结果以 [GitHub Actions](https://github.com/KaguyaJs/mihomo-web-panel/actions) 中对应提交的运行记录为准。构建日志、浏览器 trace 和测试报告作为 CI artifacts 提供，不在源码仓库保存开发机器的运行记录。

## 覆盖范围

| 测试 | 验证内容 |
| --- | --- |
| Rust 核心 | Argon2id、随机令牌、摘要 Session、过期和撤销、初始化、配置、限流、Cookie、Origin / Host、SSRF、helper 白名单 |
| Rust 集成与安全 | 实际 Mock REST / WebSocket、节点选择、测速、模式、连接、响应投影、错误与超时、CSRF、HTTP 413、会话轮换及撤销 |
| 前端单元 | 初始化、登录、节点操作、账户和服务设置、API 状态处理 |
| Playwright | 首次配置、登录、实际 API 选择和测速、凭据隔离、设置、密码、移动布局、主题、日志和离线状态 |
| systemd | 安装、非 root 主进程、Socket 激活、UID 检查、固定命令、自启、升级保留配置和默认卸载保留数据 |
| 发布工具 | 静态 ELF 和架构、版本标签、完整归档、文件权限、SHA256、可重复归档及源码隔离 |
| 仓库校验 | 文档链接、版本一致性、禁止的本地路径和凭据、运行数据隔离 |

## 开发验证

在仓库根目录执行：

```bash
npm --prefix web ci
npm --prefix web run build
npm --prefix web test
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo build --bins --examples --locked
python3 scripts/check-repository.py
python3 -m unittest discover -s tests/python -v
cd web
npx playwright install chromium
npm run test:e2e
```

Linux CI 使用 `npx playwright install --with-deps chromium` 安装所需系统库。Playwright 创建独立临时数据库，并启动测试 Mock。测试夹具中的地址和 Secret 属于公开模拟数据，不连接真实 Controller。

默认测试不修改文档截图。需要重新生成预览时，在 `web` 目录运行 `UPDATE_SCREENSHOTS=1 npm run test:e2e`，检查生成的桌面和移动端截图后提交。

## systemd 验证

`tests/systemd_smoke.py` 必须在一次性 Linux/systemd VM 或 CI runner 执行；它会安装并卸载本平台。

```bash
cargo build --release --bins --examples --locked
sudo python3 tests/systemd_smoke.py
```

使用自定义 target 时，通过 `MIHOMO_SMOKE_BIN_DIR` 和 `MIHOMO_SMOKE_MOCK` 指定该架构的主程序、helper 和 Mock。成功结果包含 `SYSTEMD_VALIDATION_PASSED`。

测试通过 Web API 请求真实 root helper，再执行真实 systemctl。它确认 enable/disable 改变 `is-enabled` 状态，当前 Web 服务继续运行；检查 UID、NoNewPrivs 和文件权限；拒绝 root、其他 UID、未知操作和额外 unit 参数；检查升级后的账户、后端与 Session；检查卸载后配置与数据库保留。

## CI 与发行版

CI 在 Linux 和 Windows 运行核心检查，并对声明的最低 Rust 版本进行编译验证。Release 使用同一测试工作流，在 x86_64 和 ARM64 原生 runner 构建 musl 静态程序，执行目标平台测试及真实 systemd 验证后才发布文件。

发布失败时，GitHub Actions 提供对应阶段的日志；浏览器失败保留截图和 trace。流程说明见 [发布文档](releasing.md)。

## 生产验收范围

自动化测试使用符合官方 API 形状的独立 Mock。真实 Mihomo 版本差异、公网 IPv6 路由、HTTPS 证书和反向代理需要按 [部署指南](deployment-guide.md) 在实际环境验收。测试不等同于所有内核版本兼容性认证、长期压力测试或外部渗透测试。
