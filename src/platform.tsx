import { createContext, type ReactNode, useContext, useEffect, useState } from "react";
import { api } from "./api";
import { Panel } from "./components";
import type { PlatformCapabilities } from "./types";

/** What the window assumes until the backend has answered: Windows, as before. */
export const WINDOWS_CAPABILITIES: PlatformCapabilities = {
  platform: "windows",
  session_type: "windows",
  tray: true,
  global_shortcuts: true,
  clipboard: true,
  active_window: true,
  notifications: true,
  notification_buttons: true,
  media: true,
  volume: true,
  lock: true,
  idle: true,
  open_url: true,
  keyring_problem: null,
};

const PlatformContext = createContext<PlatformCapabilities>(WINDOWS_CAPABILITIES);

/** Asks the backend once what this computer's desktop offers (src-tauri/src/platform.rs). */
export function PlatformProvider({ children }: { children: ReactNode }) {
  const [capabilities, setCapabilities] = useState<PlatformCapabilities>(WINDOWS_CAPABILITIES);
  useEffect(() => {
    api
      .platformCapabilities()
      .then(setCapabilities)
      .catch(() => setCapabilities(WINDOWS_CAPABILITIES));
  }, []);
  return <PlatformContext.Provider value={capabilities}>{children}</PlatformContext.Provider>;
}

export function usePlatform(): PlatformCapabilities {
  return useContext(PlatformContext);
}

export function isLinux(capabilities: PlatformCapabilities): boolean {
  return capabilities.platform === "linux";
}

/** "Follow Windows" and the like, in the words of the platform. */
export function systemName(capabilities: PlatformCapabilities): string {
  return isLinux(capabilities) ? "the system" : "Windows";
}

/** One line where a feature would be, saying why it is not there. */
export function PlatformNote({ children }: { children: ReactNode }) {
  return <p className="text-[12px] text-muted leading-relaxed">{children}</p>;
}

/** A panel that only says why its feature is missing on this computer. */
export function UnavailablePanel({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Panel title={title} unavailable={children}>
      {null}
    </Panel>
  );
}
