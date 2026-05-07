//! macOS 权限检测模块
//!
//! 检测屏幕录制和辅助功能权限状态，提供引导 UI

#[cfg(target_os = "macos")]
use std::process::Command;

/// 权限状态
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionStatus {
    pub screen_recording: bool,
    pub accessibility: bool,
    pub all_granted: bool,
}

/// 检测 macOS 权限状态
#[cfg(target_os = "macos")]
pub fn check_permissions() -> PermissionStatus {
    let screen_recording = check_screen_recording_permission();
    let accessibility = check_accessibility_permission();
    PermissionStatus {
        screen_recording,
        accessibility,
        all_granted: screen_recording && accessibility,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn check_permissions() -> PermissionStatus {
    PermissionStatus {
        screen_recording: true,
        accessibility: true,
        all_granted: true,
    }
}

/// 检测屏幕录制权限（macOS 10.15+）
#[cfg(target_os = "macos")]
fn check_screen_recording_permission() -> bool {
    // 尝试捕获屏幕，失败则无权限
    use xcap::Monitor;
    if let Ok(monitors) = Monitor::all() {
        if let Some(monitor) = monitors.first() {
            return monitor.capture_image().is_ok();
        }
    }
    false
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn check_screen_recording_permission() -> bool {
    true
}

/// 检测辅助功能权限（用于鼠标/键盘控制）
#[cfg(target_os = "macos")]
fn check_accessibility_permission() -> bool {
    // 使用系统 API 检测
    // 简化版本：尝试创建 Enigo 实例
    use enigo::{Enigo, Settings};
    Enigo::new(&Settings::default()).is_ok()
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn check_accessibility_permission() -> bool {
    true
}

/// 打开系统设置中的屏幕录制隐私面板
#[cfg(target_os = "macos")]
pub fn open_screen_recording_settings() {
    // macOS Ventura+ 使用新的隐私设置路径
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn();
}

#[cfg(not(target_os = "macos"))]
pub fn open_screen_recording_settings() {}

/// 打开系统设置中的辅助功能隐私面板
#[cfg(target_os = "macos")]
pub fn open_accessibility_settings() {
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

#[cfg(not(target_os = "macos"))]
pub fn open_accessibility_settings() {}

/// 打开系统设置的隐私面板（通用）
#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub fn open_privacy_settings() {
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy")
        .spawn();
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
pub fn open_privacy_settings() {}
