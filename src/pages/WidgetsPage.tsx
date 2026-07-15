import { useState } from "react";
import { api } from "../api";
import { Button, EmptyState, Panel } from "../components";
import type { Snapshot, WidgetItem } from "../types";

/** Configuration for the widget panel (always-on-top HA entity tiles). */
export default function WidgetsPage({
  snapshot,
  widgets: initial,
  onChanged,
}: {
  snapshot: Snapshot;
  widgets: WidgetItem[];
  onChanged: () => Promise<void>;
}) {
  const [items, setItems] = useState<WidgetItem[]>(initial);
  const [msg, setMsg] = useState<string | null>(null);

  const save = async () => {
    setMsg(null);
    try {
      await api.updateWidgets(items.filter((w) => w.entity_id.trim() !== ""));
      setMsg("Saved.");
      await onChanged();
    } catch (e) {
      setMsg(String(e));
    }
  };

  const input =
    "w-full h-8 px-2 bg-panel border border-hairline-strong rounded text-ink mono text-[12px] placeholder:text-faint focus-visible:border-ink";

  return (
    <Panel
      title="Widget panel"
      action={<Button onClick={() => setItems([...items, { entity_id: "", label: "" }])}>Add tile</Button>}
    >
      <p className="text-[12px] text-muted mb-3 leading-relaxed">
        A small always-on-top window with tiles for your Home Assistant entities:
        click a tile to toggle it, sensors are read-only. Show/hide it from the tray
        menu or a hotkey. Needs the HA API configured in Settings
        {snapshot.ha_configured ? "" : " (not configured yet!)"}.
      </p>
      {items.length === 0 && <EmptyState text="No tiles yet. Add light.xxx, switch.xxx, sensor.xxx..." />}
      <div className="space-y-2">
        {items.map((w, i) => (
          <div key={i} className="grid grid-cols-[1fr_1fr_auto] gap-2">
            <input
              className={input}
              placeholder="entity_id, e.g. light.living_room"
              value={w.entity_id}
              onChange={(e) => {
                const next = [...items];
                next[i] = { ...w, entity_id: e.target.value };
                setItems(next);
              }}
            />
            <input
              className={input}
              placeholder="label (optional, defaults to HA name)"
              value={w.label}
              onChange={(e) => {
                const next = [...items];
                next[i] = { ...w, label: e.target.value };
                setItems(next);
              }}
            />
            <Button kind="danger" onClick={() => setItems(items.filter((_, j) => j !== i))}>
              Remove
            </Button>
          </div>
        ))}
      </div>
      <div className="mt-3 flex items-center gap-3">
        <Button kind="primary" onClick={() => void save()}>
          Save tiles
        </Button>
        <Button onClick={() => void api.toggleWidgetWindow()}>Show/hide panel</Button>
        {msg && <span className="text-[12px] text-muted">{msg}</span>}
      </div>
    </Panel>
  );
}
