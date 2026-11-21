# 安装、升级与卸载

## 前提

Linux、systemd 249+、已有运行中的 Mihomo、服务器 SSH 或控制台访问权限。安装器使用 Bash、systemctl、systemd-tmpfiles、useradd/groupadd、getent、runuser 等常见系统工具。建议 Debian 12 / Ubuntu 22.04 或更新版本。

运行时只需要发布包中的主程序及权限辅助程序；前端已经嵌入。SQLite 随 Rust 程序链接，不依赖外部数据库进程。HTTPS 由 Caddy 或 Nginx 提供。

## 安装

```bash
tar -xzf mihomo-web-panel-0.2.0-linux-x86_64.tar.gz
cd mihomo-web-panel-0.2.0-linux-x86_64
sudo bash scripts/install.sh --origin https://panel.example.com
sudo systemctl is-active mihomo-web.service
sudo systemctl is-active mihomo-web-helper.socket
```

从源码构建时可指定二进制所在目录：

```bash
sudo bash scripts/install.sh --origin https://panel.example.com --binary-dir target/release
```

目录与权限：

| 路径 | 所有者 | 权限 / 作用 |
| --- | --- | --- |
| `/usr/local/bin/mihomo-web-panel` | root:root | 0755，主服务程序 |
| `/usr/local/libexec/mihomo-web-helper` | root:root | 0755，固定 systemd 功能 |
| `/etc/mihomo-web` | root:mihomo-web | 0750 |
| `/etc/mihomo-web/config.toml` | root:mihomo-web | 0640，应用只读的本地信任策略 |
| `/etc/mihomo-web/helper.env` | root:root | 0600，固定允许调用 UID |
| `/var/lib/mihomo-web` | mihomo-web:mihomo-web | 0700，SQLite、WAL、初始化令牌 |
| `app.db` / `bootstrap-token` | mihomo-web:mihomo-web | 0600 |
| `/run/mihomo-web-helper` | root:mihomo-web | 0750 |
| `control.sock` | root:mihomo-web | 0660，且 helper 校验实际 peer UID |

日志写入 journald，不额外创建无限增长的文本日志。主服务的 systemd unit 使用空能力集、只读系统、独立用户、有限写目录等限制；辅助程序拥有独立 unit 和 Unix Socket。

## 备份

SQLite 使用 WAL。不要在写入时只复制 `app.db` 而遗漏 WAL。停机复制最容易可靠复现：

```bash
sudo systemctl stop mihomo-web.service
sudo install -d -m 0700 /root/mihomo-web-backups
sudo tar -czf /root/mihomo-web-backups/backup-$(date +%Y%m%d-%H%M%S).tar.gz \
  /etc/mihomo-web /var/lib/mihomo-web
sudo systemctl start mihomo-web.service
```

也可使用 SQLite `.backup` 在线备份数据库；本地 TOML 与 helper.env 仍需单独备份。备份包含管理员密码哈希、Session 摘要与 Mihomo Secret，请保护其读权限。

## 升级

先备份，再在新发布包目录重新运行安装器。已有配置不会被 `--origin` 覆盖；管理员、后端与 SQLite 迁移数据保留。

```bash
cd mihomo-web-panel-0.2.0-linux-x86_64
sudo bash scripts/install.sh --binary-dir bin
sudo journalctl -u mihomo-web.service -n 50 --no-pager
```

程序采用同目录原子重命名替换。上一份程序保存在 `.previous`，但数据库迁移可能不支持降级；回滚程序前同时评估或恢复相应数据库备份。

```bash
sudo systemctl stop mihomo-web.service mihomo-web-helper.service
sudo cp /usr/local/bin/mihomo-web-panel.previous /usr/local/bin/mihomo-web-panel
sudo cp /usr/local/libexec/mihomo-web-helper.previous /usr/local/libexec/mihomo-web-helper
sudo systemctl start mihomo-web.service
```

## 卸载

```bash
sudo bash scripts/uninstall.sh
```

默认停止并删除本平台的程序和 systemd unit，保留配置、数据库和用户以便重装。不会操作 Mihomo 的程序、服务或控制接口。

显式删除管理员与全部平台数据：

```bash
sudo bash scripts/uninstall.sh --purge
```

`--purge` 是不可恢复的数据清理。脚本检查固定目标是否被改为符号链接。Caddy、域名、证书、防火墙与现有 Mihomo 均由原维护方式继续管理。
