import { useState } from "react";
import { api } from "../api";
import { Button, EmptyState, Field, Panel, Toggle } from "../components";
import type { AppConfig, CustomKind, Snapshot } from "../types";

export default function CommandsPage({
  snapshot,
  config,
  onChanged,
}: {
  snapshot: Snapshot;
  config: AppConfig;
  onChanged: () => Promise<void>;
}) {
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [kind, setKind] = useState<CustomKind>("button");
  const [error, setError] = useState<string | null>(null);

  const add = async () => {
    setError(null);
    const id = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "_");
    try {
      await api.addCustomCommand(id, name.trim(), command.trim(), kind);
      setName("");
      setCommand("");
      setKind("button");
      await onChanged();
    } catch (e) {
      setError(String(e));
    }
  };

  const updateSecurity = async (
    id: string,
    enabled: boolean,
    requireConfirmation: boolean,
  ) => {
    setError(null);
    try {
      await api.updateCustomCommandSecurity(id, enabled, requireConfirmation);
      await onChanged();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <>
      <Panel title="Built-in commands">
        <p className="text-[12px] text-muted mb-2 leading-relaxed">
          Each command is a button entity in Home Assistant
          (<span className="mono">button.{config.node_id}_lock</span> etc.).
          "Run" executes it locally so you can test without HA.
        </p>
        {snapshot.command_defs.map((c) => (
          <div key={c.id} className="flex items-center gap-3 h-10 border-b border-hairline last:border-b-0">
            <span className="w-44 text-[13px] font-medium">{c.name}</span>
            <span className="flex-1 mono text-[12px] text-faint truncate">cmd/{c.id}</span>
            <Button onClick={() => void api.runCommandLocal(c.id)}>Run</Button>
          </div>
        ))}
      </Panel>

      <Panel title="Custom controls">
        <p className="text-[12px] text-muted mb-3 leading-relaxed">
          Runs as PowerShell on this computer, triggered from Home Assistant.
          A <span className="mono">switch</span> passes ON/OFF and a{" "}
          <span className="mono">number</span> passes its value to the script as{" "}
          <span className="mono">$env:DESKMATE_VALUE</span>. Anyone who can trigger
          the entity runs this command — add only what you would type yourself.
        </p>
        {config.custom_commands.length === 0 && <EmptyState text="No custom controls yet." />}
        {config.custom_commands.map((c) => (
          <div key={c.id} className="grid grid-cols-[160px_70px_1fr_auto_auto_auto_auto] items-center gap-3 min-h-12 border-b border-hairline py-2">
            <span className="w-40 text-[13px] font-medium truncate">{c.name}</span>
            <span className="microlabel border border-hairline-strong rounded px-1.5 py-0.5">
              {c.kind}
            </span>
            <span className="flex-1 mono text-[12px] text-muted truncate">{c.command}</span>
            <label className="flex items-center gap-2 text-[12px]">
              <Toggle
                on={c.enabled}
                onChange={(enabled) => void updateSecurity(c.id, enabled, c.require_confirmation)}
              />
              Enabled
            </label>
            <label className="flex items-center gap-2 text-[12px]">
              <Toggle
                on={c.require_confirmation}
                onChange={(confirm) => void updateSecurity(c.id, c.enabled, confirm)}
              />
              Confirm
            </label>
            <Button disabled={!c.enabled} onClick={() => void api.runCommandLocal(`custom_${c.id}`)}>Run</Button>
            <Button
              kind="danger"
              onClick={async () => {
                await api.removeCustomCommand(c.id);
                await onChanged();
              }}
            >
              Remove
            </Button>
          </div>
        ))}

        <div className="mt-4 grid grid-cols-[150px_1fr_120px_auto] gap-3 items-end">
          <Field label="Name" value={name} onChange={setName} placeholder="Studio light" />
          <Field label="PowerShell command" value={command} onChange={setCommand} placeholder="notepad" />
          <label className="block">
            <span className="microlabel">Type</span>
            <select
              value={kind}
              onChange={(e) => setKind(e.target.value as CustomKind)}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="button">button</option>
              <option value="switch">switch</option>
              <option value="number">number</option>
            </select>
          </label>
          <Button kind="primary" onClick={() => void add()} disabled={!name.trim() || !command.trim()}>
            Add
          </Button>
        </div>
        <p className="mt-2 text-[12px] text-muted">
          New commands are disabled and require confirmation by default. Enable them only after reviewing the exact script.
        </p>
        {error && <p className="mt-2 text-[13px]">{error}</p>}
      </Panel>
    </>
  );
}
