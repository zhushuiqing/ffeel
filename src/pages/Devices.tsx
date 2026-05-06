import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { DeviceInfo, PairedDeviceInfo } from "../types";
import { getSettings, updateSettings, getPairingCode, getLocalDeviceInfo } from "../api";

interface DevicesProps {
  devices: DeviceInfo[];
  localDevice: DeviceInfo | null;
  isScanning: boolean;
  shareDir: string;
  trustedDevices: string[];
  trustedDeviceList?: PairedDeviceInfo[];
  onSelectDevice: (device: DeviceInfo) => void;
  onStartScanning: () => void;
  onUntrustDevice: (id: string) => void;
  onStatusMessage?: (msg: string) => void;
}

function getPlatformIcon(platform: string): string {
  switch (platform) {
    case "macos":
      return "🍎";
    case "windows":
      return "🪟";
    case "linux":
      return "🐧";
    default:
      return "💻";
  }
}

export default function Devices({
  devices,
  localDevice,
  isScanning,
  shareDir,
  trustedDevices,
  trustedDeviceList,
  onSelectDevice,
  onStartScanning,
  onUntrustDevice,
  onStatusMessage,
}: DevicesProps) {
  const [pairingCode, setPairingCode] = useState("");

  // 加载配对码
  useEffect(() => {
    getPairingCode().then(setPairingCode).catch(() => {});
  }, []);

  // 设置共享目录
  const handleChangeShareDir = async () => {
    try {
      const selected = await open({ directory: true, title: "选择共享目录" });
      if (selected) {
        const settings = await getSettings();
        settings.share_dir = selected as string;
        await updateSettings(settings);
        onStatusMessage?.("共享目录已更新");
        window.location.reload(); // 刷新页面以更新 shareDir
      }
    } catch (err) {
      onStatusMessage?.(`选择目录失败: ${err}`);
    }
  };

  // 复制配对链接
  const handleCopyPairLink = async () => {
    try {
      const info = await getLocalDeviceInfo();
      const code = await getPairingCode();
      const link = `https://${info.ip}:${info.port}/?pair=${code}`;
      await navigator.clipboard.writeText(link);
      onStatusMessage?.("配对链接已复制到剪贴板");
    } catch (err) {
      onStatusMessage?.(`复制失败: ${err}`);
    }
  };

  const allDevices = localDevice
    ? [localDevice, ...devices.filter((d) => d.id !== localDevice.id)]
    : devices;

  const initialScanDone = useRef(false);

  useEffect(() => {
    if (initialScanDone.current) return;
    if (!isScanning && allDevices.length <= 1) {
      initialScanDone.current = true;
      const timer = setTimeout(onStartScanning, 500);
      return () => clearTimeout(timer);
    }
  }, [isScanning, onStartScanning]);

  const handleBrowseLocal = () => {
    if (!localDevice) return;
    onSelectDevice({
      ...localDevice,
      ip: "127.0.0.1",
      name: `${localDevice.name} (本机共享)`,
    });
  };

  return (
    <div className="devices-page">
      {/* 局域网 ffeel 设备列表 */}
      <div className="page-header">
        <h2>局域网设备</h2>
        <button
          className="btn btn-primary"
          onClick={onStartScanning}
          disabled={isScanning}
        >
          {isScanning ? "扫描中..." : "刷新"}
        </button>
      </div>

      {isScanning && allDevices.length <= 1 && (
        <div className="scanning-indicator">
          <div className="spinner" />
          <span>正在扫描局域网中的 ffeel 设备...</span>
        </div>
      )}

      <div className="device-grid">
        {allDevices.map((device) => {
          const isLocal = device.id === localDevice?.id;
          return (
            <div
              key={device.id}
              className={`device-card${isLocal ? " device-card-local" : ""}`}
              onClick={() => {
                if (isLocal) {
                  handleBrowseLocal();
                } else {
                  onSelectDevice(device);
                }
              }}
              role="button"
              tabIndex={0}
              onKeyDown={(e) =>
                e.key === "Enter" && (isLocal ? handleBrowseLocal() : onSelectDevice(device))
              }
            >
              <div className="device-icon">
                {getPlatformIcon(device.platform)}
                <span
                  className={`status-dot ${device.online ? "online" : "offline"}`}
                />
              </div>
              <div className="device-name">
                {isLocal ? `${device.name} (本机)` : device.name}
              </div>
              <div className="device-ip">{device.ip}:{device.port}</div>
              <div className="device-platform">{device.platform}</div>
              {isLocal && (
                <div className="local-device-actions" onClick={(e) => e.stopPropagation()}>
                  <button className="btn btn-primary btn-xs" onClick={handleBrowseLocal}>
                    浏览文件
                  </button>
                  <button className="btn btn-xs" onClick={handleChangeShareDir}>
                    设置共享目录
                  </button>
                  {pairingCode && (
                    <button className="btn btn-xs" onClick={handleCopyPairLink}>
                      复制配对链接
                    </button>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>

      {!isScanning && allDevices.length <= 1 && (
        <div className="empty-state">
          <p>未发现其他 ffeel 桌面设备</p>
          <p className="hint">
            其他设备运行 ffeel 后才出现在列表中<br/>
            浏览器客户端直接通过 HTTPS 地址访问本机
          </p>
        </div>
      )}

      {/* 共享目录路径信息 */}
      <div className="share-dir-info">
        <span className="share-dir-label">共享目录: </span>
        <code className="share-dir-path">{shareDir || "加载中..."}</code>
      </div>

      {/* 已配对的客户端 */}
      {trustedDevices.length > 0 && (
        <section className="trusted-clients-section" style={{ marginTop: 20 }}>
          <h3>已配对的设备 ({trustedDevices.length})</h3>
          <div className="trusted-clients">
            {(trustedDeviceList || []).map((info) => (
              <div key={info.id} className="trusted-client-item">
                <span className="client-icon">🌐</span>
                <div className="client-info">
                  <div className="client-name">{info.nickname || info.name}</div>
                  <code className="client-id">{info.id.substring(0, 12)}...</code>
                </div>
                <button
                  className="btn btn-xs btn-danger"
                  onClick={() => onUntrustDevice(info.id)}
                >
                  取消信任
                </button>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
