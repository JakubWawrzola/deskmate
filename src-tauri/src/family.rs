//! Screen time and family rules, counted and enforced on this computer.
//!
//! People who use the Microsoft Family Safety integrations with Home Assistant
//! report an unofficial cloud API, sign-ins that expire after a day, limits the
//! API accepts but the device ignores, and five-minute polling. Deskmate
//! already runs in the child's Windows session: it counts the time itself and
//! keeps enforcing when Home Assistant or the internet is down. Home Assistant
//! only sets the rules.
//!
//! A child can close their own Deskmate or edit its settings file. An
//! administrator can therefore protect a Windows account: the account is listed
//! under HKLM, which a standard account cannot change, a scheduled task starts
//! Deskmate again within a minute, the rules can then only be changed from Home
//! Assistant, and Home Assistant puts back a value changed on the computer.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

use crate::config::{AppConfig, FamilyConfig};
use crate::state::AppState;

/// Elevated steps started through UAC from the Family page.
pub const PROTECT_ARG: &str = "--family-protect";
pub const UNPROTECT_ARG: &str = "--family-unprotect";
/// The scheduled task: start Deskmate again if it is not running.
pub const WATCHDOG_ARG: &str = "--family-watchdog";
/// Start without showing the window (used by the watchdog).
pub const HIDDEN_ARG: &str = "--hidden";
/// Notification button that asks Home Assistant for more time.
pub const MORE_TIME_ACTION: &str = "screen_time_more";

const TICK: Duration = Duration::from_secs(5);
/// Without keyboard or mouse input for this long the computer counts as not
/// in use, unless something plays full screen (a film).
const IDLE_LIMIT_SECS: u64 = 300;
const WARN_AT_MIN: [u32; 3] = [10, 5, 1];
/// Time to save work before the first lock; shorter when the child unlocks
/// again while the time is still up.
const FIRST_GRACE: Duration = Duration::from_secs(60);
const AGAIN_GRACE: Duration = Duration::from_secs(20);
const LOCK_MESSAGE_DELAY: Duration = Duration::from_secs(10);
const BLOCK_NOTICE_EVERY: Duration = Duration::from_secs(120);
const FULL_PUBLISH_EVERY: Duration = Duration::from_secs(60);
const SAVE_EVERY: Duration = Duration::from_secs(60);
const MAX_LIMIT_MIN: u32 = 1440;
const MAX_BONUS_MIN: u32 = 600;
const MAX_BLOCKED: usize = 40;
const TIME_PATTERN: &str = "^$|^([01][0-9]|2[0-3]):[0-5][0-9]$";

/// Programs the block list may not close: Windows itself and Deskmate.
const NEVER_BLOCK: &[&str] = &[
    "deskmate.exe",
    "deskmate-watchdog.exe",
    "explorer.exe",
    "dwm.exe",
    "winlogon.exe",
    "csrss.exe",
    "svchost.exe",
    "lsass.exe",
    "services.exe",
    "smss.exe",
    "wininit.exe",
    "sihost.exe",
    "ctfmon.exe",
    "fontdrvhost.exe",
    "logonui.exe",
    "userinit.exe",
];

/// Every family entity as (component, key), for removing them over MQTT.
pub const ENTITIES: &[(&str, &str)] = &[
    ("sensor", "screen_time_today"),
    ("sensor", "screen_time_left"),
    ("sensor", "screen_time_status"),
    ("switch", "family_rules"),
    ("number", "screen_time_limit"),
    ("number", "screen_time_bonus"),
    ("text", "allowed_from"),
    ("text", "allowed_until"),
    ("switch", "block_apps"),
    ("text", "blocked_apps"),
    ("select", "time_up_action"),
    ("text", "lock_message"),
];

const COMMANDS: &[&str] = &[
    "family_rules",
    "screen_time_limit",
    "screen_time_bonus",
    "allowed_from",
    "allowed_until",
    "block_apps",
    "blocked_apps",
    "time_up_action",
    "lock_message",
];

pub fn is_command(key: &str) -> bool {
    COMMANDS.contains(&key)
}

/// Family features on for this account: turned on locally or protected by an
/// administrator.
pub fn is_active(cfg: &AppConfig) -> bool {
    cfg.family.enabled || protection::is_protected()
}

// ------------------------------------------------------------------ entities

