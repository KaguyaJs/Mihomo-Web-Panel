# 安全模型

## 网络边界

公网只发布 HTTPS Web 入口。Mihomo Controller 监听回环或受限私网；平台通常监听回环，由 Caddy / Nginx 转发。必须同时检查 IPv4、IPv6、防火墙和云安全组。

平台仅实现白名单功能：版本、代理列表与 Selector 选择、固定 URL 测速、模式、连接和流量/日志。没有任意路径转发、任意 URL 请求、内核启动、下载、重启、升级或配置文件写入接口。Mihomo 的 `/configs` 响应只投影 `mode`，不会转发 Secret、Controller 或其他敏感配置。

Controller 必须为经过验证的 IP 字面量。默认仅回环；可信 RFC1918 / ULA CIDR 必须在本地配置显式开放。拒绝公网、未指定、组播、链路本地、IPv4 映射 IPv6、DNS 名称及云元数据地址。Reqwest 禁止重定向和环境代理；WebSocket 使用固定地址、固定路径和 Authorization 头，不通过查询传递 Secret。

## 初始化与认证

随机令牌及 Session 使用系统随机源生成 256 位随机值。首次令牌只通过受限文件和 CLI 提供，不写入启动日志。初始化接口先验证令牌与连接，再通过条件更新及单次事务消费令牌和创建唯一管理员；不能重新注册覆盖管理员。

密码使用 Argon2id：19 MiB 内存、2 次迭代、1 路并行、随机 128 位盐。密码长度 12–128 个字符，哈希与验证移到有界 blocking 池，避免阻塞异步运行时。

Session Cookie 使用 HttpOnly、SameSite=Strict、Path=/；生产 HTTPS 必须 Secure。数据库仅持有 SHA-256 摘要，不保存可直接登录的令牌。默认空闲 30 分钟、绝对 24 小时；最多保留 20 个有效会话。流量、日志与后台状态轮询不延长空闲寿命；前端在人为交互时至多每分钟发送一次认证状态请求。

账户更新验证原密码，通过比较旧哈希的条件写入避免并发覆盖，并轮换当前 Session、撤销其他会话。注销可撤销当前、其他或全部会话。已有 WebSocket 每 5 秒复查 Session，撤销/过期后关闭；慢客户端发送也有超时。

## 请求验证

- 所有管理 API 与 WebSocket 都先检查 Session。
- Host 必须与固定 `public_origin` 匹配，阻止 DNS rebinding；禁止多重 Host / Origin。
- 写操作及 WebSocket 要求同源 Origin；写操作额外需要绑定 Session 的 CSRF token。
- CORS 默认关闭，不信任跨站请求。
- 只有实际 TCP peer 位于 `trusted_proxies` 中，才从 X-Forwarded-For 右侧剥离可信代理获取客户端 IP。未列出的客户端头不能伪造限流身份。
- 登录 IP 20 次/分钟、用户名 10 次/15 分钟，8 次失败临时锁定 5 分钟；失败渐进延迟。初始化与登录路由额外按 IP 限制 30 次/分钟。
- 普通读取 900 次/分钟、写入 180 次/分钟；限流桶有上限并清理过期条目。
- 请求体最大 32 KiB，上游响应最大 8 MiB，WebSocket 消息最大 64 KiB。

## 浏览器与错误

使用 CSP、自源脚本与样式、frame-ancestors none、nosniff、no-referrer、Permissions-Policy、HTTPS HSTS。Vue 文本插值输出，不使用 `v-html` 渲染内核日志或节点名称。图片、图标、字体不从第三方地址加载；只有主题偏好写入 localStorage。

错误使用平台固定代码与友好提示，不返回上游原始错误、请求 URL、Secret、文件路径或栈。投影后的上游字符串与日志会脱敏已保存的 Secret 和 Controller 地址。日志条目长度有界，不会在普通日志中输出 Cookie、认证体或上游 Authorization。

## systemd 最小权限

主程序在 Linux 和 macOS 上拒绝 root 身份运行。Linux unit 使用独立 `mihomo-web` 用户、空 capability、NoNewPrivileges、ProtectSystem=strict、ProtectHome、PrivateTmp、有限地址族、系统调用过滤及唯一持久可写目录。

root helper 独立运行，通过 root:mihomo-web 0660 Socket 接收消息，并检查操作系统提供的 peer UID。应用还验证 helper peer 是 root。协议只接受 `enable`、`disable`、`status`，且目标 unit 固定为 `mihomo-web.service`。执行 `/usr/bin/systemctl` 的固定参数数组，不使用 shell、用户输入命令或 NOPASSWD。

本地 root、拥有平台数据目录权限的用户与已认证管理员属于受信任的权限主体。平台不是对已失陷服务器的防护；数据库与备份包含内核 Secret，因此必须保护服务器账户、文件与备份权限。

独立启动默认仅监听回环 HTTP，配置和数据存于当前用户的应用目录。Linux / macOS 私密目录权限为 0700、私密文件为 0600；Windows 数据目录和私密文件禁用继承共享访问，仅保留当前用户、Administrators 与 SYSTEM。便携模式同样保护数据目录。Windows 使用原生 Win32 ACL API，不执行字符串拼接的权限命令。

一次性初始化令牌仅在交互式独立启动控制台或显式 `bootstrap-token` 命令中显示，不进入服务日志和默认重定向输出。浏览器启动只提交经过校验的 HTTP(S) 入口，不携带令牌或 Controller Secret。

## 安全部署检查

安装后执行以下真实检查：

```bash
sudo ss -lntp
sudo ufw status verbose
sudo systemctl show mihomo-web.service -p User -p NoNewPrivileges -p MainPID
sudo systemd-analyze security mihomo-web.service
sudo stat -c '%a %U:%G %n' /etc/mihomo-web/config.toml /var/lib/mihomo-web /var/lib/mihomo-web/app.db
```

从外网确认只能访问 HTTPS 入口，无法连接 8787 / 9090，包含 IPv6 测试。具体部署和验证边界见测试报告。
