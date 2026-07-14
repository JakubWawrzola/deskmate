import { api } from "../api";
import { EmptyState, Panel, Toggle } from "../components";
import type { AppConfig, SensorDef, Snapshot } from "../types";

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
    </>
  );
}
