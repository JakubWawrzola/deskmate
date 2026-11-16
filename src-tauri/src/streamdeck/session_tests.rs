//! One connection against stand-ins, over an in-memory stream: every
//! request of the reference and every refusal.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::DuplexStream;

use super::backend::{EntityInfo, Timing};
use super::framing::MAX_LINE_BYTES;
use super::protocol::MAX_WATCHED_ENTITIES;
use super::session::run_session;
use super::stand_ins::{
    backend, eventually, quick_timing, Client, FakeActions, FakeHomeAssistant, RecordedAudit,
};

struct Harness {
    client: Client<DuplexStream>,
    home_assistant: FakeHomeAssistant,
    actions: FakeActions,
    audit: Arc<RecordedAudit>,
}

fn home() -> FakeHomeAssistant {
    FakeHomeAssistant::with_entities(&[
        ("switch.fan", "Fan", "off"),
        ("light.desk", "Desk", "on"),
        ("scene.movie", "Movie", "2026-01-01T00:00:00+00:00"),
    ])
}

fn connect_with(home_assistant: FakeHomeAssistant, timing: Timing) -> Harness {
    let actions =
        FakeActions::with_actions(&[("hotkey:lights", "Lights"), ("tray:pc_sleep", "Sleep")]);
    let (backend, audit) = backend(&home_assistant, &actions, timing);
    let (near, far) = tokio::io::duplex(4 * MAX_LINE_BYTES);
    tokio::spawn(run_session(far, backend));
    Harness {
        client: Client::new(near),
        home_assistant,
        actions,
        audit,
    }
}

fn connect() -> Harness {
    connect_with(home(), quick_timing())
}

async fn greeted() -> Harness {
    let mut harness = connect();
    assert_eq!(harness.client.hello().await["ok"], true);
    harness
}

/// A timing whose regular read never comes during a test.
fn no_regular_poll() -> Timing {
    Timing {
        watch_poll_interval: Duration::from_secs(3600),
        ..quick_timing()
    }
}

fn error_of(response: &Value) -> &str {
    assert_eq!(response["ok"], false, "{response}");
    response["error"].as_str().unwrap()
}

#[tokio::test]
async fn hello_is_answered_with_the_version_the_app_and_ha() {
    let mut harness = connect();
    assert_eq!(
        harness.client.hello().await,
        json!({"id": 1, "ok": true, "v": 1, "app": "0.9.0", "ha": true})
    );

    let unconfigured = home();
    unconfigured.set_configured(false);
    let mut harness = connect_with(unconfigured, quick_timing());
    assert_eq!(harness.client.hello().await["ha"], false);
}

#[tokio::test]
async fn any_other_request_before_hello_is_a_bad_request() {
    let mut harness = connect();
    for request in [
        json!({"op": "actions"}),
        json!({"op": "run", "action": "hotkey:lights"}),
        json!({"op": "entities"}),
        json!({"op": "watch", "entities": ["light.desk"]}),
        json!({"op": "toggle", "entity_id": "light.desk"}),
        json!({"op": "call", "service": "scene.turn_on"}),
    ] {
        let response = harness.client.request(request.clone()).await;
        assert_eq!(error_of(&response), "bad_request", "{request}");
    }
    assert!(harness.home_assistant.requests().is_empty());
    assert!(harness.actions.ran().is_empty());
    assert_eq!(harness.home_assistant.state_reads(), 0);

    // The connection is still good.
    assert_eq!(harness.client.hello().await["ok"], true);
    assert_eq!(
        harness.client.request(json!({"op": "actions"})).await["ok"],
        true
    );
}

#[tokio::test]
async fn a_hello_with_another_version_is_refused_and_greets_nobody() {
    let mut harness = connect();
    let response = harness
        .client
        .request(json!({"op": "hello", "v": 2, "client": "streamdeck", "version": "9.0.0"}))
        .await;
    assert_eq!(response, json!({"id": 1, "ok": false, "error": "version"}));
    let response = harness.client.request(json!({"op": "actions"})).await;
    assert_eq!(error_of(&response), "bad_request");
}

