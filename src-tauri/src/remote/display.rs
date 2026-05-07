//! 多显示器信息模块

use display_info::DisplayInfo;

/// 显示器信息结构
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub is_primary: bool,
}

/// 获取所有显示器信息
pub fn get_monitors() -> Result<Vec<MonitorInfo>, String> {
    let displays = DisplayInfo::all().map_err(|e| format!("获取显示器信息失败: {}", e))?;

    let monitors: Vec<MonitorInfo> = displays
        .into_iter()
        .enumerate()
        .map(|(idx, d)| MonitorInfo {
            id: idx as u32,
            name: if d.name.is_empty() {
                format!("显示器 {}", idx + 1)
            } else {
                d.name
            },
            width: d.width,
            height: d.height,
            x: d.x,
            y: d.y,
            is_primary: d.is_primary,
        })
        .collect();

    Ok(monitors)
}
