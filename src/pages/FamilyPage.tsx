import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { Button, Field, Panel, Toggle } from "../components";
import { t, tn } from "../i18n";
import type { FamilyConfig, FamilyView } from "../types";

const STATUS_LABEL: Record<string, string> = {
  off: tn("Rules off"),
  ok: tn("Within the rules"),
  time_up: tn("Time is up"),
  outside_hours: tn("Outside the allowed hours"),
};

/** Screen time and family rules (family.rs), new in 0.8.1. */
export default function FamilyPage() {
  const [view, setView] = useState<FamilyView | null>(null);
  const [draft, setDraft] = useState<FamilyConfig | null>(null);
  const [limit, setLimit] = useState("0");
  const [apps, setApps] = useState("");
  const [warnings, setWarnings] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const load = useCallback(async (resetDraft: boolean) => {
    const next = await api.getFamily();
    setView(next);
    if (resetDraft) {
      setDraft(next.rules);
      setLimit(String(next.rules.daily_limit_min));
      setApps(next.rules.blocked_apps.join(", "));
      setWarnings(next.rules.warn_at_min.join(", "));
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
      setMsg(t("Saved."));
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
      // Not a number becomes 0, which the app refuses with the reason.
      warn_at_min: warnings
        .split(/[,;]+/)
        .map((minutes) => minutes.trim())
        .filter(Boolean)
        .map((minutes) => (/^\d+$/.test(minutes) ? parseInt(minutes, 10) : 0)),
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
      <Panel title={t("Screen time")}>
        <p className="text-[12px] text-muted mb-3 leading-relaxed">
          {t(
            "Deskmate counts how long this Windows account is in use today: unlocked, with keyboard or mouse input in the last 5 minutes, or something playing full screen. It resets at midnight. Turned on, Home Assistant gets the screen time and can set the rules below. Nothing goes to Microsoft or any other cloud.",
          )}
        </p>
        <div className="flex items-center justify-between h-9">
          <span className="text-[13px]">{t("Count screen time on this account")}</span>
          <Toggle
            on={active}
            disabled={busy || locked}
            onChange={(v) => void save({ ...view.rules, enabled: v })}
          />
        </div>
        {active && (
          <div className="mt-3 grid grid-cols-4 gap-3 text-[13px]">
            <div>
              <span className="microlabel">{t("Today")}</span>
              <p className="mono mt-1">{view.today_min} min</p>
            </div>
            <div>
              <span className="microlabel">{t("Left")}</span>
              <p className="mono mt-1">
                {view.left_min === null ? t("no limit") : `${view.left_min} min`}
              </p>
            </div>
            <div>
              <span className="microlabel">{t("Extra today")}</span>
              <p className="mono mt-1">{view.bonus_min + view.local_bonus_min} min</p>
            </div>
            <div>
              <span className="microlabel">{t("Now")}</span>
              <p className="mt-1">{t(STATUS_LABEL[view.status] ?? view.status)}</p>
            </div>
          </div>
        )}
        {active && !view.rules.local_extra_off && (
          <div className="mt-3 pt-3 border-t border-hairline">
            <div className="flex items-center justify-between gap-3">
              <p className="text-[12px] text-muted leading-relaxed">
                {t(
                  "More time without Home Assistant: an administrator types the Windows password and today's limit grows by {grant} minutes, up to {max} minutes a day. Added this way today: {added} min.",
                  {
                    grant: view.local_grant_min,
                    max: view.local_daily_max_min,
                    added: view.local_bonus_min,
                  },
                )}
              </p>
              <Button
                onClick={() =>
                  void elevated(
                    api.addLocalScreenTime,
                    t("{grant} minutes added.", { grant: view.local_grant_min }),
                  )
                }
                disabled={busy || view.local_bonus_min >= view.local_daily_max_min}
              >
                +{view.local_grant_min} min
              </Button>
            </div>
          </div>
        )}
      </Panel>

      {active && (
        <Panel title={t("Rules")}>
          {locked && (
            <p className="text-[12px] border border-hairline-strong rounded p-2 mb-3 leading-relaxed">
              {t(
                "An administrator protected this account. The rules can only be changed in Home Assistant.",
              )}
            </p>
          )}
          <div className="flex items-center justify-between h-9">
            <span className="text-[13px]">{t("Enforce the limit and the allowed hours")}</span>
            <Toggle
              on={draft.rules_on}
              disabled={locked}
              onChange={(v) => setDraft({ ...draft, rules_on: v })}
            />
          </div>
          <div className="mt-2 grid grid-cols-3 gap-3">
            <Field
              label={t("Daily limit (minutes)")}
              value={limit}
              onChange={setLimit}
              placeholder="0 = no limit"
            />
            <Field
              label={t("Allowed from")}
              value={draft.allowed_from}
              onChange={(v) => setDraft({ ...draft, allowed_from: v })}
              placeholder="07:00"
            />
            <Field
              label={t("Allowed until")}
              value={draft.allowed_until}
              onChange={(v) => setDraft({ ...draft, allowed_until: v })}
              placeholder="21:00"
            />
          </div>
          <p className="mt-1 text-[12px] text-muted">
            {t('Leave the hours empty for no schedule. "22:00" to "02:00" works across midnight.')}
          </p>
          <div className="mt-3 flex items-center justify-between h-9">
            <span className="text-[13px]">
              {t("Allow extra time with an administrator password on this computer")}
            </span>
            <Toggle
              on={!draft.local_extra_off}
              disabled={locked}
              onChange={(v) => setDraft({ ...draft, local_extra_off: !v })}
            />
          </div>
          <label className="block mt-3">
            <span className="microlabel">{t("When time is up")}</span>
            <select
              value={draft.time_up_action === "sign_out" ? "sign_out" : "lock"}
              disabled={locked}
              onChange={(e) => setDraft({ ...draft, time_up_action: e.target.value })}
              className="mt-1 w-full h-9 px-2 bg-panel border border-hairline-strong rounded text-ink text-[13px] focus-visible:border-ink"
            >
              <option value="lock">{t("Lock the computer (open programs keep running)")}</option>
              <option value="sign_out">{t("Sign out (unsaved work can be lost)")}</option>
            </select>
          </label>
          <div className="mt-3">
            <Field
              label={t("Warnings (minutes before)")}
              value={warnings}
              onChange={setWarnings}
              placeholder="10, 5, 1"
              hint={t(
                "Up to five times between 1 and 120 minutes, separated by commas. Empty means 10, 5 and 1.",
              )}
            />
          </div>
          <p className="mt-1 text-[12px] text-muted">
            {t(
              "When the time is up there is a minute to save, with a button to ask Home Assistant for more time. Unlocking again locks after 20 seconds.",
            )}
          </p>

          <div className="mt-4 flex items-center justify-between h-9">
            <span className="text-[13px]">{t("Block these programs")}</span>
            <Toggle
              on={draft.block_apps}
              disabled={locked}
              onChange={(v) => setDraft({ ...draft, block_apps: v })}
            />
          </div>
          <Field
            label={t("Programs")}
            value={apps}
            onChange={setApps}
            placeholder="steam, robloxplayerbeta, discord"
            hint={t(
              "Program names separated by commas. They are closed as soon as they start; Task Manager shows the names.",
            )}
          />

          {!locked && (
            <div className="mt-3 flex items-center gap-3">
              <Button kind="primary" onClick={saveRules} disabled={busy}>
                {t("Save rules")}
              </Button>
              {msg && <span className="text-[12px] text-muted">{msg}</span>}
            </div>
          )}
        </Panel>
      )}

      {active && (
        <Panel title={t("Protection")}>
          <p className="text-[12px] text-muted mb-3 leading-relaxed">
            {t(
              "A child can close Deskmate or edit its settings. Protecting this account needs an administrator password once. Then Windows starts Deskmate again within a minute when it is closed, the rules can only be changed in Home Assistant, and Home Assistant puts back any rule changed on this computer. Use a standard (not administrator) Windows account for the child.",
            )}
          </p>
          {view.admin_account && (
            <p className="text-[12px] border border-hairline-strong rounded p-2 mb-3 leading-relaxed">
              {t(
                "This Windows account is an administrator, so it could remove the protection again. It only holds on a standard account.",
              )}
            </p>
          )}
          <div className="flex items-center justify-between gap-3 min-h-9 text-[13px]">
            <span>
              {view.protected
                ? `${t("Protected")}${
                    !view.watchdog_installed
                      ? t(" (the restart task is missing; protect again)")
                      : view.watchdog_outdated
                        ? t(" (the restart helper is from an older Deskmate)")
                        : ""
                  }`
                : t("Not protected")}
            </span>
            {view.protected ? (
              <div className="flex gap-2">
                {view.watchdog_outdated && (
                  <Button
                    kind="primary"
                    disabled={busy}
                    onClick={() =>
                      void elevated(
                        api.protectFamilyAccount,
                        t("The restart helper is up to date."),
                      )
                    }
                  >
                    {t("Update helper")}
                  </Button>
                )}
                <Button
                  disabled={busy}
                  onClick={() =>
                    void elevated(api.unprotectFamilyAccount, t("Protection removed."))
                  }
                >
                  {t("Remove protection")}
                </Button>
              </div>
            ) : (
              <Button
                kind="primary"
                disabled={busy}
                onClick={() =>
                  void elevated(api.protectFamilyAccount, t("This account is protected."))
                }
              >
                {t("Protect this account")}
              </Button>
            )}
          </div>
          {locked && msg && <p className="mt-2 text-[12px] text-muted">{msg}</p>}
        </Panel>
      )}

      {active && (
        <Panel title={t("In Home Assistant")}>
          <ul className="text-[12px] text-muted space-y-1.5 leading-relaxed">
            <li>
              <span className="text-ink">{t("Screen time today")}</span>,{" "}
              <span className="text-ink">{t("Screen time left")}</span>
              {t(" and ")}
              <span className="text-ink">{t("Screen time status")}</span>
              {t(" sensors.")}
            </li>
            <li>
              <span className="text-ink">{t("Screen time rules")}</span>,{" "}
              <span className="text-ink">{t("Daily screen time limit")}</span>,{" "}
              <span className="text-ink">{t("Allowed from")}</span> /{" "}
              <span className="text-ink">{t("Allowed until")}</span>,{" "}
              <span className="text-ink">{t("Block apps")}</span>,{" "}
              <span className="text-ink">{t("Blocked apps")}</span>
              {t(" and ")}
              <span className="text-ink">{t("When time is up")}</span>
              {t(" to change the rules.")}
            </li>
            <li>
              <span className="text-ink">{t("Extra time today")}</span>
              {t(": minutes on top of the limit, back to 0 at midnight.")}
            </li>
            <li>
              <span className="text-ink">{t("Lock with message")}</span>
              {t(": shows the text, then locks 10 seconds later.")}
            </li>
            <li>
              {t('"Ask for more time" fires the ')}
              <span className="mono">deskmate_link_notify_action</span>
              {t(" event with action ")}
              <span className="mono">screen_time_more</span>
              {t(", ready for an automation that asks you on your phone.")}
            </li>
          </ul>
        </Panel>
      )}
    </>
  );
}
