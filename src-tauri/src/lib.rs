mod config;
mod discovery;
mod error;
mod log;
mod remote;
mod security;
mod server;
mod transfer;
mod tray;

use config::Settings;
use discovery::{DeviceInfo, DiscoveryManager};
use error::AppError;
use log::OperationLog;
use remote::{ClipboardSync, ScreenRecorder};
use security::certificate::CertificateManager;
use security::pairing::{PairingManager, SharedPairingManager};
use server::http::DirEntry;
use server::ws::{ChatStore, WsConnectionTracker, WsMessage};
use transfer::download;
use transfer::queue::{TransferDirection, TransferManager, TransferStatus, TransferTask};
use transfer::upload;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tauri::{Emitter, Manager};
use tokio::sync::{broadcast, Mutex};

/// 应用全局状态
struct AppState {
    settings: Mutex<Settings>,
    discovery: Arc<DiscoveryManager>,
    server_port: Mutex<u16>,
    transfer_manager: Arc<Mutex<TransferManager>>,
    pairing: SharedPairingManager,
    device_id: String,
    operation_log: OperationLog,
    /// 与 HTTP 服务器共享的共享目录路径（可动态更新）
    share_dir: Arc<RwLock<PathBuf>>,
    /// 端口变更信号（发送新端口值触发 HTTP 服务器重新绑定）
    port_watch_tx: tokio::sync::watch::Sender<u16>,
    /// 聊天消息存储器
    chat_store: ChatStore,
    /// WebSocket 广播发送端（用于桌面端消息广播到浏览器）
    ws_tx: broadcast::Sender<WsMessage>,
    /// WebSocket 连接跟踪器
    ws_tracker: WsConnectionTracker,
    /// 远程控制停止信号
    remote_control_stop: Arc<AtomicBool>,
    /// HTTP 服务器共享的设备发现列表
    discovered_devices: Arc<RwLock<Vec<DeviceInfo>>>,
    /// 屏幕录制器
    screen_recorder: ScreenRecorder,
}

fn local_ip_address() -> String {
    // 通过 UDP 连接获取默认路由的 LAN IP（比 local_ip_address crate 更可靠）
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:53").is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                let ip = local_addr.ip();
                if !ip.is_loopback() {
                    return ip.to_string();
                }
            }
        }
    }
    if let Ok(ip) = local_ip_address::local_ip() {
        return ip.to_string();
    }
    "127.0.0.1".to_string()
}

// ==================== Tauri Commands ====================

/// 共享的下载循环逻辑（供 download_file 和 resume_download 复用）
#[allow(clippy::too_many_arguments)]
async fn run_download_loop(
    app: tauri::AppHandle,
    tm: Arc<Mutex<TransferManager>>,
    log: log::OperationLog,
    task_id: String,
    remote_addr: String,
    remote_path: String,
    local_path_buf: std::path::PathBuf,
    file_name: String,
    speed_limit: u64,
) {
    log.add(
        "download",
        &format!("开始下载: {} 从 {}", file_name, remote_addr),
        "pending",
    )
    .await;
    loop {
        match download::download_file_from_remote(
            &remote_addr,
            &remote_path,
            &local_path_buf,
            tm.clone(),
            speed_limit,
        )
        .await
        {
            Ok(()) => {
                log.add("download", &format!("完成下载: {}", file_name), "success")
                    .await;
                let _ = app.emit(
                    "transfer-complete",
                    serde_json::json!({ "id": task_id, "file_name": file_name }),
                );
                break;
            }
            Err(e) if e.message == "TRANSFER_PAUSED" => {
                log.add("download", &format!("暂停下载: {}", file_name), "pending")
                    .await;
                break;
            }
            Err(e) if e.message == "TRANSFER_CANCELLED" => {
                let mut mgr = tm.lock().await;
                mgr.cancel_task(&task_id);
                let _ = app.emit(
                    "transfer-error",
                    serde_json::json!({ "id": task_id, "error": "已取消" }),
                );
                break;
            }
            Err(e) => {
                let mut mgr = tm.lock().await;
                if !mgr.record_failure(&task_id, e.message.clone()) {
                    log.add(
                        "download",
                        &format!("下载失败: {} - {}", file_name, e.message),
                        "error",
                    )
                    .await;
                    let _ = app.emit(
                        "transfer-error",
                        serde_json::json!({ "id": task_id, "error": e.message }),
                    );
                    break;
                }
            }
        }
    }
}

#[tauri::command]
async fn start_discovery(app: tauri::AppHandle) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let mut rx = state.discovery.start_browsing().map_err(|e| AppError {
        message: format!("启动设备发现失败: {}", e),
    })?;

    let app_handle = app.clone();
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                discovery::DeviceEvent::Found(device) => {
                    let _ = app_handle.emit("device-found", &device);
                    let state = app_handle.state::<AppState>();
                    state.ws_tracker.register(device.id.clone()).await;
                    let _ = state.ws_tx.send(WsMessage::DeviceStatus {
                        device_id: device.id.clone(),
                        online: true,
                    });
                    // 更新共享设备发现列表
                    {
                        let mut devices = state.discovered_devices.write().unwrap();
                        if let Some(existing) = devices.iter_mut().find(|d| d.id == device.id) {
                            existing.ip = device.ip.clone();
                            existing.port = device.port;
                        } else {
                            devices.push(device.clone());
                        }
                    }
                }
                discovery::DeviceEvent::Lost(device_id) => {
                    let _ = app_handle.emit("device-lost", &device_id);
                    let state = app_handle.state::<AppState>();
                    state.ws_tracker.unregister(&device_id).await;
                    let _ = state.ws_tx.send(WsMessage::DeviceStatus {
                        device_id: device_id.clone(),
                        online: false,
                    });
                    // 从发现列表中移除
                    {
                        let mut devices = state.discovered_devices.write().unwrap();
                        devices.retain(|d| d.id != device_id);
                    }
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn browse_directory(
    app: tauri::AppHandle,
    device_ip: String,
    port: u16,
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<Vec<DirEntry>, AppError> {
    let state = app.state::<AppState>();
    let device_id = &state.device_id;
    let mut url = format!(
        "https://{}:{}/api/list?path={}",
        device_ip,
        port,
        urlencoding::encode(&path)
    );
    if let Some(o) = offset {
        url.push_str(&format!("&offset={}", o));
    }
    if let Some(l) = limit {
        url.push_str(&format!("&limit={}", l));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError {
            message: format!("HTTP 客户端创建失败: {}", e),
        })?;

    let response = client
        .get(&url)
        .header("x-device-id", device_id)
        .send()
        .await
        .map_err(|e| AppError {
            message: format!("连接远程设备失败: {}:{} - {}", device_ip, port, e),
        })?;

    if response.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(AppError {
            message: "PAIRING_REQUIRED".to_string(),
        });
    }

    let entries: Vec<DirEntry> = response.json().await.map_err(|e| AppError {
        message: format!("解析目录列表失败: {}", e),
    })?;

    Ok(entries)
}

#[tauri::command]
async fn download_file(
    app: tauri::AppHandle,
    device_ip: String,
    port: u16,
    remote_path: String,
    local_path: String,
    file_name: String,
    file_size: u64,
) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().await;
    let max_retries = settings.max_retries;
    let speed_limit = settings.speed_limit;
    drop(settings);

    let task_id = {
        let mut mgr = state.transfer_manager.lock().await;
        let id = mgr.generate_id();
        mgr.add_task(TransferTask {
            id: id.clone(),
            file_name: file_name.clone(),
            file_size,
            bytes_transferred: 0,
            status: TransferStatus::Pending,
            direction: TransferDirection::Download,
            remote_device: device_ip.clone(),
            remote_path: remote_path.clone(),
            local_path: local_path.clone(),
            speed: 0.0,
            error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            retry_count: 0,
            max_retries,
        });
        id
    };

    let local_path_buf = std::path::PathBuf::from(&local_path);
    let remote_addr = format!("{}:{}", device_ip, port);
    let tm = state.transfer_manager.clone();
    let log = state.operation_log.clone();

    tokio::spawn(run_download_loop(
        app,
        tm,
        log,
        task_id,
        remote_addr,
        remote_path,
        local_path_buf,
        file_name,
        speed_limit,
    ));

    Ok(())
}

