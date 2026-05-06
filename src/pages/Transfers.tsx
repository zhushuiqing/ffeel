import { useEffect, useState } from "react";
import type { TransferTask, TransferStats as TransferStatsType } from "../types";
import { getTransferStats } from "../api";

interface TransfersProps {
  transfers: TransferTask[];
  onRefresh: () => void;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
}

function formatSpeed(speed: number): string {
  if (speed === 0) return "-";
  if (speed < 1024) return `${speed.toFixed(0)} B/s`;
  if (speed < 1024 * 1024) return `${(speed / 1024).toFixed(1)} KB/s`;
  return `${(speed / (1024 * 1024)).toFixed(1)} MB/s`;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function statusLabel(status: string): { text: string; cls: string } {
  switch (status) {
    case "Pending":
      return { text: "等待中", cls: "status-pending" };
    case "Transferring":
      return { text: "传输中", cls: "status-transferring" };
    case "Paused":
      return { text: "已暂停", cls: "status-paused" };
    case "Completed":
      return { text: "已完成", cls: "status-completed" };
    case "Failed":
      return { text: "失败", cls: "status-failed" };
    case "Cancelled":
      return { text: "已取消", cls: "status-cancelled" };
    default:
      return { text: status, cls: "" };
  }
}

export default function Transfers({
  transfers,
  onRefresh,
  onPause,
  onResume,
  onCancel,
}: TransfersProps) {
  const [stats, setStats] = useState<TransferStatsType | null>(null);

  useEffect(() => {
    getTransferStats().then(setStats).catch(() => {});
  }, [transfers]);

  const activeTransfers = transfers.filter(
    (t) => t.status === "Transferring" || t.status === "Pending"
  );
  const completedTransfers = transfers.filter(
    (t) =>
      t.status === "Completed" ||
      t.status === "Failed" ||
      t.status === "Cancelled"
  );
  const pausedTransfers = transfers.filter((t) => t.status === "Paused");

  return (
    <div className="transfers-page">
      <div className="page-header">
        <h2>传输管理</h2>
        <button className="btn btn-sm" onClick={onRefresh}>
          刷新
        </button>
      </div>

      {stats && (
        <div className="transfer-stats">
          <span>已完成: {stats.total_completed}</span>
          <span>传输量: {formatFileSize(stats.total_bytes)}</span>
          <span>进行中: {stats.active_count}</span>
          <span>失败: {stats.total_failed}</span>
          <span>已取消: {stats.total_cancelled}</span>
        </div>
      )}

      {activeTransfers.length > 0 && (
        <section>
          <h3>正在传输 ({activeTransfers.length})</h3>
          <div className="transfer-list">
            {activeTransfers.map((t) => (
              <TransferItem
                key={t.id}
                task={t}
                onPause={onPause}
                onResume={onResume}
                onCancel={onCancel}
              />
            ))}
          </div>
        </section>
      )}

      {pausedTransfers.length > 0 && (
        <section>
          <h3>已暂停 ({pausedTransfers.length})</h3>
          <div className="transfer-list">
            {pausedTransfers.map((t) => (
              <TransferItem
                key={t.id}
                task={t}
                onPause={onPause}
                onResume={onResume}
                onCancel={onCancel}
              />
            ))}
          </div>
        </section>
      )}

      {completedTransfers.length > 0 && (
        <section>
          <h3>历史记录 ({completedTransfers.length})</h3>
          <div className="transfer-list">
            {completedTransfers.map((t) => (
              <TransferItem
                key={t.id}
                task={t}
                onPause={onPause}
                onResume={onResume}
                onCancel={onCancel}
              />
            ))}
          </div>
        </section>
      )}

      {transfers.length === 0 && (
        <div className="empty-state">
          <p>暂无传输任务</p>
          <p className="hint">在文件浏览器中选择文件下载即可开始传输</p>
        </div>
      )}
    </div>
  );
}

function TransferItem({
  task,
  onPause,
  onResume,
  onCancel,
}: {
  task: TransferTask;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
}) {
  const progress =
    task.file_size > 0
      ? Math.round((task.bytes_transferred / task.file_size) * 100)
      : 0;

  const { text: statusText, cls } = statusLabel(task.status);
  const dirIcon = task.direction === "Download" ? "⬇" : "⬆";

  return (
    <div className={`transfer-item ${cls}`}>
      <div className="transfer-info">
        <span className="transfer-icon">{dirIcon}</span>
        <span className="transfer-name">{task.file_name}</span>
        <span className="transfer-status">{statusText}</span>
        {task.error && <span className="transfer-error">{task.error}</span>}
        {task.retry_count > 0 && task.status !== "Failed" && (
          <span className="transfer-retry">重试 {task.retry_count}/{task.max_retries}</span>
        )}
      </div>

      <div className="transfer-progress">
        <div className="progress-bar">
          <div
            className="progress-fill"
            style={{ width: `${Math.min(progress, 100)}%` }}
          />
        </div>
        <span className="progress-text">
          {formatFileSize(task.bytes_transferred)} /{" "}
          {formatFileSize(task.file_size)} ({progress}%)
        </span>
        {task.status === "Transferring" && (
          <span className="speed">{formatSpeed(task.speed)}</span>
        )}
      </div>

      <div className="transfer-actions">
        {task.status === "Transferring" && (
          <button className="btn btn-xs" onClick={() => onPause(task.id)}>
            暂停
          </button>
        )}
        {task.status === "Paused" && (
          <button className="btn btn-xs btn-primary" onClick={() => onResume(task.id)}>
            继续
          </button>
        )}
        {(task.status === "Pending" ||
          task.status === "Transferring" ||
          task.status === "Paused") && (
          <button className="btn btn-xs btn-danger" onClick={() => onCancel(task.id)}>
            取消
          </button>
        )}
      </div>
    </div>
  );
}
