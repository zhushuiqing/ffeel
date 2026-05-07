//! 远程控制模块

mod clipboard;
mod display;
mod input;
mod permissions;
mod recording;

pub use clipboard::ClipboardSync;
pub use display::{get_monitors, MonitorInfo};
pub use input::{handle_input_event, InputEvent};
pub use permissions::{
    check_permissions, open_accessibility_settings, open_privacy_settings,
    open_screen_recording_settings, PermissionStatus,
};
pub use recording::{RecordingState, ScreenRecorder};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::Emitter;

/// JPEG quality for screen capture (1–100)
pub const DEFAULT_JPEG_QUALITY: u8 = 15;
/// Default target frame rate
pub const DEFAULT_FPS: u32 = 5;
/// Resolution scale factor (0.5 = half resolution)
pub const DEFAULT_SCALE: f32 = 0.3;

/// Captured screen frame data
pub struct ScreenFrame {
    pub jpeg_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Capture a single screen frame and encode as JPEG.
/// Returns the first (primary) monitor's content.
/// If `scale` < 1.0, the image is resized to reduce bandwidth.
pub fn capture_screen_jpeg(quality: u8, scale: f32) -> Result<ScreenFrame, String> {
    use image::codecs::jpeg::JpegEncoder;

    let monitors = xcap::Monitor::all().map_err(|e| format!("xcap error: {}", e))?;
    if monitors.is_empty() {
        return Err("No monitors found".to_string());
    }
    let monitor = monitors.first().unwrap();
    let img = monitor
        .capture_image()
        .map_err(|e| format!("capture error: {}", e))?;

    let (width, height, rgb_img) = if scale < 1.0 && scale > 0.0 {
        // 缩小分辨率以减少带宽（使用快速缩放）
        let new_width = (img.width() as f32 * scale) as u32;
        let new_height = (img.height() as f32 * scale) as u32;
        let resized = image::imageops::resize(&img, new_width, new_height, image::imageops::FilterType::Triangle);
        // 快速 RGBA → RGB 转换
        let rgb: Vec<u8> = resized.as_raw().chunks(4).map(|c| [c[0], c[1], c[2]].to_vec()).flatten().collect();
        let rgb_img = image::RgbImage::from_raw(new_width, new_height, rgb)
            .ok_or_else(|| "Failed to create RgbImage".to_string())?;
        (new_width, new_height, rgb_img)
    } else {
        let width = img.width();
        let height = img.height();
        let rgb: Vec<u8> = img.as_raw().chunks(4).map(|c| [c[0], c[1], c[2]].to_vec()).flatten().collect();
        let rgb_img = image::RgbImage::from_raw(width, height, rgb)
            .ok_or_else(|| "Failed to create RgbImage".to_string())?;
        (width, height, rgb_img)
    };

    let mut jpeg_data = Vec::new();
    {
        let mut encoder = JpegEncoder::new_with_quality(&mut jpeg_data, quality);
        encoder
            .encode(rgb_img.as_raw(), width, height, image::ExtendedColorType::Rgb8)
            .map_err(|e| format!("JPEG encode error: {}", e))?;
    }

    Ok(ScreenFrame {
        jpeg_data,
        width,
        height,
    })
}

/// Generate a test/placeholder screen capture for development or
/// when screen recording permission is unavailable.
pub fn capture_test_screen() -> ScreenFrame {
    let width: u32 = 800;
    let height: u32 = 600;
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);

    for y in 0..height {
        for x in 0..width {
            let r = ((x as f64 / width as f64) * 200.0 + 55.0) as u8;
            let g = ((y as f64 / height as f64) * 200.0 + 55.0) as u8;
            let b = 128u8;
            rgb.push(r);
            rgb.push(g);
            rgb.push(b);
        }
    }

    let img = image::RgbImage::from_raw(width, height, rgb).unwrap();
    let mut jpeg_data = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_data, 75);
    let _ = encoder.encode(img.as_raw(), width, height, image::ExtendedColorType::Rgb8);
    ScreenFrame {
        jpeg_data,
        width,
        height,
    }
}

/// Encode a JPEG frame as a MJPEG multipart chunk.
pub fn encode_mjpeg_frame(data: &[u8]) -> Vec<u8> {
    let header = format!(
        "--frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
        data.len()
    );
    let mut buf = Vec::with_capacity(header.len() + data.len() + 2);
    buf.extend_from_slice(header.as_bytes());
    buf.extend_from_slice(data);
    buf.extend_from_slice(b"\r\n");
    buf
}

