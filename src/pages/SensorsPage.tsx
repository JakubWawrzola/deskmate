import { useState } from "react";
import { api } from "../api";
import { Button, EmptyState, Field, Panel, Toggle } from "../components";
import type { AppConfig, CustomSensor, CustomSensorKind, SensorDef, Snapshot } from "../types";

const KIND_LABEL: Record<CustomSensorKind, string> = {
  process: "program running",
  window: "window open",
  service: "service state",
  powershell: "PowerShell",
};

const TARGET_LABEL: Record<CustomSensorKind, string> = {
  process: "Program name",
  window: "Window title contains",
  service: "Service name",
  powershell: "PowerShell command",
};

const TARGET_PLACEHOLDER: Record<CustomSensorKind, string> = {
  process: "steam.exe",
  window: "Microsoft Teams",
  service: "Spooler",
  powershell: "(Get-Process chrome -ErrorAction SilentlyContinue).Count",
};

/** Same keys as custom_sensors.rs: on/off sensors "csb_", the others "cs_". */
function sensorKey(sensor: CustomSensor): string {
  return sensor.kind === "process" || sensor.kind === "window" ? `csb_${sensor.id}` : `cs_${sensor.id}`;
}

function YourSensors({
  snapshot,
  config,
  onChanged,
}: {
  snapshot: Snapshot;
  config: AppConfig;
  onChanged: () => Promise<void>;
}) {
  const [name, setName] = useState("");
  const [kind, setKind] = useState<CustomSensorKind>("process");
  const [target, setTarget] = useState("");
  const [unit, setUnit] = useState("");
  const [interval, setIntervalS] = useState("60");
  const [error, setError] = useState<string | null>(null);
  const sensors = config.custom_sensors ?? [];

  const run = async (action: () => Promise<void>) => {
    setError(null);
    try {
      await action();
      await onChanged();
    } catch (e) {
      setError(String(e));
    }
  };

  const add = () =>
    run(async () => {
      await api.addCustomSensor(
        name.trim(),
        kind,
        target.trim(),
        kind === "powershell" ? unit.trim() : undefined,
        kind === "powershell" ? Math.max(30, parseInt(interval, 10) || 60) : undefined,
      );
      setName("");
      setTarget("");
      setUnit("");
    });

  return (
    <Panel title="Your sensors">
      <p className="text-[12px] text-muted mb-3 leading-relaxed">
        Extra sensors for Home Assistant: whether a program runs or a window is open (on/off),
        the state of a Windows service, or the first line a PowerShell command prints.
        PowerShell sensors run in the background every interval (30 s or more) and start
        disabled; enable one after checking the command.
      </p>
      {sensors.length === 0 && <EmptyState text="No sensors of your own yet." />}
      {sensors.map((sensor) => {
        const value = snapshot.sensor_values[sensorKey(sensor)];
        return (
          <div key={sensor.id} className="grid grid-cols-[160px_120px_1fr_90px_auto_auto] items-center gap-3 min-h-11 border-b border-hairline py-1">
            <span className="text-[13px] font-medium truncate">{sensor.name}</span>
            <span className="microlabel border border-hairline-strong rounded px-1.5 py-0.5 justify-self-start">
              {KIND_LABEL[sensor.kind]}
            </span>
            <span className="mono text-[12px] text-muted truncate">{sensor.target}</span>
            <span className="mono text-[13px] truncate">
              {sensor.enabled ? (value ?? "--") : <span className="text-faint">off</span>}
              {sensor.enabled && value !== undefined && sensor.unit ? ` ${sensor.unit}` : ""}
            </span>
            <Toggle on={sensor.enabled} onChange={(on) => void run(() => api.setCustomSensorEnabled(sensor.id, on))} />
            <Button kind="danger" onClick={() => void run(() => api.removeCustomSensor(sensor.id))}>
              Remove
            </Button>
          </div>
        );
      })}

      <div className="mt-4 grid grid-cols-[150px_170px_1fr] gap-3 items-end">
        <Field label="Name" value={name} onChange={setName} placeholder="Steam running" />
        <label className="block">
          <span className="microlabel">Type</span>
          <select
            value={kind}
            onChange={(e) => setKind(e.target.value as CustomSensorKind)}
            className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
          >
            <option value="process">Program running</option>
            <option value="window">Window open</option>
            <option value="service">Windows service state</option>
            <option value="powershell">PowerShell output</option>
          </select>
        </label>
        <Field label={TARGET_LABEL[kind]} value={target} onChange={setTarget} placeholder={TARGET_PLACEHOLDER[kind]} />
      </div>
      <div className="mt-3 grid grid-cols-[150px_170px_1fr_auto] gap-3 items-end">
        {kind === "powershell" ? (
          <>
            <Field label="Unit (optional)" value={unit} onChange={setUnit} placeholder="tabs" />
            <Field label="Every (seconds)" value={interval} onChange={setIntervalS} placeholder="60" />
          </>
        ) : (
          <>
            <span />
            <span />
          </>
        )}
        <span />
        <Button kind="primary" onClick={() => void add()} disabled={!name.trim() || !target.trim()}>
          Add
        </Button>
      </div>
      {error && <p className="mt-2 text-[13px]">{error}</p>}
    </Panel>
  );
}

