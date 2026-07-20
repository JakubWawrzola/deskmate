import { useState } from "react";
import { api } from "../api";
import { Button, EmptyState, Panel } from "../components";
import type { AppConfig, Hotkey, Snapshot, TrayAction } from "../types";
import { emptyAction } from "../types";
import ActionEditor from "./ActionEditor";

const slug = (s: string) =>
  s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");

/** Global keyboard shortcuts + quick actions in the tray menu. */
export default function HotkeysPage({
  snapshot,
  config,
  onChanged,
}: {
  snapshot: Snapshot;
  config: AppConfig;
  onChanged: () => Promise<void>;
}) {
  const [hotkeys, setHotkeys] = useState<Hotkey[]>(config.hotkeys);
  const [trayActions, setTrayActions] = useState<TrayAction[]>(config.tray_actions);
  const [msg, setMsg] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const saveHotkeys = async (next: Hotkey[]) => {
    setSaving(true);
    setMsg(null);
    try {
      const errors = await api.updateHotkeys(next);
      setHotkeys(next);
      setMsg(errors.length ? `Registered with issues: ${errors.join("; ")}` : "Hotkeys registered.");
      await onChanged();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setSaving(false);
    }
  };

  const saveTray = async (next: TrayAction[]) => {
    setSaving(true);
    setMsg(null);
    try {
      await api.updateTrayActions(next);
      setTrayActions(next);
      setMsg("Tray menu updated.");
      await onChanged();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setSaving(false);
    }
  };

  const input =
    "w-full h-8 px-2 bg-panel border border-hairline-strong rounded text-ink mono text-[12px] placeholder:text-faint focus-visible:border-ink";

  return (
    <>
      <Panel
        title="Global hotkeys"
        action={
          <Button
            onClick={() =>
              setHotkeys([
                ...hotkeys,
                { id: `hk_${hotkeys.length + 1}`, name: "", accelerator: "", action: emptyAction() },
              ])
            }
          >
            Add hotkey
          </Button>
        }
      >
        <p className="text-[12px] text-muted mb-3 leading-relaxed">
          System-wide shortcuts that work while Deskmate sits in the tray — control
          Home Assistant without a Stream Deck. Format: <span className="mono text-ink">Ctrl+Alt+L</span>,{" "}
          <span className="mono text-ink">Ctrl+Shift+F5</span>. HA actions need the HA API
          configured in Settings; the HA event trigger works without it over MQTT or Link.
        </p>
        {hotkeys.length === 0 && <EmptyState text="No hotkeys yet." />}
        <div className="space-y-4">
          {hotkeys.map((h, i) => (
            <div key={i} className="border border-hairline rounded p-3 space-y-2">
              <div className="grid grid-cols-3 gap-2">
                <input
                  className={input}
                  placeholder="name, e.g. Living room light"
                  value={h.name}
                  onChange={(e) => {
                    const next = [...hotkeys];
                    next[i] = { ...h, name: e.target.value, id: h.id || slug(e.target.value) };
                    setHotkeys(next);
                  }}
                />
                <input
                  className={input}
                  placeholder="Ctrl+Alt+L"
                  value={h.accelerator}
                  onChange={(e) => {
                    const next = [...hotkeys];
                    next[i] = { ...h, accelerator: e.target.value };
                    setHotkeys(next);
                  }}
                />
                <div className="flex justify-end">
                  <Button kind="danger" onClick={() => setHotkeys(hotkeys.filter((_, j) => j !== i))}>
                    Remove
                  </Button>
                </div>
              </div>
              <ActionEditor
                value={h.action}
                onChange={(a) => {
                  const next = [...hotkeys];
                  next[i] = { ...h, action: a };
                  setHotkeys(next);
                }}
                commandDefs={snapshot.command_defs}
                customCommands={config.custom_commands}
                haConfigured={snapshot.ha_configured}
              />
            </div>
          ))}
        </div>
        <div className="mt-3 flex items-center gap-3">
          <Button
            kind="primary"
            disabled={saving}
            onClick={() =>
              void saveHotkeys(hotkeys.map((h) => ({ ...h, id: h.id || slug(h.name) || "hotkey" })))
            }
          >
            Save hotkeys
          </Button>
          {msg && <span className="text-[12px] text-muted">{msg}</span>}
        </div>
      </Panel>

      <Panel
        title="Tray quick actions"
        action={
          <Button
            onClick={() =>
              setTrayActions([
                ...trayActions,
                { id: `qa_${trayActions.length + 1}`, name: "", action: emptyAction() },
              ])
            }
          >
            Add action
          </Button>
        }
      >
        <p className="text-[12px] text-muted mb-3 leading-relaxed">
          Extra entries in the tray menu (right-click the Deskmate icon) — one click
          to toggle a light, run a scene or a local command.
        </p>
        {trayActions.length === 0 && <EmptyState text="No quick actions yet." />}
        <div className="space-y-4">
          {trayActions.map((t, i) => (
            <div key={i} className="border border-hairline rounded p-3 space-y-2">
              <div className="grid grid-cols-[1fr_auto] gap-2">
                <input
                  className={input}
                  placeholder="menu label, e.g. Scene: movie night"
                  value={t.name}
                  onChange={(e) => {
                    const next = [...trayActions];
                    next[i] = { ...t, name: e.target.value, id: t.id || slug(e.target.value) };
                    setTrayActions(next);
                  }}
                />
                <Button kind="danger" onClick={() => setTrayActions(trayActions.filter((_, j) => j !== i))}>
                  Remove
                </Button>
              </div>
              <ActionEditor
                value={t.action}
                onChange={(a) => {
                  const next = [...trayActions];
                  next[i] = { ...t, action: a };
                  setTrayActions(next);
                }}
                commandDefs={snapshot.command_defs}
                customCommands={config.custom_commands}
                haConfigured={snapshot.ha_configured}
              />
            </div>
          ))}
        </div>
        <div className="mt-3">
          <Button
            kind="primary"
            disabled={saving}
            onClick={() =>
              void saveTray(trayActions.map((t) => ({ ...t, id: t.id || slug(t.name) || "action" })))
            }
          >
            Save tray menu
          </Button>
        </div>
      </Panel>
    </>
  );
}
