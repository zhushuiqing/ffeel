use axum::{
    body::Body,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json},
    routing::{delete, get, post},
    Router,
};
use std::convert::TryFrom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use tokio_rustls::TlsAcceptor;
use tower::ServiceExt;
use tower_http::cors::CorsLayer;

use crate::security::pairing::SharedPairingManager;
use crate::server::ws::{ChatStore, WsConnectionTracker, WsMessage};
use crate::transfer::queue::{TransferManager, TransferTask};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub size: Option<u64>,
    pub modified_at: Option<String>,
    pub is_dir: bool,
}

#[derive(Debug, Deserialize)]
pub struct DirQuery {
    pub path: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DownloadQuery {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct UploadQuery {
    pub path: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransferIdParam {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct TrustedDeviceParam {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct SettingsUpdate {
    pub require_pairing: Option<bool>,
    pub max_concurrent_transfers: Option<usize>,
    pub speed_limit: Option<u64>,
    pub max_retries: Option<u32>,
}

#[derive(Clone)]
pub struct AppState {
    /// 可动态更新的共享目录（Arc<RwLock> 允许运行时修改）
    pub share_dir: Arc<RwLock<PathBuf>>,
    pub transfer_manager: Arc<Mutex<TransferManager>>,
    pub ws_tx: broadcast::Sender<WsMessage>,
    pub pairing: SharedPairingManager,
    pub require_pairing: bool,
    pub chat_store: ChatStore,
    pub ws_tracker: WsConnectionTracker,
}

/// 构建 HTTP 路由
pub fn build_router(
    share_dir: Arc<RwLock<PathBuf>>,
    transfer_manager: Arc<Mutex<TransferManager>>,
    ws_tx: broadcast::Sender<WsMessage>,
    pairing: SharedPairingManager,
    require_pairing: bool,
    chat_store: ChatStore,
    ws_tracker: WsConnectionTracker,
) -> Router {
    let state = AppState {
        share_dir,
        transfer_manager,
        ws_tx,
        pairing,
        require_pairing,
        chat_store,
        ws_tracker,
    };

    Router::new()
        .route("/", get(web_ui_handler))
        .route("/api/list", get(list_directory))
        .route("/api/download", get(download_file))
        .route("/api/upload", post(upload_file))
        .route("/api/search", get(search_files))
        .route("/api/pair", post(pair_device_handler))
        .route("/api/pair/code", get(get_pairing_code_handler))
        .route("/api/pair/rotate", post(rotate_pairing_code_handler))
        .route("/api/pair/trusted", get(list_trusted_devices_handler))
        .route("/api/pair/trusted/:id", delete(remove_trusted_device_handler))
        .route("/api/pair/nickname/:id", post(set_device_nickname_handler))
        .route("/api/transfers", get(list_transfers_handler))
        .route("/api/transfers/pause/:id", post(pause_transfer_handler))
        .route("/api/transfers/resume/:id", post(resume_transfer_handler))
        .route("/api/transfers/cancel/:id", post(cancel_transfer_handler))
        .route("/api/settings", get(get_settings_handler).put(update_settings_handler))
        .route("/api/chat/messages", get(get_chat_messages_handler))
        .route("/api/chat/send", post(send_chat_message_handler))
        .route("/api/chat/upload", post(upload_chat_file_handler))
        .route("/api/chat/download/:file_id", get(download_chat_file_handler))
        .route("/api/chat/online", get(get_online_devices_handler))
        .route("/api/pair/check", get(check_pairing_handler))
        .route("/api/health", get(health_check))
        .route("/ws", get(crate::server::ws::ws_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            pairing_middleware,
        ))
        // 分享链接端点（独立认证，不受 pairing_middleware 影响）
        .route("/s/:code/*path", get(share_download_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// 配对检查中间件（跳过 /api/health, /api/pair）
async fn pairing_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<impl IntoResponse, AppErrorResponse> {
    if state.require_pairing {
        let path = req.uri().path();
        if path == "/" || path == "/api/health" || path == "/api/pair" || path == "/api/pair/check" || path == "/api/chat/online" {
            return Ok(next.run(req).await);
        }
        let device_id = req
            .headers()
            .get("x-device-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !state.pairing.is_trusted(device_id).await {
            return Err(AppErrorResponse((
                StatusCode::FORBIDDEN,
                "设备未配对".to_string(),
            )));
        }
    }
    Ok(next.run(req).await)
}

/// 配对处理端点
async fn pair_device_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppErrorResponse> {
    let device_id = headers
        .get("x-device-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let code = headers
        .get("x-pairing-code")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let device_name = headers
        .get("x-device-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("未知设备");

    if device_id.is_empty() || code.is_empty() {
        return Err(AppErrorResponse((
            StatusCode::BAD_REQUEST,
            "缺少设备 ID 或配对码".to_string(),
        )));
    }

    if state.pairing.verify_and_trust_with_name(device_id, code, device_name).await {
        // 持久化信任设备到磁盘
        let mut settings = crate::config::Settings::load();
        state.pairing.save_to_settings(&mut settings).await;
        let _ = settings.save();
        Ok(Json(serde_json::json!({ "status": "paired" })))
    } else {
        Err(AppErrorResponse((
            StatusCode::FORBIDDEN,
            "配对码无效".to_string(),
        )))
    }
}

/// 列出目录内容
async fn list_directory(
    state: axum::extract::State<AppState>,
    Query(query): Query<DirQuery>,
) -> Result<Json<Vec<DirEntry>>, AppErrorResponse> {
    let base = state.share_dir.read().unwrap().clone();
    let rel_path = query.path.unwrap_or_default();
    let full_path = resolve_safe_path(&base, &rel_path)?;

    if !full_path.is_dir() {
        return Err(AppErrorResponse((
            StatusCode::BAD_REQUEST,
            "路径不是目录".to_string(),
        )));
    }

    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&full_path).await.map_err(|e| {
        AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    })?;

    while let Some(entry) = read_dir.next_entry().await.map_err(|e| {
        AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    })? {
        let metadata = entry.metadata().await.ok();
        let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);

        entries.push(DirEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            entry_type: if is_dir {
                "directory".to_string()
            } else {
                "file".to_string()
            },
            size: metadata.as_ref().map(|m| m.len()),
            modified_at: metadata
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs().to_string())
                }),
            is_dir,
        });
    }

    entries.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            b.is_dir.cmp(&a.is_dir)
        } else {
            a.name.cmp(&b.name)
        }
    });

    // 分页
    let entries = if let Some(offset) = query.offset {
        let start = offset.min(entries.len());
        if let Some(limit) = query.limit {
            entries[start..start + limit.min(entries.len() - start)].to_vec()
        } else {
            entries[start..].to_vec()
        }
    } else if let Some(limit) = query.limit {
        entries[..limit.min(entries.len())].to_vec()
    } else {
        entries
    };

    Ok(Json(entries))
}

/// 下载文件（支持断点续传 Range）
async fn download_file(
    state: axum::extract::State<AppState>,
    Query(query): Query<DownloadQuery>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppErrorResponse> {
    let base = state.share_dir.read().unwrap().clone();
    let full_path = resolve_safe_path(&base, &query.path)?;

    if !full_path.exists() {
        return Err(AppErrorResponse((
            StatusCode::NOT_FOUND,
            "文件不存在".to_string(),
        )));
    }
    if full_path.is_dir() {
        return Err(AppErrorResponse((
            StatusCode::BAD_REQUEST,
            "不能下载目录".to_string(),
        )));
    }

    let filename = full_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let mime = mime_guess::from_path(&filename).first_or_octet_stream();
    let file_len = tokio::fs::metadata(&full_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let (start, end) = if let Some(range_str) = headers.get("range").and_then(|v| v.to_str().ok()) {
        if let Some(pos) = range_str.strip_prefix("bytes=") {
            if let Some((start_str, _end_str)) = pos.split_once('-') {
                if let Ok(s) = start_str.parse::<u64>() {
                    (s, file_len.saturating_sub(1))
                } else {
                    (0, file_len.saturating_sub(1))
                }
            } else {
                (0, file_len.saturating_sub(1))
            }
        } else {
            (0, file_len.saturating_sub(1))
        }
    } else {
        (0, file_len.saturating_sub(1))
    };

    use tokio::io::AsyncSeekExt;
    let mut file = tokio::fs::File::open(&full_path).await.map_err(|e| {
        AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    })?;

    if start > 0 {
        file.seek(tokio::io::SeekFrom::Start(start)).await.map_err(|e| {
            AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        })?;
    }

    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let content_length = end.saturating_sub(start) + 1;
    let is_range = start > 0;

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::try_from(mime.to_string().as_str()).unwrap());
    headers.insert(
        "Content-Disposition",
        HeaderValue::try_from(format!("attachment; filename=\"{}\"", filename).as_str()).unwrap(),
    );
    headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));

