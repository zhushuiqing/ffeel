use crate::error::AppError;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

const SERVICE_TYPE: &str = "_ffeel._tcp.local.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub platform: String,
    pub online: bool,
}

/// mDNS 设备发现管理器
pub struct DiscoveryManager {
    daemon: Arc<ServiceDaemon>,
    running: Arc<AtomicBool>,
    device_tx: broadcast::Sender<DeviceEvent>,
}

#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Found(DeviceInfo),
    Lost(String),
}

impl DiscoveryManager {
    /// 创建新 DiscoveryManager 并启动 mDNS 守护进程
    pub fn new() -> Result<Self, AppError> {
        let daemon = ServiceDaemon::new().map_err(|e| AppError {
            message: format!("mDNS 初始化失败: {}", e),
        })?;

        let (device_tx, _) = broadcast::channel(64);

        Ok(Self {
            daemon: Arc::new(daemon),
            running: Arc::new(AtomicBool::new(false)),
            device_tx,
        })
    }

    /// 注册本机服务，让局域网其他设备可以发现
    pub fn register_service(
        &self,
        device_id: &str,
        device_name: &str,
        host_ip: &str,
        port: u16,
        platform: &str,
    ) -> Result<(), AppError> {
        let txt_props = [
            ("id", device_id),
            ("name", device_name),
            ("platform", platform),
        ];

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            device_name,
            &format!("{}.local.", device_name),
            host_ip,
            port,
            &txt_props[..],
        )
        .map_err(|e| AppError {
            message: format!("mDNS 注册失败: {}", e),
        })?;

        self.daemon.register(service_info).map_err(|e| AppError {
            message: format!("mDNS 注册失败: {}", e),
        })?;

        Ok(())
    }

    /// 注销服务
    pub fn unregister_service(&self, device_name: &str) {
        let _ = self
            .daemon
            .unregister(&format!("{}.{}", device_name, SERVICE_TYPE));
    }

    /// 重新注册服务（设备重命名时调用）
    pub fn update_registration(
        &self,
        old_name: &str,
        device_id: &str,
        new_name: &str,
        host_ip: &str,
        port: u16,
        platform: &str,
    ) -> Result<(), AppError> {
        if old_name != new_name {
            self.unregister_service(old_name);
        }
        self.register_service(device_id, new_name, host_ip, port, platform)
    }

    /// 开始扫描局域网中的 ffeel 设备
    pub fn start_browsing(&self) -> Result<broadcast::Receiver<DeviceEvent>, AppError> {
        self.running.store(true, Ordering::SeqCst);

        let receiver = self.daemon.browse(SERVICE_TYPE).map_err(|e| AppError {
            message: format!("mDNS browse 失败: {}", e),
        })?;

        let device_tx = self.device_tx.clone();
        let running = self.running.clone();

        std::thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                match receiver.recv() {
                    Ok(ServiceEvent::ServiceResolved(info)) => {
                        let id = info
                            .get_property("id")
                            .map(|v| v.val_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let name = info
                            .get_property("name")
                            .map(|v| v.val_str())
                            .unwrap_or(info.get_hostname())
                            .to_string();
                        let platform = info
                            .get_property("platform")
                            .map(|v| v.val_str())
                            .unwrap_or("unknown")
                            .to_string();

                        let ip = info
                            .get_addresses()
                            .iter()
                            .next()
                            .map(|a| a.to_string())
                            .unwrap_or_default();

                        let device = DeviceInfo {
                            id,
                            name,
                            ip,
                            port: info.get_port(),
                            platform,
                            online: true,
                        };

                        let _ = device_tx.send(DeviceEvent::Found(device));
                    }
                    Ok(ServiceEvent::ServiceRemoved(_, full_name)) => {
                        let name = full_name.split('.').next().unwrap_or(&full_name);
                        let _ = device_tx.send(DeviceEvent::Lost(name.to_string()));
                    }
                    Err(e) => {
                        tracing::warn!("mDNS 接收错误: {}", e);
                    }
                    _ => {}
                }
            }
        });

        Ok(self.device_tx.subscribe())
    }

    /// 停止扫描
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

impl Drop for DiscoveryManager {
    fn drop(&mut self) {
        self.stop();
    }
}
