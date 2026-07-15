import { useState } from "react";
import { api } from "../api";
import { Button, Field, StatusDot } from "../components";
import type { AppConfig, MqttTransport, StatusView } from "../types";

/** First run: the minimum fields needed to connect to the MQTT broker. */
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
  const [transport, setTransport] = useState<MqttTransport>(config.mqtt_transport);
  const [port, setPort] = useState(String(config.broker_port || 8883));
  const [caPath, setCaPath] = useState(config.mqtt_ca_path);
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
          broker_port: parseInt(port, 10) || (transport === "tls" ? 8883 : 1883),
          mqtt_transport: transport,
          mqtt_ca_path: caPath.trim(),
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
          <label className="block">
            <span className="microlabel">Transport</span>
            <select
              value={transport}
              onChange={(e) => {
                const next = e.target.value as MqttTransport;
                setTransport(next);
                if (next === "tls" && port === "1883") setPort("8883");
                if (next === "insecure" && port === "8883") setPort("1883");
              }}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="tls">TLS — verify certificate (recommended)</option>
              <option value="insecure">Plain MQTT — trusted LAN only</option>
            </select>
          </label>
          {transport === "insecure" && (
            <p className="text-[12px] border border-hairline-strong rounded p-2">
              Plain MQTT exposes credentials, sensor values and commands on the network.
            </p>
          )}
          <div className="grid grid-cols-[1fr_92px] gap-3">
            <Field label="Broker address" value={host} onChange={setHost} placeholder="192.168.1.10" />
            <Field label="Port" value={port} onChange={setPort} placeholder="1883" />
          </div>
          {transport === "tls" && (
            <Field
              label="Custom CA certificate (PEM path)"
              value={caPath}
              onChange={setCaPath}
              placeholder="Optional — Windows trust store is used by default"
            />
          )}
          <Field label="Username" value={username} onChange={setUsername} placeholder="dedicated Deskmate MQTT user" />
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
