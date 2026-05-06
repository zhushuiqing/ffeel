# API 约定

## 端点设计

- RESTful 风格：资源用名词复数，操作用 HTTP 方法
- 路径统一小写，多词用连字符（`/api/user-profiles`）
- 版本前缀：`/api/v1/` 或按项目约定
- 响应格式统一：成功返回数据实体，错误返回 `{ error: string, code?: string }`

## 请求

- GET：查询参数传参
- POST/PUT：`Content-Type: application/json`，body 传参
- 认证信息放 Header（`Authorization: Bearer <token>` 或自定义 Header）
- 分页：`?offset=0&limit=20` 或 `?page=1&page_size=20`

## 响应

- 成功：`200`（读取/更新）、`201`（创建）、`204`（删除无内容）
- 客户端错误：`400`、`401`、`403`、`404`、`422`
- 服务端错误：`500`
- 列表接口返回 `{ data: [...], total: number }`

## WebSocket

- 消息用带标签的 JSON 格式：`{ type: "event_name", data: { ... } }`
- 客户端发消息、服务端推送都用相同格式
- 连接后客户端先发送注册/鉴权消息
- 定时心跳保持连接

## 事件命名

- 用连字符分隔：`"user-created"`、`"file-uploaded"`
- 时态一致：过去时（`-created`、`-updated`）表示已完成

## 数据结构对齐

- 前后端类型定义保持同步，字段名和类型一致
- 可选字段用 `Option<T>` / `field?: type` 对应
- 枚举值前后端统一维护