/// Entity descriptors for the Deskmate integration (also turned into MQTT
/// discovery). On a protected account the rule entities are owned by Home
/// Assistant ("restore"): it remembers them and puts them back after a change
/// made on the computer.
pub fn link_entities(cfg: &AppConfig) -> Vec<Value> {
    if !is_active(cfg) {
        return Vec::new();
    }
    let restore = protection::is_protected();
    let owned = |mut entity: Value| {
        if restore {
            entity["restore"] = json!(true);
        }
        entity
    };
    vec![
        json!({"key": "screen_time_today", "kind": "sensor", "name": "Screen time today",
               "unit": "min", "device_class": "duration", "state_class": "total_increasing",
               "icon": "mdi:timer-sand"}),
        json!({"key": "screen_time_left", "kind": "sensor", "name": "Screen time left",
               "unit": "min", "device_class": "duration", "icon": "mdi:timer-sand-complete"}),
        json!({"key": "screen_time_status", "kind": "sensor", "name": "Screen time status",
               "icon": "mdi:account-clock"}),
        owned(json!({"key": "family_rules", "kind": "switch", "name": "Screen time rules",
                     "icon": "mdi:account-child"})),
        owned(json!({"key": "screen_time_limit", "kind": "number", "name": "Daily screen time limit",
                     "unit": "min", "min": 0, "max": MAX_LIMIT_MIN, "step": 5, "icon": "mdi:timer-cog"})),
        json!({"key": "screen_time_bonus", "kind": "number", "name": "Extra time today",
               "unit": "min", "min": 0, "max": MAX_BONUS_MIN, "step": 5, "icon": "mdi:timer-plus"}),
        owned(json!({"key": "allowed_from", "kind": "text", "name": "Allowed from", "mode": "text",
                     "max": 5, "pattern": TIME_PATTERN, "icon": "mdi:clock-start"})),
        owned(json!({"key": "allowed_until", "kind": "text", "name": "Allowed until", "mode": "text",
                     "max": 5, "pattern": TIME_PATTERN, "icon": "mdi:clock-end"})),
        owned(json!({"key": "block_apps", "kind": "switch", "name": "Block apps",
                     "icon": "mdi:application-cog"})),
        owned(json!({"key": "blocked_apps", "kind": "text", "name": "Blocked apps", "mode": "text",
                     "max": 255, "icon": "mdi:application-remove"})),
        owned(json!({"key": "time_up_action", "kind": "select", "name": "When time is up",
                     "options": ["Lock", "Sign out"], "icon": "mdi:lock-clock"})),
        json!({"key": "lock_message", "kind": "text", "name": "Lock with message", "mode": "text",
               "max": 200, "icon": "mdi:message-lock"}),
    ]
}

fn on_off(value: bool) -> String {
    if value { "ON" } else { "OFF" }.into()
}

fn action_label(action: &str) -> &'static str {
    if action == "sign_out" {
        "Sign out"
    } else {
        "Lock"
    }
}

/// States of the rule entities, from the config.
fn rule_values(rules: &FamilyConfig) -> HashMap<String, String> {
    HashMap::from([
        ("family_rules".to_string(), on_off(rules.rules_on)),
        ("screen_time_limit".to_string(), rules.daily_limit_min.to_string()),
        ("allowed_from".to_string(), rules.allowed_from.clone()),
        ("allowed_until".to_string(), rules.allowed_until.clone()),
        ("block_apps".to_string(), on_off(rules.block_apps)),
        ("blocked_apps".to_string(), rules.blocked_apps.join(", ")),
        ("time_up_action".to_string(), action_label(&rules.time_up_action).to_string()),
        ("lock_message".to_string(), String::new()),
    ])
}

// ------------------------------------------------------------------ parsing

fn parse_hhmm(text: &str) -> Option<u32> {
    let (hours, minutes) = text.trim().split_once(':')?;
    let hours: u32 = hours.parse().ok()?;
    let minutes: u32 = minutes.parse().ok()?;
    (hours < 24 && minutes < 60 && text.trim().len() <= 5).then_some(hours * 60 + minutes)
}

/// "" stays empty (no limit that side); "7:05" becomes "07:05".
fn normalize_time(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    let minutes = parse_hhmm(text).ok_or("use HH:MM, for example 07:30")?;
    Ok(format!("{:02}:{:02}", minutes / 60, minutes % 60))
}

fn parse_minutes(text: &str, max: u32) -> Result<u32, String> {
    let value: f64 = text.trim().parse().map_err(|_| "not a number of minutes".to_string())?;
    if !value.is_finite() || value < 0.0 {
        return Err("not a number of minutes".into());
    }
    Ok((value.round() as u32).min(max))
}

/// "Steam, roblox.exe; Discord" -> ["steam.exe", "roblox.exe", "discord.exe"]
fn parse_app_list(text: &str) -> Result<Vec<String>, String> {
    let mut apps = Vec::new();
    for part in text.split([',', ';', '\n']) {
        if part.trim().is_empty() {
            continue;
        }
        let name = crate::processes::normalize_name(part)
            .ok_or_else(|| format!("not a program name: {}", part.trim()))?;
        if NEVER_BLOCK.contains(&name.as_str()) {
            return Err(format!("{name} is part of Windows or Deskmate and cannot be blocked"));
        }
        if !apps.contains(&name) {
            apps.push(name);
        }
    }
    if apps.len() > MAX_BLOCKED || apps.join(", ").len() > 255 {
        return Err("the list is too long".into());
    }
    Ok(apps)
}

fn normalize_action(text: &str) -> Result<String, String> {
    match text.trim().to_lowercase().as_str() {
        "" | "lock" => Ok("lock".into()),
        "sign out" | "sign_out" => Ok("sign_out".into()),
        _ => Err("when time is up: Lock or Sign out".into()),
    }
}

/// Checks and normalizes rules coming from the Family page.
pub fn normalize(mut rules: FamilyConfig) -> Result<FamilyConfig, String> {
    rules.daily_limit_min = rules.daily_limit_min.min(MAX_LIMIT_MIN);
    rules.allowed_from = normalize_time(&rules.allowed_from)?;
    rules.allowed_until = normalize_time(&rules.allowed_until)?;
    rules.blocked_apps = parse_app_list(&rules.blocked_apps.join(","))?;
    rules.time_up_action = normalize_action(&rules.time_up_action)?;
    Ok(rules)
}