    let status = if is_range {
        headers.insert(
            "Content-Range",
            HeaderValue::try_from(format!("bytes {}-{}/{}", start, end, file_len).as_str()).unwrap(),
        );
        headers.insert(
            "Content-Length",
            HeaderValue::try_from(content_length.to_string().as_str()).unwrap(),
        );
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };

    Ok((status, headers, body))
}

/// 上传文件（流式接收）
async fn upload_file(
    state: axum::extract::State<AppState>,
    Query(query): Query<UploadQuery>,
    body: Body,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let base = state.share_dir.read().unwrap().clone();
    let rel_path = query.path.unwrap_or_default();
    let target_dir = resolve_safe_path(&base, &rel_path)?;

    if !target_dir.exists() {
        tokio::fs::create_dir_all(&target_dir).await.map_err(|e| {
            AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        })?;
    }

    let filename = query.name.unwrap_or_else(|| "upload".to_string());
    // 消毒文件名，防止路径穿越
    let filename = filename.replace("..", "").replace('/', "_").replace('\\', "_");
    let save_path = target_dir.join(&filename);

    let mut file = tokio::fs::File::create(&save_path).await.map_err(|e| {
        AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    })?;

    let mut stream = body.into_data_stream();
    let mut saved = 0u64;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            AppErrorResponse((StatusCode::BAD_REQUEST, e.to_string()))
        })?;
        file.write_all(&chunk).await.map_err(|e| {
            AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        })?;
        saved += chunk.len() as u64;
    }

    Ok(Json(serde_json::json!({
        "saved_bytes": saved,
        "filename": filename,
    })))
}

