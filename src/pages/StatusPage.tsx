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
      {snapshot.update && (
        <div className="mb-3 flex items-center justify-between gap-3 border border-hairline-strong rounded-md px-4 py-3 bg-panel">
          <p className="text-[13px]">
            Deskmate <span className="mono">{snapshot.update.version}</span> is available. Install it
            over this version; settings and pairing are kept.
          </p>
          <Button kind="primary" onClick={() => void api.openUpdatePage()}>
            Download
          </Button>
        </div>
      )}
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
          <dd>{config.transport === "link" ? "Deskmate integration" : "MQTT"}</dd>
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
          This computer is registered through {config.transport === "link" ? "the Deskmate integration" : "MQTT discovery"} as device{" "}
          <span className="mono text-ink">{config.device_name}</span>. Find it under
          Settings &gt; Devices &amp; services &gt; {config.transport === "link" ? "Deskmate" : "MQTT"}. Entity ids follow the
          device name, for example{" "}
          <span className="mono text-ink">
            sensor.{config.device_name.toLowerCase().replace(/[^a-z0-9]+/g, "_")}_cpu_usage
          </span>.
        </p>
      </Panel>
    </>
  );
}