#[tokio::test]
async fn a_line_that_breaks_the_framing_closes_the_connection_without_an_answer() {
    let too_long = format!("{{\"id\":2,\"op\":\"{}\"}}\n", "a".repeat(MAX_LINE_BYTES));
    let broken: [&[u8]; 8] = [
        b"\n",
        b"not json\n",
        b"[1,2]\n",
        b"7\n",
        b"\"text\"\n",
        b"{\"id\":2,\"op\":\"actions\"\n",
        b"{\"id\":2,\"op\":\"\xff\"}\n",
        too_long.as_bytes(),
    ];
    for line in broken {
        let mut harness = greeted().await;
        harness.client.send_raw(line).await;
        harness.client.expect_closed().await;
    }
}

#[tokio::test]
async fn a_line_that_never_ends_closes_the_connection() {
    let mut harness = greeted().await;
    harness
        .client
        .send_raw("a".repeat(MAX_LINE_BYTES + 1).as_bytes())
        .await;
    harness.client.expect_closed().await;
}

#[tokio::test]
async fn a_request_is_answered_before_a_later_broken_line_closes() {
    let mut harness = greeted().await;
    harness
        .client
        .send_raw(b"{\"id\":2,\"op\":\"actions\"}\nnot json\n")
        .await;
    // The session ends at the broken line; whether the answer still went out
    // is not promised, but nothing else may.
    if let Some(message) = harness.client.next().await {
        assert_eq!(message["id"], 2);
        harness.client.expect_closed().await;
    }
}

#[tokio::test]
async fn an_unknown_op_and_a_malformed_field_are_bad_requests() {
    let mut harness = greeted().await;
    for request in [
        json!({"op": "no_such_op"}),
        json!({}),
        json!({"op": "run"}),
        json!({"op": "run", "action": "hotkey:Lights!"}),
        json!({"op": "toggle", "entity_id": "desk"}),
        json!({"op": "call", "service": "turn_on"}),
        json!({"op": "call", "service": "light.turn_on", "data": []}),
        json!({"op": "entities", "domains": "light"}),
        json!({"op": "watch", "entities": "light.desk"}),
    ] {
        let response = harness.client.request(request.clone()).await;
        assert_eq!(error_of(&response), "bad_request", "{request}");
        assert!(response.get("message").is_none());
    }
    assert!(harness.home_assistant.requests().is_empty());
    assert!(harness.audit.lines().is_empty());
}

#[tokio::test]
async fn a_request_without_a_usable_id_is_refused_without_one() {
    let mut harness = greeted().await;
    harness.client.send(json!({"op": "actions"})).await;
    assert_eq!(
        harness.client.next().await.unwrap(),
        json!({"ok": false, "error": "bad_request"})
    );
    harness
        .client
        .send(json!({"id": "2", "op": "run", "action": "hotkey:lights"}))
        .await;
    assert_eq!(
        harness.client.next().await.unwrap(),
        json!({"ok": false, "error": "bad_request"})
    );
    assert!(harness.actions.ran().is_empty());
}

#[tokio::test]
async fn unknown_fields_are_ignored() {
    let mut harness = greeted().await;
    let response = harness
        .client
        .request(json!({"op": "toggle", "entity_id": "light.desk", "later": {"x": 1}}))
        .await;
    assert_eq!(response, json!({"id": 2, "ok": true}));
}

#[tokio::test]
async fn actions_lists_hotkeys_and_tray_actions() {
    let mut harness = greeted().await;
    assert_eq!(
        harness.client.request(json!({"op": "actions"})).await,
        json!({"id": 2, "ok": true, "actions": [
            {"id": "hotkey:lights", "name": "Lights"},
            {"id": "tray:pc_sleep", "name": "Sleep"},
        ]})
    );
}

#[tokio::test]
async fn run_runs_a_known_action_and_refuses_an_unknown_one() {
    let mut harness = greeted().await;
    assert_eq!(
        harness
            .client
            .request(json!({"op": "run", "action": "hotkey:lights"}))
            .await,
        json!({"id": 2, "ok": true})
    );
    assert_eq!(
        harness
            .client
            .request(json!({"op": "run", "action": "hotkey:gone"}))
            .await,
        json!({"id": 3, "ok": false, "error": "unknown_action",
               "message": "That action no longer exists."})
    );
    assert_eq!(harness.actions.ran(), ["hotkey:lights"]);
    assert_eq!(
        harness.audit.lines(),
        [
            "streamdeck_run action=hotkey:lights result=ok",
            "streamdeck_run action=hotkey:gone result=unknown_action",
        ]
    );
}