/// 搜索文件
async fn search_files(
    state: axum::extract::State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<DirEntry>>, AppErrorResponse> {
    if query.q.is_empty() {
        return Ok(Json(Vec::new()));
    }
    let base = state.share_dir.read().unwrap().clone();
    let rel_path = query.path.unwrap_or_default();
    let full_path = resolve_safe_path(&base, &rel_path)?;
    let mut results = Vec::new();
    let pattern = query.q.to_lowercase();
    search_recursive(&full_path, &pattern, &mut results, 0).await;
    results.truncate(100);
    Ok(Json(results))
}

async fn search_recursive(
    root: &std::path::Path,
    pattern: &str,
    results: &mut Vec<DirEntry>,
    max_depth: usize,
) {
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir_path, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        let mut read_dir = match tokio::fs::read_dir(&dir_path).await {
            Ok(r) => r,
            Err(_) => continue,
        };
        while let Some(entry) = read_dir.next_entry().await.unwrap_or_else(|e| {
            tracing::warn!("读取目录项失败: {}", e);
            None
        }) {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            if name.to_lowercase().contains(pattern) {
                let metadata = entry.metadata().await.ok();
                results.push(DirEntry {
                    name,
                    entry_type: if is_dir { "directory".to_string() } else { "file".to_string() },
                    size: metadata.as_ref().map(|m| m.len()),
                    modified_at: metadata.as_ref().and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs().to_string())),
                    is_dir,
                });
            }
            if is_dir {
                stack.push((entry.path(), depth + 1));
            }
        }
    }
}

