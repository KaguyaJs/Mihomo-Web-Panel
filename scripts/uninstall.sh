#!/usr/bin/env bash
set -euo pipefail
purge=false
if [[ ${1:-} == --purge && $# -eq 1 ]]; then purge=true
elif (($#)); then printf '%s\n' '用法: sudo bash scripts/uninstall.sh [--purge]'; exit 2; fi
[[ $(id -u) -eq 0 ]] || { printf '%s\n' '卸载脚本需要 root。' >&2; exit 1; }

for unit in mihomo-web.service mihomo-web-helper.socket; do
    if systemctl cat "$unit" >/dev/null 2>&1; then systemctl disable --now "$unit"; fi
done
if systemctl cat mihomo-web-helper.service >/dev/null 2>&1; then systemctl stop mihomo-web-helper.service; fi
rm -f -- /etc/systemd/system/mihomo-web.service /etc/systemd/system/mihomo-web-helper.service /etc/systemd/system/mihomo-web-helper.socket
rm -f -- /usr/lib/tmpfiles.d/mihomo-web-helper.conf /usr/local/bin/mihomo-web-panel /usr/local/bin/mihomo-web-panel.previous /usr/local/libexec/mihomo-web-helper /usr/local/libexec/mihomo-web-helper.previous
systemctl daemon-reload
if "$purge"; then
    for target in /etc/mihomo-web /var/lib/mihomo-web; do
        [[ ! -L "$target" ]] || { printf '拒绝清理符号链接: %s\n' "$target" >&2; exit 1; }
        [[ ! -e "$target" || $(realpath -- "$target") == "$target" ]] || { printf '%s\n' '数据目录解析不符，停止清理。' >&2; exit 1; }
    done
    rm -rf -- /etc/mihomo-web /var/lib/mihomo-web
    printf '%s\n' '程序、配置、管理员和数据库已卸载。独立的 Mihomo 未受影响。'
else
    printf '%s\n' '程序已卸载，配置和数据库保留；重新安装可继续使用。独立的 Mihomo 未受影响。'
fi
