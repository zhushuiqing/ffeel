import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  DeviceInfo,
  MonitorInfo,
  PairedDeviceInfo,
  PermissionStatus,
  RemoteScreenFrame,
} from "../types";
import * as api from "../api";

interface RemoteControlProps {
  trustedDeviceList: PairedDeviceInfo[];
  discoveredDevices: DeviceInfo[];
  onlineDeviceIds: string[];
  localDeviceId: string;
  onStatusMessage?: (msg: string) => void;
}

// Avatar helpers
const AVATAR_COLORS = [
  "#FF6B6B",
  "#FFA94D",
  "#FFD43B",
  "#69DB7C",
  "#38D9A9",
  "#4DABF7",
  "#748FFC",
  "#DA77F2",
];

function getAvatarColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}

function getInitial(name: string): string {
  return name.charAt(0).toUpperCase();
}

export default function RemoteControl({
  trustedDeviceList,
  discoveredDevices,
  onlineDeviceIds,
  localDeviceId,
  onStatusMessage,
}: RemoteControlProps) {
  const [selectedDevice, setSelectedDevice] = useState<PairedDeviceInfo | null>(
    null,
  );
  const [isConnected, setIsConnected] = useState(false);
  const [screenImage, setScreenImage] = useState<string | null>(null);
  const [fps, setFps] = useState(0);
  const [frameCount, setFrameCount] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [remoteWidth] = useState(1920);
  const [remoteHeight] = useState(1080);
  const [isLocalMode, setIsLocalMode] = useState(false);
  const [monitors] = useState<MonitorInfo[]>([]);
  const [selectedMonitor, setSelectedMonitor] = useState<number>(0);
  const [isRecording, setIsRecording] = useState(false);
  const [recordingDuration, setRecordingDuration] = useState(0);
  const [permissions, setPermissions] = useState<PermissionStatus | null>(null);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const imgRef = useRef<HTMLImageElement>(null);
  const unlistenRef = useRef<(() => void)[]>([]);
  const wsRef = useRef<WebSocket | null>(null);
  const lastTouchYRef = useRef<number>(0);

  // Listen for screen frame events
  useEffect(() => {
    const setup = async () => {
      const un1 = await listen<RemoteScreenFrame>(
        "remote-screen-frame",
        (event) => {
          const frame = event.payload;
          setScreenImage(`data:image/jpeg;base64,${frame.data}`);
          setFps(Math.round(frame.fps));
          setFrameCount(frame.frame);
        },
      );
      unlistenRef.current.push(un1);

      const un2 = await listen<string>("remote-screen-error", (event) => {
        setError(event.payload as unknown as string);
        setIsConnected(false);
      });
      unlistenRef.current.push(un2);

      const un3 = await listen("remote-screen-ended", () => {
        setIsConnected(false);
        setScreenImage(null);
      });
      unlistenRef.current.push(un3);
    };
    setup();

    return () => {
      unlistenRef.current.forEach((fn) => fn());
      unlistenRef.current = [];
    };
  }, []);

  // 检测权限状态（仅 macOS）
  useEffect(() => {
    const checkPerms = async () => {
      try {
        const perms = await api.checkPermissions();
        setPermissions(perms);
      } catch {
        // 非 macOS 系统会忽略
      }
    };
    checkPerms();
  }, []);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (isConnected) {
        api.stopRemoteControl().catch(() => {});
      }
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, [isConnected]);

  // 坐标映射：将前端显示坐标映射到远程实际坐标
  const mapCoords = (event: React.MouseEvent): { x: number; y: number } => {
    if (!imgRef.current) return { x: 0, y: 0 };
    const img = imgRef.current;
    const rect = img.getBoundingClientRect();
    const scaleX = remoteWidth / rect.width;
    const scaleY = remoteHeight / rect.height;
    return {
      x: Math.round((event.clientX - rect.left) * scaleX),
      y: Math.round((event.clientY - rect.top) * scaleY),
    };
  };

  // 发送输入事件到 WebSocket
  const sendInput = (eventType: string, data: Record<string, unknown>) => {
    if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN) {
      wsRef.current.send(
        JSON.stringify({
          type: "input",
          event_type: eventType,
          ...data,
        }),
      );
    }
  };

  // 鼠标事件处理器
  const handleMouseMove = (event: React.MouseEvent) => {
    const coords = mapCoords(event);
    sendInput("mouse_move", { x: coords.x, y: coords.y });
  };

  const handleMouseDown = (event: React.MouseEvent) => {
    const coords = mapCoords(event);
    const button =
      event.button === 0 ? "left" : event.button === 2 ? "right" : "middle";
    sendInput("mouse_press", { x: coords.x, y: coords.y, button });
  };

  const handleMouseUp = (event: React.MouseEvent) => {
    const coords = mapCoords(event);
    const button =
      event.button === 0 ? "left" : event.button === 2 ? "right" : "middle";
    sendInput("mouse_release", { x: coords.x, y: coords.y, button });
  };

  const handleWheel = (event: React.WheelEvent) => {
    sendInput("mouse_scroll", { delta_y: Math.round(event.deltaY / 10) });
  };

  // 触控手势支持
  const handleTouchStart = (event: React.TouchEvent) => {
    if (event.touches.length === 1) {
      const touch = event.touches[0];
      const coords = mapCoordsFromTouch(touch);
      sendInput("mouse_move", { x: coords.x, y: coords.y });
      lastTouchYRef.current = touch.clientY;
    } else if (event.touches.length === 2) {
      lastTouchYRef.current =
        (event.touches[0].clientY + event.touches[1].clientY) / 2;
    }
  };

  const handleTouchMove = (event: React.TouchEvent) => {
    if (event.touches.length === 1) {
      const touch = event.touches[0];
      const coords = mapCoordsFromTouch(touch);
      sendInput("mouse_move", { x: coords.x, y: coords.y });
    } else if (event.touches.length === 2) {
      // 双指滚动（模拟滚轮）
      const touch1 = event.touches[0];
      const touch2 = event.touches[1];
      const centerY = (touch1.clientY + touch2.clientY) / 2;
      const deltaY = centerY - lastTouchYRef.current;
      if (Math.abs(deltaY) > 5) {
        sendInput("mouse_scroll", { delta_y: Math.round(deltaY / 2) });
        lastTouchYRef.current = centerY;
      }
    }
  };

  const handleTouchEnd = (event: React.TouchEvent) => {
    if (event.changedTouches.length === 1) {
      const touch = event.changedTouches[0];
      const coords = mapCoordsFromTouch(touch);
      sendInput("mouse_click", { x: coords.x, y: coords.y, button: "left" });
    }
    lastTouchYRef.current = 0;
  };

  // 触控坐标映射
  const mapCoordsFromTouch = (touch: React.Touch): { x: number; y: number } => {
    if (!imgRef.current) return { x: 0, y: 0 };
    const img = imgRef.current;
    const rect = img.getBoundingClientRect();
    const scaleX = remoteWidth / rect.width;
    const scaleY = remoteHeight / rect.height;
    return {
      x: Math.round((touch.clientX - rect.left) * scaleX),
      y: Math.round((touch.clientY - rect.top) * scaleY),
    };
  };

  const handleClick = (event: React.MouseEvent) => {
    const coords = mapCoords(event);
    const button =
      event.button === 0 ? "left" : event.button === 2 ? "right" : "middle";
    sendInput("mouse_click", { x: coords.x, y: coords.y, button });
  };

  // 键盘事件处理器
  const handleKeyDown = (event: React.KeyboardEvent) => {
    const key = event.key;
    // 映射特殊键
    const mappedKey = mapKey(key);
    sendInput("key_press", { key: mappedKey });
    event.preventDefault();
  };

  const handleKeyUp = (event: React.KeyboardEvent) => {
    const key = event.key;
    const mappedKey = mapKey(key);
    sendInput("key_release", { key: mappedKey });
    event.preventDefault();
  };

  // 按键映射
  const mapKey = (key: string): string => {
    const keyMap: Record<string, string> = {
      Control: "ctrl",
      Alt: "alt",
      Shift: "shift",
      Meta: "meta",
      Enter: "enter",
      Tab: "tab",
      Escape: "escape",
      Backspace: "backspace",
      Delete: "delete",
      ArrowUp: "up",
      ArrowDown: "down",
      ArrowLeft: "left",
      ArrowRight: "right",
      Home: "home",
      End: "end",
      PageUp: "pageup",
      PageDown: "pagedown",
      " ": "space",
    };
    return keyMap[key] || key.toLowerCase();
  };

  // 快捷键发送
  const sendShortcut = (keys: string) => {
    sendInput("shortcut", { keys });
  };

  // 剪贴板同步：发送本地剪贴板到远程
  const sendClipboard = async () => {
    try {
      const text = await navigator.clipboard.readText();
      if (
        text &&
        wsRef.current &&
        wsRef.current.readyState === WebSocket.OPEN
      ) {
        wsRef.current.send(
          JSON.stringify({
            type: "clipboard",
            content: text,
          }),
        );
        onStatusMessage?.("剪贴板已同步");
      }
    } catch {
      onStatusMessage?.("无法读取剪贴板");
    }
  };

  const handleConnect = async (device: PairedDeviceInfo) => {
    setSelectedDevice(device);
    setError(null);
    setScreenImage(null);
    setIsConnected(false);
    setIsLocalMode(false);

    onStatusMessage?.(`正在连接 ${device.nickname || device.name}...`);

    try {
      const targetIp = device.name.includes(".")
        ? device.name
        : await lookupDeviceIp(device.id, discoveredDevices);
      const targetPort = 51111;

      // 连接 WebSocket 用于输入事件
      const wsUrl = `wss://${targetIp}:${targetPort}/ws`;
      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onopen = () => {
        ws.send(JSON.stringify({ type: "register", device_id: localDeviceId }));
        setIsConnected(true);
        onStatusMessage?.(`已连接到 ${device.nickname || device.name}`);
      };

      ws.onerror = () => {
        setError("WebSocket 连接失败");
        onStatusMessage?.("远程控制连接失败");
      };

      ws.onclose = () => {
        setIsConnected(false);
        wsRef.current = null;
      };

      // 启动 MJPEG 流
      await api.startRemoteControl(targetIp, targetPort);
    } catch (err) {
      setError(`连接失败: ${err}`);
      onStatusMessage?.("远程控制连接失败");
    }
  };

  const handleDisconnect = async () => {
    try {
      await api.stopRemoteControl();
    } catch {
      // 停止连接时忽略错误
    }
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    setIsConnected(false);
    setScreenImage(null);
    setSelectedDevice(null);
    setIsLocalMode(false);
  };

  const handleTestCapture = async () => {
    try {
      const b64 = await api.testScreenCapture();
      setScreenImage(`data:image/jpeg;base64,${b64}`);
      setIsConnected(true);
      setIsLocalMode(true);
      setSelectedDevice({
        id: localDeviceId,
        name: "本机",
        nickname: null,
        paired_at: "",
      });
      onStatusMessage?.("本地屏幕捕获成功");
    } catch (err) {
      setError(`屏幕捕获失败: ${err}`);
    }
  };

  const handleStartRecording = async () => {
    try {
      const path = await api.startRecording();
      setIsRecording(true);
      setRecordingDuration(0);
      onStatusMessage?.(`录制开始: ${path}`);
      // 更新录制时长
      const interval = setInterval(async () => {
        try {
          const status = await api.getRecordingStatus();
          setRecordingDuration(Math.round(status.duration));
          if (!status.is_recording) {
            clearInterval(interval);
            setIsRecording(false);
          }
        } catch {
          clearInterval(interval);
        }
      }, 1000);
    } catch (err) {
      setError(`录制失败: ${err}`);
    }
  };

  const handleStopRecording = async () => {
    try {
      const path = await api.stopRecording();
      setIsRecording(false);
      onStatusMessage?.(`录制保存至: ${path}`);
    } catch (err) {
      setError(`停止录制失败: ${err}`);
    }
  };

  // 全屏切换
  const handleFullscreen = async () => {
    try {
      if (isFullscreen) {
        await document.exitFullscreen();
        setIsFullscreen(false);
      } else {
        const viewport = document.querySelector(".remote-screen-viewport");
        if (viewport) {
          await viewport.requestFullscreen();
          setIsFullscreen(true);
        }
      }
    } catch (err) {
      console.error("全屏切换失败:", err);
    }
  };

  // 全屏状态监听
  useEffect(() => {
    const handleFullscreenChange = () => {
      setIsFullscreen(document.fullscreenElement !== null);
    };
    document.addEventListener("fullscreenchange", handleFullscreenChange);
    return () => {
      document.removeEventListener("fullscreenchange", handleFullscreenChange);
    };
  }, []);

  // Filter out self and only show online devices
  const remoteDevices = trustedDeviceList.filter(
    (d) => d.id !== localDeviceId && onlineDeviceIds.includes(d.id),
  );

  // Show local option when no remote devices available
  const showLocalOption = remoteDevices.length === 0 && localDeviceId;

  return (
    <div className="remote-page">
      <div className="remote-header">
        <h3>远程控制</h3>
        <button
          className="btn btn-sm"
          onClick={handleTestCapture}
          title="测试屏幕捕获"
        >
          测试截图
        </button>
      </div>

      <div className="remote-body">
        {/* 权限引导面板 */}
        {permissions && !permissions.all_granted && !isConnected && (
          <div className="permission-warning">
            <p className="permission-warning-title">⚠️ 需要授权权限</p>
            {!permissions.screen_recording && (
              <div className="permission-item">
                <span>屏幕录制权限未开启</span>
                <button
                  className="btn btn-xs"
                  onClick={() => api.openScreenRecordingSettings()}
                >
                  打开设置
                </button>
              </div>
            )}
            {!permissions.accessibility && (
              <div className="permission-item">
                <span>辅助功能权限未开启</span>
                <button
                  className="btn btn-xs"
                  onClick={() => api.openAccessibilitySettings()}
                >
                  打开设置
                </button>
              </div>
            )}
            <p className="permission-hint">授权后重启应用生效</p>
          </div>
        )}

        {/* Device List */}
        {!isConnected && (
          <div className="remote-device-list">
            {remoteDevices.length === 0 && !showLocalOption && (
              <div className="remote-empty">
                <p>没有在线设备</p>
                <p className="hint">请确保其他设备已配对且在线</p>
              </div>
            )}

            {remoteDevices.map((device) => (
              <div
                key={device.id}
                className="remote-device-item"
                onClick={() => handleConnect(device)}
              >
                <div
                  className="remote-device-avatar"
                  style={{
                    backgroundColor: getAvatarColor(
                      device.nickname || device.name,
                    ),
                  }}
                >
                  {getInitial(device.nickname || device.name)}
                </div>
                <div className="remote-device-info">
                  <div className="remote-device-name">
                    {device.nickname || device.name}
                  </div>
                  <div className="remote-device-status">
                    在线 · 点击远程控制
                  </div>
                </div>
              </div>
            ))}

            {/* Local device option for single-machine testing */}
            {showLocalOption && (
              <div className="remote-device-item" onClick={handleTestCapture}>
                <div
                  className="remote-device-avatar"
                  style={{ backgroundColor: getAvatarColor("本机") }}
                >
                  本
                </div>
                <div className="remote-device-info">
                  <div className="remote-device-name">本机屏幕</div>
                  <div className="remote-device-status">
                    本地 · 点击查看屏幕
                  </div>
                </div>
              </div>
            )}
          </div>
        )}

        {/* Screen View */}
        {isConnected && (
          <div className="remote-screen-area">
            <div className="remote-screen-toolbar">
              <span className="remote-screen-title">
                {selectedDevice?.nickname || selectedDevice?.name || "远程屏幕"}
              </span>
              <span className="remote-screen-stats">
                {frameCount} 帧 · {fps} FPS
              </span>
              {monitors.length > 1 && (
                <select
                  className="monitor-selector"
                  value={selectedMonitor}
                  onChange={(e) => setSelectedMonitor(Number(e.target.value))}
                >
                  {monitors.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name} ({m.width}×{m.height})
                    </option>
                  ))}
                </select>
              )}
              {!isLocalMode && (
                <div className="remote-shortcuts">
                  <button
                    className="btn btn-xs"
                    onClick={() => sendShortcut("ctrl,a")}
                    title="全选"
                  >
                    全选
                  </button>
                  <button
                    className="btn btn-xs"
                    onClick={() => sendShortcut("ctrl,c")}
                    title="复制"
                  >
                    复制
                  </button>
                  <button
                    className="btn btn-xs"
                    onClick={() => sendShortcut("ctrl,v")}
                    title="粘贴"
                  >
                    粘贴
                  </button>
                  <button
                    className="btn btn-xs"
                    onClick={() => sendShortcut("ctrl,z")}
                    title="撤销"
                  >
                    撤销
                  </button>
                  <button
                    className="btn btn-xs"
                    onClick={() => sendShortcut("ctrl,s")}
                    title="保存"
                  >
                    保存
                  </button>
                  <button
                    className="btn btn-xs"
                    onClick={() => sendShortcut("ctrl,f")}
                    title="查找"
                  >
                    查找
                  </button>
                  <button className="btn btn-xs" onClick={sendClipboard}>
                    剪贴板
                  </button>
                </div>
              )}
              <button
                className="btn btn-xs"
                onClick={handleFullscreen}
                title="全屏"
              >
                {isFullscreen ? "退出全屏" : "全屏"}
              </button>
              <button
                className={`btn btn-xs ${isRecording ? "btn-recording" : ""}`}
                onClick={
                  isRecording ? handleStopRecording : handleStartRecording
                }
              >
                {isRecording ? `录制 ${recordingDuration}s` : "录制"}
              </button>
              <button
                className="btn btn-sm btn-danger"
                onClick={handleDisconnect}
              >
                断开
              </button>
            </div>

            <div className="remote-screen-viewport">
              {screenImage ? (
                <img
                  ref={imgRef}
                  src={screenImage}
                  alt="远程屏幕"
                  className="remote-screen-img"
                  onMouseMove={isLocalMode ? undefined : handleMouseMove}
                  onMouseDown={isLocalMode ? undefined : handleMouseDown}
                  onMouseUp={isLocalMode ? undefined : handleMouseUp}
                  onClick={isLocalMode ? undefined : handleClick}
                  onWheel={isLocalMode ? undefined : handleWheel}
                  onKeyDown={isLocalMode ? undefined : handleKeyDown}
                  onKeyUp={isLocalMode ? undefined : handleKeyUp}
                  onTouchStart={isLocalMode ? undefined : handleTouchStart}
                  onTouchMove={isLocalMode ? undefined : handleTouchMove}
                  onTouchEnd={isLocalMode ? undefined : handleTouchEnd}
                  tabIndex={isLocalMode ? -1 : 0}
                  onContextMenu={(e) => e.preventDefault()}
                />
              ) : (
                <div className="remote-screen-loading">
                  <div className="spinner" />
                  <p>连接中...</p>
                </div>
              )}
            </div>

            {isLocalMode && (
              <div className="remote-mode-hint">
                本地测试模式：输入控制已禁用
              </div>
            )}
          </div>
        )}

        {/* Error */}
        {error && (
          <div className="remote-error">
            <span>⚠️ {typeof error === "string" ? error : "连接失败"}</span>
          </div>
        )}
      </div>
    </div>
  );
}

// Look up device IP from discovery data, fall back to localhost
async function lookupDeviceIp(
  deviceId: string,
  discoveredDevices: DeviceInfo[],
): Promise<string> {
  const found = discoveredDevices.find((d) => d.id === deviceId);
  if (found?.ip) return found.ip;
  return "127.0.0.1";
}