/// 检查设备配对状态（免认证）
async fn check_pairing_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    let device_id = headers.get("x-device-id").and_then(|v| v.to_str().ok()).unwrap_or("");
    let trusted = state.pairing.is_trusted(device_id).await;
    Ok(Json(serde_json::json!({
        "trusted": trusted,
        "require_pairing": state.require_pairing,
    })))
}

/// 健康检查
async fn health_check() -> Json<serde_json::value::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

/// 一键分享下载：通过配对码验证后直接下载文件（无配对认证）
/// URL: /s/:code/*path
async fn share_download_handler(
    State(state): State<AppState>,
    Path((code, path)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppErrorResponse> {
    let current_code = state.pairing.get_current_code().await;
    match current_code {
        Some(ref c) if c == &code => {
            let base = state.share_dir.read().unwrap().clone();
            let full_path = resolve_safe_path(&base, &path)?;

            if !full_path.exists() {
                return Err(AppErrorResponse((StatusCode::NOT_FOUND, "文件不存在".to_string())));
            }
            if full_path.is_dir() {
                return Err(AppErrorResponse((StatusCode::BAD_REQUEST, "不能分享目录".to_string())));
            }

            let filename = full_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();

            let mime = mime_guess::from_path(&filename).first_or_octet_stream();

            let file = tokio::fs::File::open(&full_path).await.map_err(|e| {
                AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            })?;

            let stream = tokio_util::io::ReaderStream::new(file);
            let body = Body::from_stream(stream);

            let mut headers = HeaderMap::new();
            headers.insert(
                "Content-Type",
                HeaderValue::try_from(mime.to_string().as_str()).unwrap(),
            );
            headers.insert(
                "Content-Disposition",
                HeaderValue::try_from(format!("attachment; filename=\"{}\"", filename).as_str()).unwrap(),
            );

            Ok((StatusCode::OK, headers, body))
        }
        _ => Err(AppErrorResponse((StatusCode::FORBIDDEN, "分享链接已失效".to_string()))),
    }
}

// ============== Transfer Endpoints ==============

/// 获取传输列表
async fn list_transfers_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<TransferTask>>, AppErrorResponse> {
    let tm = state.transfer_manager.lock().await;
    Ok(Json(tm.list_tasks()))
}

/// 暂停传输
async fn pause_transfer_handler(
    State(state): State<AppState>,
    Path(param): Path<TransferIdParam>,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    let mut tm = state.transfer_manager.lock().await;
    tm.pause_task(&param.id).map_err(|e| {
        AppErrorResponse((StatusCode::BAD_REQUEST, e.message))
    })?;
    Ok(Json(serde_json::json!({"status": "paused"})))
}

/// 恢复传输
async fn resume_transfer_handler(
    State(state): State<AppState>,
    Path(param): Path<TransferIdParam>,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    let mut tm = state.transfer_manager.lock().await;
    tm.resume_task(&param.id).map_err(|e| {
        AppErrorResponse((StatusCode::BAD_REQUEST, e.message))
    })?;
    Ok(Json(serde_json::json!({"status": "resumed"})))
}

/// 取消传输
async fn cancel_transfer_handler(
    State(state): State<AppState>,
    Path(param): Path<TransferIdParam>,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    let mut tm = state.transfer_manager.lock().await;
    tm.cancel_task(&param.id);
    Ok(Json(serde_json::json!({"status": "cancelled"})))
}

// ============== Pairing Management Endpoints ==============

/// 获取当前配对码
async fn get_pairing_code_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    let code = state.pairing.get_current_code().await;
    Ok(Json(serde_json::json!({
        "code": code.unwrap_or_else(|| "----".to_string())
    })))
}

/// 刷新配对码
async fn rotate_pairing_code_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    let code = state.pairing.rotate_code().await;
    // 持久化到磁盘
    let mut settings = crate::config::Settings::load();
    settings.pairing_code = Some(code.clone());
    let _ = settings.save();
    Ok(Json(serde_json::json!({"code": code})))
}

