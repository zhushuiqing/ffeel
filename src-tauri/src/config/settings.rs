use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// 共享目录路径
    pub share_dir: PathBuf,
    /// 监听端口 (0 表示自动分配)
    pub port: u16,
    /// 设备名称
    pub device_name: String,
    /// 默认下载目录
    pub download_dir: PathBuf,
    /// 最大并发传输数
    pub max_concurrent_transfers: usize,
    /// 传输速度限制 (字节/秒, 0 表示不限速)
    pub speed_limit: u64,
    /// 是否需要配对认证
    pub require_pairing: bool,
    /// 已信任的设备 ID 列表
    pub trusted_devices: Vec<String>,
    /// 信任设备名称映射（device_id → name）
    #[serde(default)]
    pub trusted_device_names: HashMap<String, String>,
    /// 信任设备备注（device_id → nickname）
    #[serde(default)]
    pub trusted_device_nicknames: HashMap<String, String>,
    /// 传输失败自动重试次数
    pub max_retries: u32,
    /// 持久化的配对码（重启后保持不变）
    #[serde(default)]
    pub pairing_code: Option<String>,
    /// 持久化的设备 ID（重启后保持不变）
    #[serde(default)]
    pub device_id: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            share_dir: dirs_or_default(),
            port: 0,
            device_name: hostname_or_default(),
            download_dir: download_dir_default(),
            max_concurrent_transfers: 3,
            speed_limit: 0,
            require_pairing: false,
            trusted_devices: Vec::new(),
            trusted_device_names: HashMap::new(),
            trusted_device_nicknames: HashMap::new(),
            max_retries: 3,
            pairing_code: None,
            device_id: None,
        }
    }
}

impl Settings {
    /// 配置文件路径：系统配置目录/ffeel/settings.json
    pub fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("ffeel");
        path.push("settings.json");
        path
    }

    /// 从文件加载设置，文件不存在时返回默认值
    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str(&content) {
                return settings;
            }
        }
        Self::default()
    }

    /// 保存设置到文件
    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // 原子写入：先写临时文件，再重命名
        let tmp_path = path.with_extension("tmp");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }
}

fn download_dir_default() -> PathBuf {
    dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn dirs_or_default() -> PathBuf {
    dirs::document_dir()
        .or_else(|| dirs::home_dir())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn hostname_or_default() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "ffeel-device".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let s = Settings::default();
        assert_eq!(s.port, 0);
        assert_eq!(s.max_concurrent_transfers, 3);
        assert_eq!(s.speed_limit, 0);
        assert!(!s.require_pairing);
        assert!(s.trusted_devices.is_empty());
        assert!(!s.device_name.is_empty());
        assert_eq!(s.max_retries, 3);
    }

    #[test]
    fn test_serialize_roundtrip() {
        let s = Settings {
            share_dir: PathBuf::from("/tmp/share"),
            port: 12345,
            device_name: "test-device".to_string(),
            download_dir: PathBuf::from("/tmp/downloads"),
            max_concurrent_transfers: 5,
            speed_limit: 1024,
            require_pairing: true,
            trusted_devices: vec!["dev1".to_string(), "dev2".to_string()],
            trusted_device_names: std::collections::HashMap::new(),
            trusted_device_nicknames: std::collections::HashMap::new(),
            pairing_code: Some("1234".to_string()),
            device_id: Some("uuid-1234".to_string()),
            max_retries: 5,
        };

        let json = serde_json::to_string(&s).unwrap();
        let deserialized: Settings = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.share_dir, s.share_dir);
        assert_eq!(deserialized.port, 12345);
        assert_eq!(deserialized.device_name, "test-device");
        assert_eq!(deserialized.max_concurrent_transfers, 5);
        assert_eq!(deserialized.speed_limit, 1024);
        assert!(deserialized.require_pairing);
        assert_eq!(deserialized.trusted_devices.len(), 2);
        assert_eq!(deserialized.pairing_code, Some("1234".to_string()));
    }
}
