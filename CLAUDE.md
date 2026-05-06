# CLAUDE.md

旨在减少常见 LLM 编码错误的行为准则。可根据项目特定说明进行合并。

**权衡：** 这些准则偏向谨慎而非速度。对于简单任务，请自行判断。

## 1. 先思考，再动手

**不要假设。不要隐藏困惑。暴露权衡。**

在实施之前：
- 明确说出你的假设。如果不确定，就提问。
- 如果存在多种理解，都列出来——不要默默自行选择。
- 如果有更简单的方案，说出来。必要时提出反对。
- 如果有什么不清楚的地方，停下来。说明哪里困惑了。提问。

## 2. 简洁优先

**用最少的代码解决问题。不要做投机性的添加。**

- 不要添加需求之外的功能。
- 不要为只用一次的代码做抽象。
- 不要添加未被要求的"灵活性"或"可配置性"。
- 不要为不可能出现的场景做错误处理。
- 如果你写了 200 行但其实 50 行就够，重写。

问自己："一个资深工程师会觉得这过度复杂吗？"如果是，简化。

## 3. 精准修改

**只碰必须改的地方。只清理你自己制造的垃圾。**

编辑已有代码时：
- 不要"改进"相邻代码、注释或格式。
- 不要重构没问题的东西。
- 匹配现有代码风格，即使你有别的做法。
- 如果发现无关的死代码，提出来——不要删。

当你的修改产生了孤儿代码时：
- 删除**你的修改**导致的未使用的 import/变量/函数。
- 除非被要求，否则不要删除已有的死代码。

检验标准：每一行改动都应该能直接追溯到用户的请求。

## 4. 目标驱动执行

**定义成功标准。循环直到验证。**

将任务转化为可验证的目标：
- "添加校验" → "为无效输入编写测试，然后让它们通过"
- "修复 bug" → "编写复现该 bug 的测试，然后让它通过"
- "重构 X" → "确保测试在重构前后都能通过"

对于多步骤任务，简述计划：
```
1. [步骤] → 验证：[检查方式]
2. [步骤] → 验证：[检查方式]
3. [步骤] → 验证：[检查方式]
```

明确的成功标准让你能独立推进，模糊的标准（"让它能用"）则需要反复澄清。

---

**如果以下情况出现，说明这些准则正在生效：** diff 中不必要的变更变少了、因过度复杂导致的重写变少了、澄清问题出现在实施之前而非犯错之后。

---

# ffeel 项目指南

## 项目概述

ffeel 是一款跨平台 (Mac/Windows) 局域网文件共享桌面应用。使用 Tauri 2.0 构建，Rust 后端处理网络通信和文件 I/O，React + TypeScript 提供前端 UI。

## 架构

```
src-tauri/src/          # Rust 后端
├── lib.rs              # 入口：Tauri 命令注册、AppState 管理
├── main.rs             # 程序入口
├── error.rs            # 统一错误类型
├── config/
│   └── settings.rs     # 配置：共享目录、设备名、并发数等
├── discovery/
│   └── mod.rs          # mDNS 设备发现（注册/浏览/事件）
├── server/
│   ├── http.rs         # HTTP/2 文件服务（目录列表/上传/下载/路径安全）
│   └── ws.rs           # WebSocket 控制通道（心跳/进度推送）
├── security/
│   ├── pairing.rs      # 设备配对框架（配对码/信任管理）
│   └── certificate.rs  # TLS 证书生成与持久化
├── server/
│   ├── http.rs         # HTTPS 文件服务（目录列表/上传/下载/路径安全）
│   └── ws.rs           # WebSocket 控制通道（心跳/进度推送）
├── transfer/
│   ├── queue.rs        # 传输队列状态机（Pending→Transferring→Paused/Completed/Failed）
│   ├── download.rs     # 远程文件下载（流式传输 + 进度回调）
│   ├── upload.rs       # 文件上传到远程设备（流式上传 + 进度追踪）
│   └── rate_limiter.rs # 速率控制
└── discovery/
    └── mod.rs          # mDNS 设备发现（注册/浏览/事件）

src/                    # React 前端
├── App.tsx             # 主应用：Tab 导航 + 事件监听
├── api.ts              # Tauri invoke 封装
├── types.ts            # TypeScript 类型定义（与 Rust 数据结构对应）
├── pages/
│   ├── Devices.tsx     # 设备列表页（网格展示/扫描）
│   ├── FileBrowser.tsx # 文件浏览页（目录树/批量下载）
│   ├── Transfers.tsx   # 传输管理页（进度/暂停/继续/取消）
│   └── Settings.tsx    # 设置页
└── stores/
    └── appStore.ts     # 应用状态管理
```

## 构建与运行

```bash
npm run tauri dev       # 开发模式（热更新）
npm run tauri build     # 生产构建（输出 .app/.dmg/.msi）
npm run build           # 仅构建前端
cd src-tauri && cargo build  # 仅构建 Rust 后端
```

## 技术栈

- **框架**: Tauri 2.0
- **后端**: Rust (tokio async, axum, reqwest, rustls TLS 1.3)
- **前端**: React 19 + TypeScript + Vite
- **发现**: mDNS (mdns-sd) 设备发现
- **通信**: HTTPS 文件传输 + WebSocket 控制信令
- **安全**: TLS 1.3 加密传输 + 设备配对认证

## 关键约定

1. **数据结构对齐**: Rust 中 `#[derive(Serialize, Deserialize)]` 的结构体必须与 `src/types.ts` 中的 TypeScript 类型保持同步
2. **Tauri 命令**: 所有公有命令在 `lib.rs` 中用 `#[tauri::command]` 声明，并在 `invoke_handler!` 中注册；前端通过 `api.ts` 中封装的 `invoke()` 调用
3. **模块分层**: `discovery/` / `server/` / `transfer/` / `security/` / `config/` 各模块职责清晰，lib.rs 仅做编排
4. **路径安全**: 所有文件访问必须经过 `resolve_safe_path()` 校验，防止目录遍历攻击
5. **AppState**: 全局状态通过 Tauri 的 `manage()` 注册，命令通过 `app.state::<AppState>()` 访问
6. **事件驱动**: Rust→前端通信使用 Tauri 事件系统 (`app.emit()` + `listen()`)

## 功能状态

- [x] v0.1: 设备发现 → 文件浏览 → 文件下载 (含分页/搜索/路径安全)
- [x] v0.2: TLS 1.3 加密传输 + 设备配对认证
- [x] v0.3: 文件上传/文件夹传输/断点续传/速率限制/传输队列(暂停/继续/取消/重试)
- [x] v1.0: 拖拽交互/文件搜索/传输进度实时推送(WebSocket+Tauri Events)
- [ ] CI/CD: GitHub Actions 持续集成 + 自动构建发布
- [ ] 平台: Windows 打包适配 (Wix/NSIS 安装器)${2:-}
