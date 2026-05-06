use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

const MAX_LOG_ENTRIES: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub operation: String,
    pub detail: String,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskLog {
    entries: Vec<LogEntry>,
}

#[derive(Clone)]
pub struct OperationLog {
    inner: Arc<Mutex<Vec<LogEntry>>>,
    path: Option<PathBuf>,
}

impl OperationLog {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
            path: None,
        }
    }

    pub fn from_disk(path: &std::path::Path) -> Self {
        let log = Self::new();
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(data) = serde_json::from_str::<DiskLog>(&content) {
                let entries: Vec<LogEntry> = data.entries;
                let inner = log.inner.clone();
                tokio::spawn(async move {
                    let mut inner = inner.lock().await;
                    *inner = entries;
                });
            }
        }
        log
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    async fn save(&self) {
        if let Some(ref path) = self.path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let inner = self.inner.lock().await;
            let data = DiskLog {
                entries: inner.clone(),
            };
            if let Ok(content) = serde_json::to_string_pretty(&data) {
                let tmp = path.with_extension("tmp");
                let _ = std::fs::write(&tmp, &content);
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    pub async fn add(&self, operation: &str, detail: &str, result: &str) {
        let mut inner = self.inner.lock().await;
        inner.push(LogEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            operation: operation.to_string(),
            detail: detail.to_string(),
            result: result.to_string(),
        });
        if inner.len() > MAX_LOG_ENTRIES {
            inner.remove(0);
        }
        drop(inner);
        self.save().await;
    }

    pub async fn list(&self) -> Vec<LogEntry> {
        self.inner.lock().await.clone()
    }

    pub async fn clear(&self) {
        self.inner.lock().await.clear();
        self.save().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_new_creates_empty_log() {
        let log = OperationLog::new();
        let entries = log.list().await;
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_add_returns_entries_in_order() {
        let log = OperationLog::new();
        log.add("op1", "detail1", "ok").await;
        log.add("op2", "detail2", "fail").await;
        log.add("op3", "detail3", "ok").await;

        let entries = log.list().await;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].operation, "op1");
        assert_eq!(entries[1].operation, "op2");
        assert_eq!(entries[2].operation, "op3");
        assert_eq!(entries[0].detail, "detail1");
        assert_eq!(entries[1].detail, "detail2");
        assert_eq!(entries[2].detail, "detail3");
        assert_eq!(entries[0].result, "ok");
        assert_eq!(entries[1].result, "fail");
        assert_eq!(entries[2].result, "ok");
    }

    #[tokio::test]
    async fn test_clear_empties_log() {
        let log = OperationLog::new();
        log.add("op1", "detail1", "ok").await;
        log.add("op2", "detail2", "fail").await;
        assert_eq!(log.list().await.len(), 2);

        log.clear().await;
        assert!(log.list().await.is_empty());
    }

    #[tokio::test]
    async fn test_ring_buffer_keeps_max_200() {
        let log = OperationLog::new();
        for i in 0..201 {
            log.add(&format!("op{}", i), &format!("detail{}", i), "ok").await;
        }

        let entries = log.list().await;
        assert_eq!(entries.len(), 200);
        // Oldest entry (op0) should be removed; op1 is now first
        assert_eq!(entries[0].operation, "op1");
        assert_eq!(entries[199].operation, "op200");
    }

    #[tokio::test]
    async fn test_from_disk_loads_valid_json() {
        let dir = std::env::temp_dir().join(format!("ffeel-test-log-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("log.json");

        let json = r#"{
            "entries": [
                {
                    "timestamp": "2025-01-01T00:00:00+00:00",
                    "operation": "create",
                    "detail": "file.txt",
                    "result": "ok"
                },
                {
                    "timestamp": "2025-01-02T00:00:00+00:00",
                    "operation": "delete",
                    "detail": "old.txt",
                    "result": "ok"
                }
            ]
        }"#;
        std::fs::write(&path, json).unwrap();

        let log = OperationLog::from_disk(&path);
        // from_disk spawns a tokio task that sets inner entries; yield to let it run
        tokio::task::yield_now().await;

        let loaded = log.list().await;
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].operation, "create");
        assert_eq!(loaded[1].operation, "delete");
        assert_eq!(loaded[0].detail, "file.txt");
        assert_eq!(loaded[1].detail, "old.txt");
        assert_eq!(loaded[0].result, "ok");
        assert_eq!(loaded[1].result, "ok");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_from_disk_nonexistent_file_creates_empty_log() {
        let dir = std::env::temp_dir().join(format!("ffeel-test-log-nonexistent-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("does_not_exist.json");

        // This should not panic; should return an empty log
        let log = OperationLog::from_disk(&path);
        tokio::task::yield_now().await;

        let entries = log.list().await;
        assert!(entries.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
