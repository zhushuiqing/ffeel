import { useState, useEffect } from "react";
import { pairDevice, getLocalDeviceInfo } from "../api";

interface PairingModalProps {
  deviceIp: string;
  port: number;
  onClose: () => void;
  onSuccess: () => void;
}

export default function PairingModal({
  deviceIp,
  port,
  onClose,
  onSuccess,
}: PairingModalProps) {
  const [code, setCode] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [localDeviceId, setLocalDeviceId] = useState("");

  useEffect(() => {
    getLocalDeviceInfo().then((info) => setLocalDeviceId(info.id)).catch(() => {});
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (code.length !== 4) {
      setError("配对码为 4 位数字");
      return;
    }
    setLoading(true);
    setError("");
    try {
      await pairDevice(deviceIp, port, localDeviceId, code);
      onSuccess();
    } catch (err) {
      setError(`配对失败: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-content" onClick={(e) => e.stopPropagation()}>
        <h3>设备配对</h3>
        <p className="hint">
          请在目标设备上查看配对码（设置 → 信任设备），然后输入到此处
        </p>
        <form onSubmit={handleSubmit}>
          <input
            className="pairing-input"
            type="text"
            maxLength={4}
            placeholder="输入 4 位配对码"
            value={code}
            onChange={(e) => {
              setCode(e.target.value.replace(/\D/g, "").slice(0, 4));
              setError("");
            }}
            autoFocus
          />
          {error && <p className="pairing-error">{error}</p>}
          <div className="modal-actions">
            <button
              type="button"
              className="btn"
              onClick={onClose}
              disabled={loading}
            >
              取消
            </button>
            <button
              type="submit"
              className="btn btn-primary"
              disabled={loading || code.length !== 4}
            >
              {loading ? "配对中..." : "确认配对"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
