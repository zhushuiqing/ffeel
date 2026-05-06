/// 与 Rust 后端数据结构对应

export interface TransferStats {
  total_completed: number;
  total_failed: number;
  total_cancelled: number;
  total_bytes: number;
  active_count: number;
}

export interface LogEntry {
  timestamp: string;
  operation: string;
  detail: string;
  result: string;
}

export interface DeviceInfo {
  id: string;
  name: string;
  ip: string;
  port: number;
  platform: string;
  online: boolean;
}

export interface DirEntry {
  name: string;
  type: "file" | "directory";
  size: number | null;
  modified_at: string | null;
  is_dir: boolean;
}

export type TransferStatus =
  | "Pending"
  | "Transferring"
  | "Paused"
  | "Completed"
  | "Failed"
  | "Cancelled";

export type TransferDirection = "Download" | "Upload";

export interface TransferTask {
  id: string;
  file_name: string;
  file_size: number;
  bytes_transferred: number;
  status: TransferStatus;
  direction: TransferDirection;
  remote_device: string;
  remote_path: string;
  local_path: string;
  speed: number;
  error: string | null;
  created_at: string;
  retry_count: number;
  max_retries: number;
}

export interface PairedDeviceInfo {
  id: string;
  name: string;
  nickname: string | null;
  paired_at: string;
}

export interface ChatEntry {
  from_id: string;
  from_name: string;
  text: string;
  timestamp: string;
  message_type: string;
  file_name?: string;
  file_size?: number;
  file_id?: string;
  file_type?: string;
  to_id?: string;
}

export interface Settings {
  share_dir: string;
  port: number;
  device_name: string;
  download_dir: string;
  max_concurrent_transfers: number;
  speed_limit: number;
  require_pairing: boolean;
  trusted_devices: string[];
  max_retries: number;
}
