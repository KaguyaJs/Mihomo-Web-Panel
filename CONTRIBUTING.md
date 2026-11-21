# 贡献指南

欢迎提交问题报告、文档改进和代码变更。开发环境和启动方式见 [开发文档](docs/development.md)。

## 提交变更

1. 从 main 创建分支，保持每个变更聚焦于一个问题。
2. 为行为变化添加能验证结果的测试；UI 变化提供桌面和手机截图。
3. 执行 [测试与验收](docs/test-report.md) 中相关检查，并等待 CI 通过。
4. 提交 Pull Request，说明触发条件、最终行为和验证结果。

Cargo.lock、web/package-lock.json 随依赖变化一起提交。版本修改需要同步 Cargo.toml、前端 package / lockfile 和 CHANGELOG.md。新增或升级依赖后更新第三方许可证清单。

## 项目边界

本平台只管理已有 Mihomo Controller。变更不得加入内核安装、下载、启动、替换、通用 URL 转发或任意系统命令执行。

所有管理 API 和 WebSocket 必须进行 Session 校验；写操作验证 CSRF；上游请求使用固定功能白名单，响应采用字段投影。新数据库结构使用编号迁移，必须保留既有配置和账户。

提交内容不得包含开发配置、数据库、初始化令牌、证书私钥、Cookie、真实 Controller 地址、Secret、私人路径或日志。公开 Mock 数据可以用于测试。

## 安全问题

漏洞报告请使用 [安全报告流程](SECURITY.md)，不要将敏感复现材料放入公开 Issue。

贡献代码按项目 [MIT 许可证](LICENSE) 分发；第三方代码或资源需保留相应许可及来源。
