import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { Button } from "../components";
import type { ServiceView } from "../types";

/** How often the Status page looks at the service. */
const REFRESH_MS = 60_000;

/** A quiet line on the Status page when the service needs an update or has other settings. */
export default function ServiceBanner() {
  const [view, setView] = useState<ServiceView | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const refresh = useCallback(
    () =>
      api
        .serviceStatus()
        .then(setView)
        // The panel in Settings shows the problem; this banner stays quiet.
        .catch(() => setView(null)),
    [],
  );
  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), REFRESH_MS);
    return () => clearInterval(timer);
  }, [refresh]);

  if (!view || view.phase === "not_installed") return null;
  if (!view.needs_update && !view.settings_differ && !msg) return null;

  const update = view.needs_update;
  const blocked = update ? view.update_blocked : view.apply_blocked;
  const text = update
    ? `The Deskmate service runs version ${view.running_version ?? "unknown"}; this Deskmate brings ${view.bundled_version}.`
    : "The Deskmate service has other settings than this app.";

  const run = async () => {
    setBusy(true);
    setMsg(null);
    try {
      setMsg(await api.setUpService(update ? "update" : "apply"));
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
      void refresh();
    }
  };

  return (
    <div className="mb-3 border border-hairline rounded-md px-4 py-3 bg-panel">
      <div className="flex items-center justify-between gap-3">
        <p className="text-[13px]" aria-live="polite">
          {view.needs_update || view.settings_differ ? text : msg}
        </p>
        {(view.needs_update || view.settings_differ) && (
          <Button disabled={busy || blocked !== null} onClick={() => void run()}>
            {update ? "Update" : "Apply"}
          </Button>
        )}
      </div>
      {blocked && <p className="mt-2 text-[12px] text-muted">{blocked}</p>}
      {msg && (view.needs_update || view.settings_differ) && (
        <p className="mt-2 text-[12px] text-muted">{msg}</p>
      )}
    </div>
  );
}
