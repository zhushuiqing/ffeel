use chrono::Utc;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 信任的设备条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub name: String,
    pub nickname: Option<String>,
    pub paired_at: String,
}

/// 设备配对管理器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingManager {
    devices: HashMap<String, DeviceEntry>,
    current_code: Option<String>,
}

impl PairingManager {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            devices: HashMap::new(),
            current_code: None,
        }
    }

    /// 从信任设备 ID 列表加载（兼容旧格式）
    #[allow(dead_code)]
    pub fn from_ids(ids: &[String]) -> Self {
        let mut devices = HashMap::new();
        for id in ids {
            devices.insert(id.clone(), DeviceEntry {
                name: "未知设备".to_string(),
                nickname: None,
                paired_at: Utc::now().to_rfc3339(),
            });
        }
        Self {
            devices,
            current_code: None,
        }
    }

    /// 从详细信任列表加载（含名称）
    pub fn from_settings(
        ids: &[String],
        names: &HashMap<String, String>,
        nicknames: &HashMap<String, String>,
    ) -> Self {
        let mut devices = HashMap::new();
        let all_ids: std::collections::HashSet<String> = ids.iter().cloned().collect();
        // 合并所有 ID
        for id in names.keys() {
            let _ = all_ids.contains(id);
        }
        for id in ids {
            let name = names.get(id).cloned().unwrap_or_else(|| "未知设备".to_string());
            let nickname = nicknames.get(id).cloned();
            devices.insert(id.clone(), DeviceEntry {
                name,
                nickname,
                paired_at: Utc::now().to_rfc3339(),
            });
        }
        // 额外添加 names 中有但 ids 中没有的设备
        for (id, name) in names {
            if !devices.contains_key(id) {
                let nickname = nicknames.get(id).cloned();
                devices.insert(id.clone(), DeviceEntry {
                    name: name.clone(),
                    nickname,
                    paired_at: Utc::now().to_rfc3339(),
                });
            }
        }
        Self {
            devices,
            current_code: None,
        }
    }

    /// 获取信任设备 ID 列表
    pub fn trusted_ids(&self) -> Vec<String> {
        self.devices.keys().cloned().collect()
    }

    /// 获取所有信任设备信息（id, DeviceEntry）
    pub fn trusted_devices(&self) -> Vec<(String, DeviceEntry)> {
        self.devices.iter().map(|(id, info)| (id.clone(), info.clone())).collect()
    }

    pub fn generate_pairing_code() -> String {
        let mut rng = rand::thread_rng();
        let code: u32 = rng.gen_range(1000..10000);
        code.to_string()
    }

    #[allow(dead_code)]
    pub fn verify_pairing_code(code: &str) -> bool {
        !code.is_empty() && code.len() == 4 && code.chars().all(|c| c.is_ascii_digit())
    }

    /// 添加信任设备（旧版兼容）
    pub fn trust_device(&mut self, device_id: &str) {
        self.trust_device_with_name(device_id, "未知设备");
    }

    /// 添加信任设备（含名称）
    pub fn trust_device_with_name(&mut self, device_id: &str, name: &str) {
        self.devices.entry(device_id.to_string()).or_insert_with(|| DeviceEntry {
            name: name.to_string(),
            nickname: None,
            paired_at: Utc::now().to_rfc3339(),
        });
    }

    /// 移除信任设备
    pub fn untrust_device(&mut self, device_id: &str) {
        self.devices.remove(device_id);
    }

    /// 移除指定名称的所有信任设备（保留指定 ID）
    pub fn untrust_devices_by_name_except(&mut self, name: &str, except_id: &str) {
        let to_remove: Vec<String> = self.devices.iter()
            .filter(|(id, entry)| entry.name == name && *id != except_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in to_remove {
            self.devices.remove(&id);
        }
    }

    /// 检查是否受信任
    pub fn is_trusted(&self, device_id: &str) -> bool {
        self.devices.contains_key(device_id)
    }

    /// 获取设备显示名称（昵称>名称>ID片段）
    #[allow(dead_code)]
    pub fn get_display_name(&self, device_id: &str) -> String {
        self.devices.get(device_id).map(|entry| {
            entry.nickname.clone().unwrap_or_else(|| entry.name.clone())
        }).unwrap_or_else(|| device_id.chars().take(8).collect())
    }

    /// 设置设备昵称
    pub fn set_nickname(&mut self, device_id: &str, nickname: &str) -> String {
        if let Some(entry) = self.devices.get_mut(device_id) {
            entry.nickname = Some(nickname.to_string());
            nickname.to_string()
        } else {
            String::new()
        }
    }

    /// 更新设备名称（当设备重新连接时更新其名称）
    #[allow(dead_code)]
    pub fn update_name(&mut self, device_id: &str, name: &str) {
        if let Some(entry) = self.devices.get_mut(device_id) {
            entry.name = name.to_string();
        } else {
            self.trust_device_with_name(device_id, name);
        }
    }

    /// 生成新的配对码并返回
    pub fn rotate_code(&mut self) -> String {
        let code = Self::generate_pairing_code();
        self.current_code = Some(code.clone());
        code
    }

    /// 设置指定的配对码（用于从持久化配置恢复）
    pub fn set_code(&mut self, code: &str) {
        self.current_code = Some(code.to_string());
    }

    /// 获取当前配对码的引用
    #[allow(dead_code)]
    pub fn get_code(&self) -> Option<&str> {
        self.current_code.as_deref()
    }

    /// 获取当前配对码
    pub fn get_current_code(&self) -> Option<String> {
        self.current_code.clone()
    }

    /// 验证配对码并信任设备（旧版）
    #[allow(dead_code)]
    pub fn verify_and_trust(&mut self, device_id: &str, code: &str) -> bool {
        self.verify_and_trust_with_name(device_id, code, "未知设备")
    }

    /// 验证配对码并信任设备（含名称）
    pub fn verify_and_trust_with_name(&mut self, device_id: &str, code: &str, name: &str) -> bool {
        if let Some(ref current) = self.current_code {
            if current == code && !device_id.is_empty() {
                self.devices.insert(device_id.to_string(), DeviceEntry {
                    name: name.to_string(),
                    nickname: None,
                    paired_at: Utc::now().to_rfc3339(),
                });
                self.current_code = None;
                return true;
            }
        }
        false
    }
}

