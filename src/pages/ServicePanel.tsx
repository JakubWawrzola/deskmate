import { type ReactNode, useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { Button, Panel } from "../components";
import { t } from "../i18n";
import type { ServiceSetupAction, ServiceView } from "../types";

/** How often the panel looks at the service while it is open. */
const REFRESH_MS = 15_000;
/** The service needs a moment after a step before its status says so. */
const AFTER_STEP_MS = 2_500;

/** One line about what the service is doing, in words. */
function describeService(view: ServiceView): string {
  switch (view.phase) {
    case "not_installed":
      return t("Not installed.");
    case "not_answering":
    case "error":
      return t("Error: {detail}", { detail: view.detail ?? "" });
    case "integration_too_old":
      return t(
        "The Deskmate integration in Home Assistant is too old for the service. Update it in HACS; the service checks again within an hour.",
      );
    case "needs_update":
      return t("Needs update: version {running} runs, this Deskmate brings {bundled}.", {
        running: view.running_version ?? t("unknown"),
        bundled: view.bundled_version,
      });
    case "settings_differ":
      return t(
        "Settings differ: the service has other settings than this app, or none yet. Apply gives it the current ones.",
      );
    case "running":
      break;
  }
  if (view.paused_by_user) {
    return t(
      "Running, waiting: you pressed Disconnect, so neither Deskmate nor the service connects while Deskmate runs. If you quit Deskmate or sign out, the service connects. Press Connect on the Status page to connect again.",
    );
  }
  switch (view.activity) {
    case "connected":
      return t("Running: connected to Home Assistant.");
    case "connecting":
      return t("Running: connecting to Home Assistant.");
    case "waiting":
      return t("Running: waiting while the Deskmate app holds the connection.");
    default:
      return t("Running.");
  }
}

/** A step button, off while another step runs or while it cannot be taken; the reason is shown below the row. */
function Step({
  label,
  blocked,
  busy,
  primary,
  onClick,
}: {
  label: string;
  blocked: string | null;
  busy: boolean;
  primary?: boolean;
  onClick: () => void;
}) {
  return (
    <Button
      kind={primary ? "primary" : "ghost"}
      disabled={busy || blocked !== null}
      onClick={onClick}
    >
      {label}
    </Button>
  );
}

/** Settings > Deskmate service: the Windows service that keeps the computer in Home Assistant while nobody is signed in. */
export default function ServicePanel() {
  const [view, setView] = useState<ServiceView | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const refresh = useCallback(
    () =>
      api
        .serviceStatus()
        .then(setView)
        .catch((e) => setMsg(String(e))),
    [],
  );
  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), REFRESH_MS);
    return () => clearInterval(timer);
  }, [refresh]);

  const run = async (step: () => Promise<string>) => {
    setBusy(true);
    setMsg(null);
    try {
      setMsg(await step());
      setTimeout(() => void refresh(), AFTER_STEP_MS);
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
      void refresh();
    }
  };
  const setUp = (action: ServiceSetupAction) => void run(() => api.setUpService(action));

  let steps: ReactNode = null;
  const reasons: string[] = [];
  if (view?.phase === "not_installed") {
    steps = (
      <Step
        label={t("Install")}
        primary
        blocked={view.install_blocked}
        busy={busy}
        onClick={() => setUp("install")}
      />
    );
    if (view.install_blocked) reasons.push(view.install_blocked);
  } else if (view) {
    const offerUpdate = view.needs_update || view.phase === "not_answering";
    steps = (
      <>
        {offerUpdate && (
          <Step
            label={t("Update")}
            primary
            blocked={view.update_blocked}
            busy={busy}
            onClick={() => setUp("update")}
          />
        )}
        {view.settings_differ && (
          <Step
            label={t("Apply")}
            primary={!offerUpdate}
            blocked={view.apply_blocked}
            busy={busy}
            onClick={() => setUp("apply")}
          />
        )}
        <Button disabled={busy} onClick={() => void run(api.removeService)}>
          {t("Remove")}
        </Button>
      </>
    );
    if (offerUpdate && view.update_blocked) reasons.push(view.update_blocked);
    if (view.settings_differ && view.apply_blocked) reasons.push(view.apply_blocked);
    if (offerUpdate && view.settings_differ && !view.apply_blocked) {
      reasons.push(t("Apply also updates the service."));
    }
  }

  return (
    <Panel title={t("Deskmate service")}>
      <p className="text-[12px] text-muted mb-3 leading-relaxed">
        {t(
          "The service keeps this computer's sensors and power buttons in Home Assistant while nobody is signed in. It never shows notifications, receives files or sees anything from your session.",
        )}
      </p>
      {view && (
        <div className="space-y-2 text-[13px]">
          <div className="flex items-center justify-between gap-3 min-h-9">
            <p aria-live="polite">{describeService(view)}</p>
            <div className="flex shrink-0 gap-2">{steps}</div>
          </div>
          {view.running_version && (
            <p className="text-[12px] text-muted">
              {t("Version ")}
              <span className="mono">{view.running_version}</span>
            </p>
          )}
          {reasons.map((reason) => (
            <p key={reason} className="text-[12px] text-muted">
              {reason}
            </p>
          ))}
          <p className="text-[12px] text-muted">
            {t(
              "Install, Update, Apply and Remove each ask Windows for administrator approval once. Uninstalling Deskmate removes the service too.",
            )}
          </p>
        </div>
      )}
      {msg && (
        <p className="mt-2 text-[13px]" aria-live="polite">
          {msg}
        </p>
      )}
    </Panel>
  );
}