#[tokio::test]
async fn entities_lists_all_or_the_asked_domains_in_order() {
    let mut harness = greeted().await;
    let all = harness.client.request(json!({"op": "entities"})).await;
    assert_eq!(
        all,
        json!({"id": 2, "ok": true, "entities": [
            {"entity_id": "light.desk", "name": "Desk", "state": "on"},
            {"entity_id": "scene.movie", "name": "Movie", "state": "2026-01-01T00:00:00+00:00"},
            {"entity_id": "switch.fan", "name": "Fan", "state": "off"},
        ]})
    );
    let some = harness
        .client
        .request(json!({"op": "entities", "domains": ["light", "switch"]}))
        .await;
    assert_eq!(
        some["entities"],
        json!([
            {"entity_id": "light.desk", "name": "Desk", "state": "on"},
            {"entity_id": "switch.fan", "name": "Fan", "state": "off"},
        ])
    );
    let none = harness
        .client
        .request(json!({"op": "entities", "domains": ["climate"]}))
        .await;
    assert_eq!(none["entities"], json!([]));
}

#[tokio::test]
async fn a_long_entity_list_is_trimmed_to_one_line() {
    let mut harness = greeted().await;
    harness.home_assistant.set_entities(
        (0..4_000)
            .map(|index| EntityInfo {
                entity_id: format!("sensor.reading_{index:04}"),
                name: format!("Reading {index}"),
                state: "21.5".into(),
            })
            .collect(),
    );
    // `Client::next` checks that the line is within the limit.
    let response = harness.client.request(json!({"op": "entities"})).await;
    let listed = response["entities"].as_array().unwrap();
    assert!(
        listed.len() > 500 && listed.len() < 4_000,
        "{}",
        listed.len()
    );
    assert_eq!(listed[0]["entity_id"], "sensor.reading_0000");
}

#[tokio::test]
async fn requests_that_need_home_assistant_fail_without_an_address_and_token() {
    let unconfigured = home();
    unconfigured.set_configured(false);
    let mut harness = connect_with(unconfigured, quick_timing());
    harness.client.hello().await;
    for request in [
        json!({"op": "entities"}),
        json!({"op": "watch", "entities": ["light.desk"]}),
        json!({"op": "toggle", "entity_id": "light.desk"}),
        json!({"op": "call", "service": "scene.turn_on", "entity_id": "scene.movie"}),
    ] {
        let response = harness.client.request(request.clone()).await;
        assert_eq!(error_of(&response), "ha_not_configured", "{request}");
        assert!(response["message"].is_string());
    }
    assert!(harness.home_assistant.requests().is_empty());
    harness
        .client
        .expect_silence(Duration::from_millis(120))
        .await;
    assert_eq!(harness.home_assistant.state_reads(), 0);

    // What needs no Home Assistant still works.
    assert_eq!(
        harness.client.request(json!({"op": "actions"})).await["ok"],
        true
    );
    assert_eq!(
        harness
            .client
            .request(json!({"op": "watch", "entities": []}))
            .await["states"],
        json!({})
    );
    assert_eq!(
        harness.audit.lines(),
        [
            "streamdeck_toggle entity=light.desk result=ha_not_configured",
            "streamdeck_call service=scene.turn_on entity=scene.movie result=ha_not_configured",
        ]
    );
}

#[tokio::test]
async fn requests_fail_with_ha_error_when_home_assistant_does_not_answer() {
    let mut harness = greeted().await;
    harness.home_assistant.set_reachable(false);
    for request in [
        json!({"op": "entities"}),
        json!({"op": "toggle", "entity_id": "light.desk"}),
        json!({"op": "call", "service": "light.turn_on"}),
    ] {
        let response = harness.client.request(request.clone()).await;
        assert_eq!(error_of(&response), "ha_error", "{request}");
    }
    assert_eq!(
        harness.audit.lines(),
        [
            "streamdeck_toggle entity=light.desk result=ha_error",
            "streamdeck_call service=light.turn_on entity=- result=ha_error",
        ]
    );
}

