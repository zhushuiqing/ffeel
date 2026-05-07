use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    extract::State,
};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::remote::{ClipboardSync, InputEvent};
use crate::server::http::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WsMessage {
    TransferProgress {
        id: String,
        file_name: String,
        bytes_transferred: u64,
        total_bytes: u64,
        speed: f64,
    },
    TransferComplete {
        id: String,
        file_name: String,
    },
    TransferError {
        id: String,
        file_name: String,
        error: String,
    },
    DeviceStatus {
        device_id: String,
        online: bool,
    },
    ChatMessage {
        from_id: String,
        from_name: String,
        text: String,
        timestamp: String,
        #[serde(default)]
        message_type: String,
        #[serde(default)]
        file_name: Option<String>,
        #[serde(default)]
        file_size: Option<u64>,
        #[serde(default)]
        file_id: Option<String>,
        #[serde(default)]
        file_type: Option<String>,
        #[serde(default)]
        to_id: Option<String>,
    },
    // == Remote Control ==
    /// 请求开始远程控制 (控制端 → 被控端)
    RemoteControlRequest {
        controller_device_id: String,
        controller_name: String,
        #[serde(default = "default_rc_quality")]
        quality: String,
        #[serde(default = "default_rc_fps")]
        fps: u32,
    },
    /// 远程控制请求响应 (被控端 → 控制端)
    RemoteControlResponse {
        accepted: bool,
        reason: Option<String>,
        screen_width: Option<u32>,
        screen_height: Option<u32>,
    },
    /// 停止远程控制
    RemoteControlStop {
        reason: Option<String>,
    },
    /// 输入事件 (控制端 → 被控端)
    InputEvent {
        event_type: String,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
        #[serde(default)]
        button: Option<String>,
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        delta_x: Option<i32>,
        #[serde(default)]
        delta_y: Option<i32>,
    },
    /// 帧率统计 (被控端 → 控制端)
    RemoteControlStats {
        fps: f64,
        bytes_per_sec: f64,
        frame_count: u64,
    },
}

fn default_rc_quality() -> String { "medium".to_string() }
fn default_rc_fps() -> u32 { 15 }

/// 聊天消息记录（含文件消息支持）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatEntry {
    pub from_id: String,
    pub from_name: String,
    pub text: String,
    pub timestamp: String,
    #[serde(default)]
    pub message_type: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub file_type: Option<String>,
    #[serde(default)]
    pub to_id: Option<String>,
}

/// WebSocket 连接跟踪器 —— 追踪当前哪些设备在线
#[derive(Debug, Clone, Default)]
pub struct WsConnectionTracker {
    inner: Arc<RwLock<HashMap<String, Instant>>>,
}

impl WsConnectionTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, device_id: String) {
        self.inner.write().await.insert(device_id, Instant::now());
    }

    pub async fn unregister(&self, device_id: &str) {
        self.inner.write().await.remove(device_id);
    }

    pub async fn heartbeat(&self, device_id: &str) {
        if let Some(entry) = self.inner.write().await.get_mut(device_id) {
            *entry = Instant::now();
        }
    }

    pub async fn is_online(&self, device_id: &str) -> bool {
        self.inner.read().await.contains_key(device_id)
    }

    pub async fn get_online_devices(&self) -> Vec<String> {
        let mut map = self.inner.write().await;
        let cutoff = Instant::now() - std::time::Duration::from_secs(35);
        map.retain(|_, last_seen| *last_seen > cutoff);
        map.keys().cloned().collect()
    }
}

/// 聊天消息存储器
#[derive(Debug, Clone)]
pub struct ChatStore {
    messages: Arc<Mutex<Vec<ChatEntry>>>,
    persist_path: Arc<Mutex<Option<std::path::PathBuf>>>,
}

impl ChatStore {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
            persist_path: Arc::new(Mutex::new(None)),
        }
    }

    /// 设置持久化路径
    pub async fn set_persist_path(&self, path: std::path::PathBuf) {
        *self.persist_path.lock().await = Some(path);
    }

    /// 从磁盘加载
    pub async fn load_from_disk(&self, path: &std::path::Path) {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            if let Ok(mut msgs) = serde_json::from_str::<Vec<ChatEntry>>(&content) {
                let mut store = self.messages.lock().await;
                store.append(&mut msgs);
                let len = store.len();
                if len > 200 {
                    store.drain(0..len - 200);
                }
            }
        }
    }

    /// 保存到磁盘
    pub async fn save_to_disk(&self) {
        let path = self.persist_path.lock().await.clone();
        if let Some(path) = path {
            if let Some(parent) = path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let msgs = self.messages.lock().await;
            if let Ok(json) = serde_json::to_string_pretty(&*msgs) {
                let _ = tokio::fs::write(&path, &json).await;
            }
        }
    }

    pub async fn add_message(
        &self,
        from_id: String,
        from_name: String,
        text: String,
        message_type: String,
        file_name: Option<String>,
        file_size: Option<u64>,
        file_id: Option<String>,
        file_type: Option<String>,
        to_id: Option<String>,
    ) -> ChatEntry {
        let entry = ChatEntry {
            from_id,
            from_name,
            text,
            timestamp: Utc::now().to_rfc3339(),
            message_type,
            file_name,
            file_size,
            file_id,
            file_type,
            to_id,
        };
        let mut msgs = self.messages.lock().await;
        msgs.push(entry.clone());
        if msgs.len() > 200 {
            msgs.remove(0);
        }
        // 每添加一条消息自动持久化
        if let Some(path) = self.persist_path.lock().await.clone() {
            if let Ok(json) = serde_json::to_string_pretty(&*msgs) {
                let _ = tokio::fs::write(&path, &json).await;
            }
        }
        entry
    }

    pub async fn get_messages(&self) -> Vec<ChatEntry> {
        self.messages.lock().await.clone()
    }

    pub async fn clear(&self) {
        self.messages.lock().await.clear();
    }
}

