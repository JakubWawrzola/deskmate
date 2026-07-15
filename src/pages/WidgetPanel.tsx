import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../api";
import type { WidgetState } from "../types";

const POLL_MS = 3000;

/** Content of the "widget" window: HA entity tiles, always-on-top, draggable. */
export default function WidgetPanel() {
  const [states, setStates] = useState<WidgetState[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const busy = useRef(false);

  const refresh = async () => {
    if (busy.current) return;
    busy.current = true;
    try {
      setStates(await api.widgetStates());
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      busy.current = false;
    }
  };

  useEffect(() => {
    void refresh();
    const t = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(t);
  }, []);

  const toggle = async (s: WidgetState) => {
    if (!s.togglable) return;
    // optymistycznie: on<->off od razu, polling i tak wyrowna
    setStates((prev) =>
      prev
        ? prev.map((p) =>
            p.entity_id === s.entity_id ? { ...p, state: p.state === "on" ? "off" : "on" } : p,
          )
        : prev,
    );
    try {
      await api.widgetToggle(s.entity_id);
      setTimeout(() => void refresh(), 600);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="h-full flex flex-col bg-paper border border-hairline-strong rounded-md overflow-hidden select-none">
      {/* pasek: drag region + zamkniecie (hide) */}
      <header
        data-tauri-drag-region
        className="h-8 shrink-0 flex items-center justify-between px-3 bg-panel border-b border-hairline cursor-move"
      >
        <span data-tauri-drag-region className="microlabel pointer-events-none">
          HOMEOS
        </span>
        <button
          aria-label="Hide widgets"
          onClick={() => void getCurrentWindow().hide()}
          className="text-muted hover:text-ink text-[14px] leading-none px-1"
        >
          x
        </button>
      </header>

      <div className="flex-1 overflow-y-auto p-2 space-y-2">
        {error && (
          <p className="text-[11px] text-muted leading-snug p-2">
            {error.includes("not configured")
              ? "Configure the Home Assistant API (URL + token) in Deskmate Settings, then add tiles on the Widgets page."
              : error}
          </p>
        )}
        {!error && states !== null && states.length === 0 && (
          <p className="text-[11px] text-muted leading-snug p-2">
            No tiles. Add entities on the Widgets page in Deskmate.
          </p>
        )}
        {states?.map((s) => {
          const on = s.state === "on" || s.state === "playing" || s.state === "open";
          return (
            <button
              key={s.entity_id}
              onClick={() => void toggle(s)}
              disabled={!s.togglable}
              className={`w-full text-left rounded border px-3 py-2 transition-colors duration-100
                ${on ? "bg-ink text-panel border-ink" : "bg-panel text-ink border-hairline-strong"}
                ${s.togglable ? "cursor-pointer hover:border-ink" : "cursor-default"}`}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="text-[12px] font-medium truncate">{s.label}</span>
                <span className={`mono text-[11px] shrink-0 ${on ? "" : "text-muted"}`}>
                  {s.state}
                </span>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
