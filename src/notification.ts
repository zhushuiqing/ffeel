import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";

let permitted: boolean | null = null;

async function ensurePermission(): Promise<boolean> {
  if (permitted !== null) return permitted;
  let granted = await isPermissionGranted();
  if (!granted) {
    const permission = await requestPermission();
    granted = permission === "granted";
  }
  permitted = granted;
  return granted;
}

export async function notify(title: string, body?: string) {
  const ok = await ensurePermission();
  if (!ok) return;
  sendNotification({ title, body });
}
