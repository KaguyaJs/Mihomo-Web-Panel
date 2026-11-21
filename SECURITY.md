# 安全策略

## 支持范围

安全修复面向最新稳定发行版。部署时使用 GitHub Releases 中经过 CI 验证的文件，并核对 SHA256SUMS。升级、备份和回滚方式见 [安装文档](docs/installation.md)。

## 报告漏洞

优先通过仓库的 [Security Advisories](https://github.com/KaguyaJs/mihomo-web-panel/security/advisories) 使用 “Report a vulnerability” 向维护者私密报告。如果该入口不可用，先通过 GitHub 私密渠道联系仓库维护者。

报告应包括受影响版本、触发条件、最小复现、影响和建议修复。删除真实密码、Secret、Cookie、Controller 地址及无关个人信息。未协调公开前，不要在 Issue 或 Pull Request 中发布可利用细节。

## 安全模型

设计说明见 [安全模型](docs/security.md)。主服务必须以非 root 用户运行，Controller 限于回环或明确允许的可信内网。helper 只接受固定服务的 enable / disable / status，拒绝通用 systemctl 或 shell 命令。

HTTPS、IPv4 / IPv6 防火墙、备份权限及服务器维护属于部署安全的一部分。生产环境应只暴露 Web HTTPS 入口。
