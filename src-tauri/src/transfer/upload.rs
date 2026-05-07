use crate::error::AppError;
use crate::transfer::queue::{TransferManager, TransferStatus};
use bytes::Bytes;
use futures_util::Stream;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::AsyncRead;
use tokio::sync::Mutex;
use tokio_util::io::ReaderStream;

/// 带有进度追踪的流式上传流
struct ProgressStream<R> {
    inner: ReaderStream<R>,
    bytes_sent: u64,
    transfer_manager: Arc<Mutex<TransferManager>>,
    task_id: String,
    last_report: Instant,
}

impl<R: AsyncRead + Unpin> Stream for ProgressStream<R> {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        // 检查任务是否被暂停或取消，及时终止流
        if let Ok(mgr) = this.transfer_manager.try_lock() {
            let tasks = mgr.list_tasks();
            if let Some(task) = tasks.iter().find(|t| t.id == this.task_id) {
                if matches!(
                    task.status,
                    TransferStatus::Paused | TransferStatus::Cancelled
                ) {
                    return Poll::Ready(None);
                }
            }
        }

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.bytes_sent += chunk.len() as u64;
                let now = Instant::now();
                if now.duration_since(this.last_report) > Duration::from_millis(200) {
                    if let Ok(mut mgr) = this.transfer_manager.try_lock() {
                        let elapsed = now
                            .duration_since(this.last_report)
                            .as_secs_f64()
                            .max(0.001);
                        let speed = chunk.len() as f64 / elapsed;
                        mgr.update_progress(&this.task_id, this.bytes_sent, speed);
                    }
                    this.last_report = now;
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(None) => {
                // 流结束，确保最终进度被记录
                if let Ok(mut mgr) = this.transfer_manager.try_lock() {
                    mgr.update_progress(&this.task_id, this.bytes_sent, 0.0);
                }
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

/// 流式上传文件到远程设备（支持进度追踪）
pub async fn upload_file_to_remote(
    remote_addr: &str,
    port: u16,
    remote_path: &str,
    local_file: &Path,
    transfer_manager: Arc<Mutex<TransferManager>>,
    task_id: &str,
    _file_size: u64,
) -> Result<(), AppError> {
    let file_name = local_file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    let url = format!(
        "https://{}:{}/api/upload?path={}&name={}",
        remote_addr,
        port,
        urlencoding::encode(remote_path),
        urlencoding::encode(file_name),
    );

    let file = tokio::fs::File::open(local_file)
        .await
        .map_err(|e| AppError {
            message: format!("打开文件失败: {}", e),
        })?;

    // 创建进度追踪流
    let progress_stream = ProgressStream {
        inner: ReaderStream::new(file),
        bytes_sent: 0,
        transfer_manager: transfer_manager.clone(),
        task_id: task_id.to_string(),
        last_report: Instant::now(),
    };

    let body = reqwest::Body::wrap_stream(progress_stream);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3600))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError {
            message: format!("HTTP 客户端创建失败: {}", e),
        })?;

    let response = client
        .post(&url)
        .header("Content-Type", "application/octet-stream")
        .body(body)
        .send()
        .await
        .map_err(|e| AppError {
            message: format!("上传请求失败: {}", e),
        })?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(AppError {
            message: format!("上传失败: {}", text),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::server::http::serve_tls;
    use crate::transfer::queue::TransferManager;
    use crate::transfer::upload::upload_file_to_remote;
    use axum::body::Body;
    use axum::extract::Query;
    use axum::routing::post;
    use axum::Json;
    use axum::Router;
    use futures_util::StreamExt;
    use serde::Deserialize;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    fn test_cert() -> (String, String) {
        let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut params = rcgen::CertificateParams::new(vec!["ffeel.local".to_string()]).unwrap();
        params.distinguished_name = rcgen::DistinguishedName::new();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "ffeel-test");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let cert = params.self_signed(&key_pair).unwrap();
        (cert.pem(), key_pair.serialize_pem())
    }

    #[derive(Deserialize)]
    struct TestUploadQuery {
        path: Option<String>,
        name: Option<String>,
    }

    async fn test_upload_handler(
        Query(query): Query<TestUploadQuery>,
        body: Body,
    ) -> Json<serde_json::Value> {
        let mut stream = body.into_data_stream();
        let mut received = Vec::new();
        while let Some(chunk) = stream.next().await {
            if let Ok(data) = chunk {
                received.extend_from_slice(&data);
            }
        }
        Json(json!({
            "saved_bytes": received.len(),
            "filename": query.name.unwrap_or_default(),
        }))
    }

    #[tokio::test]
    async fn test_upload_file_successfully() {
        let app = Router::new().route("/api/upload", post(test_upload_handler));

        let (cert_pem, key_pem) = test_cert();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (_port_tx, mut port_rx) = tokio::sync::watch::channel(0);
        tokio::spawn(async move {
            let _tx = _port_tx;
            serve_tls(app, listener, &cert_pem, &key_pem, &mut port_rx)
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let content = b"Hello, this is test file content!";
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_upload_file.txt");
        tokio::fs::write(&temp_file, content).await.unwrap();

        let tm = Arc::new(Mutex::new(TransferManager::new(3)));
        let task_id = {
            let mut mgr = tm.lock().await;
            let id = mgr.generate_id();
            mgr.add_task(crate::transfer::queue::TransferTask {
                id: id.clone(),
                file_name: "test_upload_file.txt".to_string(),
                file_size: content.len() as u64,
                bytes_transferred: 0,
                status: crate::transfer::queue::TransferStatus::Pending,
                direction: crate::transfer::queue::TransferDirection::Upload,
                remote_device: addr.ip().to_string(),
                remote_path: "/remote/path/file.txt".to_string(),
                local_path: temp_file.to_string_lossy().to_string(),
                speed: 0.0,
                error: None,
                created_at: chrono::Utc::now().to_rfc3339(),
                retry_count: 0,
                max_retries: 3,
            });
            id
        };

        let result = upload_file_to_remote(
            &addr.ip().to_string(),
            addr.port(),
            "/remote/path/file.txt",
            &temp_file,
            tm.clone(),
            &task_id,
            content.len() as u64,
        )
        .await;

        assert!(result.is_ok());

        // 验证进度已更新
        {
            let mgr = tm.lock().await;
            let tasks = mgr.list_tasks();
            let task = tasks.iter().find(|t| t.id == task_id).unwrap();
            assert_eq!(task.bytes_transferred, content.len() as u64);
        }

        tokio::fs::remove_file(&temp_file).await.unwrap();
    }

    #[tokio::test]
    async fn test_upload_non_existent_file() {
        let temp_dir = std::env::temp_dir();
        let non_existent = temp_dir.join("nonexistent_file_xyz_12345.txt");

        let app = Router::new().route(
            "/api/upload",
            post(|_: Body| async move { Json(json!({"saved_bytes": 0})) }),
        );

        let (cert_pem, key_pem) = test_cert();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (_port_tx2, mut port_rx2) = tokio::sync::watch::channel(0);
        tokio::spawn(async move {
            let _tx2 = _port_tx2;
            serve_tls(app, listener, &cert_pem, &key_pem, &mut port_rx2)
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let tm = Arc::new(Mutex::new(TransferManager::new(3)));

        let result = upload_file_to_remote(
            &addr.ip().to_string(),
            addr.port(),
            "/remote/path/file.txt",
            &non_existent,
            tm.clone(),
            "test-id",
            0,
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("打开文件失败"));
    }
}
