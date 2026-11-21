# 故障排查

## 页面返回 403

检查 `public_origin` 是否与浏览器地址的协议、域名/IP、端口完全一致，不能带末尾 `/`。检查 Caddy / Nginx 是否保留 Host，是否通过不同域名、`localhost` 或内网 IP 访问已绑定的公开入口。

生产页面必须 HTTPS。开发页面请使用 `http://127.0.0.1:8787`；`localhost` 不等同于配置中绑定的 IP Origin。

## 登录失效或 429

默认空闲 30 分钟、绝对 24 小时；账户修改会撤销其他会话。Cookie 不存入 localStorage。浏览器需允许站点 Cookie。

多次错误密码会触发按 IP 与用户名的限制；临时锁定 5 分钟，用户名尝试窗口 15 分钟。等待窗口恢复，不要删除数据库规避认证限制。代理后全部登录来自同一 IP 时，检查本地 `trusted_proxies` 与代理转发头设置。

## 初始化令牌不可用

先确保 Web 服务已启动且有权限写数据目录。令牌成功使用后不可再次读取或初始化。

```bash
sudo journalctl -u mihomo-web.service -n 50 --no-pager
sudo -u mihomo-web /usr/local/bin/mihomo-web-panel --config /etc/mihomo-web/config.toml bootstrap-token
```

不要通过复制新的令牌、清空 bootstrap 标记或公开注册接口覆盖已有管理员。密码不可从 Argon2id 哈希还原；请保护你的密码与数据库备份。

## 内核离线 / Secret 不正确

检查由原来的方式独立运行的 Mihomo：

```bash
sudo ss -lntp | grep ':9090'
```

向导和设置页会对 `/version` 发起服务器端测试。错误 Secret 返回明确提示；新设置验证失败不会替换原设置。地址必须是回环或本地配置授权的私网 IP，域名、元数据和任意路径都会被拒绝。

TLS Controller 需要有效证书，平台不会忽略证书校验。较早版本缺少流量累计字段时，平台从连接快照读取累计值。

## 节点不能手动选择

只有 Selector 提供手动切换。URLTest、Fallback、LoadBalance、Relay 的状态由内核管理。刷新节点列表，确保点击的是该组的真实成员。一次只处理一个切换，不会先在前端伪造成功。

## WebSocket 没有数据

检查代理 Upgrade / Connection 配置、Origin 与 Cookie。Caddy 默认支持 WebSocket。浏览器标签隐藏时主动关闭数据连接，返回后会恢复；日志暂停会停止订阅。内核断线按 1–30 秒退避重连，Session 失效要求重新登录。

## 开机自启不支持或失败

Windows 开发环境与非 systemd 系统不支持。Linux 检查专用 Socket、helper、UID 与权限：

```bash
sudo systemctl status mihomo-web-helper.socket --no-pager
sudo journalctl -u mihomo-web-helper.service -n 50 --no-pager
sudo stat -c '%a %U:%G %n' /run/mihomo-web-helper /run/mihomo-web-helper/control.sock
sudo cat /etc/mihomo-web/helper.env
id mihomo-web
```

helper 的允许 UID 必须等于 `mihomo-web`。不要扩大成任意 systemctl/sudo 权限。自启切换只改变开机启动状态，不立即停止服务。

## 修改监听后无法访问

通过 SSH 或服务器控制台恢复本地 TOML：

```bash
sudo systemctl stop mihomo-web.service
sudo -u mihomo-web /usr/local/bin/mihomo-web-panel --config /etc/mihomo-web/config.toml reset-web-settings
sudo systemctl start mihomo-web.service
```

管理员与内核连接保留。再检查监听端口冲突、public_origin、反向代理与 IPv6 防火墙。设置页面本身不会立即重绑定或自动重启。

## 数据目录被占用 / 数据库失败

同一数据目录只允许一个服务实例。不要同时运行 `cargo run` 与安装服务指向相同目录。检查磁盘空间、文件所有者及数据目录 0700。不要只恢复单独 app.db 而忽略仍在使用的 WAL；备份和恢复时停止服务。
