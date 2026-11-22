import { useState } from "react";
import { api } from "../api";
import { Button, EmptyState, Field, Panel, Toggle } from "../components";
import { isLinux, UnavailablePanel, usePlatform } from "../platform";
import type {
  AppConfig,
  CustomSensor,
  CustomSensorKind,
  MonitorSensor,
  SensorDef,
  Snapshot,
} from "../types";

const KIND_LABEL: Record<CustomSensorKind, string> = {
  process: "program running",
  window: "window open",
  service: "service state",
  powershell: "PowerShell",
  monitor: "hardware monitor",
};

const TARGET_LABEL: Record<CustomSensorKind, string> = {
  process: "Program name",
  window: "Window title contains",
  service: "Service name",
  powershell: "PowerShell command",
  monitor: "Sensor",
};

const TARGET_PLACEHOLDER: Record<CustomSensorKind, string> = {
  process: "steam.exe",
  window: "Microsoft Teams",
  service: "Spooler",
  powershell: "(Get-Process chrome -ErrorAction SilentlyContinue).Count",
  monitor: "",
};

/** Same keys as custom_sensors.rs: on/off sensors "csb_", the others "cs_". */
function sensorKey(sensor: CustomSensor): string {
  return sensor.kind === "process" || sensor.kind === "window"
    ? `csb_${sensor.id}`
    : `cs_${sensor.id}`;
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
  // null until the user has looked for the sensors of the monitoring program.
  const [found, setFound] = useState<MonitorSensor[] | null>(null);
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
        kind === "powershell" || kind === "monitor" ? unit.trim() : undefined,
        kind === "powershell" ? Math.max(30, parseInt(interval, 10) || 60) : undefined,
      );
      setName("");
      setTarget("");
      setUnit("");
    });

  const find = async () => {
    setError(null);
    try {
      setFound(await api.listMonitorSensors());
    } catch (e) {
      setError(String(e));
    }
  };

  const pick = (identifier: string) => {
    const sensor = found?.find((candidate) => candidate.identifier === identifier);
    setTarget(identifier);
    setUnit(sensor?.unit ?? "");
    if (sensor && !name.trim()) setName(sensor.name);
  };

  return (
    <Panel title="Your sensors">
      <p className="text-[12px] text-muted mb-3 leading-relaxed">
        Extra sensors for Home Assistant: whether a program runs or a window is open (on/off), the
        state of a Windows service, or the first line a PowerShell command prints. PowerShell
        sensors run in the background every interval (30 s or more) and start disabled; enable one
        after checking the command. A hardware monitor sensor is one of the readings of
        LibreHardwareMonitor (power, clock speeds, temperatures, fans), picked from its list while
        that program runs.
      </p>
      {sensors.length === 0 && <EmptyState text="No sensors of your own yet." />}
      {sensors.map((sensor) => {
        const value = snapshot.sensor_values[sensorKey(sensor)];
        return (
          <div
            key={sensor.id}
            className="grid grid-cols-[160px_120px_1fr_90px_auto_auto] items-center gap-3 min-h-11 border-b border-hairline py-1"
          >
            <span className="text-[13px] font-medium truncate">{sensor.name}</span>
            <span className="microlabel border border-hairline-strong rounded px-1.5 py-0.5 justify-self-start">
              {KIND_LABEL[sensor.kind]}
            </span>
            <span className="mono text-[12px] text-muted truncate">{sensor.target}</span>
            <span className="mono text-[13px] truncate">
              {sensor.enabled ? (value ?? "--") : <span className="text-faint">off</span>}
              {sensor.enabled && value !== undefined && sensor.unit ? ` ${sensor.unit}` : ""}
            </span>
            <Toggle
              on={sensor.enabled}
              onChange={(on) => void run(() => api.setCustomSensorEnabled(sensor.id, on))}
            />
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
            onChange={(e) => {
              setKind(e.target.value as CustomSensorKind);
              setTarget("");
              setUnit("");
            }}
            className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
          >
            <option value="process">Program running</option>
            <option value="window">Window open</option>
            <option value="service">Windows service state</option>
            <option value="powershell">PowerShell output</option>
            <option value="monitor">Hardware monitor sensor</option>
          </select>
        </label>
        {kind === "monitor" ? (
          <div className="flex items-end gap-3">
            <label className="block flex-1 min-w-0">
              <span className="microlabel">{TARGET_LABEL[kind]}</span>
              <select
                value={target}
                onChange={(e) => pick(e.target.value)}
                disabled={!found || found.length === 0}
                className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
              >
                <option value="">
                  {found === null
                    ? "Look for sensors first"
                    : found.length === 0
                      ? "No sensors found"
                      : "Choose a sensor"}
                </option>
                {found?.map((sensor) => (
                  <option key={sensor.identifier} value={sensor.identifier}>
                    {sensor.name} ({sensor.value.toFixed(1)}
                    {sensor.unit ? ` ${sensor.unit}` : ""})
                  </option>
                ))}
              </select>
            </label>
            <Button onClick={() => void find()}>Look for sensors</Button>
          </div>
        ) : (
          <Field
            label={TARGET_LABEL[kind]}
            value={target}
            onChange={setTarget}
            placeholder={TARGET_PLACEHOLDER[kind]}
          />
        )}
      </div>
      {kind === "monitor" && found?.length === 0 && (
        <p className="mt-2 text-[12px] text-muted">
          LibreHardwareMonitor is not running, or it reports no sensors. Start it and look again.
        </p>
      )}
      <div className="mt-3 grid grid-cols-[150px_170px_1fr_auto] gap-3 items-end">
        {kind === "powershell" ? (
          <>
            <Field label="Unit (optional)" value={unit} onChange={setUnit} placeholder="tabs" />
            <Field
              label="Every (seconds)"
              value={interval}
              onChange={setIntervalS}
              placeholder="60"
            />
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

/** Which programs get a volume slider and a mute switch (app_volume.rs). */
function AppVolumePrograms({
  config,
  onChanged,
}: {
  config: AppConfig;
  onChanged: () => Promise<void>;
}) {
  const [programs, setPrograms] = useState((config.app_volume_programs ?? []).join(", "));
  const [msg, setMsg] = useState<string | null>(null);

  const save = async () => {
    setMsg(null);
    try {
      await api.setAppVolumePrograms(
        programs
          .split(/[,;\n]+/)
          .map((program) => program.trim())
          .filter(Boolean),
      );
      await onChanged();
      setMsg("Saved.");
    } catch (e) {
      setMsg(String(e));
    }
  };

  return (
    <div className="mt-3 pt-3 border-t border-hairline">
      <div className="flex items-end gap-3">
        <div className="flex-1">
          <Field
            label="Volume of single programs"
            value={programs}
            onChange={setPrograms}
            placeholder="discord, spotify, chrome"
            hint="Each program listed here gets a volume slider and a mute switch in Home Assistant, like its row in the Windows volume mixer. They have a value while the program plays sound. Program names separated by commas; Task Manager shows the names."
          />
        </div>
        <Button onClick={() => void save()}>Save</Button>
      </div>
      {msg && <p className="mt-2 text-[12px] text-muted">{msg}</p>}
    </div>
  );
}

/** Which programs the "User state" sensor counts as games (user_state.rs). */
function GamePrograms({
  config,
  onChanged,
}: {
  config: AppConfig;
  onChanged: () => Promise<void>;
}) {
  const [programs, setPrograms] = useState((config.game_programs ?? []).join(", "));
  const [msg, setMsg] = useState<string | null>(null);

  const save = async () => {
    setMsg(null);
    try {
      await api.setGamePrograms(
        programs
          .split(/[,;\n]+/)
          .map((program) => program.trim())
          .filter(Boolean),
      );
      await onChanged();
      setMsg("Saved.");
    } catch (e) {
      setMsg(String(e));
    }
  };

  return (
    <div className="mt-3 pt-3 border-t border-hairline">
      <div className="flex items-end gap-3">
        <div className="flex-1">
          <Field
            label="Games for User state"
            value={programs}
            onChange={setPrograms}
            placeholder="minecraft, robloxplayerbeta"
            hint='A game in a borderless window looks like any full-screen program to Windows. Games from Steam, Epic, GOG, Xbox, Riot and EA folders are recognised by themselves; list others here by program name and "User state" shows game for them too.'
          />
        </div>
        <Button onClick={() => void save()}>Save</Button>
      </div>
      {msg && <p className="mt-2 text-[12px] text-muted">{msg}</p>}
    </div>
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
  const linux = isLinux(usePlatform());
  const toggle = async (id: string, v: boolean) => {
    await api.setSensorEnabled(id, v);
    await onChanged();
  };

  // Screen brightness is a number too, but not a sound control.
  const isControl = (d: SensorDef) =>
    (d.component === "number" || d.component === "select") && d.id !== "brightness";
  const std = snapshot.sensor_defs.filter((d) => !d.privacy && !isControl(d) && !d.hardware);
  const priv = snapshot.sensor_defs.filter((d) => d.privacy);
  const sound = snapshot.sensor_defs.filter(isControl);
  const hardware = snapshot.sensor_defs.filter((d) => d.hardware);

  const setAll = async (ids: string[], v: boolean) => {
    await api.setSensorsEnabled(ids, v);
    await onChanged();
  };

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

      <Panel
        title="Hardware"
        action={
          hardware.length > 0 ? (
            <div className="flex items-center gap-2">
              <Button
                onClick={() =>
                  void setAll(
                    hardware.map((d) => d.id),
                    true,
                  )
                }
              >
                All on
              </Button>
              <Button
                onClick={() =>
                  void setAll(
                    hardware.map((d) => d.id),
                    false,
                  )
                }
              >
                All off
              </Button>
            </div>
          ) : undefined
        }
      >
        <p className="text-[12px] text-muted mb-2 leading-relaxed">
          Found on this computer: every drive, the GPU and temperatures. Each one is its own entity
          in Home Assistant; switch off what you do not need and it disappears there. "Disk usage"
          above is the system drive only.
        </p>
        {hardware.length === 0 && <EmptyState text="Looking for drives and sensors..." />}
        {hardware.map((d) => (
          <Row
            key={d.id}
            def={d}
            value={snapshot.sensor_values[d.id]}
            enabled={enabledOf(config, d)}
            onToggle={(v) => void toggle(d.id, v)}
          />
        ))}
      </Panel>

      <Panel title="Sound">
        <p className="text-[12px] text-muted mb-2 leading-relaxed">
          {linux
            ? 'The volume of the default playback device, through PipeWire or PulseAudio. The "Mute audio" switch comes with it. The microphone and the choice of sound devices are not part of the Linux preview.'
            : 'Controls for Home Assistant: speaker and microphone volume, and which sound devices Windows uses. "Mute audio" and "Mute microphone" switches are always there. An automation can mute the microphone when the radio starts, or switch to the headset when a call comes in.'}
        </p>
        {sound.map((d) => (
          <Row
            key={d.id}
            def={d}
            value={snapshot.sensor_values[d.id]}
            enabled={enabledOf(config, d)}
            onToggle={(v) => void toggle(d.id, v)}
          />
        ))}
        {!linux && <AppVolumePrograms config={config} onChanged={onChanged} />}
      </Panel>

      <Panel title="Privacy-sensitive sensors">
        <p className="text-[12px] text-muted mb-2 leading-relaxed">
          These expose what you are doing on this computer (window titles, playing media, network
          name) to Home Assistant. They stay off until you enable them here.
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
        {!linux && <GamePrograms config={config} onChanged={onChanged} />}
      </Panel>

      {linux ? (
        <UnavailablePanel title="Your sensors">
          Sensors of your own (programs, windows, services, PowerShell) are not part of the Linux
          preview.
        </UnavailablePanel>
      ) : (
        <YourSensors snapshot={snapshot} config={config} onChanged={onChanged} />
      )}
    </>
  );
}