#[tokio::test]
async fn watch_answers_with_a_state_for_every_entity() {
    let mut harness = greeted().await;
    let response = harness
        .client
        .request(json!({"op": "watch", "entities": ["light.desk", "light.gone", "switch.fan"]}))
        .await;
    assert_eq!(
        response,
        json!({"id": 2, "ok": true, "states": {
            "light.desk": "on", "light.gone": "unavailable", "switch.fan": "off"
        }})
    );
}

#[tokio::test]
async fn a_watched_entity_that_changes_is_reported_once() {
    let mut harness = greeted().await;
    harness
        .client
        .request(json!({"op": "watch", "entities": ["light.desk", "switch.fan"]}))
        .await;
    // Read again and again, and nothing changed: nothing is sent.
    let reads = harness.home_assistant.state_reads();
    harness
        .client
        .expect_silence(Duration::from_millis(150))
        .await;
    assert!(harness.home_assistant.state_reads() > reads);

    harness.home_assistant.set_state("light.desk", "off");
    assert_eq!(
        harness.client.event().await,
        json!({"ev": "state", "entity_id": "light.desk", "state": "off"})
    );
    harness
        .client
        .expect_silence(Duration::from_millis(150))
        .await;

    harness.home_assistant.remove("switch.fan");
    assert_eq!(
        harness.client.event().await,
        json!({"ev": "state", "entity_id": "switch.fan", "state": "unavailable"})
    );
}

#[tokio::test]
async fn nothing_is_read_from_home_assistant_while_nothing_is_watched() {
    let mut harness = greeted().await;
    harness.client.request(json!({"op": "actions"})).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(harness.home_assistant.state_reads(), 0);

    harness
        .client
        .request(json!({"op": "watch", "entities": ["light.desk"]}))
        .await;
    let home_assistant = harness.home_assistant.clone();
    eventually("the regular reads", || home_assistant.state_reads() >= 3).await;

    // An empty list clears the watch, and the reads stop.
    let response = harness
        .client
        .request(json!({"op": "watch", "entities": []}))
        .await;
    assert_eq!(response["states"], json!({}));
    tokio::time::sleep(Duration::from_millis(60)).await;
    let reads = harness.home_assistant.state_reads();
    harness.home_assistant.set_state("light.desk", "off");
    harness
        .client
        .expect_silence(Duration::from_millis(150))
        .await;
    assert_eq!(harness.home_assistant.state_reads(), reads);
}

#[tokio::test]
async fn a_new_watch_list_replaces_the_old_one() {
    let mut harness = greeted().await;
    harness
        .client
        .request(json!({"op": "watch", "entities": ["light.desk"]}))
        .await;
    let response = harness
        .client
        .request(json!({"op": "watch", "entities": ["switch.fan"]}))
        .await;
    assert_eq!(response["states"], json!({"switch.fan": "off"}));

    harness.home_assistant.set_state("light.desk", "off");
    harness
        .client
        .expect_silence(Duration::from_millis(150))
        .await;
    harness.home_assistant.set_state("switch.fan", "on");
    assert_eq!(harness.client.event().await["entity_id"], "switch.fan");
}

#[tokio::test]
async fn a_watch_list_that_is_too_long_is_refused_and_changes_nothing() {
    let mut harness = greeted().await;
    harness
        .client
        .request(json!({"op": "watch", "entities": ["light.desk"]}))
        .await;
    let too_many: Vec<String> = (0..=MAX_WATCHED_ENTITIES)
        .map(|index| format!("light.l{index}"))
        .collect();
    let response = harness
        .client
        .request(json!({"op": "watch", "entities": too_many}))
        .await;
    assert_eq!(error_of(&response), "bad_request");

    // The earlier list is still watched.
    harness.home_assistant.set_state("light.desk", "off");
    assert_eq!(harness.client.event().await["entity_id"], "light.desk");

    let at_limit: Vec<String> = (0..MAX_WATCHED_ENTITIES)
        .map(|index| format!("light.l{index}"))
        .collect();
    let response = harness
        .client
        .request(json!({"op": "watch", "entities": at_limit}))
        .await;
    assert_eq!(
        response["states"].as_object().unwrap().len(),
        MAX_WATCHED_ENTITIES
    );
}

