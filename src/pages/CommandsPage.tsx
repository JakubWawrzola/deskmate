import { useState } from "react";
import { api } from "../api";
import { Button, EmptyState, Field, Panel, Toggle } from "../components";
import { t, tn } from "../i18n";
import { isLinux, usePlatform } from "../platform";
import type { AppConfig, CustomAction, CustomKind, Snapshot } from "../types";

const ACTION_LABEL: Record<CustomAction, string> = {
  powershell: "PowerShell",
  launch: tn("start"),
  stop: tn("stop"),
  keys: tn("keys"),
};

const COMMAND_LABEL: Record<CustomAction, string> = {
  powershell: tn("PowerShell command"),
  launch: tn("Program, file or URL"),
  stop: tn("Program name"),
  keys: tn("Keys"),
};

const COMMAND_PLACEHOLDER: Record<CustomAction, string> = {
  powershell: "notepad",
  launch: t("C:\\Program Files\\Steam\\steam.exe"),
  stop: "steam.exe",
  keys: "Ctrl+F5",
};

const NAME_PLACEHOLDER: Record<CustomAction, string> = {
  powershell: t("Studio light"),
  launch: t("Start Steam"),
  stop: t("Close Steam"),
  keys: t("Refresh page"),
};

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
  const [action, setAction] = useState<CustomAction>("powershell");
  const [args, setArgs] = useState("");
  const [error, setError] = useState<string | null>(null);

  const add = async () => {
    setError(null);
    const id = name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "_");
    try {
      await api.addCustomCommand(
        id,
        name.trim(),
        command.trim(),
        action === "powershell" ? kind : "button",
        undefined,
        undefined,
        undefined,
        action,
        args.trim(),
      );
      setName("");
      setCommand("");
      setArgs("");
      setKind("button");
      await onChanged();
    } catch (e) {
      setError(String(e));
    }
  };

  const updateSecurity = async (id: string, enabled: boolean, requireConfirmation: boolean) => {
    setError(null);
    try {
      await api.updateCustomCommandSecurity(id, enabled, requireConfirmation);
      await onChanged();
    } catch (e) {
      setError(String(e));
    }
  };

  const linux = isLinux(usePlatform());

  return (
    <>
      <Panel title={t("Built-in commands")}>
        <p className="text-[12px] text-muted mb-2 leading-relaxed">
          {t("Each command is a button entity in Home Assistant (")}
          <span className="mono">button.{config.node_id}_lock</span>
          {t(' etc.). "Run" executes it locally so you can test without HA.')}
        </p>
        {snapshot.command_defs.map((c) => (
          <div
            key={c.id}
            className="flex items-center gap-3 h-10 border-b border-hairline last:border-b-0"
          >
            <span className="w-44 text-[13px] font-medium">{c.name}</span>
            <span className="flex-1 mono text-[12px] text-faint truncate">cmd/{c.id}</span>
            <Button onClick={() => void api.runCommandLocal(c.id)}>{t("Run")}</Button>
          </div>
        ))}
      </Panel>

      <Panel
        title={t("Custom controls")}
        unavailable={
          linux &&
          t(
            "Your own PowerShell commands, program controls and key combinations are not part of the Linux preview.",
          )
        }
      >
        <p className="text-[12px] text-muted mb-3 leading-relaxed">
          {t("Buttons, switches and sliders in Home Assistant that act on this computer.")}
          <span className="text-ink">{t(" Start a program")}</span>
          {t(" opens a program, file or URL,")}
          <span className="text-ink">{t(" Stop a program")}</span>
          {t(" closes every running copy of it,")}
          <span className="text-ink">{t(" Press keys")}</span>
          {t(
            " presses a key combination such as Ctrl+F5 in the window in front (refreshes the page in a browser), all without PowerShell. A ",
          )}
          <span className="text-ink">PowerShell</span>
          {t(" control runs your script; a ")}
          <span className="mono">switch</span>
          {t(" passes ON/OFF and a ")}
          <span className="mono">number</span>
          {t(" its value as ")}
          <span className="mono">$env:DESKMATE_VALUE</span>
          {t(
            ". Anyone who can trigger the entity runs it, so add only what you would run yourself.",
          )}
        </p>
        {config.custom_commands.length === 0 && <EmptyState text={t("No custom controls yet.")} />}
        {config.custom_commands.map((c) => (
          <div
            key={c.id}
            className="grid grid-cols-[160px_70px_1fr_auto_auto_auto_auto] items-center gap-3 min-h-12 border-b border-hairline py-2"
          >
            <span className="w-40 text-[13px] font-medium truncate">{c.name}</span>
            <span className="microlabel border border-hairline-strong rounded px-1.5 py-0.5">
              {(c.action ?? "powershell") === "powershell"
                ? c.kind
                : t(ACTION_LABEL[c.action ?? "powershell"])}
            </span>
            <span className="flex-1 mono text-[12px] text-muted truncate">
              {c.command}
              {c.args ? ` ${c.args}` : ""}
            </span>
            <label className="flex items-center gap-2 text-[12px]">
              <Toggle
                on={c.enabled}
                onChange={(enabled) => void updateSecurity(c.id, enabled, c.require_confirmation)}
              />
              {t("Enabled")}
            </label>
            <label className="flex items-center gap-2 text-[12px]">
              <Toggle
                on={c.require_confirmation}
                onChange={(confirm) => void updateSecurity(c.id, c.enabled, confirm)}
              />
              {t("Confirm")}
            </label>
            <Button
              disabled={!c.enabled}
              onClick={() => void api.runCommandLocal(`custom_${c.id}`)}
            >
              {t("Run")}
            </Button>
            <Button
              kind="danger"
              onClick={async () => {
                await api.removeCustomCommand(c.id);
                await onChanged();
              }}
            >
              {t("Remove")}
            </Button>
          </div>
        ))}

        <div className="mt-4 grid grid-cols-[150px_150px_1fr] gap-3 items-end">
          <Field
            label={t("Name")}
            value={name}
            onChange={setName}
            placeholder={NAME_PLACEHOLDER[action]}
          />
          <label className="block">
            <span className="microlabel">{t("Action")}</span>
            <select
              value={action}
              onChange={(e) => setAction(e.target.value as CustomAction)}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="powershell">{t("PowerShell script")}</option>
              <option value="launch">{t("Start a program")}</option>
              <option value="stop">{t("Stop a program")}</option>
              <option value="keys">{t("Press keys")}</option>
            </select>
          </label>
          <Field
            label={t(COMMAND_LABEL[action])}
            value={command}
            onChange={setCommand}
            placeholder={COMMAND_PLACEHOLDER[action]}
          />
        </div>
        <div className="mt-3 grid grid-cols-[1fr_120px_auto] gap-3 items-end">
          {action === "launch" ? (
            <Field
              label={t("Arguments (optional)")}
              value={args}
              onChange={setArgs}
              placeholder="-silent"
            />
          ) : action === "keys" ? (
            <p className="text-[12px] text-muted leading-relaxed">
              {t("Ctrl, Shift, Alt or Win plus a key: ")}
              <span className="mono">{"Ctrl+F5"}</span>,{" "}
              <span className="mono">{"Ctrl+Shift+R"}</span>,{" "}
              <span className="mono">{"Win+D"}</span>
              {t(", or several in a row: ")}
              <span className="mono">{"Ctrl+L, Ctrl+C"}</span>
              {t(". Keys go to the window in front.")}
            </p>
          ) : (
            <span />
          )}
          {action === "powershell" ? (
            <label className="block">
              <span className="microlabel">{t("Type")}</span>
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
          ) : (
            <span className="microlabel self-center">button</span>
          )}
          <Button
            kind="primary"
            onClick={() => void add()}
            disabled={!name.trim() || !command.trim()}
          >
            {t("Add")}
          </Button>
        </div>
        <p className="mt-2 text-[12px] text-muted">
          {t(
            "New commands are disabled and require confirmation by default. Enable them only after reviewing the exact script.",
          )}
        </p>
        {error && <p className="mt-2 text-[13px]">{error}</p>}
      </Panel>
    </>
  );
}
