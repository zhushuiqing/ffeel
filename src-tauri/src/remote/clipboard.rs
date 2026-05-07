//! 剪贴板同步模块 - 支持远程控制中的剪贴板内容同步

use arboard::Clipboard;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

/// 剪贴板同步服务
pub struct ClipboardSync {
    clipboard: Option<Arc<Mutex<Clipboard>>>,
    /// 本地剪贴板变化通知通道
    change_tx: broadcast::Sender<String>,
    /// 是否可用（无 X11/Wayland 环境时为 false）
    available: bool,
}

impl ClipboardSync {
    /// 创建新的剪贴板同步服务
    /// 在无显示服务器环境（如 CI）中会返回一个 noop 版本
    pub fn new() -> Result<Self, String> {
        match Clipboard::new() {
            Ok(clipboard) => {
                let (change_tx, _) = broadcast::channel::<String>(16);
                Ok(Self {
                    clipboard: Some(Arc::new(Mutex::new(clipboard))),
                    change_tx,
                    available: true,
                })
            }
            Err(_) => {
                // 无显示服务器环境（CI），返回 noop 版本
                let (change_tx, _) = broadcast::channel::<String>(16);
                Ok(Self {
                    clipboard: None,
                    change_tx,
                    available: false,
                })
            }
        }
    }

    /// 获取剪贴板文本内容
    #[allow(dead_code)]
    pub async fn get_text(&self) -> Result<String, String> {
        if !self.available {
            return Err("剪贴板不可用（无显示服务器）".to_string());
        }
        let mut clipboard = self.clipboard.as_ref().unwrap().lock().await;
        clipboard
            .get_text()
            .map_err(|e| format!("获取剪贴板失败: {}", e))
    }

    /// 设置剪贴板文本内容
    pub async fn set_text(&self, text: &str) -> Result<(), String> {
        if !self.available {
            // noop 模式下不做任何操作，但也不报错
            return Ok(());
        }
        let mut clipboard = self.clipboard.as_ref().unwrap().lock().await;
        clipboard
            .set_text(text)
            .map_err(|e| format!("设置剪贴板失败: {}", e))?;
        // 广播本地变化
        let _ = self.change_tx.send(text.to_string());
        Ok(())
    }

    /// 获取剪贴板变化通知接收器
    #[allow(dead_code)]
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.change_tx.subscribe()
    }

    /// 克隆用于共享
    pub fn clone(&self) -> Self {
        Self {
            clipboard: self.clipboard.clone(),
            change_tx: self.change_tx.clone(),
            available: self.available,
        }
    }
}

impl Clone for ClipboardSync {
    fn clone(&self) -> Self {
        self.clone()
    }
}

impl Default for ClipboardSync {
    fn default() -> Self {
        Self::new().expect("剪贴板初始化不应失败（noop 模式可用）")
    }
}