#[tokio::test]
async fn a_watch_that_home_assistant_did_not_answer_is_kept_and_reported_later() {
    let mut harness = greeted().await;
    harness.home_assistant.set_reachable(false);
    let response = harness
        .client
        .request(json!({"op": "watch", "entities": ["light.desk", "switch.fan"]}))
        .await;
    assert_eq!(error_of(&response), "ha_error");
    // Asked again and again, and nothing is sent while it does not answer.
    harness
        .client
        .expect_silence(Duration::from_millis(150))
        .await;
    assert!(harness.home_assistant.state_reads() >= 2);

    harness.home_assistant.set_reachable(true);
    assert_eq!(
        harness.client.event().await,
        json!({"ev": "state", "entity_id": "light.desk", "state": "on"})
    );
    assert_eq!(
        harness.client.event().await,
        json!({"ev": "state", "entity_id": "switch.fan", "state": "off"})
    );
}

#[tokio::test]
async fn while_home_assistant_is_away_the_last_state_stands() {
    let mut harness = greeted().await;
    harness
        .client
        .request(json!({"op": "watch", "entities": ["light.desk"]}))
        .await;
    harness.home_assistant.set_reachable(false);
    harness.home_assistant.set_state("light.desk", "off");
    harness
        .client
        .expect_silence(Duration::from_millis(150))
        .await;
    harness.home_assistant.set_reachable(true);
    assert_eq!(harness.client.event().await["state"], "off");
}

#[tokio::test]
async fn toggle_reaches_home_assistant_and_the_new_state_follows_at_once() {
    // No regular read comes during this test: the event below can only be
    // the read that follows the toggle.
    let mut harness = connect_with(home(), no_regular_poll());
    harness.client.hello().await;
    harness
        .client
        .request(json!({"op": "watch", "entities": ["light.desk"]}))
        .await;
    assert_eq!(
        harness
            .client
            .request(json!({"op": "toggle", "entity_id": "light.desk"}))
            .await,
        json!({"id": 3, "ok": true})
    );
    assert_eq!(
        harness.client.event().await,
        json!({"ev": "state", "entity_id": "light.desk", "state": "off"})
    );
    assert_eq!(harness.home_assistant.requests(), ["toggle light.desk"]);
    assert_eq!(
        harness.audit.lines(),
        ["streamdeck_toggle entity=light.desk result=ok"]
    );
}

#[tokio::test]
async fn call_passes_service_entity_and_data_and_logs_no_data() {
    let mut harness = greeted().await;
    let response = harness
        .client
        .request(json!({
            "op": "call", "service": "light.turn_on", "entity_id": "light.desk",
            "data": {"brightness_pct": 40, "note": "private text"}
        }))
        .await;
    assert_eq!(response, json!({"id": 2, "ok": true}));
    let response = harness
        .client
        .request(json!({"op": "call", "service": "script.goodnight"}))
        .await;
    assert_eq!(response["ok"], true);

    let requests = harness.home_assistant.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("call light.turn_on light.desk "));
    assert!(requests[0].contains("\"brightness_pct\":40"));
    assert_eq!(requests[1], "call script.goodnight - {}");

    let audit = harness.audit.lines();
    assert_eq!(
        audit,
        [
            "streamdeck_call service=light.turn_on entity=light.desk result=ok",
            "streamdeck_call service=script.goodnight entity=- result=ok",
        ]
    );
    assert!(!audit.iter().any(|line| line.contains("private")));
}

#[tokio::test]
async fn a_slow_answer_from_home_assistant_does_not_hold_up_another_request() {
    let mut harness = greeted().await;
    let gate = harness.home_assistant.hold_entities();
    harness
        .client
        .send(json!({"id": 10, "op": "entities"}))
        .await;
    harness
        .client
        .send(json!({"id": 11, "op": "run", "action": "hotkey:lights"}))
        .await;
    // The later request is answered first.
    assert_eq!(
        harness.client.next().await.unwrap(),
        json!({"id": 11, "ok": true})
    );
    gate.notify_one();
    assert_eq!(harness.client.next().await.unwrap()["id"], 10);
}

