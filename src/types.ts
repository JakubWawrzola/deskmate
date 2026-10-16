export type CustomKind = "button" | "switch" | "number";
/** powershell: a script; launch: open a program, file or URL; stop: stop a program by name; keys: press a key combination */
export type CustomAction = "powershell" | "launch" | "stop" | "keys";
export type CustomSensorKind = "process" | "window" | "service" | "powershell";
export type ClipboardMode = "off" | "confirm" | "automatic";
export type MqttTransport = "tls" | "insecure";
export type TransportKind = "mqtt" | "link";
export type ThemeMode = "system" | "light" | "dark";

export type ActionKind = "toggle" | "service" | "command" | "widget" | "dashboard" | "mqtt";

export interface ActionSpec {
  kind: ActionKind;
  entity_id: string;
  service: string;
  data: string;
  command_id: string;
}

export interface Hotkey {
  id: string;
  name: string;
  accelerator: string;
  action: ActionSpec;
}

export interface TrayAction {
  id: string;
  name: string;
  action: ActionSpec;
}

export interface WidgetItem {
  entity_id: string;
  label: string;
}

export interface WidgetState {
  entity_id: string;
  label: string;
  state: string;
  togglable: boolean;
}

export const emptyAction = (): ActionSpec => ({
  kind: "toggle",
  entity_id: "",
  service: "",
  data: "",
  command_id: "",
});

export interface CustomCommand {
  id: string;
  name: string;
  command: string;
  kind: CustomKind;
  num_min: number;
  num_max: number;
  num_step: number;
  enabled: boolean;
  require_confirmation: boolean;
  action?: CustomAction;
  args?: string;
}

export interface CpuTemperatureStatus {
  supported: boolean;
  pawnio_installed: boolean;
  helper_installed: boolean;
  helper_running: boolean;
  celsius: number | null;
  error: string | null;
}

export interface CustomSensor {
  id: string;
  name: string;
  kind: CustomSensorKind;
  target: string;
  unit: string;
  interval_secs: number;
  enabled: boolean;
}

/** Screen time and family rules (family.rs). */
export interface FamilyConfig {
  enabled: boolean;
  rules_on: boolean;
  daily_limit_min: number;
  allowed_from: string;
  allowed_until: string;
  block_apps: boolean;
  blocked_apps: string[];
  /** lock | sign_out */
  time_up_action: string;
}

export interface FamilyView {
  rules: FamilyConfig;
  protected: boolean;
  admin_account: boolean;
  watchdog_installed: boolean;
  today_min: number;
  left_min: number | null;
  bonus_min: number;
  /** off | ok | time_up | outside_hours */
  status: string;
}

/** A file sent to or read from this computer (security log). */
export interface FileEvent {
  time: number;
  /** received | read | cancelled */
  kind: string;
  path: string;
  result: string;
}

export interface AppConfig {
  configured: boolean;
  transport: TransportKind;
  broker_host: string;
  broker_host_remote: string;
  broker_port: number;
  mqtt_transport: MqttTransport;
  mqtt_ca_path: string;
  username: string;
  link_url: string;
  link_url_remote: string;
  link_file_roots: string[];
  link_inbox_mode: ClipboardMode;
  link_inbox_dir: string;
  link_files_max_mb: number;
  link_cascade: boolean;
  device_name: string;
  node_id: string;
  publish_interval_secs: number;
  sensors_enabled: Record<string, boolean>;
  custom_commands: CustomCommand[];
  custom_sensors?: CustomSensor[];
  launch_hidden: boolean;
  allow_input: boolean;
  tts_enabled: boolean;
  clipboard_read_mode: ClipboardMode;
  clipboard_write_mode: ClipboardMode;
  allowed_url_origins: string[];
  toast_branding: boolean;
  update_check: boolean;
  theme?: ThemeMode;
  dashboard_path?: string;
  dashboard_on_top?: boolean;
  start_with_windows?: boolean | null;
  update_notified: string;
  ha_url: string;
  ha_url_remote: string;
  hotkeys: Hotkey[];
  widgets: WidgetItem[];
  tray_actions: TrayAction[];
  family?: FamilyConfig;
}

export interface ConfigView {
  config: AppConfig;
  has_password: boolean;
  has_link_key: boolean;
  has_cascade_key: boolean;
}

export interface StatusView {
  connected: boolean;
  detail: string;
}

export interface SensorDef {
  id: string;
  name: string;
  component: string;
  unit: string | null;
  device_class: string | null;
  icon: string | null;
  privacy: boolean;
  default_enabled: boolean;
}

export interface CommandDef {
  id: string;
  name: string;
  icon: string;
}

export interface NotifyRecord {
  title: string;
  message: string;
  image: string | null;
  received_at: string;
}

export interface Snapshot {
  status: StatusView;
  sensor_values: Record<string, string>;
  published_count: number;
  notifications: NotifyRecord[];
  sensor_defs: SensorDef[];
  command_defs: CommandDef[];
  hostname: string;
  ha_configured: boolean;
  update: UpdateInfo | null;
}

export interface UpdateInfo {
  version: string;
  url: string;
}
