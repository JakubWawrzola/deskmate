import type { ActionKind, ActionSpec, CommandDef, CustomCommand } from "../types";

const KINDS: { value: ActionKind; label: string; needsApi: boolean }[] = [
  { value: "toggle", label: "Toggle HA entity", needsApi: true },
  { value: "service", label: "Call HA service", needsApi: true },
  { value: "command", label: "Local command", needsApi: false },
  { value: "widget", label: "Show/hide widgets", needsApi: false },
  { value: "mqtt", label: "HA event trigger (MQTT / Link)", needsApi: false },
];

/** Edytor ActionSpec - wspolny dla hotkeyow i tray quick actions. */
export default function ActionEditor({
  value,
  onChange,
  commandDefs,
  customCommands,
  haConfigured,
}: {
  value: ActionSpec;
  onChange: (a: ActionSpec) => void;
  commandDefs: CommandDef[];
  customCommands: CustomCommand[];
  haConfigured: boolean;
}) {
  const set = (patch: Partial<ActionSpec>) => onChange({ ...value, ...patch });
  const input =
    "w-full h-8 px-2 bg-panel border border-hairline-strong rounded text-ink mono text-[12px] placeholder:text-faint focus-visible:border-ink";

  return (
    <div className="space-y-2">
      <select
        value={value.kind}
        onChange={(e) => set({ kind: e.target.value as ActionKind })}
        className="h-8 px-2 bg-panel border border-hairline-strong rounded text-[12px]"
      >
        {KINDS.map((k) => (
          <option key={k.value} value={k.value}>
            {k.label}
            {k.needsApi && !haConfigured ? " (configure HA API first)" : ""}
          </option>
        ))}
      </select>

      {value.kind === "toggle" && (
        <input
          className={input}
          placeholder="entity_id, e.g. light.living_room"
          value={value.entity_id}
          onChange={(e) => set({ entity_id: e.target.value })}
        />
      )}

      {value.kind === "service" && (
        <div className="space-y-2">
          <input
            className={input}
            placeholder="service, e.g. scene.turn_on"
            value={value.service}
            onChange={(e) => set({ service: e.target.value })}
          />
          <input
            className={input}
            placeholder="entity_id (optional)"
            value={value.entity_id}
            onChange={(e) => set({ entity_id: e.target.value })}
          />
          <input
            className={input}
            placeholder='data JSON (optional), e.g. {"brightness_pct": 40}'
            value={value.data}
            onChange={(e) => set({ data: e.target.value })}
          />
        </div>
      )}

      {value.kind === "command" && (
        <select
          value={value.command_id}
          onChange={(e) => set({ command_id: e.target.value })}
          className="h-8 px-2 bg-panel border border-hairline-strong rounded text-[12px] w-full"
        >
          <option value="">choose a command...</option>
          {commandDefs.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
          {customCommands.filter((c) => c.enabled).map((c) => (
            <option key={c.id} value={`custom_${c.id}`}>
              {c.name} (custom)
            </option>
          ))}
        </select>
      )}

      {value.kind === "mqtt" && (
        <p className="text-[12px] text-muted leading-relaxed">
          Publishes an event through the selected transport. With MQTT it is a device
          trigger; with Link it is an event entity and a deskmate_link_trigger bus event.
        </p>
      )}
    </div>
  );
}
