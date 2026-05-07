import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { DeviceInfo, PairedDeviceInfo } from "./types";
import { useAppStore } from "./stores/appStore";
import { I18nContext, useI18nManager } from "./i18n";
import { notify } from "./notification";
import { useTheme } from "./theme";
import * as api from "./api";
import Devices from "./pages/Devices";
import FileBrowser from "./pages/FileBrowser";
import Friends from "./pages/Friends";
import RemoteControl from "./pages/RemoteControl";
import Transfers from "./pages/Transfers";
import Settings from "./pages/Settings";
import "./App.css";

type Tab = "devices" | "friends" | "transfers" | "remote" | "settings";

const tabs: { key: Tab; icon: string; labelKey: string }[] = [
  { key: "devices", icon: "📡", labelKey: "tab.devices" },
  { key: "friends", icon: "👥", labelKey: "好友" },
  { key: "remote", icon: "🖥️", labelKey: "远程" },
  { key: "transfers", icon: "⬇️", labelKey: "tab.transfers" },
  { key: "settings", icon: "⚙️", labelKey: "tab.settings" },
];

function App() {
  const i18n = useI18nManager();
  const { t } = i18n;
  const { theme, toggleTheme } = useTheme();
  const {
    store,
    init,
    startScanning,
    selectDevice,
    refreshTransfers,
    cancelTransfer,
    pauseTransfer,
    setStatusMessage,
    updateTransferProgress,
    updateTransferComplete,
    updateTransferError,
  } = useAppStore();

  const [activeTab, setActiveTab] = useState<Tab>("devices");
  const [trustedDevices, setTrustedDevices] = useState<string[]>([]);
  const [trustedDeviceList, setTrustedDeviceList] = useState<
    PairedDeviceInfo[]
  >([]);
  const [onlineDeviceIds, setOnlineDeviceIds] = useState<string[]>([]);
  const [chatMessages, setChatMessages] = useState<
    import("./types").ChatEntry[]
  >([]);
  const [unreadCount, setUnreadCount] = useState(0);

  useEffect(() => {
    init();
  }, [init]);

  // 加载信任设备列表
  useEffect(() => {
    if (store.settings) {
      api
        .getTrustedDevices()
        .then(setTrustedDevices)
        .catch(() => {});
      api
        .getTrustedDeviceList()
        .then(setTrustedDeviceList)
        .catch(() => {});
    }
  }, [store.settings]);

  const refreshTrustedDeviceList = () => {
    api
      .getTrustedDevices()
      .then(setTrustedDevices)
      .catch(() => {});
    api
      .getTrustedDeviceList()
      .then(setTrustedDeviceList)
      .catch(() => {});
  };

  const handleUntrustDevice = async (id: string) => {
    await api.untrustDevice(id);
    refreshTrustedDeviceList();
    setStatusMessage("已移除信任设备");
  };

  const handleResume = async (id: string) => {
    const task = store.transfers.find((t) => t.id === id);
    if (task?.direction === "Download") {
      await api.resumeDownload(id);
    } else {
      await api.resumeTransfer(id);
    }
    await refreshTransfers();
  };

  // 监听 Tauri 事件
  useEffect(() => {
    const unlisteners: (() => void)[] = [];

    const setupListeners = async () => {
      const unlisten1 = await listen<DeviceInfo>("device-found", (event) => {
        setStatusMessage(`发现设备: ${event.payload.name}`);
        notify("发现设备", event.payload.name);
        setOnlineDeviceIds((prev) =>
          prev.includes(event.payload.id) ? prev : [...prev, event.payload.id],
        );
      });
      unlisteners.push(unlisten1);

      const unlisten2 = await listen<string>("device-lost", (event) => {
        setStatusMessage(`设备离线: ${event.payload}`);
        setOnlineDeviceIds((prev) => prev.filter((id) => id !== event.payload));
      });
      unlisteners.push(unlisten2);

      const unlisten3 = await listen<{
        id: string;
        bytes_transferred: number;
        speed: number;
      }>("transfer-progress", (event) => {
        updateTransferProgress(
          event.payload.id,
          event.payload.bytes_transferred,
          event.payload.speed,
        );
      });
      unlisteners.push(unlisten3);

      type TransferPayload = { id: string; file_name?: string; error?: string };

      const unlisten4 = await listen<TransferPayload>(
        "transfer-complete",
        (event) => {
          updateTransferComplete(event.payload.id);
          refreshTransfers();
          setStatusMessage("传输完成");
          notify("传输完成", event.payload.file_name || "文件传输已完成");
        },
      );
      unlisteners.push(unlisten4);

      const unlisten5 = await listen<TransferPayload>(
        "transfer-error",
        (event) => {
          updateTransferError(event.payload.id, event.payload.error || "");
          refreshTransfers();
          setStatusMessage("传输出错");
          const fileName = event.payload.file_name || "";
          notify(
            "传输出错",
            fileName
              ? `${fileName}: ${event.payload.error}`
              : event.payload.error,
          );
        },
      );
      unlisteners.push(unlisten5);

      // Listen for real-time chat messages (with dedup)
      const unlisten6 = await listen<import("./types").ChatEntry>(
        "chat-message",
        (event) => {
          setChatMessages((prev) => {
            // Dedup by key fields: timestamp + from_id + message_type
            const exists = prev.some(
              (m) =>
                m.timestamp === event.payload.timestamp &&
                m.from_id === event.payload.from_id &&
                m.text === event.payload.text,
            );
            if (exists) return prev;
            return [...prev, event.payload];
          });
          setUnreadCount((prev) => prev + 1);
        },
      );
      unlisteners.push(unlisten6);

      // Listen for device online status changes
      const unlisten7 = await listen<{ device_id: string; online: boolean }>(
        "device-status",
        (event) => {
          setOnlineDeviceIds((prev) => {
            if (event.payload.online) {
              return prev.includes(event.payload.device_id)
                ? prev
                : [...prev, event.payload.device_id];
            } else {
              return prev.filter((id) => id !== event.payload.device_id);
            }
          });
        },
      );
      unlisteners.push(unlisten7);

      // Load initial state
      try {
        const online = await api.getOnlineDeviceIds();
        setOnlineDeviceIds(online);
      } catch {
        // 忽略初始加载错误
      }
      try {
        const msgs = await api.getChatMessages();
        setChatMessages(msgs);
      } catch {
        // 忽略初始加载错误
      }

      // 定期轮询在线状态（兜底，避免实时推送不可靠）
      const pollTimer = setInterval(async () => {
        try {
          const online = await api.getOnlineDeviceIds();
          setOnlineDeviceIds(online);
        } catch {
          // 轮询错误忽略
        }
      }, 10000);
      unlisteners.push(() => clearInterval(pollTimer));
    };

    setupListeners();
    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, [refreshTransfers, setStatusMessage]);

  const handleTabClick = (tab: Tab) => {
    setActiveTab(tab);
    if (tab === "transfers") {
      refreshTransfers();
    }
    if (tab === "friends") {
      setUnreadCount(0);
    }
  };

  return (
    <I18nContext.Provider value={i18n}>
      <div className="app">
        {/* macOS Titlebar — native window controls rendered by the OS */}
        <div className="app-titlebar" data-tauri-drag-region>
          <span className="titlebar-text">ffeel</span>
        </div>

        <div className="app-body">
          {/* Sidebar Navigation */}
          <aside className="sidebar">
            <nav className="sidebar-nav">
              {tabs.map(({ key, icon, labelKey }) => (
                <button
                  key={key}
                  className={`sidebar-item ${activeTab === key ? "active" : ""}`}
                  onClick={() => handleTabClick(key)}
                >
                  <span className="sidebar-icon">
                    {icon}
                    {key === "friends" && unreadCount > 0 && (
                      <span className="unread-badge">
                        {unreadCount > 99 ? "99+" : unreadCount}
                      </span>
                    )}
                  </span>
                  <span className="sidebar-label">{t(labelKey)}</span>
                </button>
              ))}
            </nav>
          </aside>

          {/* Content Panel */}
          <div className="content-panel">
            {store.statusMessage && (
              <div className="status-bar">
                <span>{store.statusMessage}</span>
              </div>
            )}

            <main className="app-content">
              {activeTab === "devices" && !store.selectedDevice && (
                <Devices
                  devices={store.devices}
                  localDevice={store.localDevice}
                  isScanning={store.isScanning}
                  shareDir={store.settings?.share_dir || ""}
                  trustedDevices={trustedDevices}
                  trustedDeviceList={trustedDeviceList}
                  onSelectDevice={selectDevice}
                  onStartScanning={startScanning}
                  onUntrustDevice={handleUntrustDevice}
                  onStatusMessage={setStatusMessage}
                />
              )}

              {activeTab === "friends" && (
                <Friends
                  trustedDeviceList={trustedDeviceList}
                  onlineDeviceIds={onlineDeviceIds}
                  messages={chatMessages}
                  onMessagesUpdate={setChatMessages}
                  onStatusMessage={setStatusMessage}
                />
              )}

              {activeTab === "remote" && (
                <RemoteControl
                  trustedDeviceList={trustedDeviceList}
                  discoveredDevices={store.devices}
                  onlineDeviceIds={onlineDeviceIds}
                  localDeviceId={store.localDevice?.id || ""}
                  onStatusMessage={setStatusMessage}
                />
              )}

              {activeTab === "settings" && (
                <Settings
                  onStatusMessage={setStatusMessage}
                  theme={theme}
                  onToggleTheme={toggleTheme}
                />
              )}

              {activeTab === "devices" && store.selectedDevice && (
                <div className="browser-container">
                  <button
                    className="btn btn-sm back-btn"
                    onClick={() => selectDevice(null)}
                  >
                    ← 返回设备列表
                  </button>
                  <FileBrowser
                    device={store.selectedDevice}
                    downloadDir={store.settings?.download_dir || ""}
                    onBatchDownload={(count) => {
                      setStatusMessage(`已开始下载 ${count} 个文件`);
                      setActiveTab("transfers");
                      refreshTransfers();
                    }}
                    onStatusMessage={setStatusMessage}
                  />
                </div>
              )}

              {activeTab === "transfers" && (
                <Transfers
                  transfers={store.transfers}
                  onRefresh={refreshTransfers}
                  onPause={pauseTransfer}
                  onResume={handleResume}
                  onCancel={cancelTransfer}
                />
              )}
            </main>

            <footer className="app-footer">
              <span>ffeel v0.1.0</span>
              <span>{t("app.subtitle")}</span>
            </footer>
          </div>
        </div>
      </div>
    </I18nContext.Provider>
  );
}

export default App;