#[tauri::command]
async fn search_files(
    app: tauri::AppHandle,
    device_ip: String,
    port: u16,
    path: String,
    query: String,
) -> Result<Vec<DirEntry>, AppError> {
    let state = app.state::<AppState>();
    let device_id = &state.device_id;
    let url = format!(
        "https://{}:{}/api/search?q={}&path={}",
        device_ip,
        port,
        urlencoding::encode(&query),
        urlencoding::encode(&path),
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError {
            message: format!("HTTP 客户端创建失败: {}", e),
        })?;

    let response = client
        .get(&url)
        .header("x-device-id", device_id)
        .send()
        .await
        .map_err(|e| AppError {
            message: format!("搜索请求失败: {}:{} - {}", device_ip, port, e),
        })?;

    let entries: Vec<DirEntry> = response.json().await.map_err(|e| AppError {
        message: format!("解析搜索结果失败: {}", e),
    })?;

    Ok(entries)
}

/// 递归列出远程目录下的所有文件
async fn list_all_files(
    device_ip: &str,
    port: u16,
    dir_path: &str,
) -> Result<Vec<(String, u64)>, AppError> {
    let mut files = Vec::new();
    let mut stack = vec![dir_path.to_string()];

    while let Some(current) = stack.pop() {
        let url = format!(
            "https://{}:{}/api/list?path={}",
            device_ip,
            port,
            urlencoding::encode(&current)
        );
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .danger_accept_invalid_certs(true)
            .build()
            .map_err(|e| AppError {
                message: format!("HTTP 客户端创建失败: {}", e),
            })?;
        let response = client.get(&url).send().await.map_err(|e| AppError {
            message: format!("获取目录失败: {}", e),
        })?;
        let entries: Vec<DirEntry> = response.json().await.map_err(|e| AppError {
            message: format!("解析目录列表失败: {}", e),
        })?;

        for entry in entries {
            let full_path = if current.is_empty() {
                entry.name.clone()
            } else {
                format!("{}/{}", current, entry.name)
            };
            if entry.is_dir {
                stack.push(full_path);
            } else {
                files.push((full_path, entry.size.unwrap_or(0)));
            }
        }
    }
    Ok(files)
}

#[tauri::command]
async fn download_directory(
    app: tauri::AppHandle,
    device_ip: String,
    port: u16,
    remote_path: String,
    local_path: String,
) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().await;
    let max_retries = settings.max_retries;
    let speed_limit = settings.speed_limit;
    drop(settings);
    let remote_addr = format!("{}:{}", device_ip, port);
    let tm = state.transfer_manager.clone();

    // 递归列出所有文件
    let files = list_all_files(&device_ip, port, &remote_path).await?;
    if files.is_empty() {
        return Err(AppError {
            message: "目录为空".to_string(),
        });
    }

    let total_size: u64 = files.iter().map(|(_, s)| s).sum();
    let folder_name = std::path::Path::new(&remote_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "folder".to_string());

    // 创建传输任务
    let task_id = {
        let mut mgr = tm.lock().await;
        let id = mgr.generate_id();
        mgr.add_task(TransferTask {
            id: id.clone(),
            file_name: folder_name.clone(),
            file_size: total_size,
            bytes_transferred: 0,
            status: TransferStatus::Pending,
            direction: TransferDirection::Download,
            remote_device: device_ip.clone(),
            remote_path: remote_path.clone(),
            local_path: local_path.clone(),
            speed: 0.0,
            error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            retry_count: 0,
            max_retries,
        });
        id
    };

    let local_base = std::path::PathBuf::from(&local_path);
    let app_handle = app.clone();
    let log = state.operation_log.clone();

    tokio::spawn(async move {
        log.add(
            "download_folder",
            &format!("开始下载文件夹: {} ({} 个文件)", folder_name, files.len()),
            "pending",
        )
        .await;
        let mut overall_progress: u64 = 0;

        for (rel_path, file_size) in &files {
            // 检查是否取消
            {
                let mgr = tm.lock().await;
                let tasks = mgr.list_tasks();
                let task = tasks.iter().find(|t| t.id == task_id);
                if task.is_some_and(|t| t.status == TransferStatus::Cancelled) {
                    return;
                }
            }

            let remote_file_path = if remote_path.is_empty() {
                rel_path.clone()
            } else {
                format!("{}/{}", remote_path, rel_path)
            };
            let local_file_path = local_base.join(rel_path);

            if let Some(parent) = local_file_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }

            // 下载单个文件
            let result = download::download_file_from_remote(
                &remote_addr,
                &remote_file_path,
                &local_file_path,
                tm.clone(),
                speed_limit,
            )
            .await;

            match result {
                Ok(()) => {
                    overall_progress += file_size;
                    let mut mgr = tm.lock().await;
                    mgr.update_progress(&task_id, overall_progress, 0.0);
                }
                Err(e) => {
                    log.add(
                        "download_folder",
                        &format!("文件夹下载失败: {} - {}", folder_name, e.message),
                        "error",
                    )
                    .await;
                    let mut mgr = tm.lock().await;
                    mgr.fail_task(&task_id, format!("{}: {}", rel_path, e.message));
                    return;
                }
            }
        }

        log.add(
            "download_folder",
            &format!("完成下载文件夹: {}", folder_name),
            "success",
        )
        .await;
        let mut mgr = tm.lock().await;
        mgr.complete_task(&task_id);
        let _ = app_handle.emit(
            "transfer-complete",
            serde_json::json!({ "id": task_id, "file_name": folder_name }),
        );
    });

    Ok(())
}

