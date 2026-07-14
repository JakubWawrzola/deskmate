export type CustomKind = "button" | "switch" | "number";

export interface CustomCommand {
  id: string;
  name: string;
  command: string;
  kind: CustomKind;
  num_min: number;
  num_max: number;
  num_step: number;
}

export interface AppConfig {
  configured: boolean;
  broker_host: string;
  broker_port: number;
  username: string;
  device_name: string;
  node_id: string;
  publish_interval_secs: number;
  sensors_enabled: Record<string, boolean>;
  custom_commands: CustomCommand[];
  launch_hidden: boolean;
  allow_input: boolean;
  tts_enabled: boolean;
}

export interface ConfigView {
  config: AppConfig;
  has_password: boolean;
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
}