/// WebSocket 升级处理
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let rx = state.ws_tx.subscribe();
    let ws_tx = state.ws_tx.clone();
    let chat_store = state.chat_store.clone();
    let ws_tracker = state.ws_tracker.clone();
    let clipboard_sync = state.clipboard_sync.clone();
    ws.on_upgrade(move |socket| handle_ws_connection(socket, rx, ws_tx, chat_store, ws_tracker, clipboard_sync))
}

async fn handle_ws_connection(
    socket: WebSocket,
    mut rx: broadcast::Receiver<WsMessage>,
    ws_tx: broadcast::Sender<WsMessage>,
    chat_store: ChatStore,
    ws_tracker: WsConnectionTracker,
    clipboard_sync: ClipboardSync,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
    let mut registered_device_id: Option<String> = None;
    // 清理函数：断开时注销设备
    let cleanup = |tracker: WsConnectionTracker, dev_id: Option<String>, tx: broadcast::Sender<WsMessage>| async move {
        if let Some(id) = dev_id {
            tracker.unregister(&id).await;
            let _ = tx.send(WsMessage::DeviceStatus {
                device_id: id,
                online: false,
            });
        }
    };

    loop {
        tokio::select! {
            Some(msg) = receiver.next() => {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                            let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            match msg_type {
                                "register" => {
                                    // 客户端注册：关联 device_id 与 WebSocket 连接
                                    let device_id = val.get("device_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    if !device_id.is_empty() {
                                        ws_tracker.register(device_id.clone()).await;
                                        registered_device_id = Some(device_id.clone());
                                        let _ = ws_tx.send(WsMessage::DeviceStatus {
                                            device_id,
                                            online: true,
                                        });
                                    }
                                }
                                "chat" => {
                                    let from_id = val.get("from_id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                    let from_name = val.get("from_name").and_then(|v| v.as_str()).unwrap_or("未知").to_string();
                                    let chat_text = val.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();

                                    if !chat_text.is_empty() {
                                        let entry = chat_store.add_message(
                                            from_id.clone(), from_name.clone(), chat_text,
                                            "text".to_string(), None, None, None, None, None,
                                        ).await;
                                        let _ = ws_tx.send(WsMessage::ChatMessage {
                                            from_id: entry.from_id,
                                            from_name: entry.from_name,
                                            text: entry.text,
                                            timestamp: entry.timestamp,
                                            message_type: "text".to_string(),
                                            file_name: None,
                                            file_size: None,
                                            file_id: None,
                                            file_type: None,
                                            to_id: None,
                                        });
                                    }
                                }
                                "ping" => {
                                    // 心跳回复
                                    if let Some(ref id) = registered_device_id {
                                        ws_tracker.heartbeat(id).await;
                                    }
                                }
                                "input" => {
                                    // 远程控制输入事件处理（使用 spawn_blocking 执行）
                                    let input_event = crate::remote::InputEvent {
                                        event_type: val.get("event_type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                        x: val.get("x").and_then(|v| v.as_i64()).map(|v| v as i32),
                                        y: val.get("y").and_then(|v| v.as_i64()).map(|v| v as i32),
                                        button: val.get("button").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        key: val.get("key").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        delta: val.get("delta_y").and_then(|v| v.as_i64()).map(|v| v as i32),
                                        text: val.get("text").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        keys: val.get("keys").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                    };
                                    tokio::task::spawn_blocking(move || {
                                        let _ = crate::remote::handle_input_event(&input_event);
                                    });
                                }
                                "clipboard" => {
                                    // 剪贴板同步：接收远程剪贴板内容并设置到本地
                                    let content = val.get("content").and_then(|v| v.as_str()).unwrap_or("");
                                    if !content.is_empty() {
                                        let sync = clipboard_sync.clone();
                                        let content_str = content.to_string();
                                        tokio::spawn(async move {
                                            let _ = sync.set_text(&content_str).await;
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(Message::Ping(d)) => {
                        let _ = sender.send(Message::Pong(d)).await;
                    }
                    _ => {}
                }
            }
            Ok(msg) = rx.recv() => {
                match &msg {
                    WsMessage::ChatMessage { .. } => {
                        tracing::trace!("WebSocket 转发 ChatMessage");
                    }
                    _ => {}
                }
                if let Ok(text) = serde_json::to_string(&msg) {
                    if sender.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
            }
            _ = interval.tick() => {
                if sender.send(Message::Ping(vec![])).await.is_err() {
                    break;
                }
                // 心跳更新
                if let Some(ref id) = registered_device_id {
                    ws_tracker.heartbeat(id).await;
                }
            }
        }
    }

    // 连接断开，注销设备
    cleanup(ws_tracker, registered_device_id, ws_tx.clone()).await;
}