// ------------------------------------------------------------------ evaluation

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Off,
    Ok,
    TimeUp,
    OutsideHours,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Off => "off",
            Status::Ok => "ok",
            Status::TimeUp => "time_up",
            Status::OutsideHours => "outside_hours",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct Usage {
    /// Local date, YYYY-MM-DD; a new day starts from zero.
    date: String,
    seconds: u64,
    /// Extra minutes for today only.
    bonus_min: u32,
    /// Warnings already shown (minutes left).
    warned: Vec<u32>,
}

#[derive(Debug, PartialEq)]
struct Evaluation {
    status: Status,
    /// Minutes of the daily limit left (rounded up); None without a limit.
    left_min: Option<u32>,
    /// Minutes until the next stop (limit or end of the allowed hours).
    next_stop_min: Option<u32>,
    /// The next stop is the end of the allowed hours, not the limit.
    stop_is_bedtime: bool,
}

/// (from, until) in minutes of the day, None when no hours are set.
fn schedule(rules: &FamilyConfig) -> Option<(u32, u32)> {
    let from = parse_hhmm(&rules.allowed_from);
    let until = parse_hhmm(&rules.allowed_until);
    if from.is_none() && until.is_none() {
        return None;
    }
    let (from, until) = (from.unwrap_or(0), until.unwrap_or(1440));
    (from != until).then_some((from, until))
}

fn evaluate(rules: &FamilyConfig, usage: &Usage, minute_of_day: u32) -> Evaluation {
    let left_secs = (rules.daily_limit_min > 0).then(|| {
        (u64::from(rules.daily_limit_min + usage.bonus_min) * 60).saturating_sub(usage.seconds)
    });
    let left_min = left_secs.map(|secs| secs.div_ceil(60) as u32);
    let (inside, bedtime_in) = match schedule(rules) {
        None => (true, None),
        Some((from, until)) => {
            let inside = if from < until {
                minute_of_day >= from && minute_of_day < until
            } else {
                minute_of_day >= from || minute_of_day < until
            };
            let until_in = (until + 1440 - minute_of_day) % 1440;
            (inside, inside.then_some(if until_in == 0 { 1440 } else { until_in }))
        }
    };
    let status = if !rules.rules_on {
        Status::Off
    } else if !inside {
        Status::OutsideHours
    } else if left_secs == Some(0) {
        Status::TimeUp
    } else {
        Status::Ok
    };
    let (next_stop_min, stop_is_bedtime) = match (left_min, bedtime_in) {
        (Some(left), Some(bed)) if bed < left => (Some(bed), true),
        (Some(left), _) => (Some(left), false),
        (None, Some(bed)) => (Some(bed), true),
        (None, None) => (None, false),
    };
    Evaluation { status, left_min, next_stop_min, stop_is_bedtime }
}

// ------------------------------------------------------------------ usage file

static USAGE: Mutex<Option<Usage>> = Mutex::new(None);

fn usage_path() -> PathBuf {
    crate::config::config_path()
        .parent()
        .map(|dir| dir.join("screen-time.json"))
        .unwrap_or_else(|| PathBuf::from("screen-time.json"))
}

