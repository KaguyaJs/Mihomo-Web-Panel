# 平台 API

所有地址同源，不返回 Mihomo Controller URL 或 Secret。JSON 写操作设置 `Content-Type: application/json` 和正确 `Origin`。登录后的写操作还需 `X-CSRF-Token`。

## 公共接口

| 方法 / 路径 | 请求 | 返回 |
| --- | --- | --- |
| GET `/api/health` | 无 | `{status:"ok"}`，仅管理进程存活 |
| GET `/api/setup/status` | 无 | `{required:boolean}` |
| POST `/api/setup/test` | `token, controller_url, secret` | `connected, version` |
| POST `/api/setup/complete` | `token, username, password, confirm_password, controller_url, secret` | 201，原子初始化 |
| POST `/api/auth/login` | `username, password` | `username, csrf_token` 与 HttpOnly Cookie |

初始化请求需要一次性令牌。没有公开注册或恢复管理员的接口。

## 认证与节点

| 方法 / 路径 | 请求 / 行为 |
| --- | --- |
| GET `/api/auth/me` | 当前用户名与绑定 Session 的 CSRF token |
| POST `/api/auth/logout` | 撤销 Session、清 Cookie，204 |
| GET `/api/status` | `configured, connected, version, meta`；离线时带固定 `error/message` |
| GET `/api/proxies` | `proxies` 映射：name、type、all、now、history、udp、alive |
| GET `/api/proxies/{name}` | 单个代理或组，名称必须 URL 编码 |
| PUT `/api/proxies/{name}` | `{name:"成员节点"}`，仅 Selector；返回内核确认后重新读取的代理组 |
| POST `/api/proxies/{name}/delay` | `{timeout_ms:5000}`，允许 1000–15000；返回 `name,delay,status` |
| POST `/api/delay/batch` | `{names:[...],timeout_ms:5000}`，1–100 个已存在节点、去重、有界并发 4 |
| GET `/api/config` | 只返回 `{mode:"rule"}` |
| PATCH `/api/config` | `{mode:"rule"|"global"|"direct"}` |

批量测速返回 `results`，每项 `name, delay, status`，status 为 ok / timeout / unavailable / busy。前端分 4 个节点一组提交，以更新进度；服务器所有单节点与批量测速共享并发限制。

## 连接与 WebSocket

| 方法 / 路径 | 行为 |
| --- | --- |
| GET `/api/connections` | `downloadTotal, uploadTotal, connections`；每项包含 id、network、source、destination、host、chains、upload、download、start、rule、process |
| DELETE `/api/connections/{id}` | 关闭指定连接，204 |
| DELETE `/api/connections` | 必须 `{confirm:true}`；关闭全部，204 |
| WebSocket `/api/traffic` | 同源 Origin + Session Cookie；接收流量 |
| WebSocket `/api/logs` | 同源 Origin + Session Cookie；接收日志 |

WebSocket 是只读协议，不接受转发任意消息。两类上游连接按订阅者共享，最后一个订阅者离开后关闭。服务重启、内核断线、Session 撤销或到期时，前端会相应断开或退避重连。

```json
{"type":"status","state":"online"}
{"type":"traffic","data":{"up":100,"down":200,"upTotal":1000,"downTotal":2000}}
{"type":"log","data":{"type":"info","payload":"已脱敏的内核日志","timestamp":1795276800}}
```

老版本内核没有 upTotal/downTotal 时保留缺省字段，由连接快照提供累计值。状态还可能为 offline / invalid_data / lagged。

## 设置

| 方法 / 路径 | 请求 / 行为 |
| --- | --- |
| GET `/api/settings` | 服务版本、运行时间、平台、架构、当前入口、网页设置、restart_required |
| PUT `/api/settings` | `listen, public_origin, session_idle_seconds, session_absolute_seconds, traffic_interval_ms` |
| GET `/api/settings/backend` | 已配置状态和连接版本，绝不返回旧地址与 Secret |
| POST `/api/settings/backend/test` | `controller_url, secret`，仅测试新输入 |
| PUT `/api/settings/backend` | `controller_url, secret`，验证成功后原子替换 |
| PATCH `/api/settings/security` | `username, current_password, new_password?, confirm_password?`；返回轮换后的认证状态和 Cookie |
| GET `/api/settings/sessions` | 当前与其他会话的摘要 ID、时间、IP、User-Agent |
| DELETE `/api/settings/sessions` | `{scope:"others"|"all"}` |
| GET `/api/settings/autostart` | `supported, enabled, active, message` |
| POST `/api/settings/autostart` | `{operation:"enable"|"disable"}`；没有 helper 时明确失败 |

输入拒绝未知 JSON 字段。不接受自定义 URL、unit、shell 命令或测试目标。监听与 Origin 只在重启时应用，不存在浏览器远程重启服务的接口。

## 错误

固定 JSON：`{"error":"backend_timeout","message":"Mihomo 响应超时，请稍后重试"}`。

401 表示认证失败；403 表示 Origin、CSRF 或初始化令牌等校验失败；409 表示正在切换或初始化；429 返回 Retry-After；502 表示上游不可用、认证失败或异常响应；504 为超时。平台不回传上游原始错误正文。