#[tauri::command]
async fn upload_file(
    app: tauri::AppHandle,
    device_ip: String,
    port: u16,
    remote_path: String,
    local_path: String,
) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().await;
    let max_retries = settings.max_retries;
    drop(settings);

    let file_path = std::path::PathBuf::from(&local_path);
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let file_size = tokio::fs::metadata(&file_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let task_id = {
        let mut mgr = state.transfer_manager.lock().await;
        let id = mgr.generate_id();
        mgr.add_task(TransferTask {
            id: id.clone(),
            file_name: file_name.clone(),
            file_size,
            bytes_transferred: 0,
            status: TransferStatus::Pending,
            direction: TransferDirection::Upload,
            remote_device: device_ip.clone(),
            remote_path: remote_path.clone(),
            local_path: local_path.clone(),
            speed: 0.0,
            error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            retry_count: 0,
            max_retries,
        });
        id
    };

    let remote_addr = device_ip.clone();
    let tm = state.transfer_manager.clone();
    let log = state.operation_log.clone();
    let log_file_name = file_name.clone();

    tokio::spawn(async move {
        log.add(
            "upload",
            &format!("开始上传: {} 到 {}", log_file_name, remote_addr),
            "pending",
        )
        .await;
        loop {
            // 检查上传任务状态（暂停/取消检测）
            {
                let mgr = tm.lock().await;
                let tasks = mgr.list_tasks();
                if let Some(task) = tasks.iter().find(|t| t.id == task_id) {
                    match task.status {
                        TransferStatus::Paused => {
                            log.add("upload", &format!("暂停上传: {}", log_file_name), "pending")
                                .await;
                            return;
                        }
                        TransferStatus::Cancelled => {
                            return;
                        }
                        _ => {}
                    }
                }
            }

            match upload::upload_file_to_remote(
                &remote_addr,
                port,
                &remote_path,
                &file_path,
                tm.clone(),
                &task_id,
                file_size,
            )
            .await
            {
                Ok(()) => {
                    let mut mgr = tm.lock().await;
                    mgr.complete_task(&task_id);
                    log.add("upload", &format!("完成上传: {}", log_file_name), "success")
                        .await;
                    let _ = app.emit(
                        "transfer-complete",
                        serde_json::json!({
                            "id": task_id,
                            "file_name": file_name,
                        }),
                    );
                    break;
                }
                Err(e) => {
                    let mut mgr = tm.lock().await;
                    if !mgr.record_failure(&task_id, e.message.clone()) {
                        log.add(
                            "upload",
                            &format!("上传失败: {} - {}", log_file_name, e.message),
                            "error",
                        )
                        .await;
                        let _ = app.emit(
                            "transfer-error",
                            serde_json::json!({
                                "id": task_id,
                                "error": e.message,
                            }),
                        );
                        break;
                    }
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn get_transfers(app: tauri::AppHandle) -> Result<Vec<TransferTask>, AppError> {
    let state = app.state::<AppState>();
    let mgr = state.transfer_manager.lock().await;
    Ok(mgr.list_tasks())
}

#[tauri::command]
async fn cancel_transfer(app: tauri::AppHandle, id: String) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let mut mgr = state.transfer_manager.lock().await;
    mgr.cancel_task(&id);
    Ok(())
}

#[tauri::command]
async fn pause_transfer(app: tauri::AppHandle, id: String) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let mut mgr = state.transfer_manager.lock().await;
    mgr.pause_task(&id)
}

#[tauri::command]
async fn resume_transfer(app: tauri::AppHandle, id: String) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let mut mgr = state.transfer_manager.lock().await;
    mgr.resume_task(&id)
}

/// 恢复暂停的下载（重新启动下载任务）
#[tauri::command]
async fn resume_download(app: tauri::AppHandle, id: String) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().await;
    let speed_limit = settings.speed_limit;
    drop(settings);

    let (remote_addr, remote_path, local_path, file_name) = {
        let mut mgr = state.transfer_manager.lock().await;
        let tasks = mgr.list_tasks();
        let task = tasks
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .ok_or_else(|| AppError {
                message: "任务不存在".to_string(),
            })?;

        if task.status != TransferStatus::Paused {
            return Err(AppError {
                message: "只能恢复已暂停的下载任务".to_string(),
            });
        }
        if task.direction != TransferDirection::Download {
            return Err(AppError {
                message: "只能恢复下载任务，不支持上传续传".to_string(),
            });
        }

        // 重置状态为 Pending
        let _ = mgr.resume_task(&id);

        (
            task.remote_device,
            task.remote_path,
            task.local_path,
            task.file_name,
        )
    };

    let local_path_buf = std::path::PathBuf::from(&local_path);
    let tm = state.transfer_manager.clone();
    let log = state.operation_log.clone();

    tokio::spawn(run_download_loop(
        app,
        tm,
        log,
        id,
        remote_addr,
        remote_path,
        local_path_buf,
        file_name,
        speed_limit,
    ));

    Ok(())
}

#[tauri::command]
async fn get_settings(app: tauri::AppHandle) -> Result<Settings, AppError> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().await;
    Ok(settings.clone())
}

#[tauri::command]
async fn update_settings(app: tauri::AppHandle, new_settings: Settings) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().await;

    // 设备重命名时重新注册 mDNS 服务
    if settings.device_name != new_settings.device_name {
        let old_name = settings.device_name.clone();
        let new_name = new_settings.device_name.clone();
        let device_id = state.device_id.clone();
        let port = *state.server_port.lock().await;
        let ip = local_ip_address();

        if let Err(e) = state.discovery.update_registration(
            &old_name,
            &device_id,
            &new_name,
            &ip,
            port,
            std::env::consts::OS,
        ) {
            tracing::warn!("mDNS 重新注册失败（不影响设置保存）: {}", e);
        }
    }

    // 同步共享目录到 HTTP 服务器
    if new_settings.share_dir != settings.share_dir {
        if let Ok(mut dir) = state.share_dir.write() {
            *dir = new_settings.share_dir.clone();
            tracing::info!("共享目录已更新: {:?}", dir);
        }
    }

    // 端口变更时通知 HTTP 服务器重新绑定
    if new_settings.port != settings.port {
        let _ = state.port_watch_tx.send(new_settings.port);
        tracing::info!("端口已变更: {} -> {}", settings.port, new_settings.port);
    }

    if let Err(e) = new_settings.save() {
        return Err(AppError {
            message: format!("保存设置失败: {}", e),
        });
    }
    // 同步配对管理器（保留名称和昵称）
    state
        .pairing
        .replace_inner(PairingManager::from_settings(
            &new_settings.trusted_devices,
            &new_settings.trusted_device_names,
            &new_settings.trusted_device_nicknames,
        ))
        .await;
    *settings = new_settings;
    Ok(())
}

