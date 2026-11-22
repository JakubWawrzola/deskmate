export type CustomKind = "button" | "switch" | "number";
/** powershell: a script; launch: open a program, file or URL; stop: stop a program by name; keys: press a key combination */
export type CustomAction = "powershell" | "launch" | "stop" | "keys";
export type CustomSensorKind = "process" | "window" | "service" | "powershell" | "monitor";

/** A sensor LibreHardwareMonitor offers (hardware_monitor.rs). */
export interface MonitorSensor {
  identifier: string;
  name: string;
  unit: string;
  value: number;
}
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
  /** Minutes before the stop at which a warning comes; empty = 10, 5, 1. */
  warn_at_min: number[];
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

/** What happens when a newer release is found (Settings > Updates). */
export type UpdateMode = "auto" | "ask" | "notify";

/** An update install that was started; settled at the next start. */
export interface UpdateAttempt {
  from: string;
  to: string;
  auto: boolean;
  hidden: boolean;
  /** Unix seconds */
  started_at: number;
}

/** What the updater remembers between runs. Changed only by the app. */
export interface UpdateState {
  /** "Later": no question before this time, Unix seconds */
  later_until: number;
  installing: UpdateAttempt | null;
  failed_version: string;
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
  /** Programs the user state counts as games, such as "eldenring.exe". */
  game_programs?: string[];
  /** Programs with a volume slider and a mute switch of their own. */
  app_volume_programs?: string[];
  launch_hidden: boolean;
  allow_input: boolean;
  tts_enabled: boolean;
  clipboard_read_mode: ClipboardMode;
  clipboard_write_mode: ClipboardMode;
  allowed_url_origins: string[];
  toast_branding: boolean;
  update_check: boolean;
  update_mode?: UpdateMode;
  update_state?: UpdateState;
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
  /** Disconnect on the Status page: no connection until Connect */
  connection_paused?: boolean;
  /** The local channel for the Stream Deck plugin (Settings > Stream Deck) */
  streamdeck_enabled?: boolean;
}

/** State of the Stream Deck channel (streamdeck/backend.rs). */
export type StreamDeckChannelState = "off" | "starting" | "listening" | "name_taken" | "failed";

/** Settings > Stream Deck: the switch, the channel and the plugin (streamdeck/app.rs). */
export interface StreamDeckStatus {
  /** the switch as stored */
  enabled: boolean;
  channel: {
    state: StreamDeckChannelState;
    /** programs connected right now */
    connected: number;
  };
  plugin: {
    /** Stream Deck's folder exists for this Windows account */
    stream_deck_found: boolean;
    /** the plugin that ships with this app; null: this build carries none */
    bundled_version: string | null;
    installed: boolean;
    /** null also when the installed copy has no readable version */
    installed_version: string | null;
    update_available: boolean;
  };
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
  /** Version of the Home Assistant integration, once it has said so in this Link session */
  integration: string | null;
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
  /** detected at runtime: drives, GPU, temperatures */
  hardware?: boolean;
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
  /** A later notification with the same tag replaces this one. */
  tag: string | null;
  priority: NotifyPriority;
}

export type NotifyPriority = "normal" | "high" | "urgent";

/** What this computer's desktop offers. Mirrors `Capabilities` in src-tauri/src/platform.rs. */
export interface PlatformCapabilities {
  platform: "windows" | "linux";
  session_type: "windows" | "x11" | "wayland" | "unknown";
  /** a tray icon can be shown, so the window may hide to it */
  tray: boolean;
  global_shortcuts: boolean;
  clipboard: boolean;
  active_window: boolean;
  notifications: boolean;
  notification_buttons: boolean;
  media: boolean;
  volume: boolean;
  lock: boolean;
  idle: boolean;
  open_url: boolean;
  /** why secrets cannot be kept in the user's keyring; null when it is there */
  keyring_problem: string | null;
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

export type UpdatePhase = "available" | "downloading" | "installing" | "failed";

/** The newer release found by the update check. */
export interface UpdateInfo {
  version: string;
  /** the release page */
  url: string;
  /** a signed installer for this computer is on offer; false: link only */
  installable: boolean;
  phase: UpdatePhase;
  downloaded_bytes: number;
  total_bytes: number | null;
  /** why the last attempt stopped */
  error: string | null;
  /** "Later" holds until tomorrow */
  postponed: boolean;
  /** installs itself once the computer is idle */
  auto_install: boolean;
}

/** Settings > Deskmate service (service/view.rs). */
export type ServicePhase =
  | "not_installed"
  | "not_answering"
  | "running"
  | "needs_update"
  | "settings_differ"
  | "integration_too_old"
  | "error";

export type ServiceActivity = "connected" | "waiting" | "connecting";

export type ServiceSetupAction = "install" | "update" | "apply";

export interface ServiceView {
  phase: ServicePhase;
  activity: ServiceActivity | null;
  /** the service's own short text, or the app's */
  detail: string;
  running_version: string | null;
  bundled_version: string;
  /** the copy in this app is newer than the one that runs */
  needs_update: boolean;
  /** the service holds other settings or keys than the app, or none */
  settings_differ: boolean;
  /** the running service is newer than the copy in this app */
  newer_than_app: boolean;
  install_blocked: string | null;
  apply_blocked: string | null;
  update_blocked: string | null;
  /** the service waits because the user pressed Disconnect */
  paused_by_user: boolean;
}
