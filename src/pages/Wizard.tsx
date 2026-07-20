import { useState } from "react";
import { api } from "../api";
import { Button, Field, StatusDot } from "../components";
import type { AppConfig, MqttTransport, StatusView, TransportKind } from "../types";

/** First run: the minimum fields needed to connect to the MQTT broker. */
export default function Wizard({
  config,
  status,
  hasLinkKey,
  onDone,
}: {
  config: AppConfig;
  status: StatusView | null;
  hasLinkKey: boolean;
  onDone: () => Promise<void>;
}) {
  const [host, setHost] = useState(config.broker_host);
  const [selectedTransport, setSelectedTransport] = useState<TransportKind>(config.transport);
  const [mqttTransport, setMqttTransport] = useState<MqttTransport>(config.mqtt_transport);
  const [port, setPort] = useState(String(config.broker_port || 8883));
  const [caPath, setCaPath] = useState(config.mqtt_ca_path);
  const [username, setUsername] = useState(config.username);
  const [password, setPassword] = useState("");
  const [linkUrl, setLinkUrl] = useState(config.link_url);
  const [linkKey, setLinkKey] = useState("");
  const [deviceName, setDeviceName] = useState(config.device_name);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const connect = async () => {
    setError(null);
    if (selectedTransport === "mqtt" ? !host.trim() : !linkUrl.trim()) {
      setError(selectedTransport === "mqtt" ? "Broker address is required." : "Link URL is required.");
      return;
    }
    if (selectedTransport === "link" && !linkKey && !hasLinkKey) {
      setError("Link pairing key is required.");
      return;
    }
    setSaving(true);
    try {
      await api.saveConfig(
        {
          ...config,
          transport: selectedTransport,
          broker_host: host.trim(),
          broker_port: parseInt(port, 10) || (mqttTransport === "tls" ? 8883 : 1883),
          mqtt_transport: mqttTransport,
          mqtt_ca_path: caPath.trim(),
          username: username.trim(),
          link_url: linkUrl.trim(),
          device_name: deviceName.trim() || config.device_name,
          configured: true,
        },
        password || undefined,
        linkKey || undefined,
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
          Choose MQTT or the encrypted Deskmate Link integration. MQTT remains the default.
        </p>

        <div className="space-y-3">
          <label className="block">
            <span className="microlabel">Transport</span>
            <select
              value={selectedTransport}
              onChange={(e) => setSelectedTransport(e.target.value as TransportKind)}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="mqtt">MQTT (default)</option>
              <option value="link">Deskmate Link</option>
            </select>
          </label>
          {selectedTransport === "mqtt" && <>
          <label className="block">
            <span className="microlabel">MQTT security</span>
            <select
              value={mqttTransport}
              onChange={(e) => {
                const next = e.target.value as MqttTransport;
                setMqttTransport(next);
                if (next === "tls" && port === "1883") setPort("8883");
                if (next === "insecure" && port === "8883") setPort("1883");
              }}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="tls">TLS — verify certificate (recommended)</option>
              <option value="insecure">Plain MQTT — trusted LAN only</option>
            </select>
          </label>
          {mqttTransport === "insecure" && (
            <p className="text-[12px] border border-hairline-strong rounded p-2">
              Plain MQTT exposes credentials, sensor values and commands on the network.
            </p>
          )}
          <div className="grid grid-cols-[1fr_92px] gap-3">
            <Field label="Broker address" value={host} onChange={setHost} placeholder="192.168.1.10" />
            <Field label="Port" value={port} onChange={setPort} placeholder="1883" />
          </div>
          {mqttTransport === "tls" && (
            <Field
              label="Custom CA certificate (PEM path)"
              value={caPath}
              onChange={setCaPath}
              placeholder="Optional — Windows trust store is used by default"
            />
          )}
          <Field label="Username" value={username} onChange={setUsername} placeholder="dedicated Deskmate MQTT user" />
          <Field label="Password" value={password} onChange={setPassword} type="password" placeholder="stored in Windows Credential Manager" />
          </>}
          {selectedTransport === "link" && <>
            <Field label="Home Assistant WebSocket URL" value={linkUrl} onChange={setLinkUrl} placeholder="ws://homeassistant.local:8123" />
            <Field
              label="Pairing key"
              value={linkKey}
              onChange={setLinkKey}
              type="password"
              placeholder={hasLinkKey ? "unchanged (stored in Credential Manager)" : "base64 key from Home Assistant"}
            />
          </>}
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