#[tauri::command]
async fn get_trusted_devices(app: tauri::AppHandle) -> Result<Vec<String>, AppError> {
    let state = app.state::<AppState>();
    Ok(state.pairing.trusted_ids().await)
}

#[tauri::command]
async fn get_trusted_device_list(
    app: tauri::AppHandle,
) -> Result<Vec<serde_json::Value>, AppError> {
    let state = app.state::<AppState>();
    let my_id = &state.device_id;
    let devices = state.pairing.trusted_devices().await;
    let result: Vec<serde_json::Value> = devices.into_iter()
        .filter(|(id, _)| id != my_id)
        .map(|(id, entry)| {
            serde_json::json!({"id": id, "name": entry.name, "nickname": entry.nickname, "paired_at": entry.paired_at})
        }).collect();
    Ok(result)
}

#[tauri::command]
async fn trust_device(
    app: tauri::AppHandle,
    device_id: String,
    device_name: Option<String>,
) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    if let Some(name) = device_name {
        state
            .pairing
            .trust_device_with_name(&device_id, &name)
            .await;
    } else {
        state.pairing.trust_device(&device_id).await;
    }
    // 持久化到 settings
    let mut settings = state.settings.lock().await;
    state.pairing.save_to_settings(&mut settings).await;
    if let Err(e) = settings.save() {
        return Err(AppError {
            message: format!("保存设置失败: {}", e),
        });
    }
    Ok(())
}

#[tauri::command]
async fn untrust_device(app: tauri::AppHandle, device_id: String) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    state.pairing.untrust_device(&device_id).await;
    let mut settings = state.settings.lock().await;
    state.pairing.save_to_settings(&mut settings).await;
    if let Err(e) = settings.save() {
        return Err(AppError {
            message: format!("保存设置失败: {}", e),
        });
    }
    Ok(())
}

#[tauri::command]
async fn get_pairing_code(app: tauri::AppHandle) -> Result<String, AppError> {
    let state = app.state::<AppState>();
    let code = state.pairing.get_current_code().await;
    Ok(code.unwrap_or_else(PairingManager::generate_pairing_code))
}

#[tauri::command]
async fn rotate_pairing_code(app: tauri::AppHandle) -> Result<String, AppError> {
    let state = app.state::<AppState>();
    let code = state.pairing.rotate_code().await;
    // 持久化配对码
    let mut settings = state.settings.lock().await;
    settings.pairing_code = Some(code.clone());
    if let Err(e) = settings.save() {
        tracing::error!("保存配对码失败: {}", e);
    }
    Ok(code)
}

#[tauri::command]
async fn pair_device(
    app: tauri::AppHandle,
    device_ip: String,
    port: u16,
    device_id: String,
    pairing_code: String,
) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().await;
    let device_name = settings.device_name.clone();
    drop(settings);

    let url = format!("https://{}:{}/api/pair", device_ip, port);
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError {
            message: format!("HTTP 客户端创建失败: {}", e),
        })?;
    let response = client
        .post(&url)
        .header("x-device-id", &device_id)
        .header("x-device-name", &device_name)
        .header("x-pairing-code", &pairing_code)
        .send()
        .await
        .map_err(|e| AppError {
            message: format!("配对请求失败: {}", e),
        })?;
    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(AppError {
            message: format!("配对失败: {}", error_text),
        });
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct TransferStats {
    total_completed: usize,
    total_failed: usize,
    total_cancelled: usize,
    total_bytes: u64,
    active_count: usize,
}

#[tauri::command]
async fn get_transfer_stats(app: tauri::AppHandle) -> Result<TransferStats, AppError> {
    let state = app.state::<AppState>();
    let mgr = state.transfer_manager.lock().await;
    let tasks = mgr.list_tasks();
    let mut total_bytes = 0u64;
    let mut total_completed = 0;
    let mut total_failed = 0;
    let mut total_cancelled = 0;
    let mut active_count = 0;
    for t in &tasks {
        match t.status {
            TransferStatus::Completed => {
                total_completed += 1;
                total_bytes += t.file_size;
            }
            TransferStatus::Failed => total_failed += 1,
            TransferStatus::Cancelled => total_cancelled += 1,
            TransferStatus::Transferring | TransferStatus::Pending => active_count += 1,
            _ => {}
        }
    }
    Ok(TransferStats {
        total_completed,
        total_failed,
        total_cancelled,
        total_bytes,
        active_count,
    })
}

#[tauri::command]
async fn get_operation_log(app: tauri::AppHandle) -> Result<Vec<log::LogEntry>, AppError> {
    let state = app.state::<AppState>();
    Ok(state.operation_log.list().await)
}

#[tauri::command]
async fn clear_operation_log(app: tauri::AppHandle) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    state.operation_log.clear().await;
    Ok(())
}

#[tauri::command]
async fn get_chat_messages(app: tauri::AppHandle) -> Result<Vec<server::ws::ChatEntry>, AppError> {
    let state = app.state::<AppState>();
    Ok(state.chat_store.get_messages().await)
}

#[tauri::command]
async fn send_chat_message(
    app: tauri::AppHandle,
    text: String,
    to_id: Option<String>,
) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let device_id = state.device_id.clone();
    let settings = state.settings.lock().await;
    let device_name = settings.device_name.clone();
    drop(settings);

    if text.is_empty() {
        return Err(AppError {
            message: "消息不能为空".to_string(),
        });
    }

    let entry = state
        .chat_store
        .add_message(
            device_id,
            device_name,
            text,
            "text".to_string(),
            None,
            None,
            None,
            None,
            to_id.clone(),
        )
        .await;
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
    tracing::debug!("桌面端广播 ChatMessage: to={:?}", entry.to_id);
    Ok(())
}

#[tauri::command]
async fn set_device_nickname(
    app: tauri::AppHandle,
    device_id: String,
    nickname: String,
) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    state.pairing.set_nickname(&device_id, &nickname).await;
    // 持久化到 settings
    let mut settings = state.settings.lock().await;
    state.pairing.save_to_settings(&mut settings).await;
    if let Err(e) = settings.save() {
        tracing::error!("保存设备昵称到设置失败: {}", e);
    }
    Ok(())
}

#[tauri::command]
async fn get_online_device_ids(app: tauri::AppHandle) -> Result<Vec<String>, AppError> {
    let state = app.state::<AppState>();
    Ok(state.ws_tracker.get_online_devices().await)
}

