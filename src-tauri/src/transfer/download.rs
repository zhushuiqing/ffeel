use crate::error::AppError;
use crate::transfer::queue::{TransferManager, TransferStatus};
use crate::transfer::rate_limiter::RateLimiter;
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// 断点续传临时文件后缀
const PART_SUFFIX: &str = ".ffeel.part";

/// 检查是否存在部分下载文件，返回已下载字节数
fn check_partial_file(local_path: &Path) -> (PathBuf, u64) {
    let part_path = local_path.with_extension(format!(
        "{}.part",
        local_path.extension().unwrap_or_default().to_string_lossy()
    ));
    // 如果没有扩展名，使用文件名+.part
    let part_path = if part_path.exists() {
        part_path
    } else {
        let fallback = local_path.with_extension("ffeel.part");
        if fallback.exists() {
            fallback
        } else {
            // 尝试原文件名 + .part
            let part_name = format!(
                "{}{}",
                local_path.file_name().unwrap_or_default().to_string_lossy(),
                PART_SUFFIX
            );
            let parent = local_path.parent().unwrap_or(Path::new("."));
            parent.join(part_name)
        }
    };

    let existing_size = if part_path.exists() {
        std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };
    (part_path, existing_size)
}

/// 从远程设备下载文件（支持断点续传）
pub async fn download_file_from_remote(
    remote_addr: &str,
    remote_path: &str,
    local_path: &PathBuf,
    transfer_manager: Arc<Mutex<TransferManager>>,
    speed_limit: u64,
) -> Result<(), AppError> {
    let (part_path, existing_size) = check_partial_file(local_path);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3600))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError {
            message: format!("HTTP 客户端创建失败: {}", e),
        })?;

    let url = format!(
        "https://{}/api/download?path={}",
        remote_addr,
        urlencoding::encode(remote_path)
    );

    let local_path_str = local_path.to_string_lossy().to_string();
    let mut req = client.get(&url);
    if existing_size > 0 {
        req = req.header("Range", format!("bytes={}-", existing_size));
    }

    let response = req.send().await.map_err(|e| AppError {
        message: format!("下载请求失败: {}", e),
    })?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError {
            message: "远程文件不存在".to_string(),
        });
    }
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(AppError {
            message: "PAIRING_REQUIRED".to_string(),
        });
    }
    if !response.status().is_success() && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
    {
        return Err(AppError {
            message: format!("下载失败: HTTP {}", response.status()),
        });
    }

    if let Some(parent) = local_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError {
                message: format!("创建目录失败: {}", e),
            })?;
    }

    // 处理 Range 响应：如果请求了 Range 但返回 200（不支持断点），从头开始下载
    let (mut file, mut downloaded) =
        if existing_size > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            // 续传：以追加模式打开
            let f = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&part_path)
                .await
                .map_err(|e| AppError {
                    message: format!("打开部分文件失败: {}", e),
                })?;
            (f, existing_size)
        } else {
            // 新建文件（覆盖已存在的部分文件，从头下载）
            let f = tokio::fs::File::create(&part_path)
                .await
                .map_err(|e| AppError {
                    message: format!("创建文件失败: {}", e),
                })?;
            (f, 0)
        };

    let mut stream = response.bytes_stream();
    let start = std::time::Instant::now();
    let mut limiter = if speed_limit > 0 {
        Some(RateLimiter::new(speed_limit))
    } else {
        None
    };
    let mut chunk_count: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError {
            message: format!("读取响应流失败: {}", e),
        })?;

        file.write_all(&chunk).await.map_err(|e| AppError {
            message: format!("写入文件失败: {}", e),
        })?;

        downloaded += chunk.len() as u64;

        if let Some(ref mut limiter) = limiter {
            limiter.consume(chunk.len() as u64).await;
        }

        let elapsed = start.elapsed().as_secs_f64().max(0.001);
        let speed = (downloaded - existing_size) as f64 / elapsed;

        {
            let mut mgr = transfer_manager.lock().await;
            for task in &mgr.list_tasks() {
                if task.local_path == local_path_str {
                    mgr.update_progress(&task.id, downloaded, speed);
                    break;
                }
            }
        }

        // 周期性检查任务状态（每 50 个块）
        chunk_count += 1;
        if chunk_count % 50 == 0 {
            let mgr = transfer_manager.lock().await;
            let tasks = mgr.list_tasks();
            if let Some(task) = tasks.iter().find(|t| t.local_path == local_path_str) {
                match task.status {
                    TransferStatus::Paused => {
                        return Err(AppError {
                            message: "TRANSFER_PAUSED".to_string(),
                        });
                    }
                    TransferStatus::Cancelled => {
                        // 取消下载，清理部分文件
                        drop(mgr);
                        let _ = tokio::fs::remove_file(&part_path).await;
                        return Err(AppError {
                            message: "TRANSFER_CANCELLED".to_string(),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // 下载完成，重命名为目标文件名
    tokio::fs::rename(&part_path, local_path)
        .await
        .map_err(|e| AppError {
            message: format!("重命名文件失败: {}", e),
        })?;

    {
        let mut mgr = transfer_manager.lock().await;
        for task in &mgr.list_tasks() {
            if task.local_path == local_path_str {
                mgr.complete_task(&task.id);
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_partial_file;
    use super::PART_SUFFIX;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU16, Ordering};

    static TEST_COUNTER: AtomicU16 = AtomicU16::new(0);

    fn test_dir() -> PathBuf {
        let pid = std::process::id();
        let count = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("ffeel_test_download_{}_{}", pid, count))
    }

    #[test]
    fn part_suffix_constant_is_correct() {
        assert_eq!(PART_SUFFIX, ".ffeel.part");
    }

    #[test]
    fn no_existing_part_file_returns_default_path_and_zero_size() {
        let dir = test_dir();
        fs::create_dir_all(&dir).unwrap();

        let local_path = dir.join("document.txt");

        let (part_path, size) = check_partial_file(&local_path);

        // With no part files on disk, the third fallback is used:
        // original filename + PART_SUFFIX
        let expected_name = format!("document.txt{}", PART_SUFFIX);
        assert_eq!(
            part_path.file_name().unwrap().to_string_lossy(),
            expected_name
        );
        assert_eq!(part_path.parent().unwrap(), dir);
        assert_eq!(size, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_empty_part_file_returns_part_path_and_zero_size() {
        let dir = test_dir();
        fs::create_dir_all(&dir).unwrap();

        let local_path = dir.join("test.txt");
        let expected_part = dir.join("test.txt.part");

        // Create the part file that the first fallback targets (txt.part)
        fs::write(&expected_part, b"").unwrap();

        let (part_path, size) = check_partial_file(&local_path);

        assert_eq!(part_path, expected_part);
        assert_eq!(size, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_part_file_with_content_returns_correct_byte_count() {
        let dir = test_dir();
        fs::create_dir_all(&dir).unwrap();

        let local_path = dir.join("test.txt");
        let expected_part = dir.join("test.txt.part");
        let content = b"Hello, this is partial download content!";

        fs::write(&expected_part, content).unwrap();

        let (part_path, size) = check_partial_file(&local_path);

        assert_eq!(part_path, expected_part);
        assert_eq!(size, content.len() as u64);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn part_file_on_second_fallback_exists() {
        let dir = test_dir();
        fs::create_dir_all(&dir).unwrap();

        // Only create the second-tier fallback: file.ffeel.part
        let local_path = dir.join("test.txt");
        let second_fallback = dir.join("test.ffeel.part");
        fs::write(&second_fallback, b"partial").unwrap();

        let (part_path, size) = check_partial_file(&local_path);

        assert_eq!(part_path, second_fallback);
        assert_eq!(size, 7);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_without_extension_handles_gracefully() {
        let dir = test_dir();
        fs::create_dir_all(&dir).unwrap();

        let local_path = dir.join("noext");
        let expected_name = format!("noext{}", PART_SUFFIX);

        let (part_path, size) = check_partial_file(&local_path);

        assert_eq!(
            part_path.file_name().unwrap().to_string_lossy(),
            expected_name
        );
        assert_eq!(size, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_fallback_takes_priority_over_second() {
        let dir = test_dir();
        fs::create_dir_all(&dir).unwrap();

        let local_path = dir.join("test.txt");
        let first = dir.join("test.txt.part");
        let second = dir.join("test.ffeel.part");

        // Both exist — first fallback should win
        fs::write(&first, b"first priority").unwrap();
        fs::write(&second, b"second priority").unwrap();

        let (part_path, size) = check_partial_file(&local_path);

        assert_eq!(part_path, first);
        assert_eq!(size, 14);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_dir_is_not_confused_with_part_file() {
        let dir = test_dir();
        fs::create_dir_all(&dir.join("subdir")).unwrap();

        // A directory named like a part file should not be detected as a part file
        let local_path = dir.join("subdir");
        let (part_path, size) = check_partial_file(&local_path);

        // Directory exists but is not a file -> metadata should error, unwrap_or returns 0
        assert_eq!(size, 0);
        // The path returned should be the third fallback
        assert!(part_path.to_string_lossy().ends_with(PART_SUFFIX));

        let _ = fs::remove_dir_all(&dir);
    }
}