fn load_usage() -> Usage {
    std::fs::read_to_string(usage_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_usage(usage: &Usage) {
    let path = usage_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(raw) = serde_json::to_string(usage) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, raw).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// Today's usage; a new day starts from zero.
fn with_usage<T>(today: &str, f: impl FnOnce(&mut Usage) -> T) -> T {
    let mut guard = USAGE.lock().unwrap_or_else(|e| e.into_inner());
    let usage = guard.get_or_insert_with(load_usage);
    if usage.date != today {
        *usage = Usage { date: today.to_string(), ..Default::default() };
        save_usage(usage);
    }
    f(usage)
}

struct Clock {
    date: String,
    minute_of_day: u32,
}

#[cfg(windows)]
fn clock() -> Clock {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let now = unsafe { GetLocalTime() };
    Clock {
        date: format!("{:04}-{:02}-{:02}", now.wYear, now.wMonth, now.wDay),
        minute_of_day: u32::from(now.wHour) * 60 + u32::from(now.wMinute),
    }
}

#[cfg(not(windows))]
fn clock() -> Clock {
    Clock { date: String::new(), minute_of_day: 0 }
}

#[cfg(windows)]
fn in_use() -> (bool, bool) {
    let locked = crate::sensors::win::is_locked();
    let active = !locked
        && (crate::sensors::win::idle_seconds() < IDLE_LIMIT_SECS
            || crate::sensors::win::is_presenting());
    (locked, active)
}

#[cfg(not(windows))]
fn in_use() -> (bool, bool) {
    (false, false)
}

// ------------------------------------------------------------------ the loop

#[derive(Default)]
struct Runtime {
    last_tick: Option<Instant>,
    carry_ms: u128,
    last_saved: Option<Instant>,
    /// Lock or sign-out countdown: (started, grace).
    pending: Option<(Instant, Duration)>,
    enforced_once: bool,
    blocked_notice: HashMap<String, Instant>,
    published: HashMap<String, String>,
    last_full_publish: Option<Instant>,
}

fn toast(title: &str, message: &str, ask_for_time: bool) {
    let payload = crate::notify::NotifyPayload {
        title: title.into(),
        message: message.into(),
        image: None,
        actions: if ask_for_time {
            vec![crate::notify::NotifyAction {
                title: "Ask for more time".into(),
                action: MORE_TIME_ACTION.into(),
            }]
        } else {
            Vec::new()
        },
    };
    if let Err(e) = crate::notify::show_toast(&payload) {
        log::warn!("family: {e}");
    }
}

/// One step: count the time, warn, enforce, block. Returns all values.
fn tick(rt: &mut Runtime, rules: &FamilyConfig) -> HashMap<String, String> {
    let now = Instant::now();
    let clock = clock();
    // A gap longer than a few ticks is sleep or hibernation, not use.
    let elapsed = rt.last_tick.map(|t| now - t).unwrap_or_default().min(TICK * 3);
    rt.last_tick = Some(now);
    let (locked, active) = in_use();

    let save_due = rt.last_saved.is_none_or(|t| now - t >= SAVE_EVERY);
    let (usage, evaluation) = with_usage(&clock.date, |usage| {
        if active {
            rt.carry_ms += elapsed.as_millis();
            usage.seconds += (rt.carry_ms / 1000) as u64;
            rt.carry_ms %= 1000;
        }
        let evaluation = evaluate(rules, usage, clock.minute_of_day);
        // More time given: the warnings may come again.
        if let Some(next) = evaluation.next_stop_min {
            usage.warned.retain(|&minutes| minutes >= next);
        }
        if rules.rules_on && evaluation.status == Status::Ok {
            if let Some(next) = evaluation.next_stop_min {
                if WARN_AT_MIN.iter().any(|&t| next <= t && !usage.warned.contains(&t)) {
                    // Every threshold already passed counts as shown, so a
                    // lowered limit gives one notification, not three.
                    for t in WARN_AT_MIN.iter().filter(|&&t| t >= next) {
                        if !usage.warned.contains(t) {
                            usage.warned.push(*t);
                        }
                    }
                    let plural = if next == 1 { "" } else { "s" };
                    let what = if evaluation.stop_is_bedtime {
                        format!("Bedtime in {next} minute{plural}")
                    } else {
                        format!("{next} minute{plural} of screen time left")
                    };
                    toast(&what, "Save your work. The computer locks when the time is up.", false);
                }
            }
        }
        if save_due {
            save_usage(usage);
        }
        (usage.clone(), evaluation)
    });
    if save_due {
        rt.last_saved = Some(now);
    }

    let enforcing = matches!(evaluation.status, Status::TimeUp | Status::OutsideHours);
    let sign_out = rules.time_up_action == "sign_out";
    if !enforcing {
        rt.pending = None;
        rt.enforced_once = false;
    } else if locked {
        rt.pending = None;
    } else {
        match rt.pending {
            None => {
                let grace = if rt.enforced_once { AGAIN_GRACE } else { FIRST_GRACE };
                rt.pending = Some((now, grace));
                let title = if evaluation.status == Status::TimeUp {
                    "Screen time is up"
                } else {
                    "Outside the allowed hours"
                };
                let action = if sign_out { "signs you out" } else { "locks" };
                toast(
                    title,
                    &format!("This computer {action} in {} seconds.", grace.as_secs()),
                    true,
                );
            }
            Some((since, grace)) if now - since >= grace => {
                rt.pending = None;
                rt.enforced_once = true;
                let result = if sign_out {
                    crate::sys_commands::sign_out()
                } else {
                    crate::sys_commands::lock_now()
                };
                crate::security::audit(
                    "family_rules",
                    match (&result, sign_out) {
                        (Ok(()), true) => "signed_out",
                        (Ok(()), false) => "locked",
                        (Err(_), _) => "enforce_failed",
                    },
                );
            }
            Some(_) => {}
        }
    }

    if rules.block_apps && !rules.blocked_apps.is_empty() {
        let running = crate::processes::snapshot();
        for name in &rules.blocked_apps {
            if !crate::processes::is_running(&running, name) {
                continue;
            }
            if let Ok(stopped) = crate::processes::stop_by_name(name) {
                if stopped == 0 {
                    continue;
                }
                crate::security::audit("family_block", "stopped");
                let due = rt
                    .blocked_notice
                    .get(name)
                    .is_none_or(|t| now - *t >= BLOCK_NOTICE_EVERY);
                if due {
                    rt.blocked_notice.insert(name.clone(), now);
                    toast(&format!("{name} is blocked"), "The family rules on this computer block it.", false);
                }
            }
        }
    }

    let mut values = rule_values(rules);
    values.insert("screen_time_today".into(), (usage.seconds / 60).to_string());
    values.insert(
        "screen_time_left".into(),
        evaluation.left_min.map(|m| m.to_string()).unwrap_or_else(|| "None".into()),
    );
    values.insert("screen_time_status".into(), evaluation.status.as_str().into());
    values.insert("screen_time_bonus".into(), usage.bonus_min.to_string());
    values
}

/// Runs for the whole life of the app, with or without a connection.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut runtime = Some(Runtime::default());
        let mut was_active = false;
        let mut was_protected = false;
        let mut was_connected = false;
        loop {
            let state = app.state::<AppState>();
            let cfg = state.config.lock().await.clone();
            let connected = state.status.lock().await.connected;
            let protected = tokio::task::spawn_blocking(protection::is_protected)
                .await
                .unwrap_or(false);
            let active = cfg.family.enabled || protected;
            if active != was_active || protected != was_protected {
                was_active = active;
                was_protected = protected;
                crate::transport::refresh_entities(&app).await;
                if let Some(rt) = runtime.as_mut() {
                    rt.published.clear();
                }
            }
            if active {
                let mut rt = runtime.take().unwrap_or_default();
                let rules = cfg.family.clone();
                let (rt_back, values) = tokio::task::spawn_blocking(move || {
                    let values = tick(&mut rt, &rules);
                    (rt, values)
                })
                .await
                .unwrap_or_else(|_| (Runtime::default(), HashMap::new()));
                let mut rt = rt_back;
                let now = Instant::now();
                let full = connected != was_connected
                    || rt.last_full_publish.is_none_or(|t| now - t >= FULL_PUBLISH_EVERY);
                was_connected = connected;
                let changed: HashMap<String, String> = values
                    .into_iter()
                    .filter(|(key, value)| full || rt.published.get(key) != Some(value))
                    .collect();
                if full {
                    rt.last_full_publish = Some(now);
                }
                if !changed.is_empty() {
                    rt.published.extend(changed.clone());
                    crate::transport::publish_states(&app, &changed).await;
                }
                runtime = Some(rt);
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

// ------------------------------------------------------------------ commands

/// A family entity changed in Home Assistant.
pub async fn handle_command(app: &AppHandle, key: &str, payload: &str) -> Result<(), String> {
    let protected = tokio::task::spawn_blocking(protection::is_protected)
        .await
        .unwrap_or(false);
    let state = app.state::<AppState>();
    let mut cfg = state.config.lock().await;
    if !(cfg.family.enabled || protected) {
        return Err("family rules are off on this computer".into());
    }
    let on = matches!(payload.trim().to_ascii_uppercase().as_str(), "ON" | "TRUE" | "1");
    let mut rules = cfg.family.clone();
    match key {
        "family_rules" => rules.rules_on = on,
        "screen_time_limit" => rules.daily_limit_min = parse_minutes(payload, MAX_LIMIT_MIN)?,
        "allowed_from" => rules.allowed_from = normalize_time(payload)?,
        "allowed_until" => rules.allowed_until = normalize_time(payload)?,
        "block_apps" => rules.block_apps = on,
        "blocked_apps" => rules.blocked_apps = parse_app_list(payload)?,
        "time_up_action" => rules.time_up_action = normalize_action(payload)?,
        "screen_time_bonus" => {
            drop(cfg);
            let minutes = parse_minutes(payload, MAX_BONUS_MIN)?;
            let today = clock().date;
            with_usage(&today, |usage| {
                usage.bonus_min = minutes;
                usage.warned.clear();
                save_usage(usage);
            });
            crate::security::audit("family_rules", "extra_time");
            let values = HashMap::from([("screen_time_bonus".to_string(), minutes.to_string())]);
            crate::transport::publish_states(app, &values).await;
            if minutes > 0 {
                let _ = tokio::task::spawn_blocking(move || {
                    toast(
                        &format!("{minutes} extra minutes today"),
                        "Home Assistant gave this computer more screen time.",
                        false,
                    )
                })
                .await;
            }
            return Ok(());
        }
        "lock_message" => {
            drop(cfg);
            return lock_with_message(payload).await;
        }
        _ => return Err(format!("unknown family entity: {key}")),
    }
    if rules != cfg.family {
        cfg.family = rules;
        crate::config::save(&cfg)?;
        crate::security::audit("family_rules", "changed_from_home_assistant");
    }
    let values = rule_values(&cfg.family);
    drop(cfg);
    crate::transport::publish_states(app, &values).await;
    Ok(())
}

/// Shows the message, then locks the computer a few seconds later.
async fn lock_with_message(text: &str) -> Result<(), String> {
    let text = crate::security::safe_preview(text.trim(), 200);
    if text.is_empty() {
        return Ok(());
    }
    crate::security::audit("family_rules", "lock_with_message");
    tokio::task::spawn_blocking(move || {
        toast("Message from home", &text, false);
        std::thread::sleep(LOCK_MESSAGE_DELAY);
        crate::sys_commands::lock_now()
    })
    .await
    .map_err(|e| e.to_string())?
}

// ------------------------------------------------------------------ the Family page

#[derive(Serialize)]
pub struct FamilyView {
    pub rules: FamilyConfig,
    /// An administrator protected this Windows account.
    pub protected: bool,
    /// This account is an administrator (protection would not hold).
    pub admin_account: bool,
    pub watchdog_installed: bool,
    pub today_min: u64,
    pub left_min: Option<u32>,
    pub bonus_min: u32,
    pub status: String,
}

#[tauri::command]
pub async fn get_family(app: AppHandle) -> Result<FamilyView, String> {
    let rules = app.state::<AppState>().config.lock().await.family.clone();
    tokio::task::spawn_blocking(move || {
        let clock = clock();
        let (seconds, bonus, evaluation) = with_usage(&clock.date, |usage| {
            (usage.seconds, usage.bonus_min, evaluate(&rules, usage, clock.minute_of_day))
        });
        FamilyView {
            protected: protection::is_protected(),
            admin_account: protection::is_admin_account(),
            watchdog_installed: protection::watchdog_installed(),
            today_min: seconds / 60,
            left_min: evaluation.left_min,
            bonus_min: bonus,
            status: evaluation.status.as_str().into(),
            rules,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_family(app: AppHandle, rules: FamilyConfig) -> Result<(), String> {
    if tokio::task::spawn_blocking(protection::is_protected).await.unwrap_or(false) {
        return Err("an administrator protected this account; change the rules in Home Assistant".into());
    }
    let rules = normalize(rules)?;
    let values = {
        let state = app.state::<AppState>();
        let mut cfg = state.config.lock().await;
        cfg.family = rules;
        crate::config::save(&cfg)?;
        rule_values(&cfg.family)
    };
    crate::transport::refresh_entities(&app).await;
    crate::transport::publish_states(&app, &values).await;
    Ok(())
}

/// UAC: list this account as protected and install the watchdog task.
#[tauri::command]
pub async fn protect_family_account() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        let sid = protection::current_sid().ok_or("cannot read this Windows account")?;
        crate::cpu_temperature::run_elevated(&format!("{PROTECT_ARG} {sid}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// UAC: remove the protection (and the task when no account needs it).
#[tauri::command]
pub async fn unprotect_family_account() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        let sid = protection::current_sid().ok_or("cannot read this Windows account")?;
        crate::cpu_temperature::run_elevated(&format!("{UNPROTECT_ARG} {sid}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Started elevated with PROTECT_ARG / UNPROTECT_ARG: returns the exit code.
/// The account comes as an argument because with an administrator's password
/// typed into UAC this process runs as that administrator, not the child.
pub fn handle_elevated_args() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    let position = args.iter().position(|a| a == PROTECT_ARG || a == UNPROTECT_ARG)?;
    let sid = args.get(position + 1).map(String::as_str).unwrap_or("");
    let result = if args[position] == PROTECT_ARG {
        protection::protect(sid)
    } else {
        protection::unprotect(sid)
    };
    if let Err(e) = &result {
        log::warn!("family protection: {e}");
    }
    Some(if result.is_ok() { 0 } else { 1 })
}

pub fn run_watchdog() {
    protection::run_watchdog();
}

// ------------------------------------------------------------------ protection

mod protection {
    const KEY: &str = r"SOFTWARE\Deskmate\Family\Protected";
    #[cfg(windows)]
    const TASK_NAME: &str = "Deskmate watchdog";
    const WATCHDOG_DIR: &str = "Deskmate watchdog";
    const WATCHDOG_EXE: &str = "deskmate-watchdog.exe";

    fn valid_sid(sid: &str) -> bool {
        sid.starts_with("S-1-5-21-")
            && sid.len() < 100
            && sid.chars().all(|c| c.is_ascii_digit() || c == '-' || c == 'S')
    }

    fn watchdog_path() -> std::path::PathBuf {
        std::env::var_os("ProgramW6432")
            .or_else(|| std::env::var_os("ProgramFiles"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Program Files"))
            .join(WATCHDOG_DIR)
            .join(WATCHDOG_EXE)
    }

    pub fn watchdog_installed() -> bool {
        watchdog_path().exists()
    }

    #[cfg(windows)]
    pub fn current_sid() -> Option<String> {
        static SID: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        SID.get_or_init(win::user_sid).clone()
    }

    #[cfg(not(windows))]
    pub fn current_sid() -> Option<String> {
        None
    }

    #[cfg(windows)]
    pub fn is_protected() -> bool {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        let Some(sid) = current_sid() else {
            return false;
        };
        RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(KEY)
            .and_then(|key| key.get_value::<u32, _>(&sid))
            .map(|value| value == 1)
            .unwrap_or(false)
    }

    #[cfg(not(windows))]
    pub fn is_protected() -> bool {
        false
    }

    #[cfg(windows)]
    pub fn is_admin_account() -> bool {
        win::is_admin_account()
    }

    #[cfg(not(windows))]
    pub fn is_admin_account() -> bool {
        false
    }

    /// Elevated: list the account, install the watchdog.
    #[cfg(windows)]
    pub fn protect(sid: &str) -> Result<(), String> {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        if !valid_sid(sid) {
            return Err("not a Windows account".into());
        }
        let (key, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .create_subkey(KEY)
            .map_err(|e| format!("cannot write the protection: {e}"))?;
        key.set_value(sid, &1u32)
            .map_err(|e| format!("cannot write the protection: {e}"))?;
        win::install_watchdog(&watchdog_path())
    }

    #[cfg(not(windows))]
    pub fn protect(_sid: &str) -> Result<(), String> {
        Err("windows only".into())
    }

    /// Elevated: remove the account; the watchdog goes with the last one.
    #[cfg(windows)]
    pub fn unprotect(sid: &str) -> Result<(), String> {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS};
        use winreg::RegKey;
        if !valid_sid(sid) {
            return Err("not a Windows account".into());
        }
        let remaining = match RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(KEY, KEY_ALL_ACCESS) {
            Ok(key) => {
                let _ = key.delete_value(sid);
                key.enum_values().count()
            }
            Err(_) => 0,
        };
        if remaining == 0 {
            win::remove_watchdog(&watchdog_path());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    pub fn unprotect(_sid: &str) -> Result<(), String> {
        Ok(())
    }

    /// The scheduled task, every minute in each signed-in account: start
    /// Deskmate again for a protected account when it is not running.
    #[cfg(windows)]
    pub fn run_watchdog() {
        if !is_protected() {
            return;
        }
        let own = std::process::id();
        let session = win::session_of(own);
        let running = crate::processes::snapshot().into_iter().any(|(pid, name)| {
            pid != own && (name == "deskmate.exe" || name == WATCHDOG_EXE) && win::session_of(pid) == session
        });
        if running {
            return;
        }
        let installed = std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .map(|dir| dir.join("Deskmate").join("deskmate.exe"))
            .filter(|path| path.exists());
        // Deleted by the child: the copy in Program Files runs Deskmate instead.
        let Some(target) = installed.or_else(|| std::env::current_exe().ok()) else {
            return;
        };
        win::start_detached(&target, super::HIDDEN_ARG);
        crate::security::audit("family_watchdog", "restarted");
    }

    #[cfg(not(windows))]
    pub fn run_watchdog() {}

    #[cfg(windows)]
    mod win {
        use windows::core::{BSTR, PWSTR};
        use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
        use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
        use windows::Win32::Security::{
            GetTokenInformation, TokenElevationType, TokenElevationTypeDefault, TokenUser,
            TOKEN_ELEVATION_TYPE, TOKEN_QUERY, TOKEN_USER,
        };
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
        };
        use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
        use windows::Win32::System::TaskScheduler::{
            ITaskFolder, ITaskService, TaskScheduler, TASK_CREATE_OR_UPDATE, TASK_LOGON_GROUP,
        };
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        use windows::Win32::System::Variant::VARIANT;
        use windows::Win32::UI::Shell::IsUserAnAdmin;

        fn token() -> Option<HANDLE> {
            let mut token = HANDLE::default();
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.ok()?;
            Some(token)
        }

        pub fn user_sid() -> Option<String> {
            let token = token()?;
            let sid = unsafe {
                let mut len = 0u32;
                let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
                // u64 buffer: TOKEN_USER holds a pointer and needs its alignment
                let mut buffer = vec![0u64; (len as usize).div_ceil(8).max(1)];
                let sid = GetTokenInformation(
                    token,
                    TokenUser,
                    Some(buffer.as_mut_ptr().cast()),
                    (buffer.len() * 8) as u32,
                    &mut len,
                )
                .ok()
                .and_then(|()| {
                    let user = &*(buffer.as_ptr() as *const TOKEN_USER);
                    let mut raw = PWSTR::null();
                    ConvertSidToStringSidW(user.User.Sid, &mut raw).ok()?;
                    let text = raw.to_string().ok();
                    let _ = LocalFree(Some(HLOCAL(raw.0.cast())));
                    text
                });
                let _ = CloseHandle(token);
                sid
            };
            sid
        }

        /// An administrator account, even while UAC runs it with standard rights.
        pub fn is_admin_account() -> bool {
            let Some(token) = token() else {
                return false;
            };
            let mut kind = TOKEN_ELEVATION_TYPE::default();
            let mut len = 0u32;
            let split = unsafe {
                let read = GetTokenInformation(
                    token,
                    TokenElevationType,
                    Some((&mut kind as *mut TOKEN_ELEVATION_TYPE).cast()),
                    std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32,
                    &mut len,
                );
                let _ = CloseHandle(token);
                read.is_ok() && kind != TokenElevationTypeDefault
            };
            split || unsafe { IsUserAnAdmin() }.as_bool()
        }

        pub fn session_of(pid: u32) -> Option<u32> {
            let mut session = 0u32;
            unsafe { ProcessIdToSessionId(pid, &mut session) }.ok()?;
            Some(session)
        }

        /// Starts Deskmate outside the task's job, so it keeps running after
        /// the task ends.
        pub fn start_detached(path: &std::path::Path, arg: &str) {
            use std::os::windows::process::CommandExt;
            const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
            let started = std::process::Command::new(path)
                .arg(arg)
                .creation_flags(CREATE_BREAKAWAY_FROM_JOB)
                .spawn()
                .or_else(|_| std::process::Command::new(path).arg(arg).spawn());
            if let Err(e) = started {
                log::warn!("watchdog: cannot start Deskmate: {e}");
            }
        }

        fn xml_escape(text: &str) -> String {
            text.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&apos;")
        }

        fn task_folder() -> Result<ITaskFolder, String> {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let service: ITaskService =
                    CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
                        .map_err(|e| e.to_string())?;
                let empty = VARIANT::default();
                service
                    .Connect(&empty, &empty, &empty, &empty)
                    .map_err(|e| e.to_string())?;
                service.GetFolder(&BSTR::from("\\")).map_err(|e| e.to_string())
            }
        }

        /// Copies Deskmate to Program Files (a child cannot delete it there)
        /// and registers a task for every member of Users: at sign-in and
        /// every minute.
        pub fn install_watchdog(target: &std::path::Path) -> Result<(), String> {
            let source = std::env::current_exe().map_err(|e| e.to_string())?;
            let dir = target.parent().ok_or("bad watchdog path")?;
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            let mut copied = std::fs::copy(&source, target);
            for _ in 0..10 {
                if copied.is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
                copied = std::fs::copy(&source, target);
            }
            copied.map_err(|e| format!("cannot copy the watchdog: {e}"))?;
            let xml = format!(
                r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Starts Deskmate again for Windows accounts with family rules.</Description></RegistrationInfo>
  <Triggers>
    <LogonTrigger><Enabled>true</Enabled></LogonTrigger>
    <TimeTrigger>
      <Repetition><Interval>PT1M</Interval><StopAtDurationEnd>false</StopAtDurationEnd></Repetition>
      <StartBoundary>2026-01-01T00:00:00</StartBoundary>
      <Enabled>true</Enabled>
    </TimeTrigger>
  </Triggers>
  <Principals><Principal id="Users"><GroupId>S-1-5-32-545</GroupId><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT5M</ExecutionTimeLimit>
    <Hidden>true</Hidden>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Users"><Exec><Command>{exe}</Command><Arguments>{arg}</Arguments></Exec></Actions>
</Task>"#,
                exe = xml_escape(&target.to_string_lossy()),
                arg = super::super::WATCHDOG_ARG,
            );
            let folder = task_folder()?;
            unsafe {
                let empty = VARIANT::default();
                folder
                    .RegisterTask(
                        &BSTR::from(super::TASK_NAME),
                        &BSTR::from(xml),
                        TASK_CREATE_OR_UPDATE.0,
                        &empty,
                        &empty,
                        TASK_LOGON_GROUP,
                        &empty,
                    )
                    .map_err(|e| format!("cannot register the task: {e}"))?;
            }
            Ok(())
        }

        pub fn remove_watchdog(target: &std::path::Path) {
            if let Ok(folder) = task_folder() {
                unsafe {
                    let _ = folder.DeleteTask(&BSTR::from(super::TASK_NAME), 0);
                }
            }
            for _ in 0..10 {
                if !target.exists() || std::fs::remove_file(target).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            if let Some(dir) = target.parent() {
                let _ = std::fs::remove_dir(dir);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::valid_sid;

        #[test]
        fn only_user_sids_are_accepted() {
            assert!(valid_sid("S-1-5-21-1004336348-1177238915-682003330-1001"));
            assert!(!valid_sid("S-1-5-32-544"));
            assert!(!valid_sid("S-1-5-21-1 & calc"));
            assert!(!valid_sid(""));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> FamilyConfig {
        FamilyConfig {
            enabled: true,
            rules_on: true,
            ..Default::default()
        }
    }

    fn usage(minutes: u64) -> Usage {
        Usage { seconds: minutes * 60, ..Default::default() }
    }

    #[test]
    fn daily_limit_counts_down_and_stops() {
        let mut r = rules();
        r.daily_limit_min = 60;
        let e = evaluate(&r, &usage(50), 12 * 60);
        assert_eq!(e.status, Status::Ok);
        assert_eq!(e.left_min, Some(10));
        assert_eq!(evaluate(&r, &usage(60), 12 * 60).status, Status::TimeUp);
        let mut extra = usage(60);
        extra.bonus_min = 15;
        assert_eq!(evaluate(&r, &extra, 12 * 60).left_min, Some(15));
    }

    #[test]
    fn rules_off_never_stop() {
        let mut r = rules();
        r.rules_on = false;
        r.daily_limit_min = 10;
        assert_eq!(evaluate(&r, &usage(90), 12 * 60).status, Status::Off);
    }

    #[test]
    fn allowed_hours_also_over_midnight() {
        let mut r = rules();
        r.allowed_from = "07:00".into();
        r.allowed_until = "21:00".into();
        assert_eq!(evaluate(&r, &usage(0), 6 * 60 + 59).status, Status::OutsideHours);
        let e = evaluate(&r, &usage(0), 20 * 60 + 50);
        assert_eq!(e.status, Status::Ok);
        assert_eq!(e.next_stop_min, Some(10));
        assert!(e.stop_is_bedtime);
        r.allowed_from = "22:00".into();
        r.allowed_until = "02:00".into();
        assert_eq!(evaluate(&r, &usage(0), 23 * 60).status, Status::Ok);
        assert_eq!(evaluate(&r, &usage(0), 12 * 60).status, Status::OutsideHours);
    }

    #[test]
    fn nearer_stop_wins() {
        let mut r = rules();
        r.daily_limit_min = 120;
        r.allowed_until = "21:00".into();
        let e = evaluate(&r, &usage(0), 20 * 60 + 30);
        assert_eq!(e.next_stop_min, Some(30));
        assert!(e.stop_is_bedtime);
    }

    #[test]
    fn inputs_are_normalized() {
        assert_eq!(normalize_time(" 7:05 ").unwrap(), "07:05");
        assert_eq!(normalize_time("").unwrap(), "");
        assert!(normalize_time("25:00").is_err());
        assert_eq!(
            parse_app_list("Steam, roblox.exe; steam").unwrap(),
            vec!["steam.exe", "roblox.exe"]
        );
        assert!(parse_app_list("explorer").is_err());
        assert!(parse_app_list("C:\\Games\\x.exe").is_err());
        assert_eq!(normalize_action("Sign out").unwrap(), "sign_out");
        assert_eq!(parse_minutes("90.4", MAX_LIMIT_MIN).unwrap(), 90);
        assert_eq!(parse_minutes("5000", MAX_LIMIT_MIN).unwrap(), MAX_LIMIT_MIN);
        assert!(parse_minutes("-1", MAX_LIMIT_MIN).is_err());
    }
}