/// 线程安全的 PairingManager 包装
#[derive(Clone)]
pub struct SharedPairingManager {
    inner: Arc<Mutex<PairingManager>>,
}

impl SharedPairingManager {
    #[allow(dead_code)]
    pub fn new(manager: PairingManager) -> Self {
        Self {
            inner: Arc::new(Mutex::new(manager)),
        }
    }

    pub async fn is_trusted(&self, device_id: &str) -> bool {
        self.inner.lock().await.is_trusted(device_id)
    }

    pub async fn trust_device(&self, device_id: &str) {
        self.inner.lock().await.trust_device(device_id);
    }

    pub async fn trust_device_with_name(&self, device_id: &str, name: &str) {
        self.inner.lock().await.trust_device_with_name(device_id, name);
    }

    pub async fn untrust_device(&self, device_id: &str) {
        self.inner.lock().await.untrust_device(device_id);
    }

    pub async fn trusted_ids(&self) -> Vec<String> {
        self.inner.lock().await.trusted_ids()
    }

    /// 获取所有信任设备信息
    pub async fn trusted_devices(&self) -> Vec<(String, DeviceEntry)> {
        self.inner.lock().await.trusted_devices()
    }

    /// 获取设备显示名称
    #[allow(dead_code)]
    pub async fn get_display_name(&self, device_id: &str) -> String {
        self.inner.lock().await.get_display_name(device_id)
    }

    /// 设置昵称
    pub async fn set_nickname(&self, device_id: &str, nickname: &str) -> String {
        self.inner.lock().await.set_nickname(device_id, nickname)
    }

    /// 更新设备名称
    #[allow(dead_code)]
    pub async fn update_name(&self, device_id: &str, name: &str) {
        self.inner.lock().await.update_name(device_id, name);
    }

    pub async fn replace_inner(&self, manager: PairingManager) {
        let mut inner = self.inner.lock().await;
        *inner = manager;
    }

    pub async fn save_to_settings(&self, settings: &mut crate::config::Settings) {
        let inner = self.inner.lock().await;
        settings.trusted_devices = inner.trusted_ids();
        settings.trusted_device_names = inner.devices.iter()
            .map(|(id, e)| (id.clone(), e.name.clone()))
            .collect();
        settings.trusted_device_nicknames = inner.devices.iter()
            .filter_map(|(id, e)| e.nickname.clone().map(|n| (id.clone(), n)))
            .collect();
    }

    pub async fn rotate_code(&self) -> String {
        self.inner.lock().await.rotate_code()
    }

    /// 同步版本，用于非 async 上下文（如 Tauri setup 闭包）
    #[allow(dead_code)]
    pub fn rotate_code_sync(&self) -> String {
        self.inner.blocking_lock().rotate_code()
    }

    pub async fn set_code(&self, code: &str) {
        self.inner.lock().await.set_code(code);
    }

    pub async fn get_current_code(&self) -> Option<String> {
        self.inner.lock().await.get_current_code()
    }

    #[allow(dead_code)]
    pub async fn verify_and_trust(&self, device_id: &str, code: &str) -> bool {
        self.inner.lock().await.verify_and_trust(device_id, code)
    }

