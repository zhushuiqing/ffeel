import { invoke } from "@tauri-apps/api/core";
import type { DeviceInfo, DirEntry, TransferTask, Settings } from "./types";

export async function startDiscovery(): Promise<void> {
  await invoke("start_discovery");
}

export async function browseDirectory(
  deviceIp: string,
  port: number,
  path: string,
  offset?: number,
  limit?: number,
): Promise<DirEntry[]> {
  return await invoke("browse_directory", {
    deviceIp,
    port,
    path,
    offset,
    limit,
  });
}

export async function searchFiles(
  deviceIp: string,
  port: number,
  path: string,
  query: string,
): Promise<DirEntry[]> {
  return await invoke("search_files", { deviceIp, port, path, query });
}

export async function downloadDirectory(
  deviceIp: string,
  port: number,
  remotePath: string,
  localPath: string,
): Promise<void> {
  await invoke("download_directory", { deviceIp, port, remotePath, localPath });
}

export async function downloadFile(
  deviceIp: string,
  port: number,
  remotePath: string,
  localPath: string,
  fileName: string,
  fileSize: number,
): Promise<void> {
  await invoke("download_file", {
    deviceIp,
    port,
    remotePath,
    localPath,
    fileName,
    fileSize,
  });
}

export async function getTransfers(): Promise<TransferTask[]> {
  return await invoke("get_transfers");
}

export async function cancelTransfer(id: string): Promise<void> {
  await invoke("cancel_transfer", { id });
}

export async function pauseTransfer(id: string): Promise<void> {
  await invoke("pause_transfer", { id });
}

export async function resumeTransfer(id: string): Promise<void> {
  await invoke("resume_transfer", { id });
}

export async function resumeDownload(id: string): Promise<void> {
  await invoke("resume_download", { id });
}

export async function getSettings(): Promise<Settings> {
  return await invoke("get_settings");
}

export async function updateSettings(settings: Settings): Promise<void> {
  await invoke("update_settings", { newSettings: settings });
}

export async function getTrustedDevices(): Promise<string[]> {
  return await invoke("get_trusted_devices");
}

export async function getTrustedDeviceList(): Promise<
  import("./types").PairedDeviceInfo[]
> {
  return await invoke("get_trusted_device_list");
}

export async function trustDevice(deviceId: string): Promise<void> {
  await invoke("trust_device", { deviceId });
}

export async function untrustDevice(deviceId: string): Promise<void> {
  await invoke("untrust_device", { deviceId });
}

export async function getPairingCode(): Promise<string> {
  return await invoke("get_pairing_code");
}

export async function rotatePairingCode(): Promise<string> {
  return await invoke("rotate_pairing_code");
}

export async function pairDevice(
  deviceIp: string,
  port: number,
  deviceId: string,
  pairingCode: string,
): Promise<void> {
  await invoke("pair_device", { deviceIp, port, deviceId, pairingCode });
}

export async function uploadFile(
  deviceIp: string,
  port: number,
  remotePath: string,
  localPath: string,
): Promise<void> {
  await invoke("upload_file", {
    deviceIp,
    port,
    remotePath,
    localPath,
  });
}

export async function getLocalDeviceInfo(): Promise<DeviceInfo> {
  return await invoke("get_local_device_info");
}

export async function getChatMessages(): Promise<
  import("./types").ChatEntry[]
> {
  return await invoke("get_chat_messages");
}

export async function sendChatMessage(
  text: string,
  toId?: string,
): Promise<void> {
  await invoke("send_chat_message", { text, toId: toId || null });
}

export async function sendChatFile(
  localPath: string,
  toId?: string,
): Promise<void> {
  await invoke("send_chat_file", { localPath, toId: toId || null });
}

export async function getOnlineDeviceIds(): Promise<string[]> {
  return await invoke("get_online_device_ids");
}

export async function setDeviceNickname(
  deviceId: string,
  nickname: string,
): Promise<void> {
  await invoke("set_device_nickname", { deviceId, nickname });
}

export async function downloadChatFile(fileId: string): Promise<string> {
  return await invoke("download_chat_file", { fileId });
}

export async function openFile(path: string): Promise<void> {
  await invoke("open_file", { path });
}

export async function startRemoteControl(
  targetIp: string,
  targetPort: number,
): Promise<void> {
  await invoke("start_remote_control", { targetIp, targetPort });
}

export async function stopRemoteControl(): Promise<void> {
  await invoke("stop_remote_control");
}

export async function testScreenCapture(): Promise<string> {
  return await invoke("test_screen_capture");
}

export async function getChatImageBase64(fileId: string): Promise<string> {
  return await invoke("get_chat_image_base64", { fileId });
}

export async function getTransferStats(): Promise<
  import("./types").TransferStats
> {
  return await invoke("get_transfer_stats");
}

export async function getOperationLog(): Promise<import("./types").LogEntry[]> {
  return await invoke("get_operation_log");
}

export async function clearOperationLog(): Promise<void> {
  await invoke("clear_operation_log");
}

export async function getMonitors(): Promise<import("./types").MonitorInfo[]> {
  // 直接 fetch HTTP API（不通过 Tauri invoke）
  const response = await fetch("/remote/monitors");
  if (!response.ok) throw new Error("获取显示器列表失败");
  return await response.json();
}

export async function startRecording(): Promise<string> {
  return await invoke("start_recording");
}

export async function stopRecording(): Promise<string> {
  return await invoke("stop_recording");
}

export async function getRecordingStatus(): Promise<{
  duration: number;
  frames: number;
  is_recording: boolean;
}> {
  return await invoke("get_recording_status");
}

export async function checkPermissions(): Promise<{
  screen_recording: boolean;
  accessibility: boolean;
  all_granted: boolean;
}> {
  return await invoke("check_permissions");
}

export async function openScreenRecordingSettings(): Promise<void> {
  await invoke("open_screen_recording_settings");
}

export async function openAccessibilitySettings(): Promise<void> {
  await invoke("open_accessibility_settings");
}
