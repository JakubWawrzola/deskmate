//! The channel on a real named pipe, with stand-ins for Home Assistant and
//! the action executor, driven by a real pipe client.
//!
//! Every pipe here has a test-only name. The name of the real channel is
//! never used: a real Deskmate may be running on this machine.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, ServerOptions};

use super::backend::{ChannelState, Timing};
use super::pipe_name::PIPE_NAME_PREFIX;
use super::server::{Channel, MAX_CONNECTIONS};
use super::stand_ins::{
    backend, eventually, quick_timing, Client, FakeActions, FakeHomeAssistant, RecordedAudit,
    PATIENCE,
};

static NEXT_PIPE: AtomicUsize = AtomicUsize::new(0);

/// A pipe path no other test and no real channel uses.
fn test_pipe_path() -> String {
    format!(
        r"\\.\pipe\{PIPE_NAME_PREFIX}test-{}-{}",
        std::process::id(),
        NEXT_PIPE.fetch_add(1, Ordering::SeqCst)
    )
}

struct Running {
    channel: Channel,
    pipe_path: String,
    home_assistant: FakeHomeAssistant,
    actions: FakeActions,
    audit: Arc<RecordedAudit>,
    /// How often the channel reported a change of its status.
    changes: Arc<AtomicUsize>,
}

fn start_at(pipe_path: String, timing: Timing) -> Running {
    let home_assistant = FakeHomeAssistant::with_entities(&[
        ("light.desk", "Desk", "on"),
        ("switch.fan", "Fan", "off"),
        ("scene.movie", "Movie", "unknown"),
    ]);
    let actions =
        FakeActions::with_actions(&[("hotkey:lights", "Lights"), ("tray:pc_sleep", "Sleep")]);
    let (backend, audit) = backend(&home_assistant, &actions, timing);
    let changes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&changes);
    let channel = Channel::start(pipe_path.clone(), backend, move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    Running {
        channel,
        pipe_path,
        home_assistant,
        actions,
        audit,
        changes,
    }
}

async fn start() -> Running {
    let running = start_at(test_pipe_path(), quick_timing());
    running.wait_for(ChannelState::Listening).await;
    running
}

impl Running {
    async fn wait_for(&self, state: ChannelState) {
        eventually(&format!("the channel to be {state:?}"), || {
            self.channel.status().state == state
        })
        .await;
    }

    async fn wait_for_connected(&self, count: usize) {
        eventually(&format!("{count} connected clients"), || {
            self.channel.status().connected == count
        })
        .await;
    }

    async fn connect(&self) -> Client<NamedPipeClient> {
        Client::new(open(&self.pipe_path).await)
    }
}

