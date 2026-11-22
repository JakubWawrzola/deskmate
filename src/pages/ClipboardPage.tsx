import { useState } from "react";
import { api } from "../api";
import { Button, Panel } from "../components";
import { t } from "../i18n";
import type { AppConfig, ClipboardMode } from "../types";

/** Clipboard to and from Home Assistant, moved out of Settings in 0.8.1. */
export default function ClipboardPage({
  config,
  snapshotClipboard,
  onSaved,
}: {
  config: AppConfig;
  snapshotClipboard: string | undefined;
  onSaved: () => Promise<void>;
}) {
  const [readMode, setReadMode] = useState<ClipboardMode>(config.clipboard_read_mode);
  const [writeMode, setWriteMode] = useState<ClipboardMode>(config.clipboard_write_mode);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const save = async () => {
    setSaving(true);
    setMsg(null);
    try {
      await api.saveConfig({
        ...config,
        clipboard_read_mode: readMode,
        clipboard_write_mode: writeMode,
      });
      setMsg(t("Saved. Reconnecting..."));
      await onSaved();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <Panel title={t("Clipboard")}>
        <p className="text-[12px] text-muted mb-3 leading-relaxed">
          {t(
            "Copy text on this computer and paste it on your phone, or the other way round. Reading and writing are separate permissions. Both are blocked while Windows is locked. Asking first means a question on this computer for every changed value (read) or every incoming text (write).",
          )}
        </p>
        <div className="grid grid-cols-2 gap-3">
          <label className="block">
            <span className="microlabel">{t("Send the clipboard to Home Assistant")}</span>
            <select
              value={readMode}
              onChange={(e) => setReadMode(e.target.value as ClipboardMode)}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="off">{t("Off")}</option>
              <option value="confirm">{t("Ask for each changed value")}</option>
              <option value="automatic">{t("Automatically")}</option>
            </select>
          </label>
          <label className="block">
            <span className="microlabel">{t("Let Home Assistant set the clipboard")}</span>
            <select
              value={writeMode}
              onChange={(e) => setWriteMode(e.target.value as ClipboardMode)}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="off">{t("Off")}</option>
              <option value="confirm">{t("Ask for every text")}</option>
              <option value="automatic">{t("Automatically")}</option>
            </select>
          </label>
        </div>
        <div className="mt-3 flex items-center gap-3">
          <Button kind="primary" onClick={save} disabled={saving}>
            {saving ? t("Saving...") : t("Save")}
          </Button>
          {msg && <span className="text-[12px] text-muted">{msg}</span>}
        </div>
      </Panel>

      <Panel title={t("In Home Assistant")}>
        <ul className="text-[12px] text-muted space-y-1.5 leading-relaxed">
          <li>
            <span className="text-ink">{t("Clipboard")}</span>
            {t(" sensor: the text copied on this computer, when sending is on.")}
          </li>
          <li>
            <span className="text-ink">{t("Set clipboard")}</span>
            {t(
              " text entity: type or automate text that lands in this computer's clipboard, when setting is on.",
            )}
          </li>
          <li>
            {t(
              "Values sent to Home Assistant can stay in its history after you switch sending off.",
            )}
          </li>
        </ul>
        {config.clipboard_read_mode !== "off" && (
          <p className="mt-3 text-[12px]">
            <span className="microlabel">{t("Last sent")}</span>
            <span className="block mt-1 mono break-all">{snapshotClipboard ?? "--"}</span>
          </p>
        )}
      </Panel>
    </>
  );
}