// ==================== Remote Control Commands ====================

/// 连接远程设备的 MJPEG 屏幕流，通过 Tauri 事件中继帧到前端
#[tauri::command]
async fn start_remote_control(
    app: tauri::AppHandle,
    target_ip: String,
    target_port: u16,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let device_id = state.device_id.clone();
    state.remote_control_stop.store(false, Ordering::SeqCst);
    let stop_flag = state.remote_control_stop.clone();

    let url = format!(
        "https://{}:{}/remote/screen/stream?device_id={}",
        target_ip, target_port, device_id
    );

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let app_clone = app.clone();
    tokio::spawn(async move {
        match client.get(&url).send().await {
            Ok(response) => {
                let app_for_relay = app_clone.clone();
                crate::remote::relay_mjpeg_stream(response, app_for_relay, stop_flag).await;
            }
            Err(e) => {
                let _ = app_clone.emit(
                    "remote-screen-error",
                    serde_json::json!({"error": format!("连接失败: {}", e)}),
                );
            }
        }
        // 流结束
        let _ = app_clone.emit("remote-screen-ended", serde_json::json!({}));
    });

    Ok(())
}

/// 停止远程控制会话
#[tauri::command]
async fn stop_remote_control(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.remote_control_stop.store(true, Ordering::SeqCst);
    let _ = app.emit("remote-screen-ended", serde_json::json!({}));
    Ok(())
}

/// 测试本地屏幕捕获 —— 返回 base64 JPEG（用于验证权限和功能）
#[tauri::command]
async fn test_screen_capture() -> Result<String, String> {
    use base64::Engine;
    let frame = crate::remote::capture_screen_jpeg(
        crate::remote::DEFAULT_JPEG_QUALITY,
        crate::remote::DEFAULT_SCALE,
    )?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&frame.jpeg_data))
}

/// 开始屏幕录制
#[tauri::command]
async fn start_recording(app: tauri::AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().await;
    let download_dir = settings.download_dir.clone();
    drop(settings);

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let output_path =
        std::path::PathBuf::from(&download_dir).join(format!("remote_recording_{}.avi", timestamp));

    state
        .screen_recorder
        .start(output_path.clone())
        .map_err(|e| format!("开始录制失败: {}", e))?;

    tracing::info!("开始屏幕录制: {:?}", output_path);
    Ok(output_path.to_string_lossy().to_string())
}

/// 停止屏幕录制
#[tauri::command]
async fn stop_recording(app: tauri::AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let path = state
        .screen_recorder
        .stop()
        .map_err(|e| format!("停止录制失败: {}", e))?;
    tracing::info!("录制保存至: {:?}", path);
    Ok(path.to_string_lossy().to_string())
}

/// 获取录制状态
#[tauri::command]
async fn get_recording_status(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let state = app.state::<AppState>();
    let duration = state.screen_recorder.duration();
    let frames = state.screen_recorder.frame_count();
    Ok(serde_json::json!({
        "duration": duration,
        "frames": frames,
        "is_recording": duration > 0.0
    }))
}

/// 检测 macOS 权限状态
#[tauri::command]
fn check_permissions() -> crate::remote::PermissionStatus {
    crate::remote::check_permissions()
}

/// 打开屏幕录制隐私设置
#[tauri::command]
fn open_screen_recording_settings() {
    crate::remote::open_screen_recording_settings();
}

/// 打开辅助功能隐私设置
#[tauri::command]
fn open_accessibility_settings() {
    crate::remote::open_accessibility_settings();
}

#[tauri::command]
async fn send_chat_file(
    app: tauri::AppHandle,
    local_path: String,
    to_id: Option<String>,
) -> Result<serde_json::Value, AppError> {
    use rand::Rng;
    let state = app.state::<AppState>();
    let device_id = state.device_id.clone();
    let settings = state.settings.lock().await;
    let device_name = settings.device_name.clone();
    drop(settings);

    let source = std::path::Path::new(&local_path);
    if !source.exists() {
        return Err(AppError {
            message: "文件不存在".to_string(),
        });
    }

    let file_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let file_size = tokio::fs::metadata(&local_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    // 简单判断文件类型
    let file_type = if file_name.ends_with(".png")
        || file_name.ends_with(".jpg")
        || file_name.ends_with(".jpeg")
        || file_name.ends_with(".gif")
        || file_name.ends_with(".webp")
        || file_name.ends_with(".bmp")
    {
        format!("image/{}", file_name.rsplit('.').next().unwrap_or("png"))
    } else {
        "application/octet-stream".to_string()
    };

    // 生成唯一 ID 并复制文件到聊天目录
    let share_dir = state.share_dir.read().unwrap().clone();
    let file_id: String = format!("cf{:016x}", rand::thread_rng().gen::<u64>());
    let chat_dir = share_dir.join(".chat_files").join(&file_id);
    tokio::fs::create_dir_all(&chat_dir)
        .await
        .map_err(|e| AppError {
            message: format!("创建目录失败: {}", e),
        })?;

    let dest = chat_dir.join(&file_name);
    tokio::fs::copy(&local_path, &dest)
        .await
        .map_err(|e| AppError {
            message: format!("复制文件失败: {}", e),
        })?;

    let is_image = file_type.starts_with("image/");
    let msg_type = if is_image {
        "image".to_string()
    } else {
        "file".to_string()
    };

    let entry = state
        .chat_store
        .add_message(
            device_id,
            device_name,
            format!("[{}]", file_name),
            msg_type,
            Some(file_name.clone()),
            Some(file_size),
            Some(file_id.clone()),
            Some(file_type),
            to_id.clone(),
        )
        .await;

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

    Ok(serde_json::json!({
        "status": "sent",
        "file_id": file_id,
        "file_name": file_name,
        "file_size": file_size,
    }))
}

/// 从聊天文件存储中下载文件到下载目录
#[tauri::command]
async fn download_chat_file(app: tauri::AppHandle, file_id: String) -> Result<String, AppError> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().await;
    let share_dir = state.share_dir.read().unwrap().clone();
    let download_dir = std::path::PathBuf::from(&settings.download_dir);
    drop(settings);

    let file_dir = share_dir.join(".chat_files").join(&file_id);
    if !file_dir.exists() {
        return Err(AppError {
            message: "聊天文件不存在".to_string(),
        });
    }

    let mut entries = tokio::fs::read_dir(&file_dir).await.map_err(|e| AppError {
        message: format!("读取聊天文件失败: {}", e),
    })?;

    let source_path = match entries.next_entry().await {
        Ok(Some(entry)) => entry.path(),
        _ => {
            return Err(AppError {
                message: "聊天文件不存在".to_string(),
            })
        }
    };

    let file_name = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download");
    let dest_path = download_dir.join(file_name);

    // 如果目标文件已存在，追加数字后缀
    let mut final_path = dest_path.clone();
    let mut counter = 1;
    while final_path.exists() {
        let stem = dest_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let ext = dest_path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| format!(".{}", s))
            .unwrap_or_default();
        final_path = download_dir.join(format!("{}({}){}", stem, counter, ext));
        counter += 1;
    }

    tokio::fs::copy(&source_path, &final_path)
        .await
        .map_err(|e| AppError {
            message: format!("保存文件失败: {}", e),
        })?;

    Ok(final_path.to_string_lossy().to_string())
}

