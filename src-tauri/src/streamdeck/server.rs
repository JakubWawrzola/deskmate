//! The pipe of the channel: claims the name, keeps one free instance
//! listening, and hands every client that connects to its own session.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::watch;

use super::backend::{
    ActionRunner, AuditSink, Backend, ChannelState, ChannelStatus, HomeAssistant, Timing,
};
use super::pipe_security::{channel_sddl, current_user_sid, open_instance};
use super::session::run_session;

/// Audit event of the channel itself: `on`, `off`, `name_taken`, `failed`.
pub const AUDIT_CHANNEL: &str = "streamdeck_channel";
/// Audit event of a client: `connected`, `disconnected`.
pub const AUDIT_CLIENT: &str = "streamdeck_client";

/// Clients served at a time. One past the limit is disconnected at once.
pub const MAX_CONNECTIONS: usize = 16;

/// The first tries to claim the name, at the short delay, before the name
/// counts as taken by another program.
const NAME_CLAIM_GRACE_TRIES: u32 = 5;
/// The wait before another try when Windows refuses a further instance.
const INSTANCE_RETRY_INTERVAL: Duration = Duration::from_secs(1);

/// `ERROR_ACCESS_DENIED`: a pipe of this name exists and is not the app's.
const ERROR_ACCESS_DENIED: i32 = 5;
/// `ERROR_PIPE_BUSY`: a pipe of this name exists and allows no more instances.
const ERROR_PIPE_BUSY: i32 = 231;

struct Shared {
    state: Mutex<ChannelState>,
    connected: AtomicUsize,
    stopped: AtomicBool,
    /// Every session ends when this value changes.
    close_sessions: watch::Sender<u64>,
    audit: Arc<dyn AuditSink>,
    on_change: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn state(&self) -> ChannelState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_state(&self, state: ChannelState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
        (self.on_change)();
    }

    fn close_sessions(&self) {
        self.close_sessions.send_modify(|epoch| *epoch += 1);
    }
}

/// The running channel. Dropping it closes the pipe and every connection.
pub struct Channel {
    shared: Arc<Shared>,
    stop: watch::Sender<bool>,
}

impl Channel {
    /// Opens the pipe at `pipe_path` and serves it until the channel is
    /// dropped. `on_change` is called whenever the status changes.
    ///
    /// Has to be called inside the Tokio runtime.
    pub fn start<H, A>(
        pipe_path: String,
        backend: Arc<Backend<H, A>>,
        on_change: impl Fn() + Send + Sync + 'static,
    ) -> Self
    where
        H: HomeAssistant,
        A: ActionRunner,
    {
        let shared = Arc::new(Shared {
            state: Mutex::new(ChannelState::Starting),
            connected: AtomicUsize::new(0),
            stopped: AtomicBool::new(false),
            close_sessions: watch::Sender::new(0),
            audit: Arc::clone(&backend.audit),
            on_change: Box::new(on_change),
        });
        let (stop, stopped) = watch::channel(false);
        tokio::spawn(serve(pipe_path, Arc::clone(&shared), backend, stopped));
        Self { shared, stop }
    }

    pub fn status(&self) -> ChannelStatus {
        ChannelStatus {
            state: self.shared.state(),
            connected: self.shared.connected.load(Ordering::SeqCst),
        }
    }

    /// Ends every connection and keeps listening. Clients that reconnect
    /// read the app's settings again.
    pub fn close_connections(&self) {
        self.shared.close_sessions();
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        // The flag comes first: a client accepted at this very moment sees
        // it and is not served.
        self.shared.stopped.store(true, Ordering::SeqCst);
        let _ = self.stop.send(true);
        self.shared.close_sessions();
        if self.shared.state() == ChannelState::Listening {
            self.shared.audit.record(AUDIT_CHANNEL, "off");
        }
        self.shared.set_state(ChannelState::Off);
    }
}

/// `true` when the channel was stopped before the delay passed.
async fn stopped_within(stopped: &mut watch::Receiver<bool>, delay: Duration) -> bool {
    tokio::select! {
        _ = stopped.changed() => true,
        () = tokio::time::sleep(delay) => false,
    }
}

