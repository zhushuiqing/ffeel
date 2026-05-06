import { useState, useCallback } from "react";
import type { DeviceInfo, TransferTask, Settings } from "../types";
import * as api from "../api";

export interface AppStore {
  devices: DeviceInfo[];
  localDevice: DeviceInfo | null;
  transfers: TransferTask[];
  settings: Settings | null;
  isScanning: boolean;
  selectedDevice: DeviceInfo | null;
  currentPath: string;
  statusMessage: string;
}

export function useAppStore() {
  const [store, setStore] = useState<AppStore>({
    devices: [],
    localDevice: null,
    transfers: [],
    settings: null,
    isScanning: false,
    selectedDevice: null,
    currentPath: "",
    statusMessage: "",
  });

  const update = useCallback(
    (partial: Partial<AppStore>) => {
      setStore((prev) => ({ ...prev, ...partial }));
    },
    []
  );

  const init = useCallback(async () => {
    const localDevice = await api.getLocalDeviceInfo();
    const settings = await api.getSettings();
    update({ localDevice, settings });
  }, [update]);

  const startScanning = useCallback(async () => {
    update({ isScanning: true, statusMessage: "正在扫描局域网设备..." });
    await api.startDiscovery();
    setTimeout(() => {
      update({ isScanning: false, statusMessage: "扫描完成" });
    }, 3000);
  }, [update]);

  const selectDevice = useCallback(
    (device: DeviceInfo | null) => {
      update({ selectedDevice: device, currentPath: "" });
    },
    [update]
  );

  const refreshTransfers = useCallback(async () => {
    const transfers = await api.getTransfers();
    update({ transfers });
  }, [update]);

  const updateTransferProgress = useCallback(
    (id: string, bytes_transferred: number, speed: number) => {
      setStore((prev) => ({
        ...prev,
        transfers: prev.transfers.map((t) =>
          t.id === id ? { ...t, bytes_transferred, speed, status: "Transferring" as const } : t
        ),
      }));
    },
    []
  );

  const updateTransferComplete = useCallback(
    (id: string) => {
      setStore((prev) => ({
        ...prev,
        transfers: prev.transfers.map((t) =>
          t.id === id ? { ...t, status: "Completed" as const, bytes_transferred: t.file_size } : t
        ),
      }));
    },
    []
  );

  const updateTransferError = useCallback(
    (id: string, error: string) => {
      setStore((prev) => ({
        ...prev,
        transfers: prev.transfers.map((t) =>
          t.id === id ? { ...t, status: "Failed" as const, error } : t
        ),
      }));
    },
    []
  );

  const cancelTransfer = useCallback(
    async (id: string) => {
      await api.cancelTransfer(id);
      await refreshTransfers();
    },
    [refreshTransfers]
  );

  const pauseTransfer = useCallback(
    async (id: string) => {
      await api.pauseTransfer(id);
      await refreshTransfers();
    },
    [refreshTransfers]
  );

  const resumeTransfer = useCallback(
    async (id: string) => {
      await api.resumeTransfer(id);
      await refreshTransfers();
    },
    [refreshTransfers]
  );

  const setStatusMessage = useCallback(
    (msg: string) => {
      update({ statusMessage: msg });
      setTimeout(() => update({ statusMessage: "" }), 5000);
    },
    [update]
  );

  return { store, init, startScanning, selectDevice, refreshTransfers, cancelTransfer, pauseTransfer, resumeTransfer, setStatusMessage, updateTransferProgress, updateTransferComplete, updateTransferError };
}
