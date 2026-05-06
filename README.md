# ffeel - 局域网文件共享

ffeel 是一款跨平台（Mac/Windows）局域网文件共享桌面应用。基于 Tauri 2.0 构建，采用 Rust 后端处理网络通信和文件 I/O，React + TypeScript 提供前端 UI。

## 功能

- **设备发现** — 基于 mDNS 的局域网设备自动发现
- **文件浏览** — 浏览远程设备共享目录，支持分页和目录结构展示
- **文件传输** — 支持文件上传/下载，带进度跟踪和断点续传
- **文件夹传输** — 递归传输整个目录结构
- **搜索** — 远程设备文件模糊搜索
- **速率限制** — 可配置的上传/下载速度限制
- **传输队列** — 并发控制、暂停/继续/取消、失败重试
- **安全加密** — TLS 1.3 加密传输 + 设备配对认证
- **拖拽交互** — 支持文件拖拽到设备进行操作

## 技术栈

| 层 | 技术 |
|---|---|
| 框架 | Tauri 2.0 |
| 后端 | Rust (tokio, axum, reqwest, rustls) |
| 前端 | React 19 + TypeScript + Vite |
| 发现 | mDNS (mdns-sd) |
| 信令 | WebSocket |
| 安全 | TLS 1.3 (rustls) |

## 开始使用

### 前置条件

- Rust 1.77+
- Node.js 20+
- macOS: Xcode Command Line Tools
- Windows: Visual Studio Build Tools + WebView2

### Linux 额外依赖

```bash
sudo apt-get install libwebkit2gtk-4.1-dev libappindicator3-dev \
  librsvg2-dev patchelf libssl-dev
```

### 开发运行

```bash
npm install
npm run tauri dev
```

### 构建

```bash
npm run tauri build
```

构建产物位于 `src-tauri/target/release/bundle/`。

## 架构

```
src-tauri/src/
├── lib.rs              # 入口：Tauri 命令注册、AppState 管理
├── security/
│   ├── pairing.rs      # 设备配对（配对码/信任管理）
│   └── certificate.rs  # TLS 证书生成与持久化
├── server/
│   ├── http.rs         # HTTPS 文件服务（目录列表/上传/下载）
│   └── ws.rs           # WebSocket 控制通道（心跳/进度推送）
├── transfer/
│   ├── queue.rs        # 传输队列状态机
│   ├── download.rs     # 远程文件下载
│   ├── upload.rs       # 文件上传
│   └── rate_limiter.rs # 速率控制
└── discovery/
    └── mod.rs          # mDNS 设备发现
```

## 许可

MIT
