//! 屏幕录制模块 - 保存 JPEG 帧序列并合成视频

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// 录制状态
#[derive(Debug, Clone, PartialEq)]
pub enum RecordingState {
    Idle,
    Recording,
    Stopped,
}

/// 屏幕录制器
pub struct ScreenRecorder {
    frames: Arc<Mutex<Vec<Vec<u8>>>>,
    state: Arc<Mutex<RecordingState>>,
    start_time: Arc<Mutex<Option<Instant>>>,
    output_path: Arc<Mutex<Option<PathBuf>>>,
}

impl ScreenRecorder {
    pub fn new() -> Self {
        Self {
            frames: Arc::new(Mutex::new(Vec::new())),
            state: Arc::new(Mutex::new(RecordingState::Idle)),
            start_time: Arc::new(Mutex::new(None)),
            output_path: Arc::new(Mutex::new(None)),
        }
    }

    /// 开始录制
    pub fn start(&self, output_path: PathBuf) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|e| format!("锁错误: {}", e))?;
        if *state != RecordingState::Idle {
            return Err("录制已在进行中".to_string());
        }

        *state = RecordingState::Recording;
        drop(state);

        let mut frames = self.frames.lock().map_err(|e| format!("锁错误: {}", e))?;
        frames.clear();
        drop(frames);

        let mut start_time = self.start_time.lock().map_err(|e| format!("锁错误: {}", e))?;
        *start_time = Some(Instant::now());
        drop(start_time);

        let mut out = self.output_path.lock().map_err(|e| format!("锁错误: {}", e))?;
        *out = Some(output_path);
        drop(out);

        Ok(())
    }

    /// 添加一帧
    pub fn add_frame(&self, jpeg_data: &[u8]) -> Result<(), String> {
        let state = self.state.lock().map_err(|e| format!("锁错误: {}", e))?;
        if *state != RecordingState::Recording {
            return Ok(());
        }
        drop(state);

        let mut frames = self.frames.lock().map_err(|e| format!("锁错误: {}", e))?;
        // 限制最大帧数防止内存溢出（约 10 分钟 @ 10fps）
        if frames.len() < 6000 {
            frames.push(jpeg_data.to_vec());
        }
        drop(frames);

        Ok(())
    }

    /// 停止录制并保存视频
    pub fn stop(&self) -> Result<PathBuf, String> {
        let mut state = self.state.lock().map_err(|e| format!("锁错误: {}", e))?;
        if *state != RecordingState::Recording {
            return Err("没有正在进行的录制".to_string());
        }
        *state = RecordingState::Stopped;
        drop(state);

        let frames = self.frames.lock().map_err(|e| format!("锁错误: {}", e))?;
        let frame_count = frames.len();
        let frames_data = frames.clone();
        drop(frames);

        let output_path = self.output_path.lock().map_err(|e| format!("锁错误: {}", e))?;
        let path = output_path.clone().ok_or_else(|| "未设置输出路径".to_string())?;
        drop(output_path);

        // 生成 MJPEG AVI 文件
        let avi_data = self.create_mjpeg_avi(&frames_data)?;

        // 确保输出目录存在
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录失败: {}", e))?;
        }

        std::fs::write(&path, &avi_data)
            .map_err(|e| format!("写入文件失败: {}", e))?;

        tracing::info!("录制完成: {} 帧, 保存至 {:?}", frame_count, path);

        // 重置状态
        let mut state = self.state.lock().map_err(|e| format!("锁错误: {}", e))?;
        *state = RecordingState::Idle;
        drop(state);

        let mut frames = self.frames.lock().map_err(|e| format!("锁错误: {}", e))?;
        frames.clear();
        drop(frames);

        Ok(path)
    }

    /// 获取录制时长（秒）
    pub fn duration(&self) -> f64 {
        let start_time = self.start_time.lock().unwrap();
        if let Some(start) = *start_time {
            start.elapsed().as_secs_f64()
        } else {
            0.0
        }
    }

    /// 获取已录制帧数
    pub fn frame_count(&self) -> usize {
        self.frames.lock().unwrap().len()
    }

    /// 创建 MJPEG AVI 文件
    /// AVI 格式简单，不需要复杂编码库
    fn create_mjpeg_avi(&self, frames: &[Vec<u8>]) -> Result<Vec<u8>, String> {
        if frames.is_empty() {
            return Err("没有帧数据".to_string());
        }

        // 从第一帧获取分辨率（解析 JPEG SOI + SOF0）
        let (width, height) = self.parse_jpeg_resolution(&frames[0])?;

        let fps: u32 = 10; // 默认帧率

        // AVI RIFF 结构
        let mut avi = Vec::new();

        // RIFF header
        avi.extend_from_slice(b"RIFF");
        let riff_size_pos = avi.len();
        avi.extend_from_slice(&[0u8; 4]); // placeholder

        avi.extend_from_slice(b"AVI ");

        // hdrl LIST
        avi.extend_from_slice(b"LIST");
        let hdrl_size_pos = avi.len();
        avi.extend_from_slice(&[0u8; 4]); // placeholder
        avi.extend_from_slice(b"hdrl");

        // avih (main AVI header)
        avi.extend_from_slice(b"avih");
        avi.extend_from_slice(&56u32.to_le_bytes()); // chunk size
        avi.extend_from_slice(&(1_000_000u32 / fps).to_le_bytes()); // microseconds per frame
        avi.extend_from_slice(&(frames.len() as u32 * 1_000_000 / fps).to_le_bytes()); // max bytes per sec
        avi.extend_from_slice(&0u32.to_le_bytes()); // padding granularity
        avi.extend_from_slice(&0u32.to_le_bytes()); // flags
        avi.extend_from_slice(&(frames.len() as u32).to_le_bytes()); // total frames
        avi.extend_from_slice(&0u32.to_le_bytes()); // initial frames
        avi.extend_from_slice(&1u32.to_le_bytes()); // streams
        avi.extend_from_slice(&0u32.to_le_bytes()); // suggested buffer size
        avi.extend_from_slice(&width.to_le_bytes()); // width
        avi.extend_from_slice(&height.to_le_bytes()); // height
        avi.extend_from_slice(&[0u8; 16]); // reserved

        // strl LIST
        avi.extend_from_slice(b"LIST");
        let strl_size_pos = avi.len();
        avi.extend_from_slice(&[0u8; 4]); // placeholder
        avi.extend_from_slice(b"strl");

        // strh (stream header)
        avi.extend_from_slice(b"strh");
        avi.extend_from_slice(&56u32.to_le_bytes());
        avi.extend_from_slice(b"vids"); // stream type
        avi.extend_from_slice(b"MJPG"); // codec
        avi.extend_from_slice(&0u32.to_le_bytes()); // flags
        avi.extend_from_slice(&0u16.to_le_bytes()); // priority
        avi.extend_from_slice(&0u16.to_le_bytes()); // language
        avi.extend_from_slice(&0u32.to_le_bytes()); // initial frames
        avi.extend_from_slice(&1u32.to_le_bytes()); // scale
        avi.extend_from_slice(&fps.to_le_bytes()); // rate
        avi.extend_from_slice(&0u32.to_le_bytes()); // start
        avi.extend_from_slice(&(frames.len() as u32).to_le_bytes()); // length
        avi.extend_from_slice(&0u32.to_le_bytes()); // suggested buffer size
        avi.extend_from_slice(&0u32.to_le_bytes()); // quality
        avi.extend_from_slice(&0u32.to_le_bytes()); // sample size
        avi.extend_from_slice(&[0u8; 8]); // reserved

        // strf (stream format)
        avi.extend_from_slice(b"strf");
        avi.extend_from_slice(&40u32.to_le_bytes());
        avi.extend_from_slice(&40u32.to_le_bytes()); // biSize
        avi.extend_from_slice(&width.to_le_bytes()); // biWidth
        avi.extend_from_slice(&height.to_le_bytes()); // biHeight
        avi.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
        avi.extend_from_slice(&24u16.to_le_bytes()); // biBitCount
        avi.extend_from_slice(b"MJPG"); // biCompression
        avi.extend_from_slice(&(width as u32 * height as u32 * 3).to_le_bytes()); // biSizeImage
        avi.extend_from_slice(&0u32.to_le_bytes()); // biXPelsPerMeter
        avi.extend_from_slice(&0u32.to_le_bytes()); // biYPelsPerMeter
        avi.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
        avi.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

        // strl size
        let strl_size = (avi.len() - strl_size_pos - 4) as u32;
        avi[strl_size_pos..strl_size_pos + 4].copy_from_slice(&strl_size.to_le_bytes());

        // hdrl size
        let hdrl_size = (avi.len() - hdrl_size_pos - 4) as u32;
        avi[hdrl_size_pos..hdrl_size_pos + 4].copy_from_slice(&hdrl_size.to_le_bytes());

        // movi LIST
        avi.extend_from_slice(b"LIST");
        let movi_size_pos = avi.len();
        avi.extend_from_slice(&[0u8; 4]); // placeholder
        avi.extend_from_slice(b"movi");

        // 添加帧
        for frame in frames {
            avi.extend_from_slice(b"00dc"); // video chunk
            let chunk_size = frame.len() as u32;
            avi.extend_from_slice(&chunk_size.to_le_bytes());
            avi.extend_from_slice(frame);
            // Pad to even length if needed
            if frame.len() % 2 != 0 {
                avi.push(0);
            }
        }

        // movi size
        let movi_size = (avi.len() - movi_size_pos - 4) as u32;
        avi[movi_size_pos..movi_size_pos + 4].copy_from_slice(&movi_size.to_le_bytes());

        // RIFF size
        let riff_size = (avi.len() - riff_size_pos - 4) as u32;
        avi[riff_size_pos..riff_size_pos + 4].copy_from_slice(&riff_size.to_le_bytes());

        Ok(avi)
    }

    /// 从 JPEG 数据解析分辨率（简单解析 SOF0 标记）
    fn parse_jpeg_resolution(&self, jpeg: &[u8]) -> Result<(u32, u32), String> {
        // JPEG SOI: FF D8
        if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
            return Err("无效 JPEG 数据".to_string());
        }

        // 找 SOF0 标记: FF C0
        let mut pos = 2;
        while pos < jpeg.len() - 4 {
            if jpeg[pos] == 0xFF && jpeg[pos + 1] == 0xC0 {
                // SOF0: FF C0 len(2) precision(1) height(2) width(2)
                let height = u16::from_be_bytes([jpeg[pos + 5], jpeg[pos + 6]]);
                let width = u16::from_be_bytes([jpeg[pos + 7], jpeg[pos + 8]]);
                return Ok((width as u32, height as u32));
            }
            // 跳过其他标记
            if jpeg[pos] == 0xFF && jpeg[pos + 1] != 0x00 && jpeg[pos + 1] != 0xD8 && jpeg[pos + 1] != 0xD9 {
                let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
                pos += 2 + len;
            } else {
                pos += 1;
            }
        }

        Err("无法解析 JPEG 分辨率".to_string())
    }
}

impl Clone for ScreenRecorder {
    fn clone(&self) -> Self {
        Self {
            frames: self.frames.clone(),
            state: self.state.clone(),
            start_time: self.start_time.clone(),
            output_path: self.output_path.clone(),
        }
    }
}

impl Default for ScreenRecorder {
    fn default() -> Self {
        Self::new()
    }
}