/// Spawn a blocking task that captures the screen in a loop and sends MJPEG
/// frames through the given channel.  The loop runs at `fps` frames per second
/// and will stop when the channel is closed (receiver dropped).
/// If a recorder is provided, frames will also be saved to it during recording.
pub fn start_screen_stream(
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    quality: u8,
    fps: u32,
    scale: f32,
    use_test: bool,
    recorder: Option<ScreenRecorder>,
) {
    let interval = Duration::from_millis(1000 / fps.max(1) as u64);

    tokio::task::spawn_blocking(move || {
        let interval = interval;
        loop {
            let start = std::time::Instant::now();

            let result = if use_test {
                Ok(capture_test_screen())
            } else {
                capture_screen_jpeg(quality, scale)
            };

            match result {
                Ok(frame) => {
                    // 保存帧到录制器（如果正在录制）
                    if let Some(ref rec) = recorder {
                        if rec.duration() > 0.0 {
                            let _ = rec.add_frame(&frame.jpeg_data);
                        }
                    }

                    let chunk = encode_mjpeg_frame(&frame.jpeg_data);
                    if tx.blocking_send(chunk).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Screen capture stream error: {}", e);
                    let test_frame = capture_test_screen();

                    // 保存测试帧到录制器
                    if let Some(ref rec) = recorder {
                        if rec.duration() > 0.0 {
                            let _ = rec.add_frame(&test_frame.jpeg_data);
                        }
                    }

                    let chunk = encode_mjpeg_frame(&test_frame.jpeg_data);
                    if tx.blocking_send(chunk).is_err() {
                        break;
                    }
                }
            }

            let elapsed = start.elapsed();
            if elapsed < interval {
                std::thread::sleep(interval - elapsed);
            }
        }
    });
}

/// Relay a remote MJPEG HTTP stream as Tauri events.
/// Parses multipart/x-mixed-replace frames and emits `remote-screen-frame`
/// events with base64-encoded JPEG data.
pub async fn relay_mjpeg_stream(
    response: reqwest::Response,
    app: tauri::AppHandle,
    stop_flag: Arc<AtomicBool>,
) {
    use base64::Engine;
    use bytes::BytesMut;
    use futures_util::StreamExt;

    let mut stream = response.bytes_stream();
    let mut buf = BytesMut::new();
    let boundary: &[u8] = b"--frame";
    let mut frame_count: u64 = 0;
    let start_time = std::time::Instant::now();

    while let Some(chunk_result) = stream.next().await {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }

        let chunk = match chunk_result {
            Ok(c) => c,
            Err(_) => break,
        };
        buf.extend_from_slice(&chunk);

        // Extract frames from buffer
        loop {
            let boundary_pos = match buf
                .windows(boundary.len())
                .position(|w| w == boundary)
            {
                Some(p) => p,
                None => break,
            };

            let after_boundary = &buf[boundary_pos + boundary.len()..];
            let header_end = match after_boundary.windows(4).position(|w| w == b"\r\n\r\n") {
                Some(p) => boundary_pos + boundary.len() + p + 4,
                None => break,
            };

            // Try to find Content-Length for reliable frame extraction
            let header_section = std::str::from_utf8(&buf[boundary_pos + boundary.len()..header_end])
                .ok();
            let content_len = header_section.and_then(|h| {
                h.lines()
                    .find(|l| l.to_lowercase().starts_with("content-length:"))
                    .and_then(|l| l.split(':').nth(1)?.trim().parse::<usize>().ok())
            });

            let frame_end = if let Some(cl) = content_len {
                // Content-Length known: need exactly cl bytes + trailing \r\n
                if buf.len() < header_end + cl + 2 {
                    break;
                }
                header_end + cl + 2
            } else {
                // No Content-Length: scan for next --frame
                let remaining = &buf[header_end..];
                if let Some(next_b) = remaining.windows(boundary.len()).position(|w| w == boundary)
                {
                    let mut end = header_end + next_b;
                    // Strip trailing \r\n
                    if end >= 2 && buf[end - 2..end] == *b"\r\n" {
                        end -= 2;
                    }
                    end
                } else {
                    break;
                }
            };

            // Extract JPEG data
            let mut jpeg_data = &buf[header_end..frame_end];
            if jpeg_data.ends_with(b"\r\n") {
                jpeg_data = &jpeg_data[..jpeg_data.len() - 2];
            }

            if !jpeg_data.is_empty() {
                frame_count += 1;
                let elapsed = start_time.elapsed().as_secs_f64();
                let b64 = base64::engine::general_purpose::STANDARD.encode(jpeg_data);
                let _ = app.emit(
                    "remote-screen-frame",
                    serde_json::json!({
                        "data": b64,
                        "frame": frame_count,
                        "fps": if elapsed > 0.0 { frame_count as f64 / elapsed } else { 0.0 },
                    }),
                );
            }

            // Remove processed data from buffer
            let processed = frame_end.min(buf.len());
            let _ = buf.split_to(processed);
        }
    }
}
