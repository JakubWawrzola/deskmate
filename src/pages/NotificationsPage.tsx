import { useState } from "react";
import { api } from "../api";
import { Button, EmptyState, Panel } from "../components";
import type { AppConfig, Snapshot } from "../types";

export default function NotificationsPage({
  snapshot,
  config,
}: {
  snapshot: Snapshot;
  config: AppConfig;
}) {
  const topic = `deskmate/${config.node_id}/notify`;
  const actionTopic = `deskmate/${config.node_id}/notify/action`;
  const [testResult, setTestResult] = useState<"ok" | string | null>(null);

  const handleTest = async () => {
    setTestResult(null);
    try {
      await api.testToast();
      setTestResult("ok");
    } catch (e) {
      setTestResult(String(e));
    }
  };
  const example = `{
  "title": "Dishwasher",
  "message": "Ready to unload",
  "image": "https://your-ha/local/img.png",
  "actions": [
    {"title": "Done", "action": "done"},
    {"title": "Snooze", "action": "snooze"}
  ]
}`;

  return (
    <>
      <Panel
        title="Windows notifications from Home Assistant"
        action={<Button onClick={() => void handleTest()}>Send test toast</Button>}
      >
        <p className="text-[13px] text-muted leading-relaxed">
          Publish a JSON payload to <span className="mono text-ink">{topic}</span> and
          it shows up as a Windows toast, with an optional image and action buttons.
        </p>
        {testResult === "ok" && (
          <p className="text-[12px] text-muted mt-2 leading-relaxed">
            Toast sent. If nothing appeared, Windows is suppressing it — check{" "}
            <span className="text-ink">Settings → System → Notifications</span> (turn them on,
            and turn off <span className="text-ink">Do not disturb / Focus</span>).
          </p>
        )}
        {testResult && testResult !== "ok" && (
          <p className="text-[12px] text-red-600 dark:text-red-400 mt-2 leading-relaxed mono">
            {testResult}
          </p>
        )}
        <pre className="mono text-[12px] bg-paper border border-hairline rounded p-3 mt-2 overflow-x-auto select-text">
          {example}
        </pre>
        <p className="text-[12px] text-muted mt-2 leading-relaxed">
          Clicking a button publishes <span className="mono text-ink">{`{"action":"done"}`}</span> to{" "}
          <span className="mono text-ink">{actionTopic}</span> — catch it with an
          automation in HA. See docs/HA-SETUP.md for a ready-made script.
        </p>
      </Panel>

      <Panel title="History (this session)">
        {snapshot.notifications.length === 0 && <EmptyState text="Nothing received yet." />}
        {snapshot.notifications.map((n, i) => (
          <div key={i} className="py-2 border-b border-hairline last:border-b-0">
            <div className="flex items-baseline justify-between gap-3">
              <p className="text-[13px] font-medium">{n.title}</p>
              <span className="mono text-[11px] text-faint shrink-0">{n.received_at}</span>
            </div>
            <p className="text-[13px] text-muted">{n.message}</p>
            {n.image && <p className="mono text-[11px] text-faint truncate">{n.image}</p>}
          </div>
        ))}
      </Panel>
    </>
  );
}