/// 获取信任设备列表（含名称）
async fn list_trusted_devices_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, AppErrorResponse> {
    let devices = state.pairing.trusted_devices().await;
    let result: Vec<serde_json::Value> = devices.into_iter().map(|(id, entry)| {
        serde_json::json!({
            "id": id,
            "name": entry.name,
            "nickname": entry.nickname,
            "paired_at": entry.paired_at,
        })
    }).collect();
    Ok(Json(result))
}

/// 移除信任设备
async fn remove_trusted_device_handler(
    State(state): State<AppState>,
    Path(param): Path<TrustedDeviceParam>,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    state.pairing.untrust_device(&param.id).await;
    // 持久化到磁盘
    let mut settings = crate::config::Settings::load();
    state.pairing.save_to_settings(&mut settings).await;
    let _ = settings.save();
    Ok(Json(serde_json::json!({"status": "removed"})))
}

/// 设置设备昵称
async fn set_device_nickname_handler(
    State(state): State<AppState>,
    Path(param): Path<TrustedDeviceParam>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    let nickname = body.get("nickname").and_then(|v| v.as_str()).unwrap_or("");
    state.pairing.set_nickname(&param.id, nickname).await;
    let mut settings = crate::config::Settings::load();
    state.pairing.save_to_settings(&mut settings).await;
    let _ = settings.save();
    Ok(Json(serde_json::json!({"status": "updated", "nickname": nickname})))
}

// ============== Settings Endpoints ==============

/// 获取设置
async fn get_settings_handler(
    State(_state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    let settings = crate::config::Settings::load();
    Ok(Json(serde_json::json!({
        "share_dir": settings.share_dir,
        "port": settings.port,
        "device_name": settings.device_name,
        "require_pairing": settings.require_pairing,
        "download_dir": settings.download_dir,
        "max_concurrent_transfers": settings.max_concurrent_transfers,
        "speed_limit": settings.speed_limit,
        "max_retries": settings.max_retries,
    })))
}

/// 更新设置
async fn update_settings_handler(
    State(_state): State<AppState>,
    Json(body): Json<SettingsUpdate>,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    let mut settings = crate::config::Settings::load();
    if let Some(v) = body.require_pairing {
        settings.require_pairing = v;
    }
    if let Some(v) = body.max_concurrent_transfers {
        settings.max_concurrent_transfers = v;
    }
    if let Some(v) = body.speed_limit {
        settings.speed_limit = v;
    }
    if let Some(v) = body.max_retries {
        settings.max_retries = v;
    }
    settings.save().map_err(|e| {
        AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    })?;
    Ok(Json(serde_json::json!({"status": "saved"})))
}

// ============== Chat Endpoints ==============

/// 获取聊天消息历史
async fn get_chat_messages_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::server::ws::ChatEntry>>, AppErrorResponse> {
    let messages = state.chat_store.get_messages().await;
    Ok(Json(messages))
}

/// 发送聊天消息（HTTP 方式，备选）
async fn send_chat_message_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    let device_id = headers.get("x-device-id").and_then(|v| v.to_str().ok()).unwrap_or("unknown");
    let device_name = headers.get("x-device-name").and_then(|v| v.to_str().ok()).unwrap_or("未知");
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let to_id = body.get("to_id").and_then(|v| v.as_str()).map(|s| s.to_string());

    if text.is_empty() {
        return Err(AppErrorResponse((StatusCode::BAD_REQUEST, "消息不能为空".to_string())));
    }

    let entry = state.chat_store.add_message(
        device_id.to_string(),
        device_name.to_string(),
        text,
        "text".to_string(),
        None, None, None, None,
        to_id.clone(),
    ).await;

    let _ = state.ws_tx.send(WsMessage::ChatMessage {
        from_id: entry.from_id.clone(),
        from_name: entry.from_name.clone(),
        text: entry.text.clone(),
        timestamp: entry.timestamp.clone(),
        message_type: "text".to_string(),
        file_name: None,
        file_size: None,
        file_id: None,
        file_type: None,
        to_id,
    });

    Ok(Json(serde_json::json!({
        "status": "sent",
        "timestamp": entry.timestamp,
    })))
}

// ============== Chat File Endpoints ==============

