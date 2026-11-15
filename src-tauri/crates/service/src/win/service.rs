//! The Windows service itself: the part the service manager talks to. It
//! turns the controls it receives into events for the runner and reports
//! the service's state back.

use std::ffi::OsString;
use std::sync::Mutex;
use std::time::Duration;
use windows_service::service::{
    PowerEventParam, ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState,
    ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{
    self, ServiceControlHandlerResult, ServiceStatusHandle,
};
use windows_service::{define_windows_service, service_dispatcher};

use super::{host, Launch};
use crate::runner::{event_channel, Ack, Event};

/// Name of the service in the service manager.
pub const SERVICE_NAME: &str = "DeskmateService";
/// Name shown in the Services console.
pub const DISPLAY_NAME: &str = "Deskmate service";
/// Description shown in the Services console.
pub const DESCRIPTION: &str = "Keeps this computer connected to Home Assistant while nobody is signed in: it reports the computer's sensors and can put it to sleep, hibernate it, shut it down or restart it. While a user is signed in, the Deskmate app holds the connection.";

/// How long Windows is kept waiting before a suspend while the service says
/// goodbye and closes its connection.
const SUSPEND_WAIT: Duration = Duration::from_millis(1500);
/// What the service manager is told a start or a stop may take.
const PENDING_HINT: Duration = Duration::from_secs(15);

/// What `run` was given; `service_main` gets no arguments of its own.
static LAUNCH: Mutex<Option<Launch>> = Mutex::new(None);

define_windows_service!(ffi_service_main, service_main);

/// Hands this process to the service manager. Returns when the service has
/// stopped. Fails at once when the process was not started by the service
/// manager.
pub fn run(launch: Launch) -> Result<(), String> {
    *LAUNCH.lock().unwrap_or_else(|e| e.into_inner()) = Some(launch);
    service_dispatcher::start(SERVICE_NAME, ffi_service_main).map_err(|error| {
        format!("cannot reach the service manager ({error}). `run` is what the service manager starts; it does not work from a terminal")
    })
}

fn report(handle: ServiceStatusHandle, state: ServiceState, exit_code: ServiceExitCode) {
    let pending = matches!(
        state,
        ServiceState::StartPending | ServiceState::StopPending
    );
    let status = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: if state == ServiceState::Running {
            // Preshutdown instead of shutdown: the two exclude each other,
            // and this one comes while the network is still up.
            ServiceControlAccept::STOP
                | ServiceControlAccept::PRESHUTDOWN
                | ServiceControlAccept::POWER_EVENT
                | ServiceControlAccept::SESSION_CHANGE
        } else {
            ServiceControlAccept::empty()
        },
        exit_code,
        checkpoint: u32::from(pending),
        wait_hint: if pending {
            PENDING_HINT
        } else {
            Duration::ZERO
        },
        process_id: None,
    };
    // Nothing can be done about a status the service manager refuses.
    let _ = handle.set_service_status(status);
}

fn service_main(_arguments: Vec<OsString>) {
    let Some(launch) = LAUNCH.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    let (events, incoming) = event_channel();
    let controls = events.clone();
    let handler = move |control| -> ServiceControlHandlerResult {
        let event = match control {
            ServiceControl::Stop => Event::Stop,
            ServiceControl::Shutdown | ServiceControl::Preshutdown => Event::Shutdown,
            ServiceControl::PowerEvent(PowerEventParam::Suspend) => {
                // Windows waits for this handler, so the goodbye still goes
                // out before the network does.
                let (ack, waiter) = Ack::pair();
                if controls.send(Event::Suspend(ack)).is_ok() {
                    waiter.wait(SUSPEND_WAIT);
                }
                return ServiceControlHandlerResult::NoError;
            }
            // Sent after every wake-up; a wake-up by the user sends
            // ResumeSuspend as well, which adds nothing.
            ServiceControl::PowerEvent(PowerEventParam::ResumeAutomatic) => Event::Resume,
            ServiceControl::PowerEvent(_) | ServiceControl::Interrogate => {
                return ServiceControlHandlerResult::NoError;
            }
            ServiceControl::SessionChange(_) => Event::SessionChange,
            _ => return ServiceControlHandlerResult::NotImplemented,
        };
        let _ = controls.send(event);
        ServiceControlHandlerResult::NoError
    };
    // Without a handler the service manager cannot be told anything.
    let Ok(handle) = service_control_handler::register(SERVICE_NAME, handler) else {
        return;
    };
    report(
        handle,
        ServiceState::StartPending,
        ServiceExitCode::NO_ERROR,
    );
    let result = host(launch, events, incoming, || {
        report(handle, ServiceState::Running, ServiceExitCode::NO_ERROR);
    });
    let exit_code = match result {
        Ok(()) => ServiceExitCode::NO_ERROR,
        Err(error) => ServiceExitCode::ServiceSpecific(error.code()),
    };
    report(handle, ServiceState::Stopped, exit_code);
}
