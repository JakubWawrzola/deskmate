//! Tests of the channel's messages: every request, and every input that has
//! to be refused.

use super::*;
use serde_json::json;

fn request(line: &str) -> Request {
    match parse_line(line) {
        Some(Incoming::Request { request, .. }) => request,
        other => panic!("not a request: {other:?}"),
    }
}

fn refusal(line: &str) -> (bool, ErrorCode) {
    match parse_line(line) {
        Some(Incoming::Refused { id, error }) => (id.is_some(), error),
        other => panic!("not refused: {other:?}"),
    }
}

fn bad_request(line: &str) {
    assert_eq!(refusal(line), (true, ErrorCode::BadRequest), "{line}");
}

fn id(number: u64) -> RequestId {
    RequestId(Number::from(number))
}

#[test]
fn a_line_that_is_not_a_json_object_closes_the_connection() {
    for line in ["", "   ", "not json", "{", "[1]", "1", "\"text\"", "null"] {
        assert_eq!(parse_line(line), None, "{line:?}");
    }
}

#[test]
fn a_request_without_a_usable_id_is_refused_without_one() {
    for line in [
        r#"{"op":"actions"}"#,
        r#"{"id":"1","op":"actions"}"#,
        r#"{"id":1.5,"op":"actions"}"#,
        r#"{"id":1e3,"op":"actions"}"#,
        r#"{"id":null,"op":"actions"}"#,
        r#"{"id":9007199254740992,"op":"actions"}"#,
        r#"{"id":-9007199254740992,"op":"actions"}"#,
    ] {
        assert_eq!(refusal(line), (false, ErrorCode::BadRequest), "{line}");
    }
}