#[derive(Debug, Clone, Deserialize)]
pub struct ChatUploadQuery {
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub name: Option<String>,
    pub file_type: Option<String>,
    pub to_id: Option<String>,
}

/// 上传聊天文件
async fn upload_chat_file_handler(
    State(state): State<AppState>,
    Query(query): Query<ChatUploadQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<serde_json::Value>, AppErrorResponse> {
    // 收集完整的请求体
    use futures_util::StreamExt;
    let mut stream = body.into_data_stream();
    let mut body_bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            AppErrorResponse((StatusCode::BAD_REQUEST, e.to_string()))
        })?;
        body_bytes.extend_from_slice(&chunk);
    }

    let device_id = query.device_id.unwrap_or_else(|| {
        headers.get("x-device-id")
            .and_then(|v| v.to_str().ok().map(|s| s.to_string()))
            .unwrap_or_else(|| "web".to_string())
    });
    let device_name = query.device_name.unwrap_or_else(|| {
        headers.get("x-device-name")
            .and_then(|v| v.to_str().ok().map(|s| s.to_string()))
            .unwrap_or_else(|| "web".to_string())
    });

    let file_name = query.name.unwrap_or_else(|| "file".to_string());
    let file_type = query.file_type.unwrap_or_else(|| "application/octet-stream".to_string());
    let to_id = query.to_id;

    let safe_name = file_name.replace("..", "").replace('/', "_").replace('\\', "_");
    let file_id = {
        let mut rng = rand::thread_rng();
        format!("cf{:016x}", rng.gen::<u64>())
    };

    let base = state.share_dir.read().unwrap().clone();
    let chat_dir = base.join(".chat_files").join(&file_id);
    tokio::fs::create_dir_all(&chat_dir).await.map_err(|e| {
        AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    })?;

    let save_path = chat_dir.join(&safe_name);
    let file_size = body_bytes.len() as u64;
    tokio::fs::write(&save_path, &body_bytes).await.map_err(|e| {
        AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    })?;

    // 判断是否为图片
    let is_image = file_type.starts_with("image/");
    let msg_type = if is_image { "image".to_string() } else { "file".to_string() };

    let entry = state.chat_store.add_message(
        device_id.clone(),
        device_name,
        format!("[{}]", safe_name),
        msg_type,
        Some(safe_name.clone()),
        Some(file_size),
        Some(file_id.clone()),
        Some(file_type),
        to_id.clone(),
    ).await;

    let _ = state.ws_tx.send(WsMessage::ChatMessage {
        from_id: entry.from_id,
        from_name: entry.from_name,
        text: entry.text,
        timestamp: entry.timestamp,
        message_type: entry.message_type,
        file_name: entry.file_name,
        file_size: entry.file_size,
        file_id: entry.file_id,
        file_type: entry.file_type,
        to_id,
    });

    Ok(Json(serde_json::json!({
        "status": "sent",
        "file_id": file_id,
        "file_name": safe_name,
        "file_size": file_size,
    })))
}