    pub async fn verify_and_trust_with_name(&self, device_id: &str, code: &str, name: &str) -> bool {
        self.inner.lock().await.verify_and_trust_with_name(device_id, code, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_is_empty() {
        let mgr = PairingManager::new();
        assert!(mgr.trusted_ids().is_empty());
    }

    #[test]
    fn test_from_ids() {
        let ids = vec!["a".to_string(), "b".to_string()];
        let mgr = PairingManager::from_ids(&ids);
        assert_eq!(mgr.trusted_ids().len(), 2);
    }

    #[test]
    fn test_trust_and_untrust() {
        let mut mgr = PairingManager::new();
        assert!(!mgr.is_trusted("device-123"));
        mgr.trust_device("device-123");
        assert!(mgr.is_trusted("device-123"));
        mgr.untrust_device("device-123");
        assert!(!mgr.is_trusted("device-123"));
    }

    #[test]
    fn test_empty_id_is_not_trusted() {
        let mgr = PairingManager::new();
        assert!(!mgr.is_trusted(""));
    }

    #[test]
    fn test_trust_with_name() {
        let mut mgr = PairingManager::new();
        mgr.trust_device_with_name("dev-1", "My MacBook");
        assert!(mgr.is_trusted("dev-1"));
        assert_eq!(mgr.get_display_name("dev-1"), "My MacBook");
    }

    #[test]
    fn test_set_nickname() {
        let mut mgr = PairingManager::new();
        mgr.trust_device_with_name("dev-1", "My MacBook");
        mgr.set_nickname("dev-1", "小明的电脑");
        assert_eq!(mgr.get_display_name("dev-1"), "小明的电脑");
    }

    #[test]
    fn test_get_display_name_fallback() {
        let mgr = PairingManager::new();
        // 未配对的设备显示 ID 前 8 位
        let name = mgr.get_display_name("long-device-id-12345");
        assert_eq!(name.len(), 8);
    }

    #[test]
    fn test_generate_pairing_code() {
        let code = PairingManager::generate_pairing_code();
        assert_eq!(code.len(), 4);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_verify_pairing_code() {
        assert!(PairingManager::verify_pairing_code("1234"));
        assert!(!PairingManager::verify_pairing_code(""));
        assert!(!PairingManager::verify_pairing_code("abc"));
        assert!(!PairingManager::verify_pairing_code("12"));
        assert!(!PairingManager::verify_pairing_code("12345"));
    }

    #[test]
    fn test_rotate_code_updates_and_returns() {
        let mut mgr = PairingManager::new();
        assert!(mgr.get_current_code().is_none());
        let code = mgr.rotate_code();
        assert_eq!(code.len(), 4);
        assert_eq!(mgr.get_current_code(), Some(code.clone()));
        let code2 = mgr.rotate_code();
        assert_ne!(code, code2);
        assert_eq!(mgr.get_current_code(), Some(code2));
    }

    #[test]
    fn test_verify_and_trust_empty_device_id_rejected() {
        let mut mgr = PairingManager::new();
        mgr.rotate_code();
        assert!(!mgr.verify_and_trust("", "1234"));
        assert!(mgr.trusted_ids().is_empty());
    }

    #[test]
    fn test_verify_and_trust_wrong_code_rejected() {
        let mut mgr = PairingManager::new();
        mgr.rotate_code();
        assert!(!mgr.verify_and_trust("device-456", "0000"));
        assert!(mgr.trusted_ids().is_empty());
    }

    #[test]
    fn test_verify_and_trust_success() {
        let mut mgr = PairingManager::new();
        let code = mgr.rotate_code();
        assert!(mgr.verify_and_trust("device-789", &code));
        assert!(mgr.is_trusted("device-789"));
        assert!(mgr.get_current_code().is_none());
    }

    #[test]
    fn test_verify_and_trust_with_name() {
        let mut mgr = PairingManager::new();
        let code = mgr.rotate_code();
        assert!(mgr.verify_and_trust_with_name("device-abc", &code, "Alice's PC"));
        assert!(mgr.is_trusted("device-abc"));
        assert_eq!(mgr.get_display_name("device-abc"), "Alice's PC");
    }

    #[test]
    fn test_shared_pairing_manager_delegates() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mgr = PairingManager::new();
            let shared = SharedPairingManager::new(mgr);
            assert!(shared.trusted_ids().await.is_empty());
            shared.trust_device_with_name("test-1", "TestDevice").await;
            assert!(shared.is_trusted("test-1").await);
            assert_eq!(shared.trusted_ids().await.len(), 1);
            assert_eq!(shared.get_display_name("test-1").await, "TestDevice");
            let code = shared.rotate_code().await;
            assert_eq!(code.len(), 4);
            assert!(shared.get_current_code().await.is_some());
            assert!(shared.verify_and_trust("test-2", &code).await);
        });
    }

    #[test]
    fn test_save_to_settings() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut mgr = PairingManager::new();
            mgr.trust_device_with_name("dev-a", "Device A");
            mgr.trust_device_with_name("dev-b", "Device B");
            let shared = SharedPairingManager::new(mgr);
            let mut settings = crate::config::Settings::default();
            shared.save_to_settings(&mut settings).await;
            assert_eq!(settings.trusted_devices.len(), 2);
            assert_eq!(settings.trusted_device_names.len(), 2);
            assert_eq!(settings.trusted_device_names.get("dev-a").unwrap(), "Device A");
        });
    }
}
