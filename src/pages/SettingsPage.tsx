import { useEffect, useState } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { api } from "../api";
import { Button, Field, Panel, Toggle } from "../components";
import type { AppConfig, ClipboardMode, MqttTransport, TransportKind } from "../types";

export default function SettingsPage({
  config,
  hasPassword,
  hasLinkKey,
  onSaved,
}: {
  config: AppConfig;
  hasPassword: boolean;
  hasLinkKey: boolean;
  onSaved: () => Promise<void>;
}) {
  const [host, setHost] = useState(config.broker_host);
  const [transport, setTransport] = useState<TransportKind>(config.transport);
  const [hostRemote, setHostRemote] = useState(config.broker_host_remote);
  const [port, setPort] = useState(String(config.broker_port));
  const [mqttTransport, setMqttTransport] = useState<MqttTransport>(config.mqtt_transport);
  const [mqttCaPath, setMqttCaPath] = useState(config.mqtt_ca_path);
  const [username, setUsername] = useState(config.username);
  const [password, setPassword] = useState("");
  const [linkUrl, setLinkUrl] = useState(config.link_url);
  const [linkUrlRemote, setLinkUrlRemote] = useState(config.link_url_remote);
  const [linkKey, setLinkKey] = useState("");
  const [deviceName, setDeviceName] = useState(config.device_name);
  const [interval, setIntervalS] = useState(String(config.publish_interval_secs));
  const [launchHidden, setLaunchHidden] = useState(config.launch_hidden);
  const [allowInput, setAllowInput] = useState(config.allow_input);
  const [ttsEnabled, setTtsEnabled] = useState(config.tts_enabled);
  const [clipboardReadMode, setClipboardReadMode] = useState<ClipboardMode>(config.clipboard_read_mode);
  const [clipboardWriteMode, setClipboardWriteMode] = useState<ClipboardMode>(config.clipboard_write_mode);
  const [allowedOrigins, setAllowedOrigins] = useState(config.allowed_url_origins.join("\n"));
  const [branding, setBranding] = useState(config.toast_branding);
  const [autostart, setAutostart] = useState(false);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [haUrl, setHaUrl] = useState(config.ha_url);
  const [haUrlRemote, setHaUrlRemote] = useState(config.ha_url_remote);
  const [haToken, setHaToken] = useState("");
  const [haMsg, setHaMsg] = useState<string | null>(null);

  const saveHa = async (thenPing: boolean) => {
    setHaMsg(null);
    try {
      await api.setHaApi(haUrl, haUrlRemote, haToken || undefined);
      setHaToken("");
      if (thenPing) {
        const r = await api.haPing();
        setHaMsg(`Connected: ${r}`);
      } else {
        setHaMsg("Saved.");
      }
      await onSaved();
    } catch (e) {
      setHaMsg(String(e));
    }
  };

  useEffect(() => {
    isEnabled().then(setAutostart).catch(() => setAutostart(false));
  }, []);

  // opt-ins apply immediately (like sensors) - entities appear without Save
  const toggleFeature = async (
    flag: "allow_input" | "tts_enabled" | "toast_branding",
    v: boolean,
    setter: (b: boolean) => void,
  ) => {
    setter(v);
    try {
      await api.setFeatureFlag(flag, v);
    } catch (e) {
      setMsg(String(e));
    }
  };

  const save = async () => {
    setSaving(true);
    setMsg(null);
    try {
      await api.saveConfig(
        {
          ...config,
          transport,
          broker_host: host.trim(),
          broker_host_remote: hostRemote.trim(),
          broker_port: parseInt(port, 10) || (mqttTransport === "tls" ? 8883 : 1883),
          mqtt_transport: mqttTransport,
          mqtt_ca_path: mqttCaPath.trim(),
          username: username.trim(),
          link_url: linkUrl.trim(),
          link_url_remote: linkUrlRemote.trim(),
          device_name: deviceName.trim() || config.device_name,
          publish_interval_secs: Math.max(2, parseInt(interval, 10) || 15),
          launch_hidden: launchHidden,
          allow_input: allowInput,
          tts_enabled: ttsEnabled,
          clipboard_read_mode: clipboardReadMode,
          clipboard_write_mode: clipboardWriteMode,
          allowed_url_origins: allowedOrigins
            .split(/[\n,]+/)
            .map((origin) => origin.trim())
            .filter(Boolean),
          toast_branding: branding,
        },
        password || undefined,
        linkKey || undefined,
      );
      setPassword("");
      setLinkKey("");
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
      <Panel title="Home Assistant transport">
        <label className="block">
          <span className="microlabel">Active transport</span>
          <select
            value={transport}
            onChange={(event) => setTransport(event.target.value as TransportKind)}
            className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
          >
            <option value="mqtt">MQTT (default)</option>
            <option value="link">Deskmate Link</option>
          </select>
        </label>
      </Panel>

      {transport === "mqtt" && <Panel title="MQTT broker">
        <div className="space-y-3">
          <label className="block">
            <span className="microlabel">Transport</span>
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
              Plain MQTT exposes credentials, sensor values and commands to anyone able to observe this network.
            </p>
          )}
          <div className="grid grid-cols-[1fr_92px] gap-3">
            <Field label="Broker address (local)" value={host} onChange={setHost} />
            <Field label="Port" value={port} onChange={setPort} />
          </div>
          <Field
            label="Fallback address (outside home)"
            value={hostRemote}
            onChange={setHostRemote}
            hint="Optional. If the local address fails, Deskmate tries this one (e.g. a Tailscale IP or public host). Leave empty to use just the local address."
          />
          {mqttTransport === "tls" && (
            <Field
              label="Custom CA certificate (PEM path)"
              value={mqttCaPath}
              onChange={setMqttCaPath}
              placeholder="Leave empty to use Windows trusted certificates"
              hint="For a private/self-signed broker CA. Certificate verification cannot be disabled in TLS mode."
            />
          )}
          <Field label="Username" value={username} onChange={setUsername} />
          <Field
            label="Password"
            value={password}
            onChange={setPassword}
            type="password"
            placeholder={hasPassword ? "unchanged (stored in Credential Manager)" : "none"}
          />
        </div>
      </Panel>}

      {transport === "link" && <Panel title="Deskmate Link">
        <div className="space-y-3">
          <Field
            label="Home Assistant WebSocket URL (local)"
            value={linkUrl}
            onChange={setLinkUrl}
            placeholder="ws://homeassistant.local:8123"
            hint="The /api/deskmate_link/ws path is added automatically."
          />
          <Field
            label="Fallback WebSocket URL"
            value={linkUrlRemote}
            onChange={setLinkUrlRemote}
            placeholder="wss://ha.example.com"
          />
          <Field
            label="Pairing key"
            value={linkKey}
            onChange={setLinkKey}
            type="password"
            placeholder={hasLinkKey ? "unchanged (stored in Credential Manager)" : "32-byte base64 key from Home Assistant"}
          />
          <p className="text-[12px] text-muted leading-relaxed">
            Link encrypts application frames end to end. MQTT settings remain saved and can be selected again at any time.
          </p>
        </div>
      </Panel>}

      <Panel title="Clipboard security">
        <p className="text-[12px] text-muted mb-3 leading-relaxed">
          Clipboard read and write permissions are independent. Both are blocked while Windows is locked. Confirmation mode asks only when the clipboard value changes (read) or for every incoming write.
        </p>
        <div className="grid grid-cols-2 gap-3">
          <label className="block">
            <span className="microlabel">Publish clipboard to HA</span>
            <select
              value={clipboardReadMode}
              onChange={(e) => setClipboardReadMode(e.target.value as ClipboardMode)}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="off">Off</option>
              <option value="confirm">Confirm each changed value</option>
              <option value="automatic">Automatic periodic publication</option>
            </select>
          </label>
          <label className="block">
            <span className="microlabel">Allow clipboard writes from HA</span>
            <select
              value={clipboardWriteMode}
              onChange={(e) => setClipboardWriteMode(e.target.value as ClipboardMode)}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="off">Off</option>
              <option value="confirm">Confirm every write</option>
              <option value="automatic">Automatic</option>
            </select>
          </label>
        </div>
        <p className="mt-2 text-[12px] text-muted">
          Published clipboard values can remain in Home Assistant history even after Deskmate stops publishing them.
        </p>
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

      <Panel title="Home Assistant API (hotkeys, widgets, tray)">
        <p className="text-[12px] text-muted mb-2 leading-relaxed">
          Optional direct channel to Home Assistant, used by hotkeys, the widget
          panel and tray quick actions. Create a long-lived access token in HA
          (your profile → Security → Long-lived access tokens). The token is
          stored in Windows Credential Manager, never in a file.
        </p>
        <div className="space-y-3">
          <Field
            label="HA URL (local)"
            value={haUrl}
            onChange={setHaUrl}
            placeholder="http://192.168.1.10:8123"
          />
          <Field
            label="HA URL fallback (outside home)"
            value={haUrlRemote}
            onChange={setHaUrlRemote}
            placeholder="https://ha.example.com — optional, HTTPS required"
          />
          <Field
            label="Long-lived access token"
            value={haToken}
            onChange={setHaToken}
            type="password"
            placeholder="paste token (kept in Credential Manager)"
            hint='Leave empty to keep the current token. Enter "-" to remove it.'
          />
          <div className="flex items-center gap-3">
            <Button kind="primary" onClick={() => void saveHa(false)}>
              Save
            </Button>
            <Button onClick={() => void saveHa(true)}>Save &amp; test</Button>
            {haMsg && <span className="text-[12px] text-muted">{haMsg}</span>}
          </div>
        </div>
      </Panel>

      <Panel title="Remote input (opt-in)">
        <p className="text-[12px] text-muted mb-2 leading-relaxed">
          These let Home Assistant act on this computer. Off by default and applied
          immediately (the entities appear in HA right away). Anyone who can reach
          your broker can trigger them, so enable only on a trusted network.
        </p>
        <div className="flex items-center justify-between h-9">
          <div>
            <span className="text-[13px]">Type text &amp; presentation control</span>
            <p className="text-[12px] text-muted">HA sends keystrokes to the active window (text, slide next/prev).</p>
          </div>
          <Toggle on={allowInput} onChange={(v) => void toggleFeature("allow_input", v, setAllowInput)} />
        </div>
        <div className="flex items-center justify-between h-9 mt-2">
          <div>
            <span className="text-[13px]">Text-to-speech</span>
            <p className="text-[12px] text-muted">HA sends text and this PC speaks it aloud.</p>
          </div>
          <Toggle on={ttsEnabled} onChange={(v) => void toggleFeature("tts_enabled", v, setTtsEnabled)} />
        </div>
        <label className="block mt-3">
          <span className="microlabel">Allowed URL origins</span>
          <textarea
            value={allowedOrigins}
            onChange={(e) => setAllowedOrigins(e.target.value)}
            rows={3}
            placeholder={"https://example.com\nhttp://homeassistant.local:8123"}
            className="mt-1 w-full px-3 py-2 bg-panel border border-hairline-strong rounded text-ink mono text-[13px] placeholder:text-faint focus-visible:border-ink"
          />
          <span className="block mt-1 text-[12px] text-muted">
            Exact scheme + host + port only. Applies to open_url and notification images. Configured HA API origins are allowed automatically.
          </span>
        </label>
      </Panel>

      <Panel title="Notifications">
        <div className="flex items-center justify-between h-9">
          <div>
            <span className="text-[13px]">Branded toasts (show as “HomeOS”)</span>
            <p className="text-[12px] text-muted">
              Off = toasts show as “Windows PowerShell” but always render. On = a Start
              Menu shortcut lets them show as “HomeOS”. If toasts stop appearing, turn this off.
            </p>
          </div>
          <Toggle on={branding} onChange={(v) => void toggleFeature("toast_branding", v, setBranding)} />
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
