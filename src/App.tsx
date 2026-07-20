import { useCallback, useEffect, useState } from "react";
import { api, onNotify, onSensors, onStatus } from "./api";
import { StatusDot } from "./components";
import type { AppConfig, NotifyRecord, Snapshot, StatusView } from "./types";
import Wizard from "./pages/Wizard";
import StatusPage from "./pages/StatusPage";
import SensorsPage from "./pages/SensorsPage";
import CommandsPage from "./pages/CommandsPage";
import HotkeysPage from "./pages/HotkeysPage";
import WidgetsPage from "./pages/WidgetsPage";
import NotificationsPage from "./pages/NotificationsPage";
import SettingsPage from "./pages/SettingsPage";

const PAGES = ["Status", "Sensors", "Commands", "Hotkeys", "Widgets", "Notifications", "Settings"] as const;
export type Page = (typeof PAGES)[number];

export default function App() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [hasPassword, setHasPassword] = useState(false);
  const [hasLinkKey, setHasLinkKey] = useState(false);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [page, setPage] = useState<Page>("Status");

  const reloadConfig = useCallback(async () => {
    const view = await api.getConfig();
    setConfig(view.config);
    setHasPassword(view.has_password);
    setHasLinkKey(view.has_link_key);
  }, []);

  const reloadSnapshot = useCallback(async () => {
    setSnapshot(await api.getSnapshot());
  }, []);

  useEffect(() => {
    void reloadConfig();
    void reloadSnapshot();
    const subs = [
      onStatus((s: StatusView) =>
        setSnapshot((prev) => (prev ? { ...prev, status: s } : prev)),
      ),
      onSensors((values) =>
        setSnapshot((prev) =>
          prev ? { ...prev, sensor_values: { ...prev.sensor_values, ...values } } : prev,
        ),
      ),
      onNotify((n: NotifyRecord) =>
        setSnapshot((prev) =>
          prev ? { ...prev, notifications: [n, ...prev.notifications].slice(0, 50) } : prev,
        ),
      ),
    ];
    return () => {
      subs.forEach((p) => p.then((un) => un()));
    };
  }, [reloadConfig, reloadSnapshot]);

  if (!config) return null;

  if (!config.configured) {
    return (
      <Wizard
        config={config}
        hasLinkKey={hasLinkKey}
        onDone={async () => {
          await reloadConfig();
          await reloadSnapshot();
        }}
        status={snapshot?.status ?? null}
      />
    );
  }

  const connected = snapshot?.status.connected ?? false;

  return (
    <div className="h-full flex">
      {/* rail nawigacji z pionowym wordmarkiem */}
      <nav className="w-44 shrink-0 border-r border-hairline bg-panel flex flex-col">
        <div className="h-12 flex items-center px-4 border-b border-hairline">
          <span className="font-semibold tracking-[0.14em] text-[13px]">DESKMATE</span>
        </div>
        <div className="flex-1 py-2">
          {PAGES.map((p) => (
            <button
              key={p}
              onClick={() => setPage(p)}
              className={`w-full text-left px-4 h-9 text-[13px] transition-colors duration-100
                ${page === p ? "bg-paper font-semibold border-r-2 border-ink" : "text-muted hover:text-ink"}`}
            >
              {p}
            </button>
          ))}
        </div>
        <div className="px-4 h-11 border-t border-hairline flex items-center gap-2">
          <StatusDot on={connected} />
          <span className="text-[12px] text-muted truncate">
            {snapshot?.status.detail ?? "..."}
          </span>
        </div>
      </nav>

      <main className="flex-1 overflow-y-auto">
        <div className="max-w-3xl mx-auto px-6 py-6 space-y-4">
          {page === "Status" && snapshot && <StatusPage snapshot={snapshot} config={config} />}
          {page === "Sensors" && snapshot && (
            <SensorsPage snapshot={snapshot} config={config} onChanged={reloadConfig} />
          )}
          {page === "Commands" && snapshot && (
            <CommandsPage snapshot={snapshot} config={config} onChanged={reloadConfig} />
          )}
          {page === "Hotkeys" && snapshot && (
            <HotkeysPage snapshot={snapshot} config={config} onChanged={reloadConfig} />
          )}
          {page === "Widgets" && snapshot && (
            <WidgetsPage snapshot={snapshot} widgets={config.widgets} onChanged={reloadConfig} />
          )}
          {page === "Notifications" && snapshot && (
            <NotificationsPage snapshot={snapshot} config={config} />
          )}
          {page === "Settings" && (
            <SettingsPage
              config={config}
              hasPassword={hasPassword}
              hasLinkKey={hasLinkKey}
              onSaved={async () => {
                await reloadConfig();
                await reloadSnapshot();
              }}
            />
          )}
        </div>
      </main>
    </div>
  );
}