#[tokio::test]
async fn a_request_past_the_limit_waits_for_one_in_progress_to_finish() {
    let mut harness = greeted().await;
    let gate = harness.home_assistant.hold_entities();
    // Sixteen requests that Home Assistant does not answer yet.
    for id in 100..116 {
        harness
            .client
            .send(json!({"id": id, "op": "entities"}))
            .await;
    }
    harness
        .client
        .send(json!({"id": 200, "op": "run", "action": "hotkey:lights"}))
        .await;
    harness
        .client
        .expect_silence(Duration::from_millis(150))
        .await;
    assert!(harness.actions.ran().is_empty());

    // One of them finishes, and the waiting request is taken up.
    gate.notify_one();
    let mut ids = vec![
        harness.client.next().await.unwrap()["id"].as_u64().unwrap(),
        harness.client.next().await.unwrap()["id"].as_u64().unwrap(),
    ];
    ids.sort_unstable();
    assert!((100..116).contains(&ids[0]), "{ids:?}");
    assert_eq!(ids[1], 200);
    assert_eq!(harness.actions.ran(), ["hotkey:lights"]);
}

#[tokio::test]
async fn requests_can_arrive_together_or_in_pieces() {
    let mut harness = greeted().await;
    harness
        .client
        .send_raw(b"{\"id\":2,\"op\":\"actions\"}\n{\"id\":3,\"op\":\"run\",\"action\":\"tray:pc_sleep\"}\n")
        .await;
    let mut ids = vec![
        harness.client.next().await.unwrap()["id"].as_u64().unwrap(),
        harness.client.next().await.unwrap()["id"].as_u64().unwrap(),
    ];
    ids.sort_unstable();
    assert_eq!(ids, [2, 3]);

    harness.client.send_raw(b"{\"id\":4,\"op\":\"act").await;
    harness
        .client
        .expect_silence(Duration::from_millis(50))
        .await;
    harness.client.send_raw(b"ions\"}\n").await;
    assert_eq!(harness.client.next().await.unwrap()["id"], 4);
}

#[tokio::test]
async fn the_session_ends_when_the_client_closes() {
    let home_assistant = home();
    let actions = FakeActions::default();
    let (backend, _audit) = backend(&home_assistant, &actions, quick_timing());
    let (near, far) = tokio::io::duplex(MAX_LINE_BYTES);
    let session = tokio::spawn(run_session(far, backend));
    let mut client = Client::new(near);
    client.hello().await;
    client
        .request(json!({"op": "watch", "entities": ["light.desk"]}))
        .await;
    drop(client);
    tokio::time::timeout(super::stand_ins::PATIENCE, session)
        .await
        .expect("the session did not end")
        .unwrap();
    // With the session gone, nothing reads Home Assistant any more.
    tokio::time::sleep(Duration::from_millis(60)).await;
    let reads = home_assistant.state_reads();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(home_assistant.state_reads(), reads);
}

#[tokio::test]
async fn work_under_way_is_recorded_after_the_client_has_gone() {
    let home_assistant = home();
    let actions = FakeActions::with_actions(&[("hotkey:lights", "Lights")]);
    let run_gate = actions.hold_runs();
    let toggle_gate = home_assistant.hold_toggles();
    let (backend, audit) = backend(&home_assistant, &actions, quick_timing());
    let (near, far) = tokio::io::duplex(MAX_LINE_BYTES);
    let session = tokio::spawn(run_session(far, backend));
    let mut client = Client::new(near);
    client.hello().await;
    client
        .send(json!({"id": 2, "op": "run", "action": "hotkey:lights"}))
        .await;
    client
        .send(json!({"id": 3, "op": "toggle", "entity_id": "light.desk"}))
        .await;
    eventually("the action to start", || actions.ran() == ["hotkey:lights"]).await;
    eventually("the toggle to reach Home Assistant", || {
        home_assistant.requests() == ["toggle light.desk"]
    })
    .await;

    drop(client);
    tokio::time::timeout(super::stand_ins::PATIENCE, session)
        .await
        .expect("the session did not end")
        .unwrap();
    assert!(audit.lines().is_empty(), "{:?}", audit.lines());

    // Both finish after the session has ended, and both are recorded.
    run_gate.notify_one();
    toggle_gate.notify_one();
    eventually("both records", || audit.lines().len() == 2).await;
    let mut lines = audit.lines();
    lines.sort();
    assert_eq!(
        lines,
        [
            "streamdeck_run action=hotkey:lights result=ok",
            "streamdeck_toggle entity=light.desk result=ok",
        ]
    );
}
