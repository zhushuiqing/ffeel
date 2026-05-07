//! 剪贴板同步模块 - 支持远程控制中的剪贴板内容同步

use arboard::Clipboard;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

/// 剪贴板同步服务
pub struct ClipboardSync {
    clipboard: Arc<Mutex<Clipboard>>,
    /// 本地剪贴板变化通知通道
    change_tx: broadcast::Sender<String>,
}

impl ClipboardSync {
    /// 创建新的剪贴板同步服务
    pub fn new() -> Result<Self, String> {
        let clipboard = Clipboard::new()
            .map_err(|e| format!("剪贴板初始化失败: {}", e))?;
        let (change_tx, _) = broadcast::channel::<String>(16);
        Ok(Self {
            clipboard: Arc::new(Mutex::new(clipboard)),
            change_tx,
        })
    }

    /// 获取剪贴板文本内容
    #[allow(dead_code)]
    pub async fn get_text(&self) -> Result<String, String> {
        let mut clipboard = self.clipboard.lock().await;
        clipboard
            .get_text()
            .map_err(|e| format!("获取剪贴板失败: {}", e))
    }

    /// 设置剪贴板文本内容
    pub async fn set_text(&self, text: &str) -> Result<(), String> {
        let mut clipboard = self.clipboard.lock().await;
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
        Self::new().expect("剪贴板初始化失败")
    }
}