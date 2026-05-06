# ffeel 局域网文件共享 — 开发实施方案

> 基于 PRD v0.1 验收清单及当前代码 (2026-04-30) 编写的分阶段开发方案。

---

## 目录

- [当前状态评估](#当前状态评估)
- [v0.1 收尾 — 补齐 MVP 缺口](#v01-收尾--补齐-mvp-缺口)
- [v0.2 安全增强](#v02-安全增强)
- [v0.3 功能完善](#v03-功能完善)
- [v1.0 正式发布](#v10-正式发布)
- [附录：关键技术决策](#附录关键技术决策)

---

## 当前状态评估

### 已实现功能（F- 编号对应 PRD）

| 功能 | 状态 | 说明 |
|------|------|------|
| F-001 mDNS 设备注册 | ✅ | `discovery::register_service()` 完整 |
| F-002 设备浏览发现 | ✅ | `discovery::start_browsing()` + 事件推送 |
| F-003 设备在线状态 | ✅ | DeviceEvent::Found/Lost + DeviceInfo.online |
| F-004 远程目录列表 | ✅ | HTTP /api/list + DirEntry 结构化返回 |
| F-005 目录导航 | ✅ | 前端目录树，上/下级导航 |
| F-007 单文件下载 | ✅ | download.rs 流式下载 + 进度回调 |
| F-008 批量文件下载 | ✅ | 前端多选 + 逐个下载 |
| F-012 传输队列 | ✅ | TransferManager 状态机 |
| F-013 传输进度显示 | ✅ | 前端 progress bar + bytes/speed |
| F-014 暂停/继续 | ✅ | pause_task / resume_task |
| F-015 取消传输 | ✅ | cancel_task |
| F-022 设置读写 | ✅ | get_settings / update_settings 命令 |
| F-023 设备名配置 | ✅ | Settings.device_name |
| F-024 共享目录配置 | ✅ | Settings.share_dir |
| F-025 并发数配置 | ✅ | Settings.max_concurrent_transfers |
| F-026 速度限制配置 | ✅ | Settings.speed_limit（已读，未生效） |
| F-020 目录沙箱 | ✅ | resolve_safe_path() 路径校验 |
| UI 四标签页 | ✅ | 设备/文件/传输/设置 |
| 暗色主题 | ✅ | 已实现 |

### 关键缺口

1. **F-009 上传功能** — 后端 `upload.rs` + HTTP /api/upload 已实现，但前端**无上传入口**（无按钮/无命令暴露）
2. **F-010 WebSocket** — `server/ws.rs` 只有骨架（心跳 Ping/Pong），**未接入 HTTP 路由器**，未推送任何实际事件
3. **F-016 断点续传** — 完全未实现
4. **F-017 文件夹传输** — 完全未实现
5. **F-018 传输限速** — `speed_limit` 字段已定义，从未被使用
6. **F-019 文件名搜索** — 完全未实现
7. **F-021 设备配对认证** — `security/pairing.rs` 框架已写，但**未集成**到 HTTP 服务或传输流程
8. **F-027 设置持久化** — `Settings` 存储于内存 `Mutex<Settings>`，应用重启后丢失
9. **F-028 信任设备管理** — 完全未实现
10. **安全** — 全程 HTTP（明文），无 TLS 加密
11. **下载路径硬编码** — `/Users/zhushuiqing/Downloads/` 写死在 `FileBrowser.tsx:77`
12. **传输历史无上限** — 持续增长无清理机制
13. **无测试覆盖**

---

## v0.1 收尾 — 补齐 MVP 缺口

**目标：** 让 v0.1 MVP 达到"可稳定使用"的标准，修复当前代码中明显的不完整项。

**预估总工时：** 4-5 人天

---

### 任务 1.1：设置持久化（F-027）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-027 |
| **前置** | 无 |
| **预估** | 0.5 人天 |

**涉及文件：**
- `src-tauri/src/config/settings.rs` — 新增 `save()` / `load()` 方法
- `src-tauri/src/lib.rs` — 启动时加载，修改时保存
- `src/types.ts` — 无需变更

**技术方案：**

```rust
// settings.rs 新增
impl Settings {
    pub fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("ffeel");
        path.push("settings.json");
        path
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str(&content) {
                return settings;
            }
        }
        Self::default()  // 不存在则返回默认值
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)
    }
}
```

在 `lib.rs` 的 `setup()` 中调用 `Settings::load()` 替代 `Settings::default()`，`update_settings` 命令中调用 `settings.save()`。

多进程安全：Tauri 单实例运行，无并发写冲突。使用原子写（写入临时文件 → rename）防止写中断导致配置损坏。

**验收标准：**
1. 修改设置并重启应用，设置值保留
2. 删除配置文件后重启，自动创建默认配置
3. 设置文件格式为合法 JSON，位于系统配置目录

---

### 任务 1.2：传输历史上限控制（F-012 增强）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-012 增强 |
| **前置** | 无 |
| **预估** | 0.5 人天 |

**涉及文件：**
- `src-tauri/src/transfer/queue.rs` — 新增历史上限逻辑
- `src-tauri/src/lib.rs` — 无需变更
- `src-tauri/src/config/settings.rs` — 可选项：增加 max_history 字段

**技术方案：**

在 `TransferManager` 中新增：
- 常量或配置字段 `max_history: usize`（默认 100）
- `complete_task()` / `fail_task()` 完成后，检查历史记录数量，超出 `max_history` 时移除最早的历史条目
- 使用 VecDeque 替代 Vec 提高移除效率（可选，也可以直接在 Vec 上操作）

```rust
// queue.rs
const MAX_HISTORY: usize = 100;

pub fn enforce_history_limit(&mut self) {
    let history_count = self.tasks.iter()
        .filter(|t| matches!(t.status, TransferStatus::Completed | TransferStatus::Failed | TransferStatus::Cancelled))
        .count();
    if history_count > MAX_HISTORY {
        let to_remove = history_count - MAX_HISTORY;
        self.tasks.retain(|t| {
            // 保留正在活跃的，以及最新的历史
            !matches!(t.status, TransferStatus::Completed | TransferStatus::Failed | TransferStatus::Cancelled)
        });
    }
}
```

**验收标准：**
1. 完成 100 个传输后，第 101 个完成时最早的历史记录被清理
2. 活跃任务（传输中/暂停中）不受影响
3. 前端能正确显示清理后的历史列表

---

### 任务 1.3：批量下载后自动刷新传输列表（F-008 增强）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-008 增强 |
| **前置** | 无 |
| **预估** | 0.5 人天 |

**涉及文件：**
- `src/pages/FileBrowser.tsx` — 在 handleDownloadSelected 后触发刷新
- `src/pages/Transfers.tsx` — 添加 onRefresh 在组件挂载时刷新（已实现定时刷新）

**技术方案：**

`FileBrowser.tsx` 中 `handleDownloadSelected` 在开始批量下载后，自动导航到传输 tab 并调用 `refreshTransfers()`。或在 `App.tsx` 层面监听 "transfer-started" 事件并自动刷新传输列表。

当前已实现：
- 前端每 2 秒定时轮询 `refreshTransfers()`
- 下载开始后传输列表会自动通过轮询更新

问题：批量下载时用户停留在文件浏览页，看不到传输进度。解决方案：批量下载后，在状态栏提示 "已开始下载 N 个文件" 并自动切换到传输 tab（或通过新增的 `autoNavigate` props 通知 App 组件）。

**更简方案：** 在 `App.tsx` 中新增一个本地状态 `batchDownloadCount`，`FileBrowser` 通过回调通知 App 批量下载开始，App 在状态栏显示"正在下载 N 个文件"并将 activeTab 切换到 "transfers"。

**验收标准：**
1. 批量下载后自动切换到传输页面
2. 状态栏显示批量下载的数量提示
3. 传输列表已显示所有新创建的传输任务

---

### 任务 1.4：下载路径选择 UI（F-007 增强）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-007 增强 |
| **前置** | 无 |
| **预估** | 1 人天 |

**涉及文件：**
- `src/pages/FileBrowser.tsx` — 移除硬编码路径，使用弹窗选择
- `src-tauri/src/lib.rs` — 新增 `pick_save_directory` 命令（可选）
- `package.json` — 已有 `tauri-plugin-dialog`，无需新增依赖

**技术方案：**

使用 Tauri 的 dialog 插件打开"选择保存目录"弹窗：

```typescript
// FileBrowser.tsx
import { save } from '@tauri-apps/plugin-dialog';

async function pickDownloadPath(defaultName: string): Promise<string | null> {
  const path = await save({
    defaultPath: defaultName,
    filters: [{ name: 'All Files', extensions: ['*'] }],
  });
  return path;
}
```

或在设置页新增"默认下载目录"配置项，让用户预先设定下载目录，替代当前硬编码。

**更优方案：** 结合两者：
1. 设置页增加"默认下载目录"配置（持久化）
2. 下载时使用默认目录，不弹对话（保持简洁）
3. 用户可通过设置修改

**验收标准：**
1. 下载文件不再使用硬编码的 `/Users/zhushuiqing/Downloads/`
2. 前端可从设置或对话框获取目标目录
3. 设置页可配置默认下载目录

---

### 任务 1.5：WebSocket 骨架增强（F-010）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-010 |
| **前置** | 无 |
| **预估** | 1 人天 |

**涉及文件：**
- `src-tauri/src/server/ws.rs` — 实现消息广播和连接管理
- `src-tauri/src/server/http.rs` — 将 WebSocket 路由接入路由器
- `src-tauri/src/lib.rs` — 将 WebSocket 的广播通道注入 AppState

**当前问题：**
- `ws.rs` 有完整的 WsMessage 枚举定义和 WebSocket 处理函数
- `ws.rs` 的 `ws_handler` 和 `handle_ws_connection` 已编写
- 但 `http.rs` 的 `build_router()` **没有**添加 WebSocket 路由
- WebSocket 连接后仅做心跳（Ping/Pong），**没有实际消息推送**

**技术方案：**

1. 在 `http.rs` 的 `build_router()` 中添加 WebSocket 路由：
   ```rust
   .route("/ws", get(server::ws::ws_handler))
   ```

2. 将 `broadcast::Sender<WsMessage>` 注入 HTTP 的 `AppState`：
   ```rust
   pub struct AppState {
       pub share_dir: PathBuf,
       pub transfer_manager: Arc<Mutex<TransferManager>>,
       pub ws_tx: broadcast::Sender<WsMessage>,
   }
   ```

3. 在 `ws.rs` 中实现消息转发：
   ```rust
   pub async fn ws_handler(
       ws: WebSocketUpgrade,
       State(state): State<AppState>,
   ) -> impl IntoResponse {
       let rx = state.ws_tx.subscribe();
       ws.on_upgrade(|socket| handle_ws_connection(socket, rx))
   }

   async fn handle_ws_connection(
       mut socket: WebSocket,
       mut rx: broadcast::Receiver<WsMessage>,
   ) {
       // 将 broadcast 的消息转发到 WebSocket
       loop {
           tokio::select! {
               msg = rx.recv() => {
                   if let Ok(msg) = msg {
                       let data = serde_json::to_string(&msg).unwrap();
                       if socket.send(Message::Text(data.into())).await.is_err() {
                           break;
                       }
                   }
               }
               ws_msg = receiver.next() => {
                   // 处理客户端消息和控制帧
               }
           }
       }
   }
   ```

4. 在 `lib.rs` 的 HTTP 服务启动处，创建 `broadcast::channel` 并传入。

**验收标准：**
1. WebSocket 路由 `/ws` 可用，客户端可连接
2. 传输进度变化时通过 WebSocket 推送 TransferProgress 事件
3. 前端可收到 WebSocket 推送的进度、完成、失败事件
4. 实现 Tauri 事件 + WebSocket 双通道通信（向下兼容现有前端）

---

## v0.2 安全增强

**目标：** 补齐安全短板，引入 TLS 加密传输和设备配对认证机制，确保局域网通信安全。

**预估总工时：** 10-12 人天

---

### 任务 2.1：TLS 1.3 加密通信（F-029）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-029（新增） |
| **前置** | v0.1 全部完成 |
| **预估** | 3 人天 |

**涉及文件：**
- `src-tauri/Cargo.toml` — 新增 `rustls`, `axum-server`, `tokio-rustls` 依赖
- `src-tauri/src/server/http.rs` — 从 HTTP 迁移到 HTTPS
- `src-tauri/src/lib.rs` — 启动时生成/加载自签名证书，创建 TLS 监听器
- `src-tauri/src/security/mod.rs` — 新增证书管理模块
- `src-tauri/src/transfer/download.rs` — 客户端使用 HTTPS 连接
- `src-tauri/src/transfer/upload.rs` — 客户端使用 HTTPS 连接

**技术方案：**

采用自签名证书方案（局域网环境无 CA）：

1. 首次启动时生成自签名证书（`rcgen` crate）：
   ```
   /证书路径/
   ├── cert.pem     # 自签名证书
   └── key.pem      # 私钥
   ```

2. 启动时加载或生成证书，使用 rustls 创建 TLS Acceptor：
   ```rust
   use axum_server::tls_rustls::RustlsConfig;
   
   let config = RustlsConfig::from_pem_file(cert_path, key_path).await?;
   axum_server::bind_rustls(address, config)
       .serve(router.into_make_service())
       .await?;
   ```

3. reqwest 客户端需要禁用证书验证（因为自签名）：
   ```rust
   let client = reqwest::Client::builder()
       .danger_accept_invalid_certs(true)
       .build()?;
   ```
   或首次连接时导入对端证书指纹。

4. 设备发现时广播 HTTPS 端口（而非当前 HTTP 端口）。

**验收标准：**
1. 设备间通信使用 HTTPS（TLS 1.3）
2. 自签名证书首次启动自动生成
3. mDNS 发现时标记端口为 HTTPS 端口
4. 前端 URL 从 `http://` 切换为 `https://`

---

### 任务 2.2：设备配对认证（F-021）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-021 |
| **前置** | 任务 2.1（TLS 依赖网络层就绪） |
| **预估** | 3 人天 |

**涉及文件：**
- `src-tauri/src/security/pairing.rs` — 重构完整配对逻辑
- `src-tauri/src/security/mod.rs` — 子模块声明
- `src-tauri/src/server/http.rs` — 新增配对验证中间件
- `src-tauri/src/lib.rs` — 新增加密握手命令
- `src/pages/PairingDialog.tsx` — 新建配对码弹窗组件
- `src/App.tsx` — 引入配对弹窗
- `src/types.ts` — 新增配对相关类型

**技术方案：**

配对流程（挑战-响应模式）：

```
设备 A                        设备 B
  |                              |
  |-- 1. 发现 B (mDNS) --------->|
  |                              |
  |-- 2. 请求配对 (HTTPS) ------>|
  |<--- 3. 返回配对码 (UI 显示) --|
  |                              |
  |-- 4. 输入配对码发送 -------->|
  |<--- 5. 验证成功, 交换公钥 ---|
  |                              |
  |-- 6. 后续通信使用公钥签名 -->|
```

**当前 pairing.rs 状态：**
- `PairingManager` 结构体已有
- `generate_pairing_code()` — 4 位码生成
- `verify_pairing_code()` — 简化版验证（仅检查长度）
- `trust_device()` / `untrust_device()` / `is_trusted()` 已有

**需要补充：**
1. **集成到 HTTP 路由器：** 新增配对 API 端点：
   - `POST /api/pair/request` — 发起配对
   - `POST /api/pair/verify` — 验证配对码
   - 非信任设备访问受限

2. **配对中间件：** 在 `http.rs` 中添加请求拦截：
   ```rust
   // 对非信任设备的文件操作请求，返回 401
   // 已信任设备直接放行
   ```

3. **前端配对弹窗：**
   - 连接非信任设备时弹出配对码输入框
   - 对端设备 UI 显示当前配对码
   - 配对成功后设备加入信任列表

**验收标准：**
1. 首次连接设备时弹出配对码确认弹窗
2. 配对成功后设备加入信任列表
3. 已信任设备不再重复配对
4. 用户可以手动移除信任设备

---

### 任务 2.3：信任设备管理 UI（F-028）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-028 |
| **前置** | 任务 2.2 |
| **预估** | 1 人天 |

**涉及文件：**
- `src/pages/Settings.tsx` — 新增"已信任设备"列表和移除操作
- `src-tauri/src/lib.rs` — 新增 `get_trusted_devices` / `remove_trusted_device` 命令
- `src-tauri/src/security/pairing.rs` — 如需补充查询方法

**技术方案：**

在设置页面新增"信任设备"区域：
- 展示所有已信任设备列表（设备名 + IP + 配对时间）
- 每项提供"移除信任"按钮
- 移除后，该设备下次连接需重新配对

**验收标准：**
1. 设置页显示已信任设备列表
2. 可以移除单个设备的信任状态
3. 移除后需要重新配对

---

### 任务 2.4：上传功能前端（F-009）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-009 |
| **前置** | 无（但建议 v0.2 做，因为涉及文件操作） |
| **预估** | 2 人天 |

**涉及文件：**
- `src/pages/FileBrowser.tsx` — 新增"上传文件"按钮和操作
- `src/api.ts` — 新增 `uploadFile` 封装
- `src-tauri/src/lib.rs` — 新增 `upload_file` Tauri 命令
- `src-tauri/src/transfer/upload.rs` — 已实现，可能需要添加进度回调
- `src/types.ts` — 无需变更

**技术方案：**

1. 后端暴露 `upload_file` Tauri 命令：
   ```rust
   #[tauri::command]
   async fn upload_file(
       app: tauri::AppHandle,
       device_ip: String,
       port: u16,
       remote_path: String,
       local_path: String,
   ) -> Result<(), AppError>
   ```
   调用 `transfer::upload::upload_file_to_remote()`。

2. 前端增加"上传"按钮，使用 Tauri dialog 选择本地文件。

3. 上传过程中创建传输任务（direction: Upload），通过 Tauri 事件或 WebSocket 推送进度。

**当前 upload.rs 状态：**
- 已实现 `upload_file_to_remote()` — 读取本地文件 → multipart 上传到远程
- 缺少：进度回调、任务注册、任务完成/失败事件

**需要补充：**
- 上传流程中注册 TransferTask（direction: Upload）
- 上传时推送进度事件（通过 callback 或周期读取已发送字节）
- 上传完成后推送完成事件

**验收标准：**
1. 文件浏览器页出现"上传"按钮
2. 点击后弹出文件选择对话框
3. 文件上传后在传输列表显示进度
4. 上传完成后目标设备刷新可看到新文件

---

### 任务 2.5：配对码 UI 弹窗（F-021 前端部分）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-021（前端部分） |
| **前置** | 任务 2.2 |
| **预估** | 1 人天 |

**涉及文件：**
- `src/pages/PairingDialog.tsx` — 创建配对弹窗组件
- `src/App.tsx` — 集成弹窗逻辑
- `src-tauri/src/lib.rs` — 暴露配对相关命令

**技术方案：**

创建独立弹窗组件 `PairingDialog`：
- 显示配对码输入框
- 显示"等待对方输入配对码"状态
- 配对成功/失败提示
- 支持重新生成配对码

集成到 App.tsx：
- 在 `AppState` 中增加 `pendingPairingDevice`
- 尝试连接但未配对的设备时自动弹出配对弹窗

**验收标准：**
1. 连接需要配对的设备时自动弹出配对弹窗
2. 配对码有效期 60 秒，过期自动刷新
3. 配对成功/失败有明确反馈

---

## v0.3 功能完善

**目标：** 补齐断点续传、文件夹传输、传输限速、文件搜索等生产力功能，完善 WebSocket 实时推送，优化大目录性能。

**预估总工时：** 12-15 人天

---

### 任务 3.1：断点续传（F-016）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-016 |
| **前置** | v0.2 TLS + v0.1 WebSocket |
| **预估** | 3 人天 |
| **难度** | 高 |

**涉及文件：**
- `src-tauri/src/transfer/download.rs` — 修改下载逻辑支持断点续传
- `src-tauri/src/transfer/queue.rs` — 新增 `range` 字段（已下载偏移量）
- `src-tauri/src/server/http.rs` — 返回 Accept-Ranges 头并处理 Range 请求
- `src-tauri/src/transfer/resume.rs` — 可选：新建模块存放续传逻辑
- `src/types.ts` — 新增 TransferTask 的 resumed_from 字段（可选）
- `src-tauri/src/config/settings.rs` — 新增部分下载缓存目录

**技术方案：**

**服务端（HTTP）：**
- 响应头返回 `Accept-Ranges: bytes`
- 处理 `Range: bytes=<start>-` 请求头
- 使用 `tokio::fs::File::seek()` + `ReadBuf` 从指定位置开始读取

**客户端（下载）：**
- 下载前检查本地是否存在部分下载文件（`.ffeel.part`）
- 存在则读取已下载大小，发送 Range 请求
- 请求头：`Range: bytes=<downloaded>-`
- 保持部分下载文件在已知位置（如共享目录下的 `.temp/`）

**队列管理层：**
- `TransferTask` 新增 `resumed: bool` 和 `bytes_already_on_disk: u64`
- 暂停时将当前偏移量存入任务状态
- 恢复时重建传输上下文

```rust
// 服务端支持 Range
async fn download_file(
    State(state): State<AppState>,
    Query(query): Query<DownloadQuery>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppErrorResponse> {
    let range = headers.get("range").and_then(|v| v.to_str().ok());
    // 解析 range，使用 tokio::fs::File 的 seek
}
```

**验收标准：**
1. 暂停下载后继续，从断点处恢复而非重新开始
2. 应用重启后部分下载文件可继续下载
3. 支持 HTTP Range 请求
4. 暂停/继续循环多次仍能正确恢复

---

### 任务 3.2：文件夹传输（F-017）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-017 |
| **前置** | 任务 3.1（断点续传，文件夹需要文件级追踪） |
| **预估** | 3 人天 |

**涉及文件：**
- `src-tauri/src/transfer/folder.rs` — 新建模块：文件夹传输管理器
- `src-tauri/src/transfer/mod.rs` — 注册新模块
- `src-tauri/src/server/http.rs` — 新增文件夹 API（如 `/api/list-recursive`）
- `src-tauri/src/lib.rs` — 新增 `download_folder` / `upload_folder` 命令
- `src/pages/FileBrowser.tsx` — 支持文件夹选择和下载
- `src/pages/Transfers.tsx` — 显示文件夹传输的总进度
- `src/types.ts` — 新增文件夹任务类型

**技术方案：**

**下载文件夹流程：**
1. 前端选择远程设备的文件夹
2. 后端递归获取该文件夹下所有文件列表（新 API: `/api/list-recursive`）
3. 为每个文件创建一个子任务，归属同一个"文件夹传输组"
4. 显示总体进度：已完成文件数 / 总文件数 + 总字节进度
5. 保留目录结构下载到本地

**文件夹任务模型：**
```rust
pub struct FolderTransferTask {
    pub group_id: String,
    pub folder_name: String,
    pub total_files: u64,
    pub completed_files: u64,
    pub sub_tasks: Vec<TransferTask>,
    pub status: TransferStatus,
}
```

**前端显示：**
- 传输列表新增"文件夹"类型卡片
- 显示 `已完成 5/12 个文件` + 总体百分比
- 可展开查看各文件传输详情

**验收标准：**
1. 可下载远程设备的完整文件夹
2. 本地保留原始目录结构
3. 传输列表正确显示文件夹总进度
4. 文件夹传输支持暂停/继续

---

### 任务 3.3：传输限速（F-018）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-018 |
| **前置** | 无（独立任务） |
| **预估** | 1 人天 |

**涉及文件：**
- `src-tauri/src/transfer/download.rs` — 下载时引入令牌桶
- `src-tauri/src/transfer/upload.rs` — 上传时引入令牌桶
- `src-tauri/Cargo.toml` — 可选：新增 governor crate
- `src-tauri/src/config/settings.rs` — speed_limit 字段已存在

**技术方案：**

使用令牌桶算法（Token Bucket）限速，无需第三方 crate，手动实现：

```rust
/// 速率限制器
pub struct RateLimiter {
    capacity: u64,       // 桶容量 (bytes)
    tokens: u64,         // 当前令牌数
    refill_rate: u64,    // 每秒补充速率 (bytes/s)
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        Self {
            capacity: bytes_per_sec,
            tokens: bytes_per_sec,
            refill_rate: bytes_per_sec,
            last_refill: Instant::now(),
        }
    }

    /// 请求消耗 n 个字节，返回需要等待的时间
    pub async fn consume(&mut self, n: u64) {
        if self.refill_rate == 0 {
            return; // 0 = 不限速
        }
        loop {
            self.refill();
            if self.tokens >= n {
                self.tokens -= n;
                return;
            }
            let wait = Duration::from_secs_f64(
                (n - self.tokens) as f64 / self.refill_rate as f64
            );
            tokio::time::sleep(wait).await;
            self.refill();
        }
    }

    fn refill(&mut self) {
        let elapsed = self.last_refill.elapsed().as_secs_f64();
        let added = (elapsed * self.refill_rate as f64) as u64;
        self.tokens = (self.tokens + added).min(self.capacity);
        self.last_refill = Instant::now();
    }
}
```

在 `download.rs` 和 `upload.rs` 的循环中，每次写入前调用 `limiter.consume(chunk.len()).await`。

**设置页联动：**
- 当前 Settings 页已有 `speed_limit` 输入框（bytes/s）
- 保存设置后，限速器立即生效

**验收标准：**
1. 设置 `speed_limit = 1048576`（1 MB/s）后，传输速度稳定在 1 MB/s 以内
2. 不限速时 (`speed_limit = 0`) 不影响传输速度
3. 运行时修改限速参数立即生效

---

### 任务 3.4：文件名搜索（F-019）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-019 |
| **前置** | 无 |
| **预估** | 1 人天 |

**涉及文件：**
- `src-tauri/src/server/http.rs` — 新增 `/api/search` 端点
- `src/pages/FileBrowser.tsx` — 新增搜索栏 UI
- `src/types.ts` — 无需变更

**技术方案：**

**后端：**
新增 `/api/search?q=<keyword>&path=<base>` 端点，递归搜索目录匹配文件名：

```rust
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub path: Option<String>,
}

async fn search_files(
    state: axum::extract::State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<DirEntry>>, AppErrorResponse> {
    let base = state.share_dir.clone();
    let rel_path = query.path.unwrap_or_default();
    let full_path = resolve_safe_path(&base, &rel_path)?;
    
    let mut results = Vec::new();
    let mut walk = tokio::fs::read_dir(&full_path).await?;
    // 递归搜索（深度优先，限制最大深度 5 防止栈溢出）
    search_recursive(&full_path, &query.q, &mut results, 0).await;
    
    Ok(Json(results))
}
```

**前端：**
- 文件浏览页顶部增加搜索输入框
- 输入时防抖 300ms 后发送搜索请求
- 搜索结果列表可点击进入对应目录或下载

**验收标准：**
1. 输入关键词后实时搜索远程设备文件
2. 搜索结果包含文件名匹配的文件和目录
3. 点击搜索结果可导航到所在目录或直接下载
4. 搜索空关键词返回空结果

---

### 任务 3.5：WebSocket 实时进度全面接入（F-010 增强）

| 字段 | 内容 |
|------|------|
| **功能编号** | F-010 增强 |
| **前置** | 任务 1.5 |
| **预估** | 1.5 人天 |

**涉及文件：**
- `src-tauri/src/transfer/download.rs` — 每次进度回调时通过 WebSocket 广播
- `src-tauri/src/transfer/upload.rs` — 上传进度通过 WebSocket 广播
- `src-tauri/src/lib.rs` — 不再需要通过 Tauri 事件转发
- `src/App.tsx` — 添加 WebSocket 客户端连接
- `src/stores/appStore.ts` — 接收 WebSocket 消息更新状态

**技术方案：**

前端建立 WebSocket 连接到后端，替换或补充轮询机制：

```typescript
// 前端 WebSocket 客户端
function connectWebSocket(ip: string, port: number) {
  const ws = new WebSocket(`ws://${ip}:${port}/ws`);
  
  ws.onmessage = (event) => {
    const msg = JSON.parse(event.data);
    switch (msg.type) {
      case 'TransferProgress':
        updateTransferProgress(msg.data);
        break;
      case 'TransferComplete':
        handleTransferComplete(msg.data);
        break;
      case 'TransferError':
        handleTransferError(msg.data);
        break;
    }
  };
}
```

保留 Tauri 事件作为兜底（当 WebSocket 断开时回退到事件监听）。

**验收标准：**
1. 传输进度通过 WebSocket 实时推送，延迟 < 500ms
2. 前端不再依赖 2 秒轮询获取进度（保留作为降级方案）
3. WebSocket 断线自动重连
4. 事件推送和 WebSocket 推送不重复

---

### 任务 3.6：大目录性能优化

| 字段 | 内容 |
|------|------|
| **功能编号** | 新 |
| **前置** | 无 |
| **预估** | 0.5 人天 |

**涉及文件：**
- `src-tauri/src/server/http.rs` — list_directory 优化为分页或流式返回
- `src-tauri/src/server/http.rs` — 延迟加载 metadata

**技术方案：**

当前 `list_directory` 同步读取所有条目和 metadata。优化：
1. **延迟 metadata 获取：** 仅在需要时获取文件大小和时间
2. **目录优先排序：** 已实现
3. **分页加载：** 新增 `offset` 和 `limit` 参数（对大目录 >1000 项有用）
   ```rust
   #[derive(Debug, Deserialize)]
   pub struct DirQuery {
       pub path: Option<String>,
       pub offset: Option<usize>,
       pub limit: Option<usize>,
   }
   ```

**验收标准：**
1. 包含 2000+ 文件的目录列表响应 < 2 秒
2. 分页参数正常工作

---

## v1.0 正式发布

**目标：** 面向生产环境的发布版本。补齐体验短板（拖拽、系统托盘、多语言），建立 CI/CD 和测试体系。

**预估总工时：** 15-18 人天

---

### 任务 4.1：拖拽下载/上传

| 字段 | 内容 |
|------|------|
| **功能编号** | 新 |
| **前置** | v0.3 全部完成 |
| **预估** | 2 人天 |

**涉及文件：**
- `src/pages/FileBrowser.tsx` — 支持拖拽操作
- `src-tauri/src/lib.rs` — 新增 drag-drop 事件处理
- `src-tauri/tauri.conf.json` — 启用 drag-drop 功能
- `src/App.tsx` — 添加 drop zone 事件监听

**技术方案：**

**拖拽下载：** 从文件浏览器将文件拖到本地 Finder/Explorer
- 使用 Tauri 的 drag-drop 插件
- 拖拽时生成文件列表的元数据
- 放置时启动文件下载

**拖拽上传：** 从本地拖文件到文件浏览器窗口
- 使用 HTML5 Drag and Drop API
- 获取文件路径信息
- 触发上传流程

```json
// tauri.conf.json
{
  "app": {
    "security": {
      "dragDropEnabled": true
    }
  }
}
```

**验收标准：**
1. 从文件浏览器拖拽文件到本地文件夹，触发下载
2. 从本地文件夹拖拽文件到文件浏览器，触发上传
3. 拖拽操作有视觉反馈（拖入高亮）

---

### 任务 4.2：系统托盘

| 字段 | 内容 |
|------|------|
| **功能编号** | 新 |
| **前置** | 无 |
| **预估** | 2 人天 |

**涉及文件：**
- `src-tauri/src/tray.rs` — 新建模块：系统托盘逻辑
- `src-tauri/src/lib.rs` — 集成托盘
- `src-tauri/tauri.conf.json` — 配置托盘图标
- `src-tauri/tray-icon.png` — 托盘图标资源文件

**技术方案：**

使用 Tauri 的 SystemTray API：

```rust
use tauri::tray::{TrayIconBuilder, TrayIconEvent, MouseButton};
use tauri::menu::{MenuBuilder, MenuItemBuilder};

pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = MenuItemBuilder::with_id("show", "显示窗口").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;
    
    let menu = MenuBuilder::new(app)
        .item(&show_item)
        .item(&quit_item)
        .build()?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "show" => { /* 显示主窗口 */ }
                "quit" => { app.exit(0); }
                _ => {}
            }
        })
        .build(app)?;
    
    Ok(())
}
```

**功能：**
- 关闭窗口时最小化到托盘（而非退出）
- 托盘菜单：显示窗口 / 退出
- 托盘图标提示当前传输数

**验收标准：**
1. 点击窗口关闭按钮最小化到托盘而非退出
2. 托盘菜单可显示/隐藏主窗口
3. 托盘图标有可读提示

---

### 任务 4.3：多语言 i18n

| 字段 | 内容 |
|------|------|
| **功能编号** | 新 |
| **前置** | 无 |
| **预估** | 2 人天 |

**涉及文件：**
- `src/i18n/` — 新建国际化目录
  - `src/i18n/index.ts` — i18n 初始化
  - `src/i18n/zh-CN.json` — 中文翻译
  - `src/i18n/en-US.json` — 英文翻译
- `src/App.tsx` — 使用翻译 hook
- `src/pages/*.tsx` — 替换所有硬编码中文文本
- `src/stores/appStore.ts` — 新增语言状态
- `src-tauri/src/config/settings.rs` — 新增 `language` 字段

**技术方案：**

轻量方案（不引入 react-i18next 等重型依赖），自建简单 Hook：

```typescript
// src/i18n/index.ts
const messages: Record<string, Record<string, string>> = {
  'zh-CN': {
    'device.title': '局域网设备',
    'transfer.title': '传输管理',
    // ...
  },
  'en-US': {
    'device.title': 'LAN Devices',
    'transfer.title': 'Transfers',
    // ...
  },
};

export function useTranslation(lang: string) {
  const t = useCallback((key: string) => {
    return messages[lang]?.[key] ?? messages['zh-CN']?.[key] ?? key;
  }, [lang]);
  return { t };
}
```

**验收标准：**
1. 设置页可选择语言（中文/英文）
2. 切换语言后 UI 立即更新
3. 语言设置持久化保存

---

### 任务 4.4：自动更新

| 字段 | 内容 |
|------|------|
| **功能编号** | 新 |
| **前置** | CI/CD 就绪 |
| **预估** | 2 人天 |

**涉及文件：**
- `src-tauri/Cargo.toml` — 新增 `tauri-plugin-updater`
- `src-tauri/tauri.conf.json` — 配置更新服务地址
- `src-tauri/src/lib.rs` — 集成更新插件
- `scripts/updater-server/` — 可选：自建更新服务器

**技术方案：**

使用 Tauri 官方 updater 插件：
```json
{
  "plugins": {
    "updater": {
      "pubkey": "...",
      "endpoints": ["https://releases.ffeel.app/{{target}}/{{current_version}}"],
      "dialog": true
    }
  }
}
```

发布流程：
1. CI 构建后上传到更新服务器
2. 生成签名和更新清单
3. 应用启动时检查更新
4. 有新版时弹出更新提示

**验收标准：**
1. 应用启动时检查是否有新版本
2. 有新版时弹出"发现新版本"提示
3. 用户确认后下载并安装更新
4. 更新后正确重启

---

### 任务 4.5：CI/CD 流水线

| 字段 | 内容 |
|------|------|
| **功能编号** | 新 |
| **前置** | 无 |
| **预估** | 2 人天 |

**涉及文件：**
- `.github/workflows/build.yml` — 新建 CI/CD 配置
- `.github/workflows/release.yml` — 新建发布配置
- `scripts/notarize.sh` — macOS 公证脚本
- `scripts/sign.ps1` — Windows 签名脚本

**技术方案：**

GitHub Actions 配置：

```yaml
name: Build
on: [push, pull_request]

jobs:
  build:
    strategy:
      matrix:
        os: [macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rust-lang/setup-rust-toolchain@v1
      - run: npm ci
      - run: npm run tauri build
      - uses: actions/upload-artifact@v4
        with:
          path: src-tauri/target/release/bundle/*
```

发布流水线额外处理：
- macOS 公证（notarization）
- Windows 代码签名
- GitHub Release 发布
- 更新清单生成

**验收标准：**
1. PR 提交后自动触发构建和测试
2. Release 标签推送后自动生成安装包
3. 构建产物包含 .app / .dmg（macOS）和 .msi（Windows）

---

### 任务 4.6：测试覆盖

| 字段 | 内容 |
|------|------|
| **功能编号** | 新 |
| **前置** | v0.3 全部完成 |
| **预估** | 3-4 人天 |

**涉及文件：**
- `src-tauri/tests/` — Rust 集成测试目录
- `src-tauri/src/transfer/queue.rs` — 新增单元测试
- `src-tauri/src/security/pairing.rs` — 新增单元测试
- `src-tauri/src/server/http.rs` — 新增集成测试
- `src/__tests__/` — 前端测试目录
- `vitest.config.ts` — 前端测试配置

**技术方案：**

**Rust 后端测试：**

单元测试（`#[cfg(test)]` 模块）：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_queue_basic() {
        let mut mgr = TransferManager::new(3);
        assert_eq!(mgr.active_count(), 0);
        // ...
    }

    #[test]
    fn test_history_limit() {
        // 测试历史上限控制
    }

    #[test]
    fn test_resolve_safe_path_blocks_escape() {
        assert!(resolve_safe_path(&base, "../etc/passwd").is_err());
    }

    #[test]
    fn test_pairing_code_generation() {
        let code = PairingManager::generate_pairing_code();
        assert_eq!(code.len(), 4);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }
}
```

集成测试（`tests/` 目录）：
- 启动 HTTP 服务实例，发送真实请求
- 测试目录列表、文件下载、文件上传、路径安全

**前端测试：**
- 使用 Vitest + React Testing Library
- 组件渲染测试
- 事件处理测试
- 状态流转测试

```typescript
// App.test.tsx 示例
describe('App', () => {
  it('renders four tabs', () => {
    render(<App />);
    expect(screen.getByText('设备')).toBeInTheDocument();
    expect(screen.getByText('文件')).toBeInTheDocument();
    expect(screen.getByText('传输')).toBeInTheDocument();
    expect(screen.getByText('设置')).toBeInTheDocument();
  });
});
```

**验收标准：**
1. Rust 单元测试覆盖率 > 60%
2. 所有测试通过（`cargo test`）
3. 关键路径（路径安全、配对逻辑、队列状态机）有 100% 分支覆盖
4. 前端核心组件有渲染测试

---

## 附录：关键技术决策

### 依赖管理决策

| 场景 | 方案 | 理由 |
|------|------|------|
| TLS 证书 | rcgen + rustls | 纯 Rust TLS 栈，无需 OpenSSL |
| 令牌桶限速 | 自实现 | 无需外部依赖，逻辑简单可控 |
| i18n | 自建 Hook | 避免 react-i18next 等重型依赖 |
| 自动更新 | tauri-plugin-updater | Tauri 官方维护，集成度高 |
| 测试框架 | cargo test + Vitest | 各自生态的标准选择 |

### 渐进式 WebSocket 策略

当前 Tauri 事件（`app.emit()`）已经实现了 Rust→前端通信。WebSocket 将逐步引入：

1. **v0.1 收尾：** WebSocket 骨架接入路由器，实现基本连接和广播
2. **v0.2：** 传输进度通过 WebSocket 推送
3. **v0.3：** 前端全面迁移到 WebSocket 实时更新，Tauri 事件作为降级

### 多平台注意事项

| 平台 | 配置目录 | 特别关注 |
|------|----------|----------|
| macOS | `~/Library/Application Support/ffeel/` | 需要公证、代码签名 |
| Windows | `%APPDATA%/ffeel/` | 需要代码签名、MSI 打包 |

### 当前代码需立即修复的缺陷

1. 硬编码下载路径: `FileBrowser.tsx:77` — `savePath` 写死了 `/Users/zhushuiqing/Downloads/`
2. WebSocket 未接入路由器: `http.rs` 未注册 `/ws` 路由
3. 上传未注册 Tauri 命令: `lib.rs` 缺少 `upload_file` 命令
4. speed_limit 未生效: 设置项无对应限速逻辑
5. PairingManager verify 为 stub: `verify_pairing_code()` 仅检查长度
6. 无设置持久化: 内存存储，重启丢失
7. 无日志分类: 日志级别固定为 `info`

---

## 工时汇总

| 阶段 | 人天数 | 主要交付物 |
|------|--------|-----------|
| v0.1 收尾 | 4-5 | 设置持久化、历史上限、下载路径、WebSocket 骨架 |
| v0.2 安全增强 | 10-12 | TLS 加密、设备配对、信任管理、上传 UI |
| v0.3 功能完善 | 12-15 | 断点续传、文件夹传输、限速、搜索、性能优化 |
| v1.0 正式发布 | 15-18 | 拖拽交互、系统托盘、i18n、自动更新、CI/CD、测试 |
| **总计** | **41-50** | |

> 实际工时取决于开发环境配置（如 CI/CD 平台设置、macOS 公证配置等）和开发并行度。建议按 v0.1 → v0.2 → v0.3 → v1.0 顺序推进，每个阶段结束时有可发布的版本。