/// 下载聊天文件
async fn download_chat_file_handler(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<impl IntoResponse, AppErrorResponse> {
    let base = state.share_dir.read().unwrap().clone();
    let file_dir = base.join(".chat_files").join(&file_id);

    if !file_dir.exists() {
        return Err(AppErrorResponse((StatusCode::NOT_FOUND, "文件不存在".to_string())));
    }

    let mut entries = tokio::fs::read_dir(&file_dir).await.map_err(|e| {
        AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    })?;

    let file_path = match entries.next_entry().await {
        Ok(Some(entry)) => entry.path(),
        _ => return Err(AppErrorResponse((StatusCode::NOT_FOUND, "文件不存在".to_string()))),
    };

    let file_name = file_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let data = tokio::fs::read(&file_path).await.map_err(|e| {
        AppErrorResponse((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    })?;

    let headers = [
        (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", file_name)),
        (axum::http::header::CONTENT_TYPE, "application/octet-stream".to_string()),
    ];

    Ok((headers, data))
}

/// 获取在线设备列表
async fn get_online_devices_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<String>>, AppErrorResponse> {
    let online = state.ws_tracker.get_online_devices().await;
    Ok(Json(online))
}

/// Web UI 页面
const WEB_UI_HTML: &str = include_str!("web_ui.html");

async fn web_ui_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        WEB_UI_HTML,
    )
}

/// 安全地解析路径，防止目录遍历攻击
fn resolve_safe_path(base: &PathBuf, rel_path: &str) -> Result<PathBuf, AppErrorResponse> {
    let clean = rel_path.trim_start_matches('/').trim_start_matches('\\');
    let target = if clean.is_empty() {
        base.clone()
    } else {
        base.join(clean)
    };

    let canonical_base =
        base.canonicalize()
            .map_err(|_| AppErrorResponse((StatusCode::FORBIDDEN, "无法解析目录路径".to_string())))?;

    let canonical_target = target
        .canonicalize()
        .map_err(|_| AppErrorResponse((StatusCode::NOT_FOUND, "路径不存在".to_string())))?;

    if !canonical_target.starts_with(&canonical_base) {
        return Err(AppErrorResponse((
            StatusCode::FORBIDDEN,
            "访问被拒绝：路径超出共享目录范围".to_string(),
        )));
    }

    Ok(canonical_target)
}

// ---- Error Response ----

#[derive(Debug)]
pub struct AppErrorResponse(pub (StatusCode, String));

impl IntoResponse for AppErrorResponse {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = self.0;
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

/// 启动 HTTPS 服务（TLS 1.3）
/// 通过 port_rx 监听端口变更信号，自动重新绑定
pub async fn serve_tls(
    router: Router,
    listener: TcpListener,
    cert_pem: &str,
    key_pem: &str,
    port_rx: &mut tokio::sync::watch::Receiver<u16>,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let certs = rustls_pemfile::certs(&mut cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()?;
    let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())?
        .ok_or("未找到私钥")?;

    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    let tls_acceptor = TlsAcceptor::from(std::sync::Arc::new(server_config));

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (tcp_stream, _) = result?;
                let tls_acceptor = tls_acceptor.clone();
                let app = router.clone();

                tokio::spawn(async move {
                    let tls_stream = match tls_acceptor.accept(tcp_stream).await {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("TLS 握手失败: {}", e);
                            return;
                        }
                    };

                    let io = TokioIo::new(tls_stream);

                    let tower_service = app
                        .into_service()
                        .map_request(|req: axum::http::Request<hyper::body::Incoming>| {
                            req.map(axum::body::Body::new)
                        });

                    let hyper_service = TowerToHyperService::new(tower_service);

                    if let Err(e) = Builder::new(TokioExecutor::new())
                        .serve_connection_with_upgrades(io, hyper_service)
                        .await
                    {
                        tracing::warn!("连接处理失败: {}", e);
                    }
                });
            }
            _ = port_rx.changed() => {
                tracing::info!("收到端口变更信号，重新绑定");
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::pairing::PairingManager;
    use crate::transfer::queue::TransferManager;

    /// 生成测试用自签名证书
    fn test_cert() -> (String, String) {
        let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut params =
            rcgen::CertificateParams::new(vec!["ffeel.local".to_string()]).unwrap();
        params.distinguished_name = rcgen::DistinguishedName::new();
        params.distinguished_name
            .push(rcgen::DnType::CommonName, "ffeel-test");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let cert = params.self_signed(&key_pair).unwrap();
        (cert.pem(), key_pair.serialize_pem())
    }

    fn tls_client() -> reqwest::Client {
        reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap()
    }

    // ===== resolve_safe_path unit tests =====

    #[test]
    fn test_resolve_safe_path_normal() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let result = resolve_safe_path(&dir.path().to_path_buf(), "test.txt");
        assert!(result.is_ok());
        assert_eq!(
            result.as_ref().ok().unwrap(),
            &file_path.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_resolve_safe_path_traversal_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_safe_path(&dir.path().to_path_buf(), "../");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_safe_path_empty_returns_base() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_safe_path(&dir.path().to_path_buf(), "");
        assert!(result.is_ok());
        assert_eq!(
            result.as_ref().ok().unwrap(),
            &dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn test_resolve_safe_path_nonexistent_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_safe_path(&dir.path().to_path_buf(), "nonexistent.txt");
        assert!(result.is_err());
    }

    // ===== Integration tests =====

    async fn setup_test_server_tls() -> (tempfile::TempDir, std::net::SocketAddr) {
        let dir = tempfile::tempdir().unwrap();

        // Create test files and directories
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("large.txt"), "x".repeat(1000)).unwrap();
        std::fs::write(dir.path().join("searchable_doc.txt"), "find me").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        std::fs::write(
            dir.path().join("subdir").join("nested.txt"),
            "nested content",
        )
        .unwrap();

        let (ws_tx, _) = tokio::sync::broadcast::channel(256);
        let tm = Arc::new(Mutex::new(TransferManager::new(3)));
        let pairing = SharedPairingManager::new(PairingManager::new());

        let app = build_router(Arc::new(RwLock::new(dir.path().to_path_buf())), tm, ws_tx, pairing, false, ChatStore::new(), WsConnectionTracker::new());

        let (cert_pem, key_pem) = test_cert();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (_port_tx, mut port_rx) = tokio::sync::watch::channel(0);

        tokio::spawn(async move {
            let _tx = _port_tx; // keep sender alive so port_rx.changed() doesn't return Err(Closed)
            serve_tls(app, listener, &cert_pem, &key_pem, &mut port_rx)
                .await
                .unwrap();
        });

        // Give the server a moment to start
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        (dir, addr)
    }

    #[tokio::test]
    async fn test_health_check_returns_ok() {
        let (_dir, addr) = setup_test_server_tls().await;
        let client = tls_client();
        let resp = client
            .get(format!("https://{}/api/health", addr))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn test_list_directory_returns_entries() {
        let (_dir, addr) = setup_test_server_tls().await;
        let client = tls_client();
        let resp = client
            .get(format!("https://{}/api/list", addr))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let entries: Vec<DirEntry> = resp.json().await.unwrap();
        assert!(!entries.is_empty(), "should contain entries");
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"test.txt"), "should list test.txt");
        assert!(names.contains(&"subdir"), "should list subdir");
    }

    #[tokio::test]
    async fn test_list_directory_pagination() {
        let (_dir, addr) = setup_test_server_tls().await;
        let client = tls_client();
        let resp = client
            .get(format!("https://{}/api/list?offset=0&limit=2", addr))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let entries: Vec<DirEntry> = resp.json().await.unwrap();
        assert!(entries.len() <= 2, "pagination should limit to 2 entries");
    }

    #[tokio::test]
    async fn test_download_file_returns_content() {
        let (_dir, addr) = setup_test_server_tls().await;
        let client = tls_client();
        let resp = client
            .get(format!("https://{}/api/download?path=test.txt", addr))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.text().await.unwrap();
        assert_eq!(body, "hello");
    }

    #[tokio::test]
    async fn test_search_finds_matching_files() {
        let (_dir, addr) = setup_test_server_tls().await;
        let client = tls_client();
        let resp = client
            .get(format!("https://{}/api/search?q=searchable", addr))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let entries: Vec<DirEntry> = resp.json().await.unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"searchable_doc.txt"),
            "should find searchable_doc.txt"
        );
    }

    #[tokio::test]
    async fn test_search_empty_query_returns_empty() {
        let (_dir, addr) = setup_test_server_tls().await;
        let client = tls_client();
        let resp = client
            .get(format!("https://{}/api/search?q=", addr))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let entries: Vec<DirEntry> = resp.json().await.unwrap();
        assert!(
            entries.is_empty(),
            "empty query should return no results"
        );
    }

    #[tokio::test]
    async fn test_list_nonexistent_path_returns_error() {
        let (_dir, addr) = setup_test_server_tls().await;
        let client = tls_client();
        let resp = client
            .get(format!("https://{}/api/list?path=nonexistent", addr))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_download_nonexistent_file_returns_error() {
        let (_dir, addr) = setup_test_server_tls().await;
        let client = tls_client();
        let resp = client
            .get(format!("https://{}/api/download?path=nonexistent.txt", addr))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
