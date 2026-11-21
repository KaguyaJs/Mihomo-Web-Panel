# 实际部署：已有 Mihomo 的 IPv6 Linux 服务器

本文假设你已经能通过 SSH 管理服务器，Mihomo 已独立运行。示例域名 `panel.example.com` 与文档 IPv6 `2001:db8::1234` 都要换成实际值。

## 1. 确认内核和服务器

```bash
uname -m
systemctl --version
ip -6 addr show scope global
sudo ss -lntp
```

现有 Mihomo Controller 应为 `127.0.0.1:9090` 或 `[::1]:9090`，不是 `0.0.0.0:9090` / `[::]:9090`。由你使用原来的内核配置方式设置 `external-controller` 和强 Secret；本平台不会改动内核。

从 [GitHub Releases](https://github.com/KaguyaJs/mihomo-web-panel/releases/latest) 下载对应 CPU 的发布包和 SHA256SUMS。x86_64 适用于 Intel / AMD 64 位服务器，aarch64 适用于 ARM64 服务器。Release 工作流在对应架构构建并验证文件。

```bash
sha256sum --ignore-missing -c SHA256SUMS
```

## 2. 域名与 HTTPS

在 DNS 添加 `panel.example.com` 的 AAAA 记录，内容为实际公网 IPv6。若同时提供 IPv4，可额外配置 A 记录。DNS 可从客户端验证：

```bash
dig AAAA panel.example.com
```

推荐域名 + Caddy 自动证书。只使用原始 IPv6 也可以，但需要对该 IP 有效的可信证书；证书申请方式取决于 CA/Caddy 版本。自签证书只适合你已经把自己的 CA 安装到客户端信任库的受控环境。

## 3. 安装平台

```bash
tar -xzf mihomo-web-panel-0.2.0-linux-x86_64.tar.gz
cd mihomo-web-panel-0.2.0-linux-x86_64
sha256sum -c SHA256SUMS
sudo bash scripts/install.sh --origin https://panel.example.com
sudo systemctl is-active mihomo-web.service
sudo systemctl show mihomo-web.service -p User -p MainPID
```

安装器只新增 mihomo-web 账户、平台程序、配置、数据库目录和专用 helper，不调用任何 Mihomo 安装/启动命令。主服务内部监听回环 8787，配置权限为 root:mihomo-web 0640。

## 4. 配置 Caddy

```bash
sudo apt-get update
sudo apt-get install -y caddy
sudo tee /etc/caddy/Caddyfile >/dev/null <<'EOF'
panel.example.com {
    encode zstd gzip
    reverse_proxy 127.0.0.1:8787
}
EOF
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl reload caddy
```

如已有 Caddy 配置，请加入该站点块，保留现有站点。WebSocket 自动支持。Nginx 替代方案见 `deploy/nginx.conf`，其中必须保留同源 Host，并转发 WebSocket Upgrade。

原始 IPv6 示例：在 `/etc/mihomo-web/config.toml` 使用 `public_origin = "https://[实际IPv6]"`，保持 Secure Cookie。Caddy 中配置相同站点地址与对应证书：

```caddyfile
https://[2001:db8::1234] {
    tls /etc/caddy/panel-ip.crt /etc/caddy/panel-ip.key
    reverse_proxy 127.0.0.1:8787
}
```

使用非默认 HTTPS 端口时，Origin、站点地址和防火墙需包含该端口。

## 5. 同时保护 IPv4 与 IPv6

先确认 SSH 实际端口，以免断开维护入口：

```bash
sudo apt-get install -y ufw
sudo sed -i 's/^IPV6=.*/IPV6=yes/' /etc/default/ufw
sudo ufw allow 22/tcp
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw deny 8787/tcp
sudo ufw deny 9090/tcp
sudo ufw enable
sudo ufw status verbose
```

80 提供证书验证/HTTPS 跳转，443 提供网页。云防火墙/安全组也要设置同样的 IPv6 入站限制。不要把已有 Mihomo 的代理端口与 Controller 混为一谈，其他内核端口按你的原网络策略管理。

## 6. 安全初始化

```bash
sudo -u mihomo-web /usr/local/bin/mihomo-web-panel \
  --config /etc/mihomo-web/config.toml bootstrap-token
```

用手机或电脑打开 `https://panel.example.com`。填写一次性令牌、管理员用户名和至少 12 字符的密码；下一步填已有 Controller 与 Secret，点击测试连接；测试成功后完成配置，再登录。

令牌只能使用一次。管理员、Secret 和 Session 摘要保存在服务器的权限受限 SQLite 中；浏览器只拿到自己输入过的值和不含后端凭据的管理响应。

## 7. 验收

```bash
sudo ss -lntp
sudo systemctl is-active mihomo-web.service
sudo systemctl is-enabled mihomo-web.service
sudo systemd-analyze security mihomo-web.service
sudo journalctl -u mihomo-web.service -n 50 --no-pager
```

网页登录后实际切换一个 Selector 节点，并通过 Mihomo 原有控制方式核对状态；运行单节点测速；观察实时流量、连接和日志。在设置中关闭/开启自启，`systemctl is-enabled` 应对应变化，当前 Web 服务保持运行。

从服务器外执行 `curl -6 -I https://panel.example.com`。确认 IPv6 上无法连接公网 8787 / 9090；关闭浏览器会话后不能调用管理接口。

## 8. 日常维护

备份、升级、卸载见 installation.md；监听恢复见 configuration.md。平台与 Mihomo 分别维护，重启平台不会接管内核生命周期。