/// Opens the pipe like any client. A pipe whose free instance was just
/// taken is busy for a moment; that is tried again.
async fn open(pipe_path: &str) -> NamedPipeClient {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        match ClientOptions::new().open(pipe_path) {
            Ok(client) => return client,
            Err(error) => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "cannot open the pipe: {error}"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
}

#[tokio::test]
async fn a_client_talks_to_the_channel_over_a_real_pipe() {
    let running = start().await;
    let mut client = running.connect().await;

    assert_eq!(
        client.hello().await,
        json!({"id": 1, "ok": true, "v": 1, "app": "0.9.0", "ha": true})
    );
    assert_eq!(
        client.request(json!({"op": "actions"})).await["actions"],
        json!([
            {"id": "hotkey:lights", "name": "Lights"},
            {"id": "tray:pc_sleep", "name": "Sleep"},
        ])
    );
    assert_eq!(
        client
            .request(json!({"op": "run", "action": "hotkey:lights"}))
            .await["ok"],
        true
    );
    assert_eq!(
        client
            .request(json!({"op": "run", "action": "hotkey:missing"}))
            .await["error"],
        "unknown_action"
    );
    assert_eq!(
        client
            .request(json!({"op": "entities", "domains": ["light"]}))
            .await["entities"],
        json!([{"entity_id": "light.desk", "name": "Desk", "state": "on"}])
    );
    assert_eq!(
        client
            .request(json!({"op": "watch", "entities": ["light.desk"]}))
            .await["states"],
        json!({"light.desk": "on"})
    );
    assert_eq!(
        client
            .request(json!({"op": "toggle", "entity_id": "light.desk"}))
            .await["ok"],
        true
    );
    assert_eq!(
        client.event().await,
        json!({"ev": "state", "entity_id": "light.desk", "state": "off"})
    );
    assert_eq!(
        client
            .request(json!({"op": "call", "service": "scene.turn_on",
                            "entity_id": "scene.movie", "data": {}}))
            .await["ok"],
        true
    );
    assert_eq!(
        client.request(json!({"op": "no_such_op"})).await["error"],
        "bad_request"
    );

    assert_eq!(running.actions.ran(), ["hotkey:lights"]);
    assert_eq!(
        running.home_assistant.requests(),
        [
            "entities",
            "toggle light.desk",
            "call scene.turn_on scene.movie {}"
        ]
    );

    drop(client);
    running.wait_for_connected(0).await;
    assert_eq!(
        running.audit.lines(),
        [
            "streamdeck_channel on",
            "streamdeck_client connected",
            "streamdeck_run action=hotkey:lights result=ok",
            "streamdeck_run action=hotkey:missing result=unknown_action",
            "streamdeck_toggle entity=light.desk result=ok",
            "streamdeck_call service=scene.turn_on entity=scene.movie result=ok",
            "streamdeck_client disconnected",
        ]
    );
}

#[tokio::test]
async fn several_clients_are_served_at_once_each_with_its_own_watch_list() {
    let running = start().await;
    let mut first = running.connect().await;
    let mut second = running.connect().await;
    first.hello().await;
    second.hello().await;
    running.wait_for_connected(2).await;

    first
        .request(json!({"op": "watch", "entities": ["light.desk"]}))
        .await;
    second
        .request(json!({"op": "watch", "entities": ["switch.fan"]}))
        .await;

    running.home_assistant.set_state("light.desk", "off");
    assert_eq!(first.event().await["entity_id"], "light.desk");
    second.expect_silence(Duration::from_millis(150)).await;

    running.home_assistant.set_state("switch.fan", "on");
    assert_eq!(second.event().await["entity_id"], "switch.fan");
    first.expect_silence(Duration::from_millis(150)).await;

    // A free instance is listening while both are connected.
    let mut third = running.connect().await;
    assert_eq!(third.hello().await["ok"], true);
    running.wait_for_connected(3).await;

    drop(first);
    running.wait_for_connected(2).await;
    assert_eq!(second.request(json!({"op": "actions"})).await["ok"], true);
}

#[tokio::test]
async fn a_connection_has_to_say_hello_itself() {
    let running = start().await;
    let mut greeted = running.connect().await;
    greeted.hello().await;
    let mut silent = running.connect().await;
    assert_eq!(
        silent.request(json!({"op": "actions"})).await["error"],
        "bad_request"
    );
}

#[tokio::test]
async fn the_status_follows_the_channel_and_its_clients() {
    let running = start_at(test_pipe_path(), quick_timing());
    running.wait_for(ChannelState::Listening).await;
    assert_eq!(running.channel.status().connected, 0);
    let changes_when_listening = running.changes.load(Ordering::SeqCst);
    assert!(changes_when_listening >= 1);

    let client = running.connect().await;
    running.wait_for_connected(1).await;
    assert!(running.changes.load(Ordering::SeqCst) > changes_when_listening);
    let changes_when_connected = running.changes.load(Ordering::SeqCst);

    drop(client);
    running.wait_for_connected(0).await;
    assert!(running.changes.load(Ordering::SeqCst) > changes_when_connected);
}

#[tokio::test]
async fn switching_the_channel_off_closes_the_pipe_and_its_connections() {
    let running = start().await;
    let mut client = running.connect().await;
    client.hello().await;
    client
        .request(json!({"op": "watch", "entities": ["light.desk"]}))
        .await;
    let Running {
        channel,
        pipe_path,
        home_assistant,
        audit,
        ..
    } = running;

    drop(channel);

    client.expect_closed().await;
    // The name is gone as soon as the last client has let go of it.
    drop(client);
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while ClientOptions::new().open(&pipe_path).is_ok() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the pipe is still open"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // Nothing is read from Home Assistant any more.
    tokio::time::sleep(Duration::from_millis(80)).await;
    let reads = home_assistant.state_reads();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(home_assistant.state_reads(), reads);

    let lines = audit.lines();
    assert!(
        lines.contains(&"streamdeck_channel off".to_string()),
        "{lines:?}"
    );
    assert_eq!(
        lines.last().map(String::as_str),
        Some("streamdeck_client disconnected")
    );
}

#[tokio::test]
async fn closing_the_connections_keeps_the_channel_and_lets_clients_read_ha_again() {
    let running = start().await;
    running.home_assistant.set_configured(false);
    let mut client = running.connect().await;
    assert_eq!(client.hello().await["ha"], false);

    // The user fills in the Home Assistant address and token.
    running.home_assistant.set_configured(true);
    running.channel.close_connections();
    client.expect_closed().await;
    running.wait_for_connected(0).await;
    assert_eq!(running.channel.status().state, ChannelState::Listening);

    let mut client = running.connect().await;
    assert_eq!(client.hello().await["ha"], true);
    assert_eq!(client.request(json!({"op": "entities"})).await["ok"], true);
}

#[tokio::test]
async fn a_name_held_by_another_program_is_reported_and_not_joined() {
    let pipe_path = test_pipe_path();
    // Another program got to the name first, with rules of its own that
    // would let the app add instances.
    let squatter = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_path)
        .unwrap();

    let running = start_at(pipe_path, quick_timing());
    running.wait_for(ChannelState::NameTaken).await;
    assert_eq!(running.audit.lines(), ["streamdeck_channel name_taken"]);

    // A client that opens the name reaches the other program, not the app.
    let client = ClientOptions::new().open(&running.pipe_path).unwrap();
    tokio::time::timeout(PATIENCE, squatter.connect())
        .await
        .expect("the client did not reach the other program")
        .unwrap();
    assert_eq!(running.channel.status().connected, 0);

    // Once the other program is gone, the channel comes up by itself.
    drop(client);
    drop(squatter);
    running.wait_for(ChannelState::Listening).await;
    let mut client = running.connect().await;
    assert_eq!(client.hello().await["ok"], true);
    assert_eq!(
        running.audit.lines()[..2],
        ["streamdeck_channel name_taken", "streamdeck_channel on"]
    );
}

