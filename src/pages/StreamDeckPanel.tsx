import { useCallback, useEffect, useState } from "react";
import { api, onStreamDeck } from "../api";
import { Button, Panel, StatusDot, Toggle } from "../components";
import type { StreamDeckStatus } from "../types";

type Channel = StreamDeckStatus["channel"];
type Plugin = StreamDeckStatus["plugin"];

function describeChannel(channel: Channel): string {
  switch (channel.state) {
    case "off":
      return "Off.";
    case "starting":
      return "Starting...";
    case "listening":
      if (channel.connected === 0) return "Listening. No program is connected.";
      return channel.connected === 1
        ? "Listening. 1 program is connected."
        : `Listening. ${channel.connected} programs are connected.`;
    case "name_taken":
      return "Another program on this computer has taken the channel's name, so Deskmate does not open it. Close that program, or sign out of Windows and back in; Deskmate keeps trying.";
    case "failed":
      return "The channel could not be opened. Turn the switch off and on again.";
  }
}

function describeInstalled(plugin: Plugin): string {
  if (!plugin.installed) return "not installed";
  return plugin.installed_version ?? "installed, version unknown";
}

/** Settings > Stream Deck: the local channel and the plugin (streamdeck/). */
export default function StreamDeckPanel() {
  const [status, setStatus] = useState<StreamDeckStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const refresh = useCallback(
    () =>
      api
        .streamdeckStatus()
        .then(setStatus)
        .catch((e) => setMsg(String(e))),
    [],
  );

  useEffect(() => {
    void refresh();
    const subscription = onStreamDeck(() => void refresh());
    return () => {
      void subscription.then((unlisten) => unlisten());
    };
  }, [refresh]);

  const run = async (action: () => Promise<void>, done: string | null) => {
    setBusy(true);
    setMsg(null);
    try {
      await action();
      setMsg(done);
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
      void refresh();
    }
  };

  const plugin = status?.plugin;
  const canInstall = plugin?.stream_deck_found === true && plugin.bundled_version !== null;

  return (
    <Panel title="Stream Deck">
      <p className="text-[12px] text-muted mb-3 leading-relaxed">
        The Deskmate plugin for Elgato Stream Deck puts Home Assistant entities, services and scenes
        on keys, together with the hotkeys and tray quick actions you defined here. It talks to
        Deskmate on this computer, so the plugin needs no Home Assistant token of its own.
      </p>
      {status && (
        <div className="space-y-3 text-[13px]">
          <div className="flex items-center justify-between gap-3 min-h-9">
            <div>
              <span>Let Stream Deck use Deskmate</span>
              <p className="text-[12px] text-muted">
                While this is on, programs running as you on this computer can list and control your
                Home Assistant entities, and run your hotkeys and tray quick actions, through
                Deskmate.
              </p>
            </div>
            <Toggle
              label="Let Stream Deck use Deskmate"
              on={status.enabled}
              disabled={busy}
              onChange={(enabled) => void run(() => api.setStreamdeckEnabled(enabled), null)}
            />
          </div>
          <p className="flex items-start gap-2" role="status">
            <span className="mt-[5px] shrink-0">
              <StatusDot on={status.channel.state === "listening"} />
            </span>
            <span>Channel: {describeChannel(status.channel)}</span>
          </p>

          <div className="border-t border-hairline pt-3 space-y-2">
            <p>
              Stream Deck software:{" "}
              {status.plugin.stream_deck_found
                ? "found"
                : "not found for this Windows account. Install Stream Deck and start it once."}
            </p>
            <p>
              Plugin in this Deskmate:{" "}
              <span className="mono">
                {status.plugin.bundled_version ?? "this build does not contain the plugin"}
              </span>
            </p>
            <div className="flex items-center justify-between gap-3 min-h-9">
              <span>
                Plugin in Stream Deck:{" "}
                <span className="mono">{describeInstalled(status.plugin)}</span>
              </span>
              <div className="flex gap-2">
                {!status.plugin.installed && (
                  <Button
                    kind="primary"
                    disabled={busy || !canInstall}
                    onClick={() =>
                      void run(
                        () => api.installStreamdeckPlugin(),
                        "Plugin installed. Restart Stream Deck to load it.",
                      )
                    }
                  >
                    Install plugin
                  </Button>
                )}
                {status.plugin.installed && status.plugin.update_available && (
                  <Button
                    kind="primary"
                    disabled={busy || !canInstall}
                    onClick={() =>
                      void run(
                        () => api.installStreamdeckPlugin(),
                        "Plugin updated. Start Stream Deck again to load it.",
                      )
                    }
                  >
                    Update plugin
                  </Button>
                )}
                {status.plugin.installed && (
                  <Button
                    disabled={busy}
                    onClick={() => void run(() => api.removeStreamdeckPlugin(), "Plugin removed.")}
                  >
                    Remove plugin
                  </Button>
                )}
              </div>
            </div>
            <p className="text-[12px] text-muted leading-relaxed">
              Stream Deck loads a plugin when it starts, so restart Stream Deck after installing.
              Before updating or removing the plugin, quit Stream Deck (right-click its icon next to
              the clock, then Quit Stream Deck): it keeps the plugin's files in use while it runs.
            </p>
          </div>
        </div>
      )}
      {msg && (
        <p className="mt-2 text-[13px]" role="status">
          {msg}
        </p>
      )}
    </Panel>
  );
}