/// 用系统默认程序打开文件
#[tauri::command]
async fn open_file(path: String) -> Result<(), AppError> {
    open::that(&path).map_err(|e| AppError {
        message: format!("打开文件失败: {}", e),
    })?;
    Ok(())
}

/// 获取聊天图片文件的 Base64 数据（用于桌面客户端内预览）
#[tauri::command]
async fn get_chat_image_base64(app: tauri::AppHandle, file_id: String) -> Result<String, AppError> {
    let state = app.state::<AppState>();
    let share_dir = state.share_dir.read().unwrap().clone();

    let file_dir = share_dir.join(".chat_files").join(&file_id);
    if !file_dir.exists() {
        return Err(AppError {
            message: "图片不存在".to_string(),
        });
    }

    let mut entries = tokio::fs::read_dir(&file_dir).await.map_err(|e| AppError {
        message: format!("读取失败: {}", e),
    })?;

    let source_path = match entries.next_entry().await {
        Ok(Some(entry)) => entry.path(),
        _ => {
            return Err(AppError {
                message: "图片不存在".to_string(),
            })
        }
    };

    let ext = source_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "image/png",
    };

    let data = tokio::fs::read(&source_path).await.map_err(|e| AppError {
        message: format!("读取图片失败: {}", e),
    })?;

    Ok(format!(
        "data:{};base64,{}",
        mime,
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data)
    ))
}

#[tauri::command]
async fn get_local_device_info(app: tauri::AppHandle) -> Result<DeviceInfo, AppError> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().await;
    let port = *state.server_port.lock().await;

    Ok(DeviceInfo {
        id: state.device_id.clone(),
        name: settings.device_name.clone(),
        ip: local_ip_address(),
        port,
        platform: std::env::consts::OS.to_string(),
        online: true,
    })
}

