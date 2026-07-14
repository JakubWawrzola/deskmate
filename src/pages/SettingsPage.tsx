import { useEffect, useState } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { api } from "../api";
import { Button, Field, Panel, Toggle } from "../components";
import type { AppConfig } from "../types";

export default function SettingsPage({
  config,
  hasPassword,
  onSaved,
}: {
  config: AppConfig;
  hasPassword: boolean;
  onSaved: () => Promise<void>;
}) {
  const [host, setHost] = useState(config.broker_host);
  const [port, setPort] = useState(String(config.broker_port));
  const [username, setUsername] = useState(config.username);
  const [password, setPassword] = useState("");
  const [deviceName, setDeviceName] = useState(config.device_name);
  const [interval, setIntervalS] = useState(String(config.publish_interval_secs));
  const [launchHidden, setLaunchHidden] = useState(config.launch_hidden);
  const [allowInput, setAllowInput] = useState(config.allow_input);
  const [ttsEnabled, setTtsEnabled] = useState(config.tts_enabled);
  const [autostart, setAutostart] = useState(false);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    isEnabled().then(setAutostart).catch(() => setAutostart(false));
  }, []);

  const save = async () => {
    setSaving(true);
    setMsg(null);
    try {
      await api.saveConfig(
        {
          ...config,
          broker_host: host.trim(),
          broker_port: parseInt(port, 10) || 1883,
          username: username.trim(),
          device_name: deviceName.trim() || config.device_name,
          publish_interval_secs: Math.max(2, parseInt(interval, 10) || 15),
          launch_hidden: launchHidden,
          allow_input: allowInput,
          tts_enabled: ttsEnabled,
        },
        password || undefined,
      );
      setPassword("");
      setMsg("Saved. Reconnecting...");
      await onSaved();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setSaving(false);
    }
  };

  const toggleAutostart = async (v: boolean) => {
    try {
      if (v) await enable();
      else await disable();
      setAutostart(v);
    } catch (e) {
      setMsg(String(e));
    }
  };

  return (
    <>
      <Panel title="MQTT broker">
        <div className="space-y-3">
          <div className="grid grid-cols-[1fr_92px] gap-3">
            <Field label="Broker address" value={host} onChange={setHost} />
            <Field label="Port" value={port} onChange={setPort} />
          </div>
          <Field label="Username" value={username} onChange={setUsername} />
          <Field
            label="Password"
            value={password}
            onChange={setPassword}
            type="password"
            placeholder={hasPassword ? "unchanged (stored in Credential Manager)" : "none"}
          />
        </div>
      </Panel>

      <Panel title="Device">
        <div className="space-y-3">
          <Field label="Device name" value={deviceName} onChange={setDeviceName} hint="Shown in Home Assistant. Entity ids keep the original node id." />
          <Field label="Publish interval (seconds)" value={interval} onChange={setIntervalS} />
          <div className="flex items-center justify-between h-9">
            <span className="text-[13px]">Start with Windows</span>
            <Toggle on={autostart} onChange={(v) => void toggleAutostart(v)} />
          </div>
          <div className="flex items-center justify-between h-9">
            <span className="text-[13px]">Start minimized to tray</span>
            <Toggle on={launchHidden} onChange={setLaunchHidden} />
          </div>
        </div>
      </Panel>

      <Panel title="Remote input (opt-in)">
        <p className="text-[12px] text-muted mb-2 leading-relaxed">
          These let Home Assistant act on this computer. Off by default. Anyone
          who can reach your broker can trigger them, so enable only on a trusted
          network.
        </p>
        <div className="flex items-center justify-between h-9">
          <div>
            <span className="text-[13px]">Type text &amp; presentation control</span>
            <p className="text-[12px] text-muted">HA sends keystrokes to the active window (text, slide next/prev).</p>
          </div>
          <Toggle on={allowInput} onChange={setAllowInput} />
        </div>
        <div className="flex items-center justify-between h-9 mt-2">
          <div>
            <span className="text-[13px]">Text-to-speech</span>
            <p className="text-[12px] text-muted">HA sends text and this PC speaks it aloud.</p>
          </div>
          <Toggle on={ttsEnabled} onChange={setTtsEnabled} />
        </div>
      </Panel>

      <div className="flex items-center gap-3">
        <Button kind="primary" onClick={() => void save()} disabled={saving}>
          {saving ? "Saving..." : "Save & reconnect"}
        </Button>
        {msg && <span className="text-[13px] text-muted">{msg}</span>}
      </div>
    </>
  );
}