async fn serve<H, A>(
    pipe_path: String,
    shared: Arc<Shared>,
    backend: Arc<Backend<H, A>>,
    mut stopped: watch::Receiver<bool>,
) where
    H: HomeAssistant,
    A: ActionRunner,
{
    let sddl = match current_user_sid().and_then(|sid| channel_sddl(&sid)) {
        Ok(sddl) => sddl,
        Err(error) => {
            log::warn!("streamdeck: cannot build the access rules of the pipe: {error}");
            fail(&shared, "failed");
            return;
        }
    };
    let Some(mut listener) =
        claim_name(&pipe_path, &sddl, &shared, backend.timing, &mut stopped).await
    else {
        return;
    };
    if shared.stopped.load(Ordering::SeqCst) {
        return;
    }
    shared.set_state(ChannelState::Listening);
    shared.audit.record(AUDIT_CHANNEL, "on");

    loop {
        let connected = tokio::select! {
            _ = stopped.changed() => return,
            result = listener.connect() => result.is_ok(),
        };
        // The next free instance is opened while this one is still held, so
        // the name never stops being the app's.
        let next = loop {
            match open_instance(&pipe_path, &sddl, false) {
                Ok(next) => break next,
                Err(error) => {
                    log::warn!("streamdeck: cannot open another pipe instance: {error}");
                    if stopped_within(&mut stopped, INSTANCE_RETRY_INTERVAL).await {
                        return;
                    }
                }
            }
        };
        let stream = std::mem::replace(&mut listener, next);
        if connected {
            adopt(&shared, stream, &backend);
        }
    }
}

/// Opens the first instance of the pipe. While another program holds the
/// name this keeps trying, and says so in the status once the short grace
/// period is over. `None`: the channel was stopped first.
async fn claim_name(
    pipe_path: &str,
    sddl: &str,
    shared: &Shared,
    timing: Timing,
    stopped: &mut watch::Receiver<bool>,
) -> Option<NamedPipeServer> {
    let mut tries = 0u32;
    loop {
        let error = match open_instance(pipe_path, sddl, true) {
            Ok(listener) => return Some(listener),
            Err(error) => error,
        };
        tries += 1;
        if tries == NAME_CLAIM_GRACE_TRIES {
            if is_name_taken(&error) {
                log::warn!("streamdeck: another program holds the name of the pipe");
                shared.audit.record(AUDIT_CHANNEL, "name_taken");
                shared.set_state(ChannelState::NameTaken);
            } else {
                log::warn!("streamdeck: cannot open the pipe: {error}");
                fail(shared, "failed");
            }
        }
        let delay = if tries < NAME_CLAIM_GRACE_TRIES {
            timing.name_claim_grace_delay
        } else {
            timing.name_claim_retry_interval
        };
        if stopped_within(stopped, delay).await {
            return None;
        }
    }
}

fn is_name_taken(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(ERROR_ACCESS_DENIED | ERROR_PIPE_BUSY)
    )
}

fn fail(shared: &Shared, result: &str) {
    shared.audit.record(AUDIT_CHANNEL, result);
    shared.set_state(ChannelState::Failed);
}

/// Gives a connected client its own session.
fn adopt<H, A>(shared: &Arc<Shared>, stream: NamedPipeServer, backend: &Arc<Backend<H, A>>)
where
    H: HomeAssistant,
    A: ActionRunner,
{
    // Subscribed before the flag is read: a stop that comes after the read
    // changes the value this session waits on.
    let mut closing = shared.close_sessions.subscribe();
    if shared.stopped.load(Ordering::SeqCst)
        || shared.connected.load(Ordering::SeqCst) >= MAX_CONNECTIONS
    {
        return;
    }
    shared.connected.fetch_add(1, Ordering::SeqCst);
    shared.audit.record(AUDIT_CLIENT, "connected");
    (shared.on_change)();

    let shared = Arc::clone(shared);
    let backend = Arc::clone(backend);
    tokio::spawn(async move {
        tokio::select! {
            () = run_session(stream, backend) => {}
            _ = closing.changed() => {}
        }
        shared.connected.fetch_sub(1, Ordering::SeqCst);
        shared.audit.record(AUDIT_CLIENT, "disconnected");
        (shared.on_change)();
    });
}