#[tokio::test]
async fn a_client_still_holding_the_previous_channel_does_not_count_as_a_squatter() {
    // A grace period long enough that a busy machine does not use it up
    // before the client below lets go.
    let timing = Timing {
        name_claim_grace_delay: Duration::from_millis(250),
        ..quick_timing()
    };
    let pipe_path = test_pipe_path();
    let previous = start_at(pipe_path.clone(), timing);
    previous.wait_for(ChannelState::Listening).await;
    let mut lingering = previous.connect().await;
    lingering.hello().await;
    drop(previous);

    // Switched on again at once, while the client has not let go yet.
    let running = start_at(pipe_path, timing);
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(lingering);

    running.wait_for(ChannelState::Listening).await;
    assert_eq!(running.audit.lines(), ["streamdeck_channel on"]);
    let mut client = running.connect().await;
    assert_eq!(client.hello().await["ok"], true);
}

#[tokio::test]
async fn a_channel_switched_off_while_the_name_is_taken_stops_trying() {
    let pipe_path = test_pipe_path();
    let squatter = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_path)
        .unwrap();
    let running = start_at(pipe_path.clone(), quick_timing());
    running.wait_for(ChannelState::NameTaken).await;
    let audit = Arc::clone(&running.audit);
    drop(running);
    drop(squatter);

    // Nobody claims the name now.
    tokio::time::sleep(quick_timing().name_claim_retry_interval * 3).await;
    assert!(ClientOptions::new().open(&pipe_path).is_err());
    assert_eq!(audit.lines(), ["streamdeck_channel name_taken"]);
}

#[tokio::test]
async fn clients_past_the_limit_are_disconnected_at_once() {
    let running = start().await;
    let mut clients = Vec::new();
    for _ in 0..MAX_CONNECTIONS {
        let mut client = running.connect().await;
        assert_eq!(client.hello().await["ok"], true);
        clients.push(client);
    }
    running.wait_for_connected(MAX_CONNECTIONS).await;

    let mut one_too_many = running.connect().await;
    one_too_many.expect_closed().await;
    assert_eq!(running.channel.status().connected, MAX_CONNECTIONS);

    // When one leaves, there is room again.
    clients.pop();
    running.wait_for_connected(MAX_CONNECTIONS - 1).await;
    let mut client = running.connect().await;
    assert_eq!(client.hello().await["ok"], true);
}

/// The plugin's own channel client against this server.
///
/// Starts the channel on a test-only pipe and runs
/// `streamdeck-plugin/test/channel-app.test.ts` with Node, pointed at that
/// pipe through `DESKMATE_STREAMDECK_PIPE`. Needs Node 24 on the PATH, which
/// the Rust job of CI does not have, so it runs on request:
/// `cargo test -p deskmate plugin_client -- --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Node 24 on the PATH"]
async fn the_plugin_client_talks_to_the_channel() {
    let running = Arc::new(start().await);
    // The plugin's test asks for this action to have its connection dropped,
    // which is what the app does when the Home Assistant settings change.
    running.actions.set_actions(
        [
            ("hotkey:lights", "Lights"),
            ("tray:pc_sleep", "Sleep"),
            ("tray:drop_connections", "Drop connections"),
        ]
        .iter()
        .map(|(id, name)| super::backend::ActionEntry {
            id: id.to_string(),
            name: name.to_string(),
        })
        .collect(),
    );
    let dropper = Arc::downgrade(&running);
    running.actions.on_run(move |action_id| {
        if action_id == "tray:drop_connections" {
            if let Some(running) = dropper.upgrade() {
                running.channel.close_connections();
            }
        }
    });

    let plugin_folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("streamdeck-plugin");
    let output = tokio::task::spawn_blocking({
        let pipe_path = running.pipe_path.clone();
        move || {
            std::process::Command::new("node")
                .args(["--test", "test/channel-app.test.ts"])
                .current_dir(plugin_folder)
                .env("DESKMATE_STREAMDECK_PIPE", pipe_path)
                .output()
        }
    })
    .await
    .unwrap()
    .expect("node could not be started");

    assert!(
        output.status.success(),
        "the plugin's test failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // A test that was skipped also exits with success; the summary of the
    // test runner tells them apart, in either of its two report formats.
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("pass 1") && text.contains("skipped 0"),
        "the test did not run:\n{text}"
    );
    assert_eq!(
        running.actions.ran(),
        ["hotkey:lights", "tray:drop_connections"]
    );
}
