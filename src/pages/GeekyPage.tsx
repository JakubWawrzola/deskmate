import { useState } from "react";
import { api } from "../api";
import { Button, Field, Panel, Toggle } from "../components";
import { t } from "../i18n";
import { isLinux, usePlatform } from "../platform";
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
  const platform = usePlatform();
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
          ? t("Cascade enabled. The connection stays down until Home Assistant has the same key.")
          : t("Cascade disabled. Turn it off in Home Assistant too."),
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
      <Panel title={t("Cascade encryption")}>
        <div className="space-y-3">
          <p className="text-[13px] leading-relaxed">
            {t(
              "Deskmate Link already encrypts every frame with AES-256-GCM under a key derived per session. Cascade adds a second layer on top: ChaCha20-Poly1305 with its own key, its own HKDF labels and its own authentication tag.",
            )}
          </p>
          <p className="text-[12px] text-muted leading-relaxed">
            {t(
              "It only helps if one of the two ciphers is ever broken. Both keys are stored in the same places (Credential Manager here, the integration entry in Home Assistant), so it does not help if a key is stolen. Protection against a stolen key comes from the X25519 exchange below, which is always on. The extra pass per frame costs nothing you could measure at Deskmate's message rate.",
            )}
          </p>

          {!linkActive && (
            <p className="text-[12px] border border-hairline-strong rounded p-2 leading-relaxed">
              {t(
                "Cascade applies to the Deskmate Link transport. This computer currently uses MQTT, so the setting has no effect until you switch transports.",
              )}
            </p>
          )}

          <div className="flex items-center justify-between border border-hairline rounded-md px-3 py-2">
            <span className="text-[13px]">{t("Encrypt every frame twice")}</span>
            <Toggle on={cascade} onChange={setCascade} />
          </div>

          <Field
            label={t("Cascade key")}
            value={cascadeKey}
            onChange={setCascadeKey}
            type="password"
            placeholder={
              hasCascadeKey
                ? t("unchanged (stored in Credential Manager)")
                : t("32-byte base64 key from Home Assistant")
            }
            hint={t(
              "Home Assistant generates it: open the Deskmate integration entry, choose Reconfigure, then Enable cascade encryption.",
            )}
          />

          <p className="text-[12px] text-muted leading-relaxed">
            {t(
              "Both ends must agree. Home Assistant refuses a handshake where one side asks for cascade and the other does not, rather than quietly falling back to the weaker single layer.",
            )}
          </p>

          <div className="flex items-center gap-3">
            <Button onClick={() => void save()} disabled={busy}>
              {busy ? t("Saving...") : t("Save & reconnect")}
            </Button>
            {result && <span className="text-[12px] text-muted">{result}</span>}
            {error && <span className="text-[12px] text-red-600">{error}</span>}
          </div>
        </div>
      </Panel>

      <Panel title={t("What is actually on the wire")}>
        <dl className="grid grid-cols-[190px_1fr] gap-y-2 text-[13px]">
          <dt className="text-muted">{t("Handshake")}</dt>
          <dd>
            {t(
              "Protocol v2. HMAC-SHA256 with the pairing key over the whole transcript: version, node, nonces, timestamps, both X25519 public keys and the cascade flag",
            )}
          </dd>
          <dt className="text-muted">{t("Session keys")}</dt>
          <dd>
            {t(
              "HKDF-SHA256 over an ephemeral X25519 secret mixed with the pairing key. Fresh per connection, separate per direction, wiped from memory afterwards",
            )}
          </dd>
          <dt className="text-muted">{t("Forward secrecy")}</dt>
          <dd>{t("Yes. A pairing key leaked later does not decrypt recorded sessions")}</dd>
          <dt className="text-muted">{t("Frames")}</dt>
          <dd className="mono">
            {cascade ? "ChaCha20-Poly1305(AES-256-GCM(json))" : "AES-256-GCM(json)"}
          </dd>
          <dt className="text-muted">{t("Replay protection")}</dt>
          <dd>{t("Strictly increasing counter per direction, single-use handshake nonce")}</dd>
          <dt className="text-muted">{t("Key storage")}</dt>
          <dd>
            {isLinux(platform)
              ? t("Your keyring (Secret Service)")
              : t("Windows Credential Manager")}
            {t(", never in config.json")}
          </dd>
        </dl>
        <p className="text-[12px] text-muted leading-relaxed mt-3">
          {t(
            "Application-layer encryption is independent of the transport, so this holds on a plain ",
          )}
          <span className="mono">ws://</span>
          {t(
            " connection inside your own network just as it does through a tunnel. Deskmate accepts ",
          )}
          <span className="mono">ws://</span>
          {t(" only for LAN, .local and Tailscale addresses; anything else needs ")}
          <span className="mono">{"wss://"}</span>.
        </p>
      </Panel>
    </>
  );
}
