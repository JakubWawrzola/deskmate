import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, ConfigView, NotifyRecord, Snapshot, StatusView } from "./types";

export const api = {
  getConfig: () => invoke<ConfigView>("get_config"),
  saveConfig: (newConfig: AppConfig, password?: string) =>
    invoke<void>("save_config", { newConfig, password: password ?? null }),
  getSnapshot: () => invoke<Snapshot>("get_snapshot"),
  setSensorEnabled: (id: string, enabled: boolean) =>
    invoke<void>("set_sensor_enabled", { id, enabled }),
  addCustomCommand: (
    id: string,
    name: string,
    command: string,
    kind: string = "button",
    numMin?: number,
    numMax?: number,
    numStep?: number,
  ) =>
    invoke<void>("add_custom_command", {
      id,
      name,
      command,
      kind,
      numMin: numMin ?? null,
      numMax: numMax ?? null,
      numStep: numStep ?? null,
    }),
  removeCustomCommand: (id: string) => invoke<void>("remove_custom_command", { id }),
  restartConnection: () => invoke<void>("restart_connection"),
  testToast: () => invoke<void>("test_toast"),
  runCommandLocal: (id: string) => invoke<void>("run_command_local", { id }),
};

export function onStatus(cb: (s: StatusView) => void): Promise<UnlistenFn> {
  return listen<StatusView>("deskmate://status", (e) => cb(e.payload));
}
export function onSensors(cb: (v: Record<string, string>) => void): Promise<UnlistenFn> {
  return listen<Record<string, string>>("deskmate://sensors", (e) => cb(e.payload));
}
export function onNotify(cb: (n: NotifyRecord) => void): Promise<UnlistenFn> {
  return listen<NotifyRecord>("deskmate://notify", (e) => cb(e.payload));
}
