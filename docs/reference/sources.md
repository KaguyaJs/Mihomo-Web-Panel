# Mihomo 官方接口研究来源

实现依据官方 API 文档和 Meta 分支实际 Go 路由定义；测试 Mock 使用同样的响应与请求形状。新增接口或兼容性修改时，应重新核对相应版本的路由和响应字段。

- [官方 API 文档](https://wiki.metacubex.one/api/)
- [server.go](https://github.com/MetaCubeX/mihomo/blob/Meta/hub/route/server.go)：认证、版本、Traffic `{up, down, upTotal, downTotal}`、默认日志 `{type, payload}`。
- [proxies.go](https://github.com/MetaCubeX/mihomo/blob/Meta/hub/route/proxies.go)：代理列表、选择 `{name}`、URL/timeout 查询测速。
- [configs.go](https://github.com/MetaCubeX/mihomo/blob/Meta/hub/route/configs.go)：模式配置 GET 与 PATCH；平台不投影其他字段。
- [connections.go](https://github.com/MetaCubeX/mihomo/blob/Meta/hub/route/connections.go)：连接快照、指定连接与全部连接删除。

平台独立实现客户端协议，不使用或分发 Mihomo 内核源码或二进制。
