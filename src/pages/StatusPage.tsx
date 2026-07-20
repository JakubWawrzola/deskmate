import { api } from "../api";
import { Button, Panel, StatusDot } from "../components";
import type { AppConfig, Snapshot } from "../types";

/** Duze odczyty mono - sygnatura wizualna aplikacji. */
function Readout({ label, value, unit }: { label: string; value: string; unit?: string }) {
  return (
    <div className="border border-hairline rounded-md px-4 py-3 bg-panel">
      <p className="microlabel">{label}</p>
      <p className="mono text-[28px] leading-9 mt-1">
        {value}
        {unit && <span className="text-[14px] text-muted ml-1">{unit}</span>}
      </p>
    </div>
  );
}

export default function StatusPage({ snapshot, config }: { snapshot: Snapshot; config: AppConfig }) {
  const v = snapshot.sensor_values;
  const num = (id: string) => (v[id] !== undefined ? v[id] : "--");

  return (
    <>
      <div className="grid grid-cols-3 gap-3">
        <Readout label="CPU" value={num("cpu")} unit="%" />
        <Readout label="Memory" value={num("memory")} unit="%" />
        <Readout label="Disk" value={num("disk")} unit="%" />
      </div>

      <Panel
        title="Connection"
        action={
          <Button onClick={() => void api.restartConnection()}>Reconnect</Button>
        }
      >
        <dl className="grid grid-cols-[140px_1fr] gap-y-2 text-[13px]">
          <dt className="text-muted">Status</dt>
          <dd className="flex items-center gap-2">
            <StatusDot on={snapshot.status.connected} />
            {snapshot.status.detail}
          </dd>
          <dt className="text-muted">Transport</dt>
          <dd>{config.transport === "link" ? "Deskmate Link" : "MQTT"}</dd>
          <dt className="text-muted">Endpoint</dt>
          <dd className="mono">{config.transport === "link" ? config.link_url : `${config.broker_host}:${config.broker_port}`}</dd>
          <dt className="text-muted">Device</dt>
          <dd>{config.device_name}</dd>
          <dt className="text-muted">Node ID</dt>
          <dd className="mono">{config.node_id}</dd>
          <dt className="text-muted">Messages published</dt>
          <dd className="mono">{snapshot.published_count}</dd>
          <dt className="text-muted">Interval</dt>
          <dd className="mono">{config.publish_interval_secs}s</dd>
        </dl>
      </Panel>

      <Panel title="In Home Assistant">
        <p className="text-[13px] text-muted leading-relaxed">
          This computer is registered through {config.transport === "link" ? "the Deskmate Link integration" : "MQTT discovery"} as device{" "}
          <span className="mono text-ink">{config.device_name}</span>. Find it under
          Settings &gt; Devices &amp; services &gt; {config.transport === "link" ? "Deskmate Link" : "MQTT"}. Entity ids follow the pattern{" "}
          <span className="mono text-ink">sensor.{config.node_id}_cpu</span>.
        </p>
      </Panel>
    </>
  );
}
