import { useEffect, useState } from "react";
import { api } from "../api";
import { parsePairingCode } from "../pairing";
import { applyTheme } from "../theme";
import CpuTemperaturePanel from "./CpuTemperaturePanel";
import { Button, Field, Panel, Toggle } from "../components";
import type { AppConfig, MqttTransport, TransportKind, ThemeMode } from "../types";

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
  const onPairingInput = (value: string) => {
    const code = parsePairingCode(value);
    setPairedNode(code?.node ?? null);
    setPairedCascade(code?.cascadeKey ?? null);
    if (!code) {
      setLinkKey(value);
      return;
    }
    setLinkKey(code.key);
    if (code.url) setLinkUrl(code.url);
    setLinkUrlRemote(code.urlRemote ?? "");
  };
  const [pairedNode, setPairedNode] = useState<string | null>(null);
  const [pairedCascade, setPairedCascade] = useState<string | null>(null);
  const [deviceName, setDeviceName] = useState(config.device_name);
  const [interval, setIntervalS] = useState(String(config.publish_interval_secs));
  const [launchHidden, setLaunchHidden] = useState(config.launch_hidden);
  const [theme, setTheme] = useState<ThemeMode>(config.theme ?? "system");
  const [dashboardPath, setDashboardPath] = useState(config.dashboard_path ?? "");
  const [dashboardOnTop, setDashboardOnTop] = useState(config.dashboard_on_top ?? false);
  const [dashboardMsg, setDashboardMsg] = useState<string | null>(null);
  const [allowInput, setAllowInput] = useState(config.allow_input);
  const [ttsEnabled, setTtsEnabled] = useState(config.tts_enabled);
  const [allowedOrigins, setAllowedOrigins] = useState(config.allowed_url_origins.join("\n"));
  const [branding, setBranding] = useState(config.toast_branding);
  const [updateCheck, setUpdateCheck] = useState(config.update_check ?? true);
  const [updateMsg, setUpdateMsg] = useState<string | null>(null);
  const checkUpdates = async () => {
    setUpdateMsg("Checking...");
    try {
      const found = await api.checkUpdatesNow();
      setUpdateMsg(found ? `Deskmate ${found.version} is available.` : "You have the latest version.");
    } catch (e) {
      setUpdateMsg(`Could not check: ${String(e)}`);
    }
  };
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
    api.getStartWithWindows().then(setAutostart).catch(() => setAutostart(false));
  }, []);

  // opt-ins apply immediately (like sensors) - entities appear without Save
  const toggleFeature = async (
    flag: "allow_input" | "tts_enabled" | "toast_branding" | "update_check",
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
          // A code for another Windows account joins the computer it was made for.
          node_id: pairedNode ?? config.node_id,
          link_cascade: pairedCascade ? true : config.link_cascade,
          device_name: deviceName.trim() || config.device_name,
          publish_interval_secs: Math.max(2, parseInt(interval, 10) || 15),
          launch_hidden: launchHidden,
          allow_input: allowInput,
          tts_enabled: ttsEnabled,
          allowed_url_origins: allowedOrigins
            .split(/[\n,]+/)
            .map((origin) => origin.trim())
            .filter(Boolean),
          toast_branding: branding,
          update_check: updateCheck,
          theme,
          dashboard_path: dashboardPath.trim(),
          dashboard_on_top: dashboardOnTop,
        },
        password || undefined,
        linkKey || undefined,
        pairedCascade ?? undefined,
      );
      setPassword("");
      setLinkKey("");
      setPairedNode(null);
      setPairedCascade(null);
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
      // Through the app, so the choice is remembered and survives updates (#7).
      await api.setStartWithWindows(v);
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
            <option value="link">Deskmate integration (recommended)</option>
            <option value="mqtt">MQTT broker</option>
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

      {transport === "link" && <Panel title="Deskmate integration">
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
            label="Pairing code or key"
            value={linkKey}
            onChange={onPairingInput}
            type="password"
            placeholder={hasLinkKey ? "unchanged (stored in Credential Manager)" : "DMP1... code from Home Assistant"}
            hint="Pasting the pairing code fills in the addresses as well."
          />
          {pairedNode && pairedNode !== config.node_id && (
            <p className="text-[12px] text-muted leading-relaxed">
              This code is for a computer that is already paired. After saving, this Windows
              account joins it as <span className="mono text-ink">{pairedNode}</span>.
            </p>
          )}
          <p className="text-[12px] text-muted leading-relaxed">
            This computer identifies itself as <span className="mono text-ink">{config.node_id}</span>.
            In Home Assistant add the Deskmate integration, paste the pairing code here and save.
            The entry binds itself to this computer on the first successful connection. Reinstalled
            Deskmate, or another Windows account on this computer: in Home Assistant open the
            Deskmate entry, choose Reconfigure, then Show the pairing code.
          </p>
          <p className="text-[12px] text-muted leading-relaxed">
            The connection is encrypted end to end (Deskmate Link protocol). MQTT settings remain saved and can be selected again at any time.
          </p>
        </div>
      </Panel>}

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
          <label className="block">
            <span className="microlabel">Appearance</span>
            <select
              value={theme}
              onChange={(e) => {
                const next = e.target.value as ThemeMode;
                setTheme(next);
                applyTheme(next);
              }}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="system">Follow Windows</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
            <span className="block mt-1 text-[12px] text-muted">Applied right away; kept after Save.</span>
          </label>
        </div>
      </Panel>

      <CpuTemperaturePanel />

      <Panel title="Home Assistant dashboard">
        <p className="text-[12px] text-muted mb-2 leading-relaxed">
          Opens a Home Assistant dashboard in a Deskmate window, from the tray menu, a hotkey or a
          tray quick action. Log in to Home Assistant there once; the session is kept. The page
          has no access to Deskmate itself.
        </p>
        <div className="space-y-3">
          <Field
            label="Dashboard path"
            value={dashboardPath}
            onChange={setDashboardPath}
            placeholder="empty: your default dashboard, or e.g. lovelace/0"
          />
          <div className="flex items-center justify-between h-9">
            <span className="text-[13px]">Keep the dashboard window on top</span>
            <Toggle on={dashboardOnTop} onChange={setDashboardOnTop} />
          </div>
          <div className="flex items-center gap-3">
            <Button
              onClick={async () => {
                setDashboardMsg(null);
                try {
                  await api.openDashboard(dashboardPath.trim());
                } catch (e) {
                  setDashboardMsg(String(e));
                }
              }}
            >
              Open now
            </Button>
            <span className="text-[12px] text-muted">{dashboardMsg ?? "Path and on-top are kept after Save."}</span>
          </div>
        </div>
      </Panel>

      <Panel title="Home Assistant API (hotkeys, widgets, tray)">
        <p className="text-[12px] text-muted mb-2 leading-relaxed">
          Optional, and separate from the connection above: only hotkeys, the widget
          panel and tray quick actions use it. Create a long-lived access token in HA
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
            <Button onClick={() => void saveHa(true)}>Save &amp; test API</Button>
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
            <span className="text-[13px]">Branded toasts (show as “Deskmate”)</span>
            <p className="text-[12px] text-muted">
              On = notifications show as “Deskmate”. Off = they show as “Windows PowerShell”.
              No PowerShell is started either way. If toasts stop appearing, turn this off.
            </p>
          </div>
          <Toggle on={branding} onChange={(v) => void toggleFeature("toast_branding", v, setBranding)} />
        </div>
      </Panel>

      <Panel title="Updates">
        <div className="flex items-center justify-between h-9">
          <div>
            <span className="text-[13px]">Check for updates once a day</span>
            <p className="text-[12px] text-muted">
              Asks GitHub for the latest release. Nothing is downloaded or installed: a new
              version shows up in the tray menu and on the Status page.
            </p>
          </div>
          <Toggle on={updateCheck} onChange={(v) => void toggleFeature("update_check", v, setUpdateCheck)} />
        </div>
        <div className="mt-3 flex items-center gap-3">
          <Button onClick={() => void checkUpdates()}>Check now</Button>
          {updateMsg && <span className="text-[13px] text-muted">{updateMsg}</span>}
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
