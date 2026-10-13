import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppConfig,
  CustomAction,
  CustomSensorKind,
  ConfigView,
  Hotkey,
  NotifyRecord,
  Snapshot,
  StatusView,
  TrayAction,
  UpdateInfo,
  WidgetItem,
  WidgetState,
} from "./types";

export const api = {
  getConfig: () => invoke<ConfigView>("get_config"),
  saveConfig: (
    newConfig: AppConfig,
    password?: string,
    linkKey?: string,
    cascadeKey?: string,
  ) =>
    invoke<void>("save_config", {
      newConfig,
      password: password ?? null,
      linkKey: linkKey ?? null,
      cascadeKey: cascadeKey ?? null,
    }),
  getSnapshot: () => invoke<Snapshot>("get_snapshot"),
  setSensorEnabled: (id: string, enabled: boolean) =>
    invoke<void>("set_sensor_enabled", { id, enabled }),
  setFeatureFlag: (flag: string, enabled: boolean) =>
    invoke<void>("set_feature_flag", { flag, enabled }),
  checkUpdatesNow: () => invoke<UpdateInfo | null>("check_updates_now"),
  getStartWithWindows: () => invoke<boolean>("get_start_with_windows"),
  setStartWithWindows: (enabled: boolean) => invoke<void>("set_start_with_windows", { enabled }),
  openUpdatePage: () => invoke<void>("open_update_page"),
  addCustomCommand: (
    id: string,
    name: string,
    command: string,
    kind: string = "button",
    numMin?: number,
    numMax?: number,
    numStep?: number,
    action: CustomAction = "powershell",
    args?: string,
  ) =>
    invoke<void>("add_custom_command", {
      id,
      name,
      command,
      kind,
      numMin: numMin ?? null,
      numMax: numMax ?? null,
      numStep: numStep ?? null,
      action,
      args: args ?? null,
    }),
  addCustomSensor: (
    name: string,
    kind: CustomSensorKind,
    target: string,
    unit?: string,
    intervalSecs?: number,
  ) =>
    invoke<void>("add_custom_sensor", {
      name,
      kind,
      target,
      unit: unit ?? null,
      intervalSecs: intervalSecs ?? null,
    }),
  setCustomSensorEnabled: (id: string, enabled: boolean) =>
    invoke<void>("set_custom_sensor_enabled", { id, enabled }),
  removeCustomSensor: (id: string) => invoke<void>("remove_custom_sensor", { id }),
  removeCustomCommand: (id: string) => invoke<void>("remove_custom_command", { id }),
  updateCustomCommandSecurity: (
    id: string,
    enabled: boolean,
    requireConfirmation: boolean,
  ) =>
    invoke<void>("update_custom_command_security", {
      id,
      enabled,
      requireConfirmation,
    }),
  restartConnection: () => invoke<void>("restart_connection"),
  testToast: () => invoke<void>("test_toast"),
  runCommandLocal: (id: string) => invoke<void>("run_command_local", { id }),
  // HA API (F1)
  setHaApi: (url: string, urlRemote: string, token?: string) =>
    invoke<void>("set_ha_api", { url, urlRemote, token: token ?? null }),
  haPing: () => invoke<string>("ha_ping"),
  // hotkeys (F2) - returns the list of registration errors
  updateHotkeys: (hotkeys: Hotkey[]) => invoke<string[]>("update_hotkeys", { hotkeys }),
  // widgets (F3)
  updateWidgets: (widgets: WidgetItem[]) => invoke<void>("update_widgets", { widgets }),
  widgetStates: () => invoke<WidgetState[]>("widget_states"),
  widgetToggle: (entityId: string) => invoke<void>("widget_toggle", { entityId }),
  toggleWidgetWindow: () => invoke<void>("toggle_widget_window"),
  // tray quick actions (F4)
  updateTrayActions: (trayActions: TrayAction[]) =>
    invoke<void>("update_tray_actions", { trayActions }),
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