// ==================== App Entry Point ====================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // 持久化设备 ID：复用已保存的值，或生成新值并保存
            let mut settings = Settings::load();
            let saved_id = settings.device_id.clone();
            let device_id = saved_id.unwrap_or_else(|| {
                let new_id = uuid::Uuid::new_v4().to_string();
                settings.device_id = Some(new_id.clone());
                let _ = settings.save();
                new_id
            });
            let discovery = DiscoveryManager::new().expect("mDNS 初始化失败");

            let (ws_tx, _ws_rx) = broadcast::channel::<WsMessage>(256);

            // 从磁盘加载传输历史
            let history_path = {
                let mut p = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                p.push("ffeel");
                p.push("transfers.json");
                p
            };
            let mut transfer_mgr = TransferManager::load_from_disk(&history_path);
            transfer_mgr.set_max_concurrent(settings.max_concurrent_transfers);
            transfer_mgr.set_ws_tx(ws_tx.clone());
            let transfer_manager = Arc::new(Mutex::new(transfer_mgr));

            // 初始化操作日志
            let log_path = {
                let mut p = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                p.push("ffeel");
                p.push("operation_log.json");
                p
            };
            let mut operation_log = OperationLog::from_disk(&log_path);
            operation_log.set_path(log_path);

            // 创建可动态更新的共享目录 Arc<RwLock>，与 HTTP 服务器共享
            let share_dir_arc = Arc::new(RwLock::new(settings.share_dir.clone()));

            // 创建聊天消息存储器
            let chat_messages = ChatStore::new();
            // 设置聊天持久化路径
            {
                let mut p = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                p.push("ffeel");
                p.push("chat_history.json");
                let chat_history_path = p;
                let cm = chat_messages.clone();
                tauri::async_runtime::block_on(async {
                    cm.load_from_disk(&chat_history_path).await;
                    cm.set_persist_path(chat_history_path).await;
                });
            }

            // WebSocket 连接跟踪器
            let ws_tracker = WsConnectionTracker::new();
            let ws_tracker_for_http = ws_tracker.clone();

            // 剪贴板同步服务
            let clipboard_sync = ClipboardSync::new().unwrap_or_else(|e| {
                tracing::error!("剪贴板同步初始化失败: {}", e);
                ClipboardSync::default()
            });
            let clipboard_sync_for_http = clipboard_sync.clone();

            // 屏幕录制器
            let screen_recorder = ScreenRecorder::new();
            let screen_recorder_for_http = screen_recorder.clone();

            // 配对管理器：从设置加载信任列表（含名称），并自动信任本机设备
            let mut pairing_mgr = PairingManager::from_settings(
                &settings.trusted_devices,
                &settings.trusted_device_names,
                &settings.trusted_device_nicknames,
            );
            // 清理重复的本机条目（同名但不同 ID 的旧记录）
            pairing_mgr.untrust_devices_by_name_except(&settings.device_name, &device_id);
            pairing_mgr.trust_device_with_name(&device_id, &settings.device_name);
            let pairing = SharedPairingManager::new(pairing_mgr);

            // 端口变更信号通道——用于动态重启 HTTP 服务器
            let (port_watch_tx, port_watch_rx) = tokio::sync::watch::channel(settings.port);

            let chat_store_for_tui = chat_messages.clone();
            let ws_tx_for_tui = ws_tx.clone();

            // 共享设备发现列表（同时被 Tauri 和 HTTP 服务器使用）
            let discovered_devices = Arc::new(RwLock::new(Vec::new()));

            let state = AppState {
                settings: Mutex::new(settings.clone()),
                discovery: Arc::new(discovery),
                server_port: Mutex::new(0),
                transfer_manager: transfer_manager.clone(),
                pairing: pairing.clone(),
                device_id: device_id.clone(),
                operation_log,
                share_dir: share_dir_arc.clone(),
                port_watch_tx,
                chat_store: chat_store_for_tui,
                ws_tx: ws_tx_for_tui,
                ws_tracker: ws_tracker.clone(),
                remote_control_stop: Arc::new(AtomicBool::new(false)),
                discovered_devices: discovered_devices.clone(),
                screen_recorder,
            };

            // 恢复或生成配对码
            let pairing_for_restore = state.pairing.clone();
            let pairing_code_settings = settings.pairing_code.clone();
            tauri::async_runtime::block_on(async {
                if let Some(code) = pairing_code_settings {
                    tracing::info!("从配置恢复配对码");
                    pairing_for_restore.set_code(&code).await;
                } else {
                    let code = pairing_for_restore.rotate_code().await;
                    tracing::info!("生成新配对码: {}", code);
                }
            });

            // 持久化配对码到设置（setup 闭包非 async）
            let pairing_code = tauri::async_runtime::block_on(state.pairing.get_current_code());
            let mut init_settings = state.settings.blocking_lock();
            init_settings.pairing_code = pairing_code;
            if let Err(e) = init_settings.save() {
                tracing::error!("保存初始配对码失败: {}", e);
            }
            drop(init_settings);

            // 启动 HTTP 文件服务 + WebSocket（外层循环支持端口变更后自动重新绑定）
            let share_dir_for_http = share_dir_arc.clone();
            let tm = transfer_manager.clone();
            let require_pairing = settings.require_pairing;
            let device_name = settings.device_name.clone();

            app.manage(state);

            // 将本机设备注册到在线跟踪器（标记为在线）
            let wt_self = ws_tracker.clone();
            let did_self = device_id.clone();
            tauri::async_runtime::spawn(async move {
                wt_self.register(did_self).await;
            });

            // 订阅 WebSocket 广播，实时转发传输进度 + 聊天消息到前端
            let progress_tx = ws_tx.subscribe();
            let _progress_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut rx = progress_tx;
                while let Ok(msg) = rx.recv().await {
                    #[allow(clippy::collapsible_match)]
                    match &msg {
                        WsMessage::TransferProgress {
                            id,
                            file_name,
                            bytes_transferred,
                            total_bytes,
                            speed,
                        } => {
                            if *bytes_transferred < *total_bytes {
                                let _ = _progress_app.emit(
                                    "transfer-progress",
                                    serde_json::json!({
                                        "id": id,
                                        "file_name": file_name,
                                        "bytes_transferred": bytes_transferred,
                                        "total_bytes": total_bytes,
                                        "speed": speed,
                                    }),
                                );
                            }
                        }
                        WsMessage::TransferComplete { id, file_name } => {
                            let _ = _progress_app.emit(
                                "transfer-complete",
                                serde_json::json!({
                                    "id": id,
                                    "file_name": file_name,
                                }),
                            );
                        }
                        WsMessage::TransferError {
                            id,
                            file_name,
                            error,
                        } => {
                            let _ = _progress_app.emit(
                                "transfer-error",
                                serde_json::json!({
                                    "id": id,
                                    "file_name": file_name,
                                    "error": error,
                                }),
                            );
                        }
                        WsMessage::ChatMessage {
                            from_id,
                            from_name,
                            text,
                            timestamp,
                            message_type,
                            file_name,
                            file_size,
                            file_id,
                            file_type,
                            to_id,
                        } => {
                            let _ = _progress_app.emit(
                                "chat-message",
                                serde_json::json!({
                                    "from_id": from_id,
                                    "from_name": from_name,
                                    "text": text,
                                    "timestamp": timestamp,
                                    "message_type": message_type,
                                    "file_name": file_name,
                                    "file_size": file_size,
                                    "file_id": file_id,
                                    "file_type": file_type,
                                    "to_id": to_id,
                                }),
                            );
                        }
                        WsMessage::DeviceStatus { device_id, online } => {
                            let _ = _progress_app.emit(
                                "device-status",
                                serde_json::json!({
                                    "device_id": device_id,
                                    "online": online,
                                }),
                            );
                        }
                        // 远程控制消息暂不处理（由专门的 Tauri 命令处理）
                        _ => {}
                    }
                }
            });

            // 初始化系统托盘
            if let Err(e) = tray::build_tray(app) {
                tracing::error!("系统托盘初始化失败: {}", e);
            }

            // 加载 TLS 证书
            let config_dir = {
                let mut p = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                p.push("ffeel");
                p
            };
            let (cert_pem, key_pem) = CertificateManager::load_or_create(&config_dir)
                .unwrap_or_else(|e| {
                    tracing::error!("TLS 证书初始化失败: {}", e);
                    std::process::exit(1);
                });
            let cert_pem_owned = cert_pem.to_string();
            let key_pem_owned = key_pem.to_string();
            let app_handle = app.handle().clone();
            let ws_tracker_for_build = ws_tracker_for_http.clone();

            let device_id_for_http = device_id.clone();
            tauri::async_runtime::spawn(async move {
                // 首次创建 router（之后每次 re-bind 复用 router 即可）
                let router = server::http::build_router(
                    share_dir_for_http,
                    tm,
                    ws_tx,
                    pairing,
                    require_pairing,
                    chat_messages.clone(),
                    ws_tracker_for_build,
                    device_id_for_http,
                    discovered_devices.clone(),
                    clipboard_sync_for_http,
                    screen_recorder_for_http,
                );

                let mut port_rx = port_watch_rx;

                loop {
                    // 每次循环从 watch channel 读取最新配置端口
                    let current_config_port = *port_rx.borrow_and_update();
                    let bind_addr = if current_config_port > 0 {
                        format!("0.0.0.0:{}", current_config_port)
                    } else {
                        "0.0.0.0:0".to_string()
                    };

                    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
                        Ok(l) => l,
                        Err(e) => {
                            tracing::error!(
                                "无法绑定端口 {} ({})，等待 5 秒后重试",
                                current_config_port,
                                e
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue;
                        }
                    };

                    let port = listener.local_addr().unwrap().port();
                    // 更新运行的端口号
                    let state = app_handle.state::<AppState>();
                    *state.server_port.lock().await = port;

                    tracing::info!("HTTPS 服务启动在端口: {} (TLS 1.3)", port);

                    // mDNS 注册
                    if let Err(e) = state.discovery.register_service(
                        &device_id,
                        &device_name,
                        &local_ip_address(),
                        port,
                        std::env::consts::OS,
                    ) {
                        tracing::error!("mDNS 注册失败: {}", e);
                    }

                    // 启动 TLS 服务，监听端口变更信号
                    if let Err(e) = server::http::serve_tls(
                        router.clone(),
                        listener,
                        &cert_pem_owned,
                        &key_pem_owned,
                        &mut port_rx,
                    )
                    .await
                    {
                        tracing::error!("HTTPS 服务异常退出: {}", e);
                    }

                    tracing::info!("HTTPS 服务已停止，准备重新绑定...");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_discovery,
            browse_directory,
            search_files,
            download_file,
            download_directory,
            upload_file,
            get_transfers,
            cancel_transfer,
            pause_transfer,
            resume_transfer,
            resume_download,
            get_settings,
            update_settings,
            get_trusted_devices,
            get_trusted_device_list,
            trust_device,
            untrust_device,
            get_pairing_code,
            rotate_pairing_code,
            pair_device,
            get_local_device_info,
            get_operation_log,
            clear_operation_log,
            get_transfer_stats,
            get_chat_messages,
            send_chat_message,
            set_device_nickname,
            get_online_device_ids,
            start_remote_control,
            stop_remote_control,
            test_screen_capture,
            start_recording,
            stop_recording,
            get_recording_status,
            check_permissions,
            open_screen_recording_settings,
            open_accessibility_settings,
            send_chat_file,
            download_chat_file,
            open_file,
            get_chat_image_base64,
        ])
        .run(tauri::generate_context!())
        .expect("启动 ffeel 失败");
}

