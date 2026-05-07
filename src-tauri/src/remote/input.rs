//! 远程输入控制模块 - 鼠标/键盘输入模拟
//!
//! 使用 enigo 库模拟输入事件。由于 enigo::Enigo 不是 Send，
//! 我们使用 spawn_blocking 来执行实际输入操作。

use enigo::{
    Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings,
};

/// 输入事件类型
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InputEvent {
    pub event_type: String,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub button: Option<String>,
    pub key: Option<String>,
    pub delta: Option<i32>,
    pub text: Option<String>,
    pub keys: Option<String>, // 快捷键组合，逗号分隔
}

/// 处理输入事件（在阻塞线程中执行）
pub fn handle_input_event(event: &InputEvent) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("输入控制器初始化失败: {}", e))?;

    match event.event_type.as_str() {
        "mouse_move" => {
            if let (Some(x), Some(y)) = (event.x, event.y) {
                enigo
                    .move_mouse(x, y, Coordinate::Abs)
                    .map_err(|e| format!("鼠标移动失败: {}", e))?;
            }
        }
        "mouse_click" => {
            let btn = parse_button(event.button.as_deref().unwrap_or("left"));
            enigo
                .button(btn, Direction::Click)
                .map_err(|e| format!("鼠标点击失败: {}", e))?;
        }
        "mouse_press" => {
            let btn = parse_button(event.button.as_deref().unwrap_or("left"));
            enigo
                .button(btn, Direction::Press)
                .map_err(|e| format!("鼠标按下失败: {}", e))?;
        }
        "mouse_release" => {
            let btn = parse_button(event.button.as_deref().unwrap_or("left"));
            enigo
                .button(btn, Direction::Release)
                .map_err(|e| format!("鼠标释放失败: {}", e))?;
        }
        "mouse_scroll" => {
            let delta = event.delta.unwrap_or(0);
            let btn = if delta > 0 {
                Button::ScrollUp
            } else {
                Button::ScrollDown
            };
            // 每次点击一个单位，需要多次点击来模拟滚动量
            for _ in 0..delta.abs() {
                enigo
                    .button(btn, Direction::Click)
                    .map_err(|e| format!("滚轮滚动失败: {}", e))?;
            }
        }
        "key_click" => {
            let key = parse_key(event.key.as_deref().unwrap_or(""));
            enigo
                .key(key, Direction::Click)
                .map_err(|e| format!("按键点击失败: {}", e))?;
        }
        "key_press" => {
            let key = parse_key(event.key.as_deref().unwrap_or(""));
            enigo
                .key(key, Direction::Press)
                .map_err(|e| format!("按键按下失败: {}", e))?;
        }
        "key_release" => {
            let key = parse_key(event.key.as_deref().unwrap_or(""));
            enigo
                .key(key, Direction::Release)
                .map_err(|e| format!("按键释放失败: {}", e))?;
        }
        "text_input" => {
            if let Some(text) = &event.text {
                enigo
                    .text(text)
                    .map_err(|e| format!("文本输入失败: {}", e))?;
            }
        }
        "shortcut" => {
            if let Some(keys_str) = &event.keys {
                let keys: Vec<&str> = keys_str.split(',').collect();
                // 先按下所有键
                for k in &keys {
                    let key = parse_key(*k);
                    enigo
                        .key(key, Direction::Press)
                        .map_err(|e| format!("快捷键按下失败: {}", e))?;
                }
                // 再释放所有键（逆序）
                for k in keys.iter().rev() {
                    let key = parse_key(*k);
                    enigo
                        .key(key, Direction::Release)
                        .map_err(|e| format!("快捷键释放失败: {}", e))?;
                }
            }
        }
        _ => {}
    }

    Ok(())
}

/// 解析按键字符串为 Button 枚举
fn parse_button(button: &str) -> Button {
    match button.to_lowercase().as_str() {
        "left" => Button::Left,
        "right" => Button::Right,
        "middle" => Button::Middle,
        "scroll_up" => Button::ScrollUp,
        "scroll_down" => Button::ScrollDown,
        _ => Button::Left,
    }
}

/// 解析按键字符串为 Key 枚举
fn parse_key(key: &str) -> Key {
    match key.to_lowercase().as_str() {
        "ctrl" | "control" => Key::Control,
        "alt" => Key::Alt,
        "shift" => Key::Shift,
        "meta" | "cmd" | "command" | "win" | "windows" => Key::Meta,
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "escape" | "esc" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "up" | "arrowup" => Key::UpArrow,
        "down" | "arrowdown" => Key::DownArrow,
        "left" | "arrowleft" => Key::LeftArrow,
        "right" | "arrowright" => Key::RightArrow,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "space" => Key::Space,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        // 单字符使用 Unicode
        s if s.len() == 1 => {
            let c = s.chars().next().unwrap();
            Key::Unicode(c)
        }
        _ => Key::Unicode('a'),
    }
}