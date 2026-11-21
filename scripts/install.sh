#!/usr/bin/env bash
set -euo pipefail
umask 077

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd -- "$script_dir/.." && pwd)"
binary_dir="$project_dir/bin"
origin=""

usage() { printf '%s\n' '用法: sudo bash scripts/install.sh --origin https://panel.example.com [--binary-dir /path/to/bin]'; }
while (($#)); do
    case "$1" in
        --origin) [[ $# -ge 2 ]] || { usage; exit 2; }; origin="$2"; shift 2 ;;
        --binary-dir) [[ $# -ge 2 ]] || { usage; exit 2; }; binary_dir="$2"; shift 2 ;;
        --help|-h) usage; exit 0 ;;
        *) usage; exit 2 ;;
    esac
done

[[ $(id -u) -eq 0 ]] || { printf '%s\n' '安装脚本需要 root。Web 服务会以独立普通用户运行。' >&2; exit 1; }
[[ -d /run/systemd/system ]] || { printf '%s\n' '当前系统没有运行 systemd。' >&2; exit 1; }
command -v systemctl >/dev/null
command -v systemd-tmpfiles >/dev/null
[[ -x /usr/bin/systemctl ]] || { printf '%s\n' '需要 /usr/bin/systemctl。' >&2; exit 1; }
for binary in mihomo-web-panel mihomo-web-helper; do
    [[ -f "$binary_dir/$binary" ]] || { printf '缺少二进制文件: %s\n' "$binary" >&2; exit 1; }
done

if [[ -n "$origin" ]] && ! printf '%s\n' "$origin" | grep -Eq '^https://(\[[0-9A-Fa-f:]+\]|[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?)(:[0-9]{1,5})?$'; then
    printf '%s\n' '访问入口必须为 HTTPS 域名或方括号包围的 IPv6，不能含路径、查询或凭据。' >&2
    exit 1
fi
if [[ ! -f /etc/mihomo-web/config.toml && -z "$origin" ]]; then usage; exit 2; fi
if ! getent group mihomo-web >/dev/null; then groupadd --system mihomo-web; fi
if ! id mihomo-web >/dev/null 2>&1; then useradd --system --gid mihomo-web --home-dir /var/lib/mihomo-web --no-create-home --shell /usr/sbin/nologin mihomo-web; fi
[[ $(id -u mihomo-web) -ne 0 ]] || { printf '%s\n' 'mihomo-web 用户不能为 root。' >&2; exit 1; }

install -d -o root -g mihomo-web -m 0750 /etc/mihomo-web
install -d -o mihomo-web -g mihomo-web -m 0700 /var/lib/mihomo-web
install -d -o root -g root -m 0755 /usr/local/bin /usr/local/libexec
for destination in /usr/local/bin/mihomo-web-panel /usr/local/libexec/mihomo-web-helper; do
    if [[ -f "$destination" ]]; then install -o root -g root -m 0755 "$destination" "$destination.previous"; fi
done
install -o root -g root -m 0755 "$binary_dir/mihomo-web-panel" /usr/local/bin/.mihomo-web-panel.new
install -o root -g root -m 0755 "$binary_dir/mihomo-web-helper" /usr/local/libexec/.mihomo-web-helper.new
mv -f -- /usr/local/bin/.mihomo-web-panel.new /usr/local/bin/mihomo-web-panel
mv -f -- /usr/local/libexec/.mihomo-web-helper.new /usr/local/libexec/mihomo-web-helper

if [[ ! -f /etc/mihomo-web/config.toml ]]; then
    sed "s#https://panel.example.com#$origin#" "$project_dir/deploy/config.production.toml" > /etc/mihomo-web/config.toml.new
    chown root:mihomo-web /etc/mihomo-web/config.toml.new
    chmod 0640 /etc/mihomo-web/config.toml.new
    mv -- /etc/mihomo-web/config.toml.new /etc/mihomo-web/config.toml
else
    printf '%s\n' '保留现有配置、管理员、后端连接与数据库。--origin 不覆盖已有配置。'
fi
printf 'MIHOMO_WEB_UID=%s\n' "$(id -u mihomo-web)" > /etc/mihomo-web/helper.env
chown root:root /etc/mihomo-web/helper.env
chmod 0600 /etc/mihomo-web/helper.env
runuser -u mihomo-web -- /usr/local/bin/mihomo-web-panel --config /etc/mihomo-web/config.toml check-config

install -o root -g root -m 0644 "$project_dir/deploy/mihomo-web.service" /etc/systemd/system/mihomo-web.service
install -o root -g root -m 0644 "$project_dir/deploy/mihomo-web-helper.service" /etc/systemd/system/mihomo-web-helper.service
install -o root -g root -m 0644 "$project_dir/deploy/mihomo-web-helper.socket" /etc/systemd/system/mihomo-web-helper.socket
install -o root -g root -m 0644 "$project_dir/deploy/mihomo-web-helper.tmpfiles" /usr/lib/tmpfiles.d/mihomo-web-helper.conf
systemd-tmpfiles --create /usr/lib/tmpfiles.d/mihomo-web-helper.conf
systemctl daemon-reload
systemctl try-restart mihomo-web-helper.service
systemctl enable --now mihomo-web-helper.socket
systemctl enable mihomo-web.service
systemctl restart mihomo-web.service
systemctl is-active --quiet mihomo-web.service
printf '%s\n' '安装完成。请配置 HTTPS 反向代理与 IPv6 防火墙。'
printf '%s\n' '读取初始化令牌: sudo -u mihomo-web /usr/local/bin/mihomo-web-panel --config /etc/mihomo-web/config.toml bootstrap-token'