function enabledOf(cfg: AppConfig, def: SensorDef): boolean {
  const v = cfg.sensors_enabled[def.id];
  return v === undefined ? def.default_enabled : v;
}

function Row({
  def,
  value,
  enabled,
  onToggle,
}: {
  def: SensorDef;
  value: string | undefined;
  enabled: boolean;
  onToggle: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-3 h-11 border-b border-hairline last:border-b-0">
      <div className="w-44 shrink-0">
        <p className="text-[13px] font-medium leading-4">{def.name}</p>
        <p className="mono text-[11px] text-faint">{def.id}</p>
      </div>
      <div className="flex-1 mono text-[13px] text-muted truncate">
        {enabled ? (value ?? "--") : <span className="text-faint">off</span>}
        {enabled && value !== undefined && def.unit ? ` ${def.unit}` : ""}
      </div>
      {def.privacy && (
        <span className="microlabel border border-hairline-strong rounded px-1.5 py-0.5">
          privacy
        </span>
      )}
      <Toggle on={enabled} onChange={onToggle} />
    </div>
  );
}

export default function SensorsPage({
  snapshot,
  config,
  onChanged,
}: {
  snapshot: Snapshot;
  config: AppConfig;
  onChanged: () => Promise<void>;
}) {
  const toggle = async (id: string, v: boolean) => {
    await api.setSensorEnabled(id, v);
    await onChanged();
  };

  const std = snapshot.sensor_defs.filter((d) => !d.privacy && d.component !== "number");
  const priv = snapshot.sensor_defs.filter((d) => d.privacy);

  return (
    <>
      <Panel title="System sensors">
        {std.length === 0 && <EmptyState text="No sensors." />}
        {std.map((d) => (
          <Row
            key={d.id}
            def={d}
            value={snapshot.sensor_values[d.id]}
            enabled={enabledOf(config, d)}
            onToggle={(v) => void toggle(d.id, v)}
          />
        ))}
      </Panel>

      <Panel title="Privacy-sensitive sensors">
        <p className="text-[12px] text-muted mb-2 leading-relaxed">
          These expose what you are doing on this computer (window titles, playing
          media, network name) to Home Assistant. They stay off until you enable
          them here.
        </p>
        {priv.map((d) => (
          <Row
            key={d.id}
            def={d}
            value={snapshot.sensor_values[d.id]}
            enabled={enabledOf(config, d)}
            onToggle={(v) => void toggle(d.id, v)}
          />
        ))}
      </Panel>

      <YourSensors snapshot={snapshot} config={config} onChanged={onChanged} />
    </>
  );
}
