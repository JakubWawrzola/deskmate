import { useState } from "react";
import { api } from "../api";
import { Button, Field, Panel, Toggle } from "../components";
import type { AppConfig } from "../types";

/**
 * Opt-in hardening that most people will never need. Kept off the Settings page
 * on purpose: everything here can break a working connection if only one side
 * of it is changed.
 */
export default function GeekyPage({
  config,
  hasCascadeKey,
  onSaved,
}: {
  config: AppConfig;
  hasCascadeKey: boolean;
  onSaved: () => Promise<void>;
}) {
  const [cascade, setCascade] = useState(config.link_cascade);
  const [cascadeKey, setCascadeKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const linkActive = config.transport === "link";

  async function save() {
    setBusy(true);
    setResult(null);
    setError(null);
    try {
      await api.saveConfig(
        { ...config, link_cascade: cascade },
        undefined,
        undefined,
        cascadeKey.trim() === "" ? undefined : cascadeKey.trim(),
      );
      setCascadeKey("");
      setResult(
        cascade
          ? "Cascade enabled. The connection stays down until Home Assistant has the same key."
          : "Cascade disabled. Turn it off in Home Assistant too.",
      );
      await onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <Panel title="Cascade encryption">
        <div className="space-y-3">
          <p className="text-[13px] leading-relaxed">
            Deskmate Link already encrypts every frame with AES-256-GCM under a key derived
            per session. Cascade adds a second, independent layer on top:
            ChaCha20-Poly1305 with its own key, its own HKDF labels and its own
            authentication tag. The layers share no key material, so breaking one cipher
            does not reveal the traffic.
          </p>
          <p className="text-[12px] text-muted leading-relaxed">
            This is defence in depth against a future weakness in one algorithm, not a fix
            for anything known to be broken today. It costs one extra encryption pass per
            frame, which is unmeasurable at Deskmate's message rate.
          </p>

          {!linkActive && (
            <p className="text-[12px] border border-hairline-strong rounded p-2 leading-relaxed">
              Cascade applies to the Deskmate Link transport. This computer currently uses
              MQTT, so the setting has no effect until you switch transports.
            </p>
          )}

          <div className="flex items-center justify-between border border-hairline rounded-md px-3 py-2">
            <span className="text-[13px]">Encrypt every frame twice</span>
            <Toggle on={cascade} onChange={setCascade} />
          </div>

          <Field
            label="Cascade key"
            value={cascadeKey}
            onChange={setCascadeKey}
            type="password"
            placeholder={
              hasCascadeKey
                ? "unchanged (stored in Credential Manager)"
                : "32-byte base64 key from Home Assistant"
            }
            hint="Home Assistant generates it: open the Deskmate Link entry, choose Reconfigure, then Enable cascade encryption."
          />

          <p className="text-[12px] text-muted leading-relaxed">
            Both ends must agree. Home Assistant refuses a handshake where one side asks
            for cascade and the other does not, rather than quietly falling back to the
            weaker single layer.
          </p>

          <div className="flex items-center gap-3">
            <Button onClick={() => void save()} disabled={busy}>
              {busy ? "Saving..." : "Save & reconnect"}
            </Button>
            {result && <span className="text-[12px] text-muted">{result}</span>}
            {error && <span className="text-[12px] text-red-600">{error}</span>}
          </div>
        </div>
      </Panel>

      <Panel title="What is actually on the wire">
        <dl className="grid grid-cols-[190px_1fr] gap-y-2 text-[13px]">
          <dt className="text-muted">Handshake</dt>
          <dd>HMAC-SHA256 over a client nonce, a server nonce and a timestamp</dd>
          <dt className="text-muted">Session keys</dt>
          <dd>HKDF-SHA256, fresh per connection, separate per direction</dd>
          <dt className="text-muted">Frames</dt>
          <dd className="mono">
            {cascade ? "ChaCha20-Poly1305(AES-256-GCM(json))" : "AES-256-GCM(json)"}
          </dd>
          <dt className="text-muted">Replay protection</dt>
          <dd>Strictly increasing counter per direction, single-use handshake nonce</dd>
          <dt className="text-muted">Key storage</dt>
          <dd>Windows Credential Manager, never in config.json</dd>
        </dl>
        <p className="text-[12px] text-muted leading-relaxed mt-3">
          Application-layer encryption is independent of the transport, so this holds on a
          plain <span className="mono">ws://</span> connection inside your own network just
          as it does through a tunnel.
        </p>
      </Panel>
    </>
  );
}
