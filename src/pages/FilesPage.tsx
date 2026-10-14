import { useEffect, useState } from "react";
import { api } from "../api";
import { Button, EmptyState, Field, Panel } from "../components";
import type { AppConfig, ClipboardMode, FileEvent } from "../types";

const KIND_LABEL: Record<string, string> = {
  received: "Received",
  read: "Read by Home Assistant",
  cancelled: "Cancelled",
};

function when(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString();
}

/** Files from Home Assistant and read-only folders, moved out of Settings in 0.8.1. */
export default function FilesPage({ config, onSaved }: { config: AppConfig; onSaved: () => Promise<void> }) {
  const [fileRoots, setFileRoots] = useState(config.link_file_roots);
  const [inboxMode, setInboxMode] = useState<ClipboardMode>(config.link_inbox_mode ?? "off");
  const [inboxDir, setInboxDir] = useState(config.link_inbox_dir ?? "");
  const [filesMaxMb, setFilesMaxMb] = useState(String(config.link_files_max_mb ?? 256));
  const [fileRootDraft, setFileRootDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [events, setEvents] = useState<FileEvent[] | null>(null);

  useEffect(() => {
    api.recentFileTransfers().then(setEvents).catch(() => setEvents([]));
  }, []);

  const addFileRoot = () => {
    const root = fileRootDraft.trim();
    if (!root || fileRoots.some((item) => item.toLocaleLowerCase() === root.toLocaleLowerCase())) return;
    setFileRoots((items) => [...items, root]);
    setFileRootDraft("");
  };

  const save = async () => {
    setSaving(true);
    setMsg(null);
    try {
      await api.saveConfig({
        ...config,
        link_file_roots: fileRoots,
        link_inbox_mode: inboxMode,
        link_inbox_dir: inboxDir.trim(),
        link_files_max_mb: Math.min(4096, Math.max(1, parseInt(filesMaxMb, 10) || 256)),
      });
      setMsg("Saved. Reconnecting...");
      await onSaved();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <Panel title="Receive files">
        <p className="text-[12px] text-muted mb-3 leading-relaxed">
          Send files from your phone or laptop to this computer through the Deskmate page in the Home Assistant
          sidebar. Files only ever land in the folder below. Home Assistant cannot overwrite, rename, delete or read
          anything through it; a name that already exists gets a number. Received files are marked as downloaded from
          another computer, so Windows treats them like browser downloads.
        </p>
        <div className="grid grid-cols-[1fr_120px] gap-3">
          <label className="block">
            <span className="microlabel">Files from Home Assistant</span>
            <select
              value={inboxMode}
              onChange={(e) => setInboxMode(e.target.value as ClipboardMode)}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="off">Off</option>
              <option value="confirm">Ask on this computer for every file</option>
              <option value="automatic">Accept automatically</option>
            </select>
          </label>
          <Field label="Max size (MB)" value={filesMaxMb} onChange={setFilesMaxMb} placeholder="256" />
        </div>
        <div className="mt-3">
          <Field
            label="Save to folder"
            value={inboxDir}
            onChange={setInboxDir}
            placeholder="Downloads\Deskmate (default)"
            hint="Leave empty for Downloads\Deskmate. Local folder only; it is created if missing."
          />
        </div>
        {inboxMode === "confirm" && (
          <p className="mt-2 text-[12px] text-muted">
            While Windows is locked nobody can accept, so transfers are refused.
          </p>
        )}
        {inboxMode === "automatic" && (
          <p className="mt-2 text-[12px] border border-hairline-strong rounded p-2">
            Anyone with an admin account in Home Assistant can put files on this computer without asking.
          </p>
        )}
        <p className="mt-2 text-[12px] text-muted">The size limit also applies to files Home Assistant reads from the folders below.</p>
      </Panel>

      <Panel title="Shared folders (read-only)">
        <p className="text-[12px] border border-hairline-strong rounded p-2 mb-3 leading-relaxed">
          Home Assistant can list, inspect and download files inside the folders below, never change them. Leave the
          list empty to keep this off. Never add a folder with secrets you do not want in Home Assistant.
        </p>
        <div className="flex items-end gap-2">
          <div className="flex-1">
            <Field
              label="Allowed local folder"
              value={fileRootDraft}
              onChange={setFileRootDraft}
              placeholder="C:\\Users\\Kuba\\Documents"
              hint="Absolute local path only. UNC shares, symbolic links and alternate data streams are rejected."
            />
          </div>
          <Button onClick={addFileRoot} disabled={!fileRootDraft.trim()}>Add folder</Button>
        </div>
        {fileRoots.length === 0 ? (
          <p className="mt-3 text-[12px] text-muted">Off: no folders are shared.</p>
        ) : (
          <div className="mt-3 space-y-2">
            {fileRoots.map((root) => (
              <div key={root.toLocaleLowerCase()} className="flex items-center gap-2 border-b border-hairline pb-2 last:border-b-0">
                <span className="flex-1 mono text-[12px] break-all">{root}</span>
                <Button kind="danger" onClick={() => setFileRoots((items) => items.filter((item) => item !== root))}>
                  Remove
                </Button>
              </div>
            ))}
          </div>
        )}
      </Panel>

      <div className="flex items-center gap-3">
        <Button kind="primary" onClick={save} disabled={saving}>
          {saving ? "Saving..." : "Save"}
        </Button>
        {msg && <span className="text-[12px] text-muted">{msg}</span>}
      </div>

      <Panel title="Recent transfers">
        {events === null && <p className="text-[12px] text-muted">Loading...</p>}
        {events?.length === 0 && <EmptyState text="Nothing sent or read yet." />}
        {events && events.length > 0 && (
          <div>
            {events.map((event, index) => (
              <div
                key={`${event.time}-${index}`}
                className="flex items-baseline gap-3 py-1.5 border-b border-hairline last:border-b-0 text-[12px]"
              >
                <span className="w-36 shrink-0 text-muted">{when(event.time)}</span>
                <span className="w-40 shrink-0">{KIND_LABEL[event.kind] ?? event.kind}</span>
                <span className="flex-1 mono break-all">{event.path}</span>
                {event.result !== "ok" && <span className="text-muted">{event.result}</span>}
              </div>
            ))}
          </div>
        )}
        <p className="mt-2 text-[12px] text-muted">From the local security log. File contents are never logged.</p>
      </Panel>
    </>
  );
}
