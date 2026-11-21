# 配置与数据存储

无参数运行会在当前用户的应用目录生成配置和数据库。`--portable` 使用可执行文件旁的配置与数据；位置和参数见 [直接运行说明](standalone.md)。使用 `--config /path/config.toml` 加载已存在的 TOML，配置中的相对 `data_dir` 相对配置文件所在目录解析。开发环境见 `config.example.toml`；生产见 `deploy/config.production.toml`。

| 字段 | 默认值 | 说明 |
| --- | --- | --- |
| `listen` | `127.0.0.1:8787` | 非 root 端口 1024–65535；IPv6 写为 `[::1]:8787` 或 `[::]:8787` |
| `public_origin` | `http://127.0.0.1:8787` | 浏览器访问的完整 Origin，无路径、尾部斜杠、查询或凭据 |
| `data_dir` | `./data` | 安装后为 `/var/lib/mihomo-web`；建议绝对路径 |
| `secure_cookies` | `false` | 仅回环 HTTP 开发可关闭；公网必须 true + HTTPS |
| `trusted_proxies` | `[]` | 显式代理 CIDR 白名单，生产 Caddy 示例仅信任回环 |
| `trusted_backend_networks` | `[]` | 除回环外允许的特定 RFC1918 / ULA 网段 |
| `session_idle_seconds` | `1800` | 300–86400，且不能超过绝对寿命 |
| `session_absolute_seconds` | `86400` | 3600–604800；创建 Session 时固定其绝对到期时间 |
| `traffic_interval_ms` | `1000` | 500–5000；不会使上游采样频率超过内核能力 |
| `request_timeout_seconds` | `10` | 1–30，上游 HTTP 总超时；连接超时 3 秒 |
| `delay_test_url` | gstatic HTTPS generate_204 | 固定测试目标；浏览器不能提交自己的 URL |
| `helper_socket` | `/run/mihomo-web-helper/control.sock` | 本机辅助程序 Socket |

## 内核连接配置

通过初始化向导或设置页写入 SQLite `backend` 表。只接受 IP 字面量的 HTTP/HTTPS 根地址。默认允许 IPv4 / IPv6 回环；域名、用户信息、查询、片段和路径被拒绝。

跨内网连接示例：在由服务器管理员维护的 TOML 中加入精确白名单：

```toml
trusted_backend_networks = ["10.20.30.40/32", "fd12:3456::40/128"]
```

随后重启本平台，在网页输入该机器的 Controller 地址。任何公网、链路本地、云元数据或 IPv4 映射 IPv6 地址仍被拒绝；明确阻止 `fd00:ec2::254`。建议优先回环部署。远端 Controller 的防火墙只允许平台所在服务器访问。

Secret 仅存入受文件权限保护的数据库；它不提供“读取旧 Secret”的 API。更换连接必须重新输入地址与 Secret，测试成功后由事务替换。失败保留原配置。上游 HTTPS 验证证书，不能关闭校验。

## 网页服务设置

网页修改的设置保存于 SQLite `web_settings`，启动时覆盖 TOML 中同名的可编辑字段。监听与公开 Origin 只在重启后生效；运行中的入口不会被立即重绑定。空闲超时和流量发送间隔动态生效，绝对寿命用于新建 Session。

本地可信代理、私网白名单、数据目录、Secure Cookie 策略和测速 URL 不可从网页修改。更改本地信任策略时，也要确认已保存的 Controller 仍位于新策略范围内。

无法访问新监听入口时恢复到本地 TOML：

```bash
sudo systemctl stop mihomo-web.service
sudo -u mihomo-web /usr/local/bin/mihomo-web-panel \
  --config /etc/mihomo-web/config.toml reset-web-settings
sudo systemctl start mihomo-web.service
```

该命令需要停止服务并获取独占数据锁。它仅删除网页服务设置，保留管理员、密码、内核连接与迁移记录。

## 持久化

SQLite WAL + FULL synchronous、5 个连接、5 秒 busy timeout、SQLx 编号迁移。初始化使用一次事务创建管理员、保存内核并消费令牌；配置存储使用事务。启动时恢复既有配置，升级不清空数据库。

Session 数据库只保存令牌摘要。Linux / macOS 初始化令牌文件 0600、数据目录 0700；Windows 私密文件和数据目录使用禁止继承共享权限的 ACL，仅当前用户、Administrators 和 SYSTEM 可访问。成功消费后令牌文件删除，数据库已消费状态立即生效。Linux 服务日志交给 journald，直接运行模式使用控制台。仅交互式独立启动显示一次性令牌，重定向输出和服务日志不包含令牌。
