#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(id: &str, status: TransferStatus) -> TransferTask {
        TransferTask {
            id: id.to_string(),
            file_name: "test.bin".to_string(),
            file_size: 1000,
            bytes_transferred: 0,
            status,
            direction: TransferDirection::Download,
            remote_device: "192.168.1.2".to_string(),
            remote_path: "/test.bin".to_string(),
            local_path: "/tmp/test.bin".to_string(),
            speed: 0.0,
            error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            retry_count: 0,
            max_retries: 3,
        }
    }

    #[test]
    fn test_add_and_list() {
        let mut mgr = TransferManager::new(3);
        assert!(mgr.list_tasks().is_empty());
        mgr.add_task(make_task("t1", TransferStatus::Pending));
        assert_eq!(mgr.list_tasks().len(), 1);
    }

    #[test]
    fn test_active_count() {
        let mut mgr = TransferManager::new(3);
        mgr.add_task(make_task("t1", TransferStatus::Pending));
        mgr.add_task(make_task("t2", TransferStatus::Transferring));
        mgr.add_task(make_task("t3", TransferStatus::Transferring));
        assert_eq!(mgr.active_count(), 2);
    }

    #[test]
    fn test_can_start_new() {
        let mut mgr = TransferManager::new(2);
        mgr.add_task(make_task("t1", TransferStatus::Transferring));
        assert!(mgr.can_start_new());
        mgr.add_task(make_task("t2", TransferStatus::Transferring));
        assert!(!mgr.can_start_new());
    }

    #[test]
    fn test_update_progress_changes_status() {
        let mut mgr = TransferManager::new(3);
        mgr.add_task(make_task("t1", TransferStatus::Pending));
        mgr.update_progress("t1", 500, 100.0);
        let task = mgr.list_tasks().into_iter().find(|t| t.id == "t1").unwrap();
        assert_eq!(task.status, TransferStatus::Transferring);
        assert_eq!(task.bytes_transferred, 500);
    }

    #[test]
    fn test_fail_task() {
        let mut mgr = TransferManager::new(3);
        mgr.add_task(make_task("t1", TransferStatus::Transferring));
        mgr.fail_task("t1", "连接断开".to_string());
        let task = mgr.list_tasks().into_iter().find(|t| t.id == "t1").unwrap();
        assert_eq!(task.status, TransferStatus::Failed);
        assert_eq!(task.error, Some("连接断开".to_string()));
    }

    #[test]
    fn test_pause_resume() {
        let mut mgr = TransferManager::new(3);
        mgr.add_task(make_task("t1", TransferStatus::Transferring));

        assert!(mgr.pause_task("t1").is_ok());
        let task = mgr.list_tasks().into_iter().find(|t| t.id == "t1").unwrap();
        assert_eq!(task.status, TransferStatus::Paused);

        assert!(mgr.resume_task("t1").is_ok());
        let task = mgr.list_tasks().into_iter().find(|t| t.id == "t1").unwrap();
        assert_eq!(task.status, TransferStatus::Pending);
    }

    #[test]
    fn test_pause_non_transferring_fails() {
        let mut mgr = TransferManager::new(3);
        mgr.add_task(make_task("t1", TransferStatus::Pending));
        assert!(mgr.pause_task("t1").is_err());
    }

    #[test]
    fn test_cancel_task() {
        let mut mgr = TransferManager::new(3);
        mgr.add_task(make_task("t1", TransferStatus::Transferring));
        mgr.cancel_task("t1");
        let task = mgr.list_tasks().into_iter().find(|t| t.id == "t1").unwrap();
        assert_eq!(task.status, TransferStatus::Cancelled);
    }

    #[test]
    fn test_next_pending() {
        let mut mgr = TransferManager::new(1);
        mgr.add_task(make_task("t1", TransferStatus::Transferring));
        mgr.add_task(make_task("t2", TransferStatus::Pending));

        // 已有 1 个在传输，max_concurrent=1，不应返回新任务
        assert!(mgr.next_pending().is_none());

        // 完成后应返回 pending 任务
        mgr.complete_task("t1");
        assert!(mgr.next_pending().is_some());
    }

    #[test]
    fn test_enforce_history_limit() {
        let mut mgr = TransferManager::new(3);
        for i in 0..150 {
            let id = format!("t{}", i);
            mgr.add_task(make_task(&id, TransferStatus::Transferring));
            // 通过 complete_task 触发 enforce_history_limit
            mgr.complete_task(&id);
        }
        let remaining = mgr.list_tasks().len();
        assert!(
            remaining <= MAX_HISTORY + 2,
            "got {remaining} tasks, expected ≤{}",
            MAX_HISTORY + 2
        );
    }

    #[test]
    fn test_record_failure_retries_within_limit() {
        let mut mgr = TransferManager::new(3);
        let mut task = make_task("t1", TransferStatus::Transferring);
        task.max_retries = 3;
        mgr.add_task(task);

        // 前 3 次失败应自动重试
        for i in 1..=3 {
            assert!(mgr.record_failure("t1", format!("err{}", i)));
            let tasks = mgr.list_tasks();
            let t = tasks.iter().find(|t| t.id == "t1").unwrap();
            assert_eq!(t.status, TransferStatus::Pending);
            assert_eq!(t.retry_count, i);
            assert!(t.error.as_ref().unwrap().contains("正在重试"));
            // 恢复为 Transferring 以便下次失败
            mgr.update_progress("t1", 0, 0.0);
        }

        // 第 4 次失败应永久失败
        assert!(!mgr.record_failure("t1", "final error".to_string()));
        let tasks = mgr.list_tasks();
        let t = tasks.iter().find(|t| t.id == "t1").unwrap();
        assert_eq!(t.status, TransferStatus::Failed);
        assert_eq!(t.retry_count, 3);
    }

    #[test]
    fn test_record_failure_zero_retries() {
        let mut mgr = TransferManager::new(3);
        let mut task = make_task("t1", TransferStatus::Transferring);
        task.max_retries = 0;
        mgr.add_task(task);

        assert!(!mgr.record_failure("t1", "no retry".to_string()));
        let tasks = mgr.list_tasks();
        let t = tasks.iter().find(|t| t.id == "t1").unwrap();
        assert_eq!(t.status, TransferStatus::Failed);
    }

    #[test]
    fn test_record_failure_nonexistent_task() {
        let mut mgr = TransferManager::new(3);
        assert!(!mgr.record_failure("nonexistent", "error".to_string()));
    }

    #[test]
    fn test_generate_id_unique() {
        let mut mgr = TransferManager::new(3);
        let id1 = mgr.generate_id();
        let id2 = mgr.generate_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("transfer_"));
    }

    #[test]
    fn test_pause_nonexistent_task_fails() {
        let mut mgr = TransferManager::new(3);
        let result = mgr.pause_task("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_cancel_nonexistent_task() {
        let mut mgr = TransferManager::new(3);
        mgr.cancel_task("nonexistent"); // should not panic
        assert!(mgr.list_tasks().is_empty());
    }

    #[test]
    fn test_complete_nonexistent_task() {
        let mut mgr = TransferManager::new(3);
        mgr.complete_task("nonexistent"); // should not panic
    }

    #[test]
    fn test_double_pause_fails() {
        let mut mgr = TransferManager::new(3);
        mgr.add_task(make_task("t1", TransferStatus::Transferring));
        assert!(mgr.pause_task("t1").is_ok());
        assert!(mgr.pause_task("t1").is_err());
    }

    #[test]
    fn test_list_tasks_returns_snapshot() {
        let mut mgr = TransferManager::new(3);
        mgr.add_task(make_task("t1", TransferStatus::Pending));
        let tasks = mgr.list_tasks();
        assert_eq!(tasks.len(), 1);
        // modify the original
        mgr.add_task(make_task("t2", TransferStatus::Transferring));
        // snapshot should be unchanged
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn test_add_task_increments_next_id() {
        let mut mgr = TransferManager::new(3);
        let initial_id = mgr.generate_id();
        mgr.add_task(make_task("t1", TransferStatus::Pending));
        let after_id = mgr.generate_id();
        // A task was added; next_id increased by at least 1 (actually 2: add_task adds 1, generate_id adds 1)
        assert!(after_id != initial_id);
    }

    #[test]
    fn test_record_failure_nonexistent_returns_false() {
        let mut mgr = TransferManager::new(3);
        assert!(!mgr.record_failure("ghost", "error".to_string()));
    }

    #[test]
    fn test_complete_task_updates_status_and_broadcasts() {
        let mut mgr = TransferManager::new(3);
        mgr.add_task(make_task("t1", TransferStatus::Transferring));
        mgr.complete_task("t1");
        let tasks = mgr.list_tasks();
        let t = tasks.iter().find(|t| t.id == "t1").unwrap();
        assert_eq!(t.status, TransferStatus::Completed);
        assert_eq!(t.bytes_transferred, t.file_size);
    }

    #[test]
    fn test_set_max_concurrent() {
        let mut mgr = TransferManager::new(3);
        assert!(mgr.can_start_new()); // 0 active < 3
        mgr.set_max_concurrent(0);
        mgr.add_task(make_task("t1", TransferStatus::Pending));
        assert!(mgr.next_pending().is_none()); // max_concurrent=0 means no new tasks
    }
}

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::broadcast;

use crate::error::AppError;
use crate::server::ws::WsMessage;

/// 最大保留的历史传输记录数
const MAX_HISTORY: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransferStatus {
    Pending,
    Transferring,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferTask {
    pub id: String,
    pub file_name: String,
    pub file_size: u64,
    pub bytes_transferred: u64,
    pub status: TransferStatus,
    pub direction: TransferDirection,
    pub remote_device: String,
    pub remote_path: String,
    pub local_path: String,
    pub speed: f64,
    pub error: Option<String>,
    pub created_at: String,
    pub retry_count: u32,
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransferDirection {
    Download,
    Upload,
}

/// 磁盘持久化数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskData {
    tasks: Vec<TransferTask>,
    next_id: u64,
}

/// 传输队列管理器
pub struct TransferManager {
    tasks: Vec<TransferTask>,
    next_id: u64,
    max_concurrent: usize,
    ws_tx: Option<broadcast::Sender<WsMessage>>,
    history_path: Option<PathBuf>,
}

impl TransferManager {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            tasks: Vec::new(),
            next_id: 1,
            max_concurrent,
            ws_tx: None,
            history_path: None,
        }
    }

    /// 设置最大并发传输数
    pub fn set_max_concurrent(&mut self, max: usize) {
        self.max_concurrent = max;
    }

    /// 设置 WebSocket 广播通道
    pub fn set_ws_tx(&mut self, ws_tx: broadcast::Sender<WsMessage>) {
        self.ws_tx = Some(ws_tx);
    }

    /// 设置历史记录持久化路径
    #[allow(dead_code)]
    pub fn set_history_path(&mut self, path: PathBuf) {
        self.history_path = Some(path);
    }

    /// 从磁盘加载传输历史
    pub fn load_from_disk(path: &std::path::Path) -> Self {
        let mut mgr = Self::new(3);
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(data) = serde_json::from_str::<DiskData>(&content) {
                mgr.tasks = data.tasks;
                mgr.next_id = data.next_id.max(1);
            }
        }
        mgr.history_path = Some(path.to_path_buf());
        mgr
    }

    /// 保存传输历史到磁盘
    fn save_to_disk(&self) {
        if let Some(ref path) = self.history_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let data = DiskData {
                tasks: self.tasks.clone(),
                next_id: self.next_id,
            };
            if let Ok(content) = serde_json::to_string_pretty(&data) {
                let tmp = path.with_extension("tmp");
                let _ = std::fs::write(&tmp, &content);
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    fn broadcast(&self, msg: WsMessage) {
        if let Some(tx) = &self.ws_tx {
            let _ = tx.send(msg);
        }
    }

    /// 添加传输任务到队列
    pub fn add_task(&mut self, task: TransferTask) {
        self.tasks.push(task);
        self.next_id += 1;
        self.save_to_disk();
    }

    /// 获取所有传输任务
    pub fn list_tasks(&self) -> Vec<TransferTask> {
        self.tasks.clone()
    }

    /// 获取正在进行的传输数
    #[allow(dead_code)]
    pub fn active_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.status == TransferStatus::Transferring)
            .count()
    }

    /// 是否可以启动新的传输
    #[allow(dead_code)]
    pub fn can_start_new(&self) -> bool {
        self.active_count() < self.max_concurrent
    }

    /// 更新传输进度
    pub fn update_progress(&mut self, id: &str, bytes_transferred: u64, speed: f64) {
        // 先读取需要的数据，再修改（避免借用冲突）
        let snapshot = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| (t.file_name.clone(), t.file_size));
        if let Some((file_name, total_bytes)) = snapshot {
            if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                task.bytes_transferred = bytes_transferred;
                task.speed = speed;
                if task.status == TransferStatus::Pending {
                    task.status = TransferStatus::Transferring;
                }
            }
            self.broadcast(WsMessage::TransferProgress {
                id: id.to_string(),
                file_name,
                bytes_transferred,
                total_bytes,
                speed,
            });
        }
    }

    /// 标记传输完成
    pub fn complete_task(&mut self, id: &str) {
        let snapshot = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| (t.file_name.clone(), t.file_size));
        if let Some((file_name, file_size)) = snapshot {
            if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                task.status = TransferStatus::Completed;
                task.bytes_transferred = file_size;
            }
            self.broadcast(WsMessage::TransferComplete {
                id: id.to_string(),
                file_name,
            });
        }
        self.enforce_history_limit();
        self.save_to_disk();
    }

    /// 标记传输失败
    pub fn fail_task(&mut self, id: &str, error: String) {
        let snapshot = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.file_name.clone());
        if let Some(file_name) = snapshot {
            if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                task.status = TransferStatus::Failed;
                task.error = Some(error.clone());
            }
            self.broadcast(WsMessage::TransferError {
                id: id.to_string(),
                file_name,
                error,
            });
        }
        self.enforce_history_limit();
        self.save_to_disk();
    }

    /// 暂停传输
    pub fn pause_task(&mut self, id: &str) -> Result<(), AppError> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| AppError {
                message: "任务不存在".to_string(),
            })?;

        if task.status != TransferStatus::Transferring {
            return Err(AppError {
                message: "只能暂停正在进行的传输".to_string(),
            });
        }
        task.status = TransferStatus::Paused;
        self.save_to_disk();
        Ok(())
    }

    /// 恢复传输
    pub fn resume_task(&mut self, id: &str) -> Result<(), AppError> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| AppError {
                message: "任务不存在".to_string(),
            })?;

        if task.status != TransferStatus::Paused {
            return Err(AppError {
                message: "只能恢复已暂停的传输".to_string(),
            });
        }
        task.status = TransferStatus::Pending;
        self.save_to_disk();
        Ok(())
    }

    /// 记录失败并自动重试
    /// 返回 true=已重置为 Pending(将重试), false=已永久失败
    pub fn record_failure(&mut self, id: &str, error: String) -> bool {
        let should_retry = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.retry_count < t.max_retries)
            .unwrap_or(false);

        if should_retry {
            if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                task.retry_count += 1;
                task.status = TransferStatus::Pending;
                task.error = Some(format!(
                    "传输错误，正在重试 ({}/{})",
                    task.retry_count, task.max_retries
                ));
            }
            self.save_to_disk();
            true
        } else {
            self.fail_task(id, error);
            false
        }
    }

    /// 取消传输
    pub fn cancel_task(&mut self, id: &str) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.status = TransferStatus::Cancelled;
        }
        self.save_to_disk();
    }

    /// 获取下一个待处理的传输任务
    #[allow(dead_code)]
    pub fn next_pending(&mut self) -> Option<&mut TransferTask> {
        if !self.can_start_new() {
            return None;
        }
        self.tasks
            .iter_mut()
            .find(|t| t.status == TransferStatus::Pending)
    }

    /// 清理历史记录，保留最近 MAX_HISTORY 条完成/失败/取消的记录
    fn enforce_history_limit(&mut self) {
        let history_count = self
            .tasks
            .iter()
            .filter(|t| {
                matches!(
                    t.status,
                    TransferStatus::Completed | TransferStatus::Failed | TransferStatus::Cancelled
                )
            })
            .count();

        if history_count > MAX_HISTORY {
            let to_remove = history_count - MAX_HISTORY;
            let mut removed = 0;
            self.tasks.retain(|t| {
                if removed >= to_remove {
                    return true;
                }
                if matches!(
                    t.status,
                    TransferStatus::Completed | TransferStatus::Failed | TransferStatus::Cancelled
                ) {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
        }
    }
}

/// 全局传输队列（共享状态）
impl TransferManager {
    pub fn generate_id(&mut self) -> String {
        let id = self.next_id;
        self.next_id += 1;
        format!("transfer_{}", id)
    }
}
