import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { Button, Field, Panel, Toggle } from "../components";
import type { FamilyConfig, FamilyView } from "../types";

const STATUS_LABEL: Record<string, string> = {
  off: "Rules off",
  ok: "Within the rules",
  time_up: "Time is up",
  outside_hours: "Outside the allowed hours",
};

/** Screen time and family rules (family.rs), new in 0.8.1. */
export default function FamilyPage() {
  const [view, setView] = useState<FamilyView | null>(null);
  const [draft, setDraft] = useState<FamilyConfig | null>(null);
  const [limit, setLimit] = useState("0");
  const [apps, setApps] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const load = useCallback(async (resetDraft: boolean) => {
    const next = await api.getFamily();
    setView(next);
    if (resetDraft) {
      setDraft(next.rules);
      setLimit(String(next.rules.daily_limit_min));
      setApps(next.rules.blocked_apps.join(", "));
    }
  }, []);

  useEffect(() => {
    load(true).catch((e) => setMsg(String(e)));
    const timer = setInterval(() => void load(false).catch(() => undefined), 15000);
    return () => clearInterval(timer);
  }, [load]);

  if (!view || !draft) return null;
  const locked = view.protected;

  const save = async (rules: FamilyConfig) => {
    setBusy(true);
    setMsg(null);
    try {
      await api.setFamily(rules);
      await load(true);
      setMsg("Saved.");
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
    }
  };

  const saveRules = () =>
    save({
      ...draft,
      daily_limit_min: Math.max(0, parseInt(limit, 10) || 0),
      blocked_apps: apps
        .split(/[,;\n]+/)
        .map((app) => app.trim())
        .filter(Boolean),
    });

  const elevated = async (action: () => Promise<void>, done: string) => {
    setBusy(true);
    setMsg(null);
    try {
      await action();
      await load(true);
      setMsg(done);
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
    }
  };

  const active = view.rules.enabled || view.protected;

  return (
    <>
      <Panel title="Screen time">
        <p className="text-[12px] text-muted mb-3 leading-relaxed">
          Deskmate counts how long this Windows account is in use today: unlocked, with keyboard or mouse input in the
          last 5 minutes, or something playing full screen. It resets at midnight. Turned on, Home Assistant gets the
          screen time and can set the rules below. Nothing goes to Microsoft or any other cloud.
        </p>
        <div className="flex items-center justify-between h-9">
          <span className="text-[13px]">Count screen time on this account</span>
          <Toggle
            on={active}
            disabled={busy || locked}
            onChange={(v) => void save({ ...view.rules, enabled: v })}
          />
        </div>
        {active && (
          <div className="mt-3 grid grid-cols-4 gap-3 text-[13px]">
            <div>
              <span className="microlabel">Today</span>
              <p className="mono mt-1">{view.today_min} min</p>
            </div>
            <div>
              <span className="microlabel">Left</span>
              <p className="mono mt-1">{view.left_min === null ? "no limit" : `${view.left_min} min`}</p>
            </div>
            <div>
              <span className="microlabel">Extra today</span>
              <p className="mono mt-1">{view.bonus_min} min</p>
            </div>
            <div>
              <span className="microlabel">Now</span>
              <p className="mt-1">{STATUS_LABEL[view.status] ?? view.status}</p>
            </div>
          </div>
        )}
      </Panel>

      {active && (
        <Panel title="Rules">
          {locked && (
            <p className="text-[12px] border border-hairline-strong rounded p-2 mb-3 leading-relaxed">
              An administrator protected this account. The rules can only be changed in Home Assistant.
            </p>
          )}
          <div className="flex items-center justify-between h-9">
            <span className="text-[13px]">Enforce the limit and the allowed hours</span>
            <Toggle on={draft.rules_on} disabled={locked} onChange={(v) => setDraft({ ...draft, rules_on: v })} />
          </div>
          <div className="mt-2 grid grid-cols-3 gap-3">
            <Field label="Daily limit (minutes)" value={limit} onChange={setLimit} placeholder="0 = no limit" />
            <Field
              label="Allowed from"
              value={draft.allowed_from}
              onChange={(v) => setDraft({ ...draft, allowed_from: v })}
              placeholder="07:00"
            />
            <Field
              label="Allowed until"
              value={draft.allowed_until}
              onChange={(v) => setDraft({ ...draft, allowed_until: v })}
              placeholder="21:00"
            />
          </div>
          <p className="mt-1 text-[12px] text-muted">
            Leave the hours empty for no schedule. "22:00" to "02:00" works across midnight.
          </p>
          <label className="block mt-3">
            <span className="microlabel">When time is up</span>
            <select
              value={draft.time_up_action === "sign_out" ? "sign_out" : "lock"}
              disabled={locked}
              onChange={(e) => setDraft({ ...draft, time_up_action: e.target.value })}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="lock">Lock the computer (open programs keep running)</option>
              <option value="sign_out">Sign out (unsaved work can be lost)</option>
            </select>
          </label>
          <p className="mt-1 text-[12px] text-muted">
            Warnings come 10, 5 and 1 minute before. When the time is up there is a minute to save, with a button to
            ask Home Assistant for more time. Unlocking again locks after 20 seconds.
          </p>

          <div className="mt-4 flex items-center justify-between h-9">
            <span className="text-[13px]">Block these programs</span>
            <Toggle on={draft.block_apps} disabled={locked} onChange={(v) => setDraft({ ...draft, block_apps: v })} />
          </div>
          <Field
            label="Programs"
            value={apps}
            onChange={setApps}
            placeholder="steam, robloxplayerbeta, discord"
            hint="Program names separated by commas. They are closed as soon as they start; Task Manager shows the names."
          />

          {!locked && (
            <div className="mt-3 flex items-center gap-3">
              <Button kind="primary" onClick={saveRules} disabled={busy}>
                Save rules
              </Button>
              {msg && <span className="text-[12px] text-muted">{msg}</span>}
            </div>
          )}
        </Panel>
      )}

      {active && (
        <Panel title="Protection">
          <p className="text-[12px] text-muted mb-3 leading-relaxed">
            A child can close Deskmate or edit its settings. Protecting this account needs an administrator password
            once. Then Windows starts Deskmate again within a minute when it is closed, the rules can only be changed
            in Home Assistant, and Home Assistant puts back any rule changed on this computer. Use a standard
            (not administrator) Windows account for the child.
          </p>
          {view.admin_account && (
            <p className="text-[12px] border border-hairline-strong rounded p-2 mb-3 leading-relaxed">
              This Windows account is an administrator, so it could remove the protection again. It only holds on a
              standard account.
            </p>
          )}
          <div className="flex items-center justify-between gap-3 min-h-9 text-[13px]">
            <span>
              {view.protected
                ? `Protected${view.watchdog_installed ? "" : " (the restart task is missing; protect again)"}`
                : "Not protected"}
            </span>
            {view.protected ? (
              <Button
                disabled={busy}
                onClick={() => void elevated(api.unprotectFamilyAccount, "Protection removed.")}
              >
                Remove protection
              </Button>
            ) : (
              <Button
                kind="primary"
                disabled={busy}
                onClick={() => void elevated(api.protectFamilyAccount, "This account is protected.")}
              >
                Protect this account
              </Button>
            )}
          </div>
          {locked && msg && <p className="mt-2 text-[12px] text-muted">{msg}</p>}
        </Panel>
      )}

      {active && (
        <Panel title="In Home Assistant">
          <ul className="text-[12px] text-muted space-y-1.5 leading-relaxed">
            <li>
              <span className="text-ink">Screen time today</span>, <span className="text-ink">Screen time left</span>{" "}
              and <span className="text-ink">Screen time status</span> sensors.
            </li>
            <li>
              <span className="text-ink">Screen time rules</span>, <span className="text-ink">Daily screen time limit</span>,{" "}
              <span className="text-ink">Allowed from</span> / <span className="text-ink">until</span>,{" "}
              <span className="text-ink">Block apps</span>, <span className="text-ink">Blocked apps</span> and{" "}
              <span className="text-ink">When time is up</span> to change the rules.
            </li>
            <li>
              <span className="text-ink">Extra time today</span>: minutes on top of the limit, back to 0 at midnight.
            </li>
            <li>
              <span className="text-ink">Lock with message</span>: shows the text, then locks 10 seconds later.
            </li>
            <li>
              "Ask for more time" fires the <span className="mono">deskmate_link_notify_action</span> event with action{" "}
              <span className="mono">screen_time_more</span>, ready for an automation that asks you on your phone.
            </li>
          </ul>
        </Panel>
      )}
    </>
  );
}
