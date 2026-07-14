import { useState } from "react";
import { api } from "../api";
import { Button, Field, StatusDot } from "../components";
import type { AppConfig, StatusView } from "../types";

/** Pierwsze uruchomienie: minimum pol do polaczenia z brokerem MQTT. */
export default function Wizard({
  config,
  status,
  onDone,
}: {
  config: AppConfig;
  status: StatusView | null;
  onDone: () => Promise<void>;
}) {
  const [host, setHost] = useState(config.broker_host);
  const [port, setPort] = useState(String(config.broker_port || 1883));
  const [username, setUsername] = useState(config.username);
  const [password, setPassword] = useState("");
  const [deviceName, setDeviceName] = useState(config.device_name);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const connect = async () => {
    setError(null);
    if (!host.trim()) {
      setError("Broker address is required.");
      return;
    }
    setSaving(true);
    try {
      await api.saveConfig(
        {
          ...config,
          broker_host: host.trim(),
          broker_port: parseInt(port, 10) || 1883,
          username: username.trim(),
          device_name: deviceName.trim() || config.device_name,
          configured: true,
        },
        password || undefined,
      );
      await onDone();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="h-full grid place-items-center">
      <div className="w-[420px] bg-panel border border-hairline rounded-md p-6">
        <p className="font-semibold tracking-[0.14em] text-[13px] mb-1">DESKMATE</p>
        <h1 className="text-lg font-semibold mb-1">Connect to Home Assistant</h1>
        <p className="text-[13px] text-muted mb-5 leading-relaxed">
          Deskmate talks to Home Assistant over MQTT. Point it at your broker
          (for Home Assistant OS that is the Mosquitto add-on) and the device
          will appear in HA automatically.
        </p>

        <div className="space-y-3">
          <div className="grid grid-cols-[1fr_92px] gap-3">
            <Field label="Broker address" value={host} onChange={setHost} placeholder="192.168.1.10" />
            <Field label="Port" value={port} onChange={setPort} placeholder="1883" />
          </div>
          <Field label="Username" value={username} onChange={setUsername} placeholder="mqtt user (optional)" />
          <Field label="Password" value={password} onChange={setPassword} type="password" placeholder="stored in Windows Credential Manager" />
          <Field
            label="Device name"
            value={deviceName}
            onChange={setDeviceName}
            hint="How this computer shows up in Home Assistant."
          />
        </div>

        {error && <p className="mt-3 text-[13px] text-ink border border-hairline-strong rounded px-3 py-2">{error}</p>}

        <div className="mt-5 flex items-center justify-between">
          <span className="flex items-center gap-2 text-[12px] text-muted">
            <StatusDot on={status?.connected ?? false} />
            {status?.detail ?? "Not connected"}
          </span>
          <Button kind="primary" onClick={connect} disabled={saving}>
            {saving ? "Connecting..." : "Save & connect"}
          </Button>
        </div>
      </div>
    </div>
  );
}
