export type CustomKind = "button" | "switch" | "number";
export type ClipboardMode = "off" | "confirm" | "automatic";
export type MqttTransport = "tls" | "insecure";
export type TransportKind = "mqtt" | "link";

export type ActionKind = "toggle" | "service" | "command" | "widget" | "mqtt";

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
  device_name: string;
  node_id: string;
  publish_interval_secs: number;
  sensors_enabled: Record<string, boolean>;
  custom_commands: CustomCommand[];
  launch_hidden: boolean;
  allow_input: boolean;
  tts_enabled: boolean;
  clipboard_read_mode: ClipboardMode;
  clipboard_write_mode: ClipboardMode;
  allowed_url_origins: string[];
  toast_branding: boolean;
  ha_url: string;
  ha_url_remote: string;
  hotkeys: Hotkey[];
  widgets: WidgetItem[];
  tray_actions: TrayAction[];
}

export interface ConfigView {
  config: AppConfig;
  has_password: boolean;
  has_link_key: boolean;
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
}
