import { useEffect, useState } from "react";
import { api } from "../api";
import { Button, Panel } from "../components";
import type { CpuTemperatureStatus } from "../types";

const ERRORS: Record<string, string> = {
  pawnio_missing: "PawnIO is not installed.",
  pawnio_not_running: "PawnIO is installed but its driver is not running. Restart Windows.",
  unsupported_cpu: "This processor is not supported (Intel, or AMD Ryzen and newer).",
  busy: "Another monitoring tool held the sensor; the next reading will retry.",
};

function describe(error: string): string {
  if (ERRORS[error]) return ERRORS[error];
  if (error.startsWith("module_rejected")) return `PawnIO refused the sensor module (${error}). Update PawnIO.`;
  return `The helper could not read the sensor (${error}).`;
}

/** CPU temperature through PawnIO: an optional elevated helper (cpu_temperature.rs). */
export default function CpuTemperaturePanel() {
  const [status, setStatus] = useState<CpuTemperatureStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const refresh = () => api.cpuTemperatureStatus().then(setStatus).catch((e) => setMsg(String(e)));
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
    <Panel title="CPU temperature">
      <p className="text-[12px] text-muted mb-3 leading-relaxed">
        Windows does not let normal programs read the processor's temperature. Deskmate can do it
        through PawnIO, a signed open-source driver, with a small helper that Windows starts with
        administrator rights at sign-in. Deskmate itself keeps running without them. The CPU
        temperature sensor then shows up in Home Assistant on its own.
      </p>
      {status && !status.supported && (
        <p className="text-[13px]">Not available on ARM processors (no such sensors to read).</p>
      )}
      {status?.supported && (
        <div className="space-y-2 text-[13px]">
          <div className="flex items-center justify-between gap-3 min-h-9">
            <span>1. PawnIO driver: {status.pawnio_installed ? "installed" : "not installed"}</span>
            {!status.pawnio_installed && (
              <Button onClick={() => void api.openPawnioSite()}>Get PawnIO</Button>
            )}
          </div>
          <div className="flex items-center justify-between gap-3 min-h-9">
            <span>
              2. Deskmate helper:{" "}
              {status.helper_running ? "running" : status.helper_installed ? "installed, not running" : "not installed"}
            </span>
            <div className="flex gap-2">
              <Button
                kind="primary"
                disabled={busy || !status.pawnio_installed}
                onClick={() => void run(() => api.installCpuTemperatureHelper(), "Helper installed.")}
              >
                {status.helper_installed ? "Reinstall" : "Install helper"}
              </Button>
              {status.helper_installed && (
                <Button disabled={busy} onClick={() => void run(() => api.removeCpuTemperatureHelper(), "Helper removed.")}>
                  Remove
                </Button>
              )}
            </div>
          </div>
          <p className="mono text-[13px]">
            {status.celsius !== null
              ? `Now: ${status.celsius.toFixed(1)} °C`
              : status.error
                ? describe(status.error)
                : status.helper_running
                  ? "Waiting for the first reading..."
                  : "No reading yet."}
          </p>
          <p className="text-[12px] text-muted">
            Install and Remove ask Windows for administrator approval once. The helper needs an
            administrator account; it is removed here, not by uninstalling Deskmate.
          </p>
        </div>
      )}
      {msg && <p className="mt-2 text-[13px]">{msg}</p>}
    </Panel>
  );
}