#[test]
fn the_id_is_echoed_as_it_was_sent() {
    for (line, expected) in [
        (r#"{"id":7,"op":"actions"}"#, json!(7)),
        (r#"{"id":0,"op":"actions"}"#, json!(0)),
        (r#"{"id":-3,"op":"actions"}"#, json!(-3)),
        (
            r#"{"id":9007199254740991,"op":"actions"}"#,
            json!(9_007_199_254_740_991u64),
        ),
    ] {
        let Some(Incoming::Request { id, .. }) = parse_line(line) else {
            panic!("not a request: {line}");
        };
        assert_eq!(success(&id, Message::new())["id"], expected);
    }
}

#[test]
fn an_unknown_or_missing_op_is_a_bad_request() {
    bad_request(r#"{"id":1}"#);
    bad_request(r#"{"id":1,"op":7}"#);
    bad_request(r#"{"id":1,"op":"no_such_op"}"#);
    bad_request(r#"{"id":1,"op":"HELLO"}"#);
}

#[test]
fn hello_is_read_and_unknown_fields_are_ignored() {
    assert_eq!(
        request(
            r#"{"id":1,"op":"hello","v":1,"client":"streamdeck","version":"0.2.0","extra":[1]}"#
        ),
        Request::Hello
    );
}

#[test]
fn hello_with_another_version_is_refused_with_version() {
    for line in [
        r#"{"id":1,"op":"hello","v":2,"client":"streamdeck","version":"0.2.0"}"#,
        r#"{"id":1,"op":"hello","v":0,"client":"streamdeck","version":"0.2.0"}"#,
        // The other fields are not looked at: a later version may differ.
        r#"{"id":1,"op":"hello","v":2}"#,
    ] {
        assert_eq!(refusal(line), (true, ErrorCode::Version), "{line}");
    }
}

#[test]
fn hello_with_malformed_fields_is_a_bad_request() {
    bad_request(r#"{"id":1,"op":"hello","client":"streamdeck","version":"0.2.0"}"#);
    bad_request(r#"{"id":1,"op":"hello","v":"1","client":"streamdeck","version":"0.2.0"}"#);
    bad_request(r#"{"id":1,"op":"hello","v":1.0,"client":"streamdeck","version":"0.2.0"}"#);
    bad_request(r#"{"id":1,"op":"hello","v":-1,"client":"streamdeck","version":"0.2.0"}"#);
    bad_request(r#"{"id":1,"op":"hello","v":1,"version":"0.2.0"}"#);
    bad_request(r#"{"id":1,"op":"hello","v":1,"client":"streamdeck"}"#);
    bad_request(r#"{"id":1,"op":"hello","v":1,"client":"","version":"0.2.0"}"#);
    bad_request(r#"{"id":1,"op":"hello","v":1,"client":"Stream Deck","version":"0.2.0"}"#);
    bad_request(r#"{"id":1,"op":"hello","v":1,"client":"a\nb","version":"0.2.0"}"#);
    bad_request(r#"{"id":1,"op":"hello","v":1,"client":"streamdeck","version":"0.2 beta"}"#);
    let long = "a".repeat(MAX_CLIENT_FIELD_BYTES + 1);
    bad_request(&format!(
        r#"{{"id":1,"op":"hello","v":1,"client":"{long}","version":"0.2.0"}}"#
    ));
}

#[test]
fn run_takes_an_action_id() {
    assert_eq!(
        request(r#"{"id":3,"op":"run","action":"hotkey:lights"}"#),
        Request::Run {
            action: "hotkey:lights".into()
        }
    );
    // The shape of an id, whatever it names: the lookup decides.
    assert!(is_valid_action_id("command:lock"));
    bad_request(r#"{"id":3,"op":"run"}"#);
    bad_request(r#"{"id":3,"op":"run","action":7}"#);
    bad_request(r#"{"id":3,"op":"run","action":""}"#);
    bad_request(r#"{"id":3,"op":"run","action":"hotkey:Lights"}"#);
    bad_request(r#"{"id":3,"op":"run","action":"hotkey:a b"}"#);
    bad_request(r#"{"id":3,"op":"run","action":"hotkey:a\nresult=ok"}"#);
    let long = "a".repeat(MAX_ACTION_ID_BYTES + 1);
    bad_request(&format!(r#"{{"id":3,"op":"run","action":"{long}"}}"#));
}

#[test]
fn entities_takes_optional_domains() {
    assert_eq!(
        request(r#"{"id":4,"op":"entities"}"#),
        Request::Entities { domains: None }
    );
    assert_eq!(
        request(r#"{"id":4,"op":"entities","domains":["light","switch"]}"#),
        Request::Entities {
            domains: Some(vec!["light".into(), "switch".into()])
        }
    );
    assert_eq!(
        request(r#"{"id":4,"op":"entities","domains":[]}"#),
        Request::Entities {
            domains: Some(Vec::new())
        }
    );
    bad_request(r#"{"id":4,"op":"entities","domains":"light"}"#);
    bad_request(r#"{"id":4,"op":"entities","domains":null}"#);
    bad_request(r#"{"id":4,"op":"entities","domains":[1]}"#);
    bad_request(r#"{"id":4,"op":"entities","domains":["Light"]}"#);
    bad_request(r#"{"id":4,"op":"entities","domains":["light.desk"]}"#);
    bad_request(r#"{"id":4,"op":"entities","domains":[""]}"#);
    let many = vec!["light"; MAX_DOMAINS + 1];
    bad_request(&json!({"id": 4, "op": "entities", "domains": many}).to_string());
}

#[test]
fn watch_takes_a_list_of_entity_ids() {
    assert_eq!(
        request(r#"{"id":5,"op":"watch","entities":["light.desk","switch.fan","light.desk"]}"#),
        Request::Watch {
            entities: vec!["light.desk".into(), "switch.fan".into()]
        }
    );
    assert_eq!(
        request(r#"{"id":5,"op":"watch","entities":[]}"#),
        Request::Watch {
            entities: Vec::new()
        }
    );
    bad_request(r#"{"id":5,"op":"watch"}"#);
    bad_request(r#"{"id":5,"op":"watch","entities":"light.desk"}"#);
    bad_request(r#"{"id":5,"op":"watch","entities":[7]}"#);
    bad_request(r#"{"id":5,"op":"watch","entities":["desk"]}"#);
    bad_request(r#"{"id":5,"op":"watch","entities":["light.desk/../x"]}"#);
}

#[test]
fn a_watch_list_longer_than_the_limit_is_refused() {
    let list = |count: usize| -> Vec<String> {
        (0..count).map(|index| format!("light.l{index}")).collect()
    };
    let at_limit = json!({"id": 5, "op": "watch", "entities": list(MAX_WATCHED_ENTITIES)});
    let Request::Watch { entities } = request(&at_limit.to_string()) else {
        panic!("not a watch request");
    };
    assert_eq!(entities.len(), MAX_WATCHED_ENTITIES);
    let over = json!({"id": 5, "op": "watch", "entities": list(MAX_WATCHED_ENTITIES + 1)});
    bad_request(&over.to_string());
}

#[test]
fn toggle_takes_an_entity_id() {
    assert_eq!(
        request(r#"{"id":6,"op":"toggle","entity_id":"light.desk"}"#),
        Request::Toggle {
            entity_id: "light.desk".into()
        }
    );
    bad_request(r#"{"id":6,"op":"toggle"}"#);
    bad_request(r#"{"id":6,"op":"toggle","entity_id":["light.desk"]}"#);
    bad_request(r#"{"id":6,"op":"toggle","entity_id":"light"}"#);
    bad_request(r#"{"id":6,"op":"toggle","entity_id":"light.desk.lamp"}"#);
    bad_request(r#"{"id":6,"op":"toggle","entity_id":"Light.Desk"}"#);
    bad_request(r#"{"id":6,"op":"toggle","entity_id":"light.desk?x=1"}"#);
    bad_request(r#"{"id":6,"op":"toggle","entity_id":" light.desk"}"#);
    let long = format!("light.{}", "a".repeat(MAX_ENTITY_ID_BYTES));
    bad_request(&format!(r#"{{"id":6,"op":"toggle","entity_id":"{long}"}}"#));
}

#[test]
fn call_takes_a_service_and_optional_entity_and_data() {
    assert_eq!(
        request(
            r#"{"id":7,"op":"call","service":"scene.turn_on","entity_id":"scene.movie","data":{}}"#
        ),
        Request::Call(ServiceCall {
            domain: "scene".into(),
            service: "turn_on".into(),
            entity_id: Some("scene.movie".into()),
            data: Map::new(),
        })
    );
    let Request::Call(call) =
        request(r#"{"id":7,"op":"call","service":"light.turn_on","data":{"brightness_pct":40}}"#)
    else {
        panic!("not a call");
    };
    assert_eq!(call.entity_id, None);
    assert_eq!(call.data["brightness_pct"], 40);

    bad_request(r#"{"id":7,"op":"call"}"#);
    bad_request(r#"{"id":7,"op":"call","service":"turn_on"}"#);
    bad_request(r#"{"id":7,"op":"call","service":"light.turn_on.now"}"#);
    bad_request(r#"{"id":7,"op":"call","service":"light/turn_on"}"#);
    bad_request(r#"{"id":7,"op":"call","service":"Light.turn_on"}"#);
    bad_request(r#"{"id":7,"op":"call","service":".turn_on"}"#);
    bad_request(r#"{"id":7,"op":"call","service":"light.turn_on","entity_id":""}"#);
    bad_request(r#"{"id":7,"op":"call","service":"light.turn_on","entity_id":null}"#);
    bad_request(r#"{"id":7,"op":"call","service":"light.turn_on","entity_id":"desk"}"#);
    bad_request(r#"{"id":7,"op":"call","service":"light.turn_on","data":[1]}"#);
    bad_request(r#"{"id":7,"op":"call","service":"light.turn_on","data":"{}"}"#);
    bad_request(r#"{"id":7,"op":"call","service":"light.turn_on","data":null}"#);
    let long = "a".repeat(MAX_NAME_PART_BYTES + 1);
    bad_request(&format!(
        r#"{{"id":7,"op":"call","service":"light.{long}"}}"#
    ));
}

#[test]
fn a_failure_carries_its_code_and_sometimes_a_message() {
    assert_eq!(
        Value::Object(failure(Some(&id(3)), ErrorCode::UnknownAction)),
        json!({"id": 3, "ok": false, "error": "unknown_action",
               "message": "That action no longer exists."})
    );
    assert_eq!(
        Value::Object(failure(Some(&id(1)), ErrorCode::Version)),
        json!({"id": 1, "ok": false, "error": "version"})
    );
    assert_eq!(
        Value::Object(failure(None, ErrorCode::BadRequest)),
        json!({"ok": false, "error": "bad_request"})
    );
    assert_eq!(ErrorCode::HaNotConfigured.as_str(), "ha_not_configured");
    assert_eq!(ErrorCode::HaError.as_str(), "ha_error");
}

#[test]
fn the_hello_response_and_the_state_event_match_the_reference() {
    assert_eq!(
        Value::Object(hello_response(&id(1), "0.9.0", true)),
        json!({"id": 1, "ok": true, "v": 1, "app": "0.9.0", "ha": true})
    );
    assert_eq!(
        Value::Object(state_event("light.desk", "off")),
        json!({"ev": "state", "entity_id": "light.desk", "state": "off"})
    );
}

#[test]
fn a_short_list_is_sent_whole() {
    let items = vec![json!({"id": "hotkey:lights", "name": "Lights"})];
    assert_eq!(
        Value::Object(list_response(&id(2), "actions", items)),
        json!({"id": 2, "ok": true,
               "actions": [{"id": "hotkey:lights", "name": "Lights"}]})
    );
}

#[test]
fn a_list_is_trimmed_to_fit_one_line() {
    let items: Vec<Value> = (0..5_000)
        .map(|index| {
            json!({"entity_id": format!("light.lamp_{index}"),
                   "name": format!("Lamp {index} \u{142}"), "state": "on"})
        })
        .collect();
    let response = list_response(&id(4), "entities", items.clone());
    let line = serde_json::to_vec(&response).unwrap();
    assert!(line.len() <= MAX_LINE_BYTES);

    let kept = response["entities"].as_array().unwrap();
    assert!(kept.len() < items.len());
    assert_eq!(kept[..], items[..kept.len()]);
    // Nothing more would have fitted: the next item is longer than the
    // room that is left.
    let room = MAX_LINE_BYTES - line.len();
    assert!(serialized_len(&items[kept.len()]) + 1 > room);
}

#[test]
fn an_item_too_long_for_a_line_is_left_out_and_the_rest_is_kept() {
    let items = vec![
        json!({"entity_id": "light.a", "name": "A", "state": "on"}),
        json!({"entity_id": "light.b", "name": "b".repeat(MAX_LINE_BYTES), "state": "on"}),
        json!({"entity_id": "light.c", "name": "C", "state": "off"}),
    ];
    let response = list_response(&id(4), "entities", items.clone());
    assert_eq!(
        response["entities"],
        json!([items[0].clone(), items[2].clone()])
    );
}

#[test]
fn a_list_that_exactly_fills_the_line_is_kept_whole() {
    let empty = serialized_len(&list_response(&id(4), "entities", Vec::new()));
    // One string item: two quotes around the filler.
    let filler = "a".repeat(MAX_LINE_BYTES - empty - 2);
    let response = list_response(&id(4), "entities", vec![json!(filler)]);
    assert_eq!(serialized_len(&response), MAX_LINE_BYTES);
    assert_eq!(response["entities"].as_array().unwrap().len(), 1);

    let too_long = format!("{filler}a");
    let response = list_response(&id(4), "entities", vec![json!(too_long)]);
    assert!(response["entities"].as_array().unwrap().is_empty());
}
