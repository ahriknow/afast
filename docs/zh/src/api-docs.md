# 交互式文档

启用 `doc` Feature 后，访问 `http://host:port/doc` 可查看交互式 API 文档。

## 设置

```rust
let app = AFast::new()
    .service(svc)
    .document(afast::DocConfig::with("My API", "./docs").basic_auth("username", "password"))
    .http("0.0.0.0:5001");
```

- `GET /doc` — 索引页，列出所有服务（名称以 `_` 开头的服务会被隐藏）
- `GET /doc/{service}` — 服务文档，包含类型定义和在线测试面板
- 深色/浅色主题切换
- 静态 HTML 文件写入 `./docs` 目录

## 功能

### 二进制 Handler 测试

每个二进制 handler 显示一个表单，包含：
- `Custom`、`Data` 和 `State` 参数的输入字段
- 发送按钮，将数据序列化为二进制协议
- 响应面板，显示反序列化后的结果

### Ordinary HTTP 测试

REST 端点（`GET`、`POST` 等）显示：
- 路径参数输入
- 查询参数输入
- JSON 请求体编辑器
- 响应状态码和响应体显示

### WebSocket 调试

WS 路由（`#[ws]`）显示调试面板，包含：
- 路径参数输入（从路由模式自动检测）
- 查询参数输入
- 连接/断开按钮
- 消息输入框和发送按钮（Enter 快捷键）
- 实时日志面板，显示发送/接收的消息

面板连接到 HTTP 端口（ordinary WS 路由使用 HTTP 升级）。

### SSE 调试

SSE 路由（`#[sse]`）显示调试面板，包含：
- 路径参数输入
- 查询参数输入
- 连接/断开按钮
- 实时事件日志，显示命名事件和数据

### 配置面板

右上角的设置面板可配置：
- **传输层**: `ws`（二进制）、`fetch`（HTTP）、`tcp`
- **Host**: 服务器主机名（默认: `localhost`）
- **Port**: 从服务器配置自动检测
- **TLS**: 启用安全连接

## 服务可见性

名称以 `_`（下划线）开头的服务会从文档索引页隐藏，但仍可通过直接 URL 访问（`/doc/_service`）。适用于仅内部使用的端点。
