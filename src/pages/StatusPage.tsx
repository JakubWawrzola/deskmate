import { useState } from "react";
import { api } from "../api";
import { Button, Panel, StatusDot } from "../components";
import { t } from "../i18n";
import { isLinux, usePlatform } from "../platform";
import type { AppConfig, Snapshot } from "../types";
import ServiceBanner from "./ServiceBanner";
import UpdateBanner from "./UpdateBanner";

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

export default function StatusPage({
  snapshot,
  config,
  onChanged,
}: {
  snapshot: Snapshot;
  config: AppConfig;
  onChanged: () => Promise<void>;
}) {
  const v = snapshot.sensor_values;
  const num = (id: string) => (v[id] !== undefined ? v[id] : "--");
  const paused = config.connection_paused ?? false;
  const [busy, setBusy] = useState(false);

  // Disconnect stays until Connect, also after a restart (#10).
  const setPaused = async (value: boolean) => {
    setBusy(true);
    try {
      await api.setConnectionPaused(value);
      await onChanged();
    } finally {
      setBusy(false);
    }
  };

  const linux = isLinux(usePlatform());

  return (
    <>
      {snapshot.update && <UpdateBanner update={snapshot.update} />}
      {!linux && <ServiceBanner />}
      <div className="grid grid-cols-3 gap-3">
        <Readout label={"CPU"} value={num("cpu")} unit="%" />
        <Readout label={t("Memory")} value={num("memory")} unit="%" />
        <Readout label={t("Disk")} value={num("disk")} unit="%" />
      </div>

      <Panel
        title={t("Connection")}
        action={
          paused ? (
            <Button kind="primary" disabled={busy} onClick={() => void setPaused(false)}>
              {t("Connect")}
            </Button>
          ) : (
            <div className="flex items-center gap-2">
              <Button disabled={busy} onClick={() => void api.restartConnection()}>
                {t("Reconnect")}
              </Button>
              <Button disabled={busy} onClick={() => void setPaused(true)}>
                {t("Disconnect")}
              </Button>
            </div>
          )
        }
      >
        <dl className="grid grid-cols-[140px_1fr] gap-y-2 text-[13px]">
          <dt className="text-muted">{t("Status")}</dt>
          <dd className="flex items-center gap-2">
            <StatusDot on={snapshot.status.connected} />
            {snapshot.status.detail}
          </dd>
          <dt className="text-muted">{t("Transport")}</dt>
          <dd>{config.transport === "link" ? t("Deskmate integration") : "MQTT"}</dd>
          {snapshot.status.integration && (
            <>
              <dt className="text-muted">{t("Integration")}</dt>
              <dd className="mono">{snapshot.status.integration}</dd>
            </>
          )}
          <dt className="text-muted">{t("Endpoint")}</dt>
          <dd className="mono">
            {config.transport === "link"
              ? config.link_url
              : `${config.broker_host}:${config.broker_port}`}
          </dd>
          <dt className="text-muted">{t("Device")}</dt>
          <dd>{config.device_name}</dd>
          <dt className="text-muted">{t("Node ID")}</dt>
          <dd className="mono">{config.node_id}</dd>
          <dt className="text-muted">{t("Messages published")}</dt>
          <dd className="mono">{snapshot.published_count}</dd>
          <dt className="text-muted">{t("Interval")}</dt>
          <dd className="mono">{config.publish_interval_secs}s</dd>
        </dl>
      </Panel>

      <Panel title={t("In Home Assistant")}>
        <p className="text-[13px] text-muted leading-relaxed">
          {t("This computer is registered through ")}
          {config.transport === "link" ? t("the Deskmate integration") : t("MQTT discovery")}
          {t(" as device ")}
          <span className="mono text-ink">{config.device_name}</span>
          {t(". Find it under Settings > Devices & services > ")}
          {config.transport === "link" ? "Deskmate" : "MQTT"}
          {t(". Entity ids follow the device name, for example ")}
          <span className="mono text-ink">
            sensor.{config.device_name.toLowerCase().replace(/[^a-z0-9]+/g, "_")}_cpu_usage
          </span>
          .
        </p>
      </Panel>
    </>
  );
}
