import { useEffect, useState } from "react";
import { api } from "../api";
import { Button, Panel } from "../components";
import { t, tn } from "../i18n";
import type { CpuTemperatureStatus } from "../types";

const ERRORS: Record<string, string> = {
  pawnio_missing: tn("PawnIO is not installed."),
  pawnio_not_running: tn("PawnIO is installed but its driver is not running. Restart Windows."),
  unsupported_cpu: tn("This processor is not supported (Intel, or AMD Ryzen and newer)."),
  busy: tn("Another monitoring tool held the sensor; the next reading will retry."),
};

function describe(error: string): string {
  if (ERRORS[error]) return t(ERRORS[error]);
  if (error.startsWith("module_rejected"))
    return t("PawnIO refused the sensor module ({error}). Update PawnIO.", { error });
  return t("The helper could not read the sensor ({error}).", { error });
}

/** CPU temperature through PawnIO: an optional elevated helper (cpu_temperature.rs). */
export default function CpuTemperaturePanel() {
  const [status, setStatus] = useState<CpuTemperatureStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const refresh = () =>
    api
      .cpuTemperatureStatus()
      .then(setStatus)
      .catch((e) => setMsg(String(e)));
  useEffect(() => {
    void refresh();
  }, []);

  const run = async (action: () => Promise<void>, done: string) => {
    setBusy(true);
    setMsg(null);
    try {
      await action();
      setMsg(done);
      setTimeout(() => void refresh(), 2500);
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
      void refresh();
    }
  };

  return (
    <Panel title={t("CPU temperature")}>
      <p className="text-[12px] text-muted mb-3 leading-relaxed">
        {t(
          "Windows does not let normal programs read the processor's temperature. Deskmate can do it through PawnIO, a signed open-source driver, with a small helper that Windows starts with administrator rights at sign-in. Deskmate itself keeps running without them. The CPU temperature sensor then shows up in Home Assistant on its own.",
        )}
      </p>
      {status && !status.supported && (
        <p className="text-[13px]">
          {t("Not available on ARM processors (no such sensors to read).")}
        </p>
      )}
      {status?.supported && (
        <div className="space-y-2 text-[13px]">
          <div className="flex items-center justify-between gap-3 min-h-9">
            <span>
              {t("1. PawnIO driver: ")}
              {status.pawnio_installed ? t("installed") : t("not installed")}
            </span>
            {!status.pawnio_installed && (
              <Button onClick={() => void api.openPawnioSite()}>{t("Get PawnIO")}</Button>
            )}
          </div>
          <div className="flex items-center justify-between gap-3 min-h-9">
            <span>
              {t("2. Deskmate helper: ")}
              {status.helper_running
                ? t("running")
                : status.helper_installed
                  ? t("installed, not running")
                  : t("not installed")}
              {status.helper_outdated && t(" (from an older Deskmate)")}
            </span>
            <div className="flex gap-2">
              <Button
                kind="primary"
                disabled={busy || !status.pawnio_installed}
                onClick={() =>
                  void run(() => api.installCpuTemperatureHelper(), t("Helper installed."))
                }
              >
                {status.helper_outdated
                  ? t("Update helper")
                  : status.helper_installed
                    ? t("Reinstall")
                    : t("Install helper")}
              </Button>
              {status.helper_installed && (
                <Button
                  disabled={busy}
                  onClick={() =>
                    void run(() => api.removeCpuTemperatureHelper(), t("Helper removed."))
                  }
                >
                  {t("Remove")}
                </Button>
              )}
            </div>
          </div>
          <p className="mono text-[13px]">
            {status.celsius !== null
              ? t("Now: {celsius} °C", { celsius: status.celsius.toFixed(1) })
              : status.error
                ? describe(status.error)
                : status.helper_running
                  ? t("Waiting for the first reading...")
                  : t("No reading yet.")}
          </p>
          <p className="text-[12px] text-muted">
            {t(
              "Install and Remove ask Windows for administrator approval once. The helper needs an administrator account; it is removed here, not by uninstalling Deskmate.",
            )}
          </p>
        </div>
      )}
      {msg && <p className="mt-2 text-[13px]">{msg}</p>}
    </Panel>
  );
}
