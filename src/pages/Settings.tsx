import { useState, useEffect, useContext } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { Settings as SettingsType, LogEntry, PairedDeviceInfo } from "../types";
import { getSettings, updateSettings, getTrustedDeviceList, untrustDevice, setDeviceNickname, getPairingCode, rotatePairingCode, getOperationLog, clearOperationLog } from "../api";
import { I18nContext, type Locale } from "../i18n";

interface SettingsProps {
  onStatusMessage: (msg: string) => void;
  theme: "dark" | "light";
  onToggleTheme: () => void;
}

export default function Settings({ onStatusMessage, theme, onToggleTheme }: SettingsProps) {
  const { t, locale, setLocale } = useContext(I18nContext);
  const [settings, setSettings] = useState<SettingsType | null>(null);
  const [loading, setLoading] = useState(true);
  const [trustedDeviceList, setTrustedDeviceList] = useState<PairedDeviceInfo[]>([]);
  const [editingNickDeviceId, setEditingNickDeviceId] = useState<string | null>(null);
  const [editingNickValue, setEditingNickValue] = useState("");
  const [pairingCode, setPairingCode] = useState("");
  const [rotatingCode, setRotatingCode] = useState(false);
  const [logEntries, setLogEntries] = useState<LogEntry[]>([]);
  const [showLog, setShowLog] = useState(false);

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch((err) => onStatusMessage(`加载设置失败: ${err}`))
      .finally(() => setLoading(false));
    getTrustedDeviceList()
      .then(setTrustedDeviceList)
      .catch(() => {});
    getPairingCode()
      .then(setPairingCode)
      .catch(() => {});
  }, [onStatusMessage]);

  const handleSelectDir = async (field: "share_dir" | "download_dir") => {
    try {
      const selected = await open({ directory: true, title: "选择目录" });
      if (selected) {
        setSettings({ ...settings!, [field]: selected as string });
      }
    } catch (err) {
      onStatusMessage(`选择目录失败: ${err}`);
    }
  };

  const handleLoadLog = async () => {
    try {
      const entries = await getOperationLog();
      setLogEntries(entries);
      setShowLog(!showLog);
    } catch (err) {
      onStatusMessage(`加载操作日志失败: ${err}`);
    }
  };

  const handleClearLog = async () => {
    try {
      await clearOperationLog();
      setLogEntries([]);
      onStatusMessage("操作日志已清除");
    } catch (err) {
      onStatusMessage(`清除操作日志失败: ${err}`);
    }
  };

  const handleRotateCode = async () => {
    setRotatingCode(true);
    try {
      const newCode = await rotatePairingCode();
      setPairingCode(newCode);
      onStatusMessage("配对码已更新");
    } catch (err) {
      onStatusMessage(`更新配对码失败: ${err}`);
    } finally {
      setRotatingCode(false);
    }
  };

  const handleSave = async () => {
    if (!settings) return;
    try {
      await updateSettings(settings);
      onStatusMessage("设置已保存");
    } catch (err) {
      onStatusMessage(`保存设置失败: ${err}`);
    }
  };

  if (loading) {
    return (
      <div className="loading">
        <div className="spinner" />
        <span>加载中...</span>
      </div>
    );
  }

  if (!settings) {
    return <div className="empty-state"><p>加载设置失败</p></div>;
  }

  return (
    <div className="settings-page">
      <div className="page-header">
        <h2>设置</h2>
        <button className="btn btn-primary" onClick={handleSave}>
          保存
        </button>
      </div>

      <div className="settings-form">
        <div className="form-group">
          <label>语言 / Language</label>
          <select
            value={locale}
            onChange={(e) => setLocale(e.target.value as Locale)}
            style={{
              padding: "8px 10px",
              background: "var(--bg-card)",
              border: "1px solid var(--border)",
              color: "var(--text-primary)",
              borderRadius: "var(--radius)",
              fontSize: "13px",
            }}
          >
            <option value="zh-CN">简体中文</option>
            <option value="en-US">English</option>
          </select>
        </div>

        <div className="form-group">
          <label>主题</label>
          <button className="btn btn-sm" onClick={onToggleTheme} style={{ width: "fit-content" }}>
            {theme === "dark" ? "☀️ 切换亮色主题" : "🌙 切换暗色主题"}
          </button>
        </div>

        <div className="form-group">
          <label>{t("settings.device_name")}</label>
          <input
            type="text"
            value={settings.device_name}
            onChange={(e) =>
              setSettings({ ...settings, device_name: e.target.value })
            }
          />
          <p className="hint">{t("settings.device_name_hint")}</p>
        </div>

        <div className="form-group">
          <label>服务端口</label>
          <input
            type="number"
            min={0}
            max={65535}
            value={settings.port}
            onChange={(e) =>
              setSettings({ ...settings, port: parseInt(e.target.value) || 0 })
            }
          />
          <p className="hint">局域网文件服务端口（0 = 自动分配，修改需重启应用生效）</p>
        </div>

        <div className="form-group">
          <label>{t("settings.share_dir")}</label>
          <div className="dir-picker">
            <input
              type="text"
              value={settings.share_dir}
              onChange={(e) =>
                setSettings({ ...settings, share_dir: e.target.value })
              }
            />
            <button className="btn btn-sm" onClick={() => handleSelectDir("share_dir")}>
              浏览...
            </button>
          </div>
          <p className="hint">{t("settings.share_dir_hint")}</p>
        </div>

        <div className="form-group">
          <label>{t("settings.download_dir")}</label>
          <div className="dir-picker">
            <input
              type="text"
              value={settings.download_dir}
              onChange={(e) =>
                setSettings({ ...settings, download_dir: e.target.value })
              }
            />
            <button className="btn btn-sm" onClick={() => handleSelectDir("download_dir")}>
              浏览...
            </button>
          </div>
          <p className="hint">{t("settings.download_dir_hint")}</p>
        </div>

        <div className="form-group">
          <label>{t("settings.max_concurrent")}</label>
          <input
            type="number"
            min={1}
            max={10}
            value={settings.max_concurrent_transfers}
            onChange={(e) =>
              setSettings({
                ...settings,
                max_concurrent_transfers: parseInt(e.target.value) || 3,
              })
            }
          />
        </div>

        <div className="form-group">
          <label>{t("settings.speed_limit")}</label>
          <input
            type="number"
            min={0}
            value={settings.speed_limit}
            onChange={(e) =>
              setSettings({
                ...settings,
                speed_limit: parseInt(e.target.value) || 0,
              })
            }
          />
        </div>

        <div className="form-group">
          <label>传输失败重试次数</label>
          <input
            type="number"
            min={0}
            max={10}
            value={settings.max_retries}
            onChange={(e) =>
              setSettings({
                ...settings,
                max_retries: parseInt(e.target.value) || 3,
              })
            }
          />
          <p className="hint">传输失败后自动重试次数（0=不重试）</p>
        </div>

        <div className="form-group">
          <label>
            <input
              type="checkbox"
              checked={settings.require_pairing}
              onChange={(e) =>
                setSettings({
                  ...settings,
                  require_pairing: e.target.checked,
                })
              }
            />
            {t("settings.require_pairing")}
          </label>
          <p className="hint">{t("settings.require_pairing_hint")}</p>
        </div>

        <div className="form-group">
          <label>配对码</label>
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <code
              style={{
                fontSize: 24,
                letterSpacing: 6,
                fontFamily: "monospace",
                color: "var(--accent)",
                padding: "4px 12px",
                background: "var(--bg-primary)",
                borderRadius: "var(--radius)",
              }}
            >
              {pairingCode || "----"}
            </code>
            <button
              className="btn btn-sm"
              onClick={handleRotateCode}
              disabled={rotatingCode}
            >
              {rotatingCode ? "更新中..." : "刷新"}
            </button>
          </div>
          <p className="hint">将此配对码告诉要连接的设备</p>
        </div>

        <div className="form-group">
          <label>信任设备 ({trustedDeviceList.length})</label>
          {trustedDeviceList.length === 0 ? (
            <p className="hint">暂无信任设备</p>
          ) : (
            <ul className="trusted-device-list">
              {trustedDeviceList.map((info) => {
                const displayName = info.nickname || info.name;
                const isEditing = editingNickDeviceId === info.id;
                return (
                  <li key={info.id} className="trusted-device-item">
                    {isEditing ? (
                      <div style={{ display: "flex", gap: 4, flex: 1, alignItems: "center" }}>
                        <input
                          type="text"
                          className="friend-nick-input"
                          value={editingNickValue}
                          onChange={(e) => setEditingNickValue(e.target.value)}
                          onKeyDown={async (e) => {
                            if (e.key === "Enter") {
                              const name = editingNickValue.trim();
                              if (name) {
                                try {
                                  await setDeviceNickname(info.id, name);
                                  setTrustedDeviceList(prev =>
                                    prev.map(d => d.id === info.id ? { ...d, nickname: name } : d)
                                  );
                                  onStatusMessage("昵称已更新");
                                } catch (err) {
                                  onStatusMessage(`设置昵称失败: ${err}`);
                                }
                              }
                              setEditingNickDeviceId(null);
                            }
                            if (e.key === "Escape") setEditingNickDeviceId(null);
                          }}
                          onBlur={() => setEditingNickDeviceId(null)}
                          autoFocus
                        />
                      </div>
                    ) : (
                      <div style={{ flex: 1, display: "flex", alignItems: "center", gap: 8 }}>
                        <span style={{ fontWeight: 500 }}>{displayName}</span>
                        <code className="device-id" style={{ fontSize: 11, opacity: 0.6 }}>{info.id.substring(0, 8)}...</code>
                        <button
                          className="btn btn-xs"
                          onClick={() => {
                            setEditingNickDeviceId(info.id);
                            setEditingNickValue(info.nickname || info.name);
                          }}
                          title="编辑备注"
                          style={{ fontSize: 12, padding: "0 4px" }}
                        >
                          ✏️
                        </button>
                      </div>
                    )}
                    <button
                      className="btn btn-xs btn-danger"
                      onClick={async () => {
                        await untrustDevice(info.id);
                        setTrustedDeviceList((prev) => prev.filter((d) => d.id !== info.id));
                        onStatusMessage(`已移除信任设备`);
                      }}
                    >
                      取消信任
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>

        <div className="form-group">
          <label>操作日志</label>
          <button className="btn btn-sm" onClick={handleLoadLog} style={{ width: "fit-content" }}>
            {showLog ? "隐藏日志" : "查看日志"}
          </button>
          {' '}
          {showLog && logEntries.length > 0 && (
            <button className="btn btn-xs btn-danger" onClick={handleClearLog} style={{ marginLeft: 8 }}>
              清除
            </button>
          )}
          {showLog && (
            <div className="operation-log">
              {logEntries.length === 0 ? (
                <p className="hint">暂无操作记录</p>
              ) : (
                <table className="log-table">
                  <thead>
                    <tr>
                      <th>时间</th>
                      <th>操作</th>
                      <th>详情</th>
                      <th>结果</th>
                    </tr>
                  </thead>
                  <tbody>
                    {logEntries.map((entry, i) => (
                      <tr key={i}>
                        <td className="log-time">{new Date(entry.timestamp).toLocaleString()}</td>
                        <td>{entry.operation}</td>
                        <td className="log-detail">{entry.detail}</td>
                        <td className={`log-result log-${entry.result}`}>{entry.result}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