#[cfg(test)]
mod tests {
    use crate::error::AppError;
    use crate::TransferStats;

    // ==================== Helper: download loop decision logic ====================

    /// 模拟下载循环的错误处理决策逻辑。
    /// 根据错误类型和重试可用性，决定循环应继续重试还是退出。
    ///
    /// 对应实际代码中 `download_file` 的 loop + match 模式：
    /// - `Ok(())` => 成功，退出循环
    /// - `TRANSFER_PAUSED` / `TRANSFER_CANCELLED` => 特殊错误，退出循环
    /// - 其他错误 => 取决于 `record_failure` 是否返回 true（有重试机会则继续）
    fn should_retry_in_download_loop(result: &Result<(), AppError>, can_retry: bool) -> bool {
        match result {
            Ok(_) => false,
            Err(e) if e.message == "TRANSFER_PAUSED" || e.message == "TRANSFER_CANCELLED" => false,
            Err(_) => can_retry,
        }
    }

    // ==================== TransferStats tests ====================

    #[test]
    fn test_transfer_stats_serialization() {
        let stats = TransferStats {
            total_completed: 5,
            total_failed: 2,
            total_cancelled: 1,
            total_bytes: 1024,
            active_count: 3,
        };
        let json = serde_json::to_string(&stats).expect("should serialize");
        assert!(json.contains("\"total_completed\":5"));
        assert!(json.contains("\"total_failed\":2"));
        assert!(json.contains("\"total_bytes\":1024"));
    }

    #[test]
    fn test_transfer_stats_field_types() {
        let stats = TransferStats {
            total_completed: 0,
            total_failed: 0,
            total_cancelled: 0,
            total_bytes: 0,
            active_count: 0,
        };
        let json = serde_json::to_value(&stats).expect("should serialize");
        assert_eq!(json["total_completed"].as_u64(), Some(0));
        assert_eq!(json["total_failed"].as_u64(), Some(0));
        assert_eq!(json["total_cancelled"].as_u64(), Some(0));
        assert_eq!(json["total_bytes"].as_u64(), Some(0));
        assert_eq!(json["active_count"].as_u64(), Some(0));
    }

    #[test]
    fn test_transfer_stats_non_zero_values() {
        let stats = TransferStats {
            total_completed: 10,
            total_failed: 3,
            total_cancelled: 2,
            total_bytes: 9_999_999_999,
            active_count: 1,
        };
        let json = serde_json::to_value(&stats).expect("should serialize");
        assert_eq!(json["total_completed"].as_u64(), Some(10));
        assert_eq!(json["total_failed"].as_u64(), Some(3));
        assert_eq!(json["total_cancelled"].as_u64(), Some(2));
        assert_eq!(json["total_bytes"].as_u64(), Some(9_999_999_999));
        assert_eq!(json["active_count"].as_u64(), Some(1));
    }

    // ==================== local_ip_address tests ====================

    #[test]
    fn test_local_ip_address_returns_non_empty() {
        let ip = crate::local_ip_address();
        assert!(!ip.is_empty(), "IP address should not be empty");
    }

    #[test]
    fn test_local_ip_address_valid_or_fallback() {
        let ip = crate::local_ip_address();
        let is_valid = ip.parse::<std::net::IpAddr>().is_ok();
        assert!(
            is_valid,
            "local_ip_address() should return a valid IP or the fallback, got: {}",
            ip
        );
    }

    // ==================== Download loop decision tests ====================

    #[test]
    fn test_should_retry_on_success() {
        let result: Result<(), AppError> = Ok(());
        assert!(!should_retry_in_download_loop(&result, true));
        assert!(!should_retry_in_download_loop(&result, false));
    }

    #[test]
    fn test_should_retry_on_paused() {
        let result: Result<(), AppError> = Err(AppError {
            message: "TRANSFER_PAUSED".into(),
        });
        assert!(!should_retry_in_download_loop(&result, true));
        assert!(!should_retry_in_download_loop(&result, false));
    }

    #[test]
    fn test_should_retry_on_cancelled() {
        let result: Result<(), AppError> = Err(AppError {
            message: "TRANSFER_CANCELLED".into(),
        });
        assert!(!should_retry_in_download_loop(&result, true));
        assert!(!should_retry_in_download_loop(&result, false));
    }

    #[test]
    fn test_should_retry_on_other_error_with_retry_available() {
        let result: Result<(), AppError> = Err(AppError {
            message: "连接超时".into(),
        });
        assert!(should_retry_in_download_loop(&result, true));
    }

    #[test]
    fn test_should_retry_on_other_error_without_retry() {
        let result: Result<(), AppError> = Err(AppError {
            message: "连接超时".into(),
        });
        assert!(!should_retry_in_download_loop(&result, false));
    }

    #[test]
    fn test_should_retry_on_pairing_required_error() {
        let result: Result<(), AppError> = Err(AppError {
            message: "PAIRING_REQUIRED".into(),
        });
        assert!(!should_retry_in_download_loop(&result, false));
    }

    #[test]
    fn test_should_retry_special_errors_never_retry() {
        let paused: Result<(), AppError> = Err(AppError {
            message: "TRANSFER_PAUSED".into(),
        });
        let cancelled: Result<(), AppError> = Err(AppError {
            message: "TRANSFER_CANCELLED".into(),
        });
        let network_err: Result<(), AppError> = Err(AppError {
            message: "network error".into(),
        });
        let pairing: Result<(), AppError> = Err(AppError {
            message: "PAIRING_REQUIRED".into(),
        });

        // Special errors never retry regardless of can_retry
        assert!(!should_retry_in_download_loop(&paused, true));
        assert!(!should_retry_in_download_loop(&cancelled, true));

        // Regular errors depend on can_retry
        assert!(should_retry_in_download_loop(&network_err, true));
        assert!(!should_retry_in_download_loop(&network_err, false));
        assert!(should_retry_in_download_loop(&pairing, true));
        assert!(!should_retry_in_download_loop(&pairing, false));
    }
}
