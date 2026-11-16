//! Installing and removing the service. Both need an elevated terminal and
//! refuse to run without one.
//!
//! `install` copies this executable to Program Files, creates the data
//! folder, registers the service and starts it. Run over an existing
//! installation it replaces the executable and keeps settings and keys.
//! `uninstall` makes the service forget its settings and keys, removes the
//! service and deletes both folders.

use std::ffi::{c_void, OsStr, OsString};
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
use windows::Win32::System::Services::{
    ChangeServiceConfig2W, SC_HANDLE, SERVICE_CONFIG_REQUIRED_PRIVILEGES_INFO,
    SERVICE_REQUIRED_PRIVILEGES_INFOW,
};
use windows_service::service::{
    Service, ServiceAccess, ServiceAction, ServiceActionType, ServiceErrorControl, ServiceExitCode,
    ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceState,
    ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

use super::pipes::{read_provisioning_frame, AdminClient, PipeNames, ProcessExit};
use super::security::{check_private_folder, create_private_folder, is_elevated};
use super::service::{DESCRIPTION, DISPLAY_NAME, SERVICE_NAME};
use super::{data_dir, data_parent, program_dir, StartError, EXECUTABLE};
use crate::admin::{self, Request};

/// The privileges the service keeps. Windows removes every other one from
/// the service's token: bypass traverse checking is what every process
/// needs to open files by path, and shutdown is what sleep, hibernate, shut
/// down and restart need.
const REQUIRED_PRIVILEGES: [&str; 2] = ["SeChangeNotifyPrivilege", "SeShutdownPrivilege"];
/// The program a provisioning pipe must be served by.
pub const APP_PROGRAM: &str = "deskmate.exe";
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
const ERROR_SERVICE_NOT_ACTIVE: i32 = 1062;
const STATE_WAIT: Duration = Duration::from_secs(30);
const DEPROVISION_FRAME: &[u8] = b"{\"v\":1,\"op\":\"deprovision\"}\n";

/// What `install` was asked to do beyond installing.
pub struct InstallOptions {
    /// Last part of the name of the one-time pipe on which the app offers
    /// the provisioning frame.
    pub provision_pipe: Option<String>,
    /// File name of the program that must serve that pipe.
    pub provision_program: String,
    /// Arguments of the registered service after `run`.
    pub run_arguments: Vec<OsString>,
}

fn require_elevation(command: &str) -> Result<(), String> {
    if is_elevated() {
        Ok(())
    } else {
        Err(format!(
            "`{command}` changes the system and must be run from an elevated terminal (Run as administrator)"
        ))
    }
}

fn service_error(action: &str, error: windows_service::Error) -> String {
    format!("cannot {action}: {error}")
}

fn os_error_code(error: &windows_service::Error) -> Option<i32> {
    match error {
        windows_service::Error::Winapi(error) => error.raw_os_error(),
        _ => None,
    }
}

/// Opens the service, or None when it is not registered.
fn open_service(
    manager: &ServiceManager,
    access: ServiceAccess,
) -> Result<Option<Service>, String> {
    match manager.open_service(SERVICE_NAME, access) {
        Ok(service) => Ok(Some(service)),
        Err(error) if os_error_code(&error) == Some(ERROR_SERVICE_DOES_NOT_EXIST) => Ok(None),
        Err(error) => Err(service_error("open the service", error)),
    }
}

/// Waits until the service is in `wanted`. A service that stops while it
/// should be starting says why through its exit code.
fn wait_for(service: &Service, wanted: ServiceState) -> Result<(), String> {
    let deadline = Instant::now() + STATE_WAIT;
    loop {
        let status = service
            .query_status()
            .map_err(|e| service_error("ask for the service's state", e))?;
        if status.current_state == wanted {
            return Ok(());
        }
        if wanted == ServiceState::Running && status.current_state == ServiceState::Stopped {
            return Err(match status.exit_code {
                ServiceExitCode::ServiceSpecific(code) => format!(
                    "the service stopped right after starting: {} (code {code})",
                    StartError::explain(code)
                ),
                ServiceExitCode::Win32(code) => {
                    format!("the service stopped right after starting (Windows error {code})")
                }
            });
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the service did not reach the state {wanted:?} in time"
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn stop(service: &Service) -> Result<(), String> {
    let status = service
        .query_status()
        .map_err(|e| service_error("ask for the service's state", e))?;
    if status.current_state == ServiceState::Stopped {
        return Ok(());
    }
    // The service reports Stopped before its process has ended, and until
    // then its executable can be neither replaced nor deleted. A process
    // that cannot be opened has usually ended already; the wait is then for
    // the state alone, as it was before.
    let process = status
        .process_id
        .and_then(|process_id| ProcessExit::open(process_id).ok());
    match service.stop() {
        Ok(_) => {}
        Err(error) if os_error_code(&error) == Some(ERROR_SERVICE_NOT_ACTIVE) => {}
        Err(error) => return Err(service_error("stop the service", error)),
    }
    wait_for(service, ServiceState::Stopped)?;
    match process {
        Some(process) => process
            .wait(STATE_WAIT)
            .map_err(|error| format!("the service stopped, but {error}")),
        None => Ok(()),
    }
}

fn start(service: &Service) -> Result<(), String> {
    service
        .start::<&OsStr>(&[])
        .map_err(|e| service_error("start the service", e))?;
    wait_for(service, ServiceState::Running)
}

/// Limits the service's token to `REQUIRED_PRIVILEGES`.
fn set_required_privileges(service: &Service) -> Result<(), String> {
    // A list of NUL-terminated names with one more NUL at the end.
    let mut names: Vec<u16> = REQUIRED_PRIVILEGES
        .iter()
        .flat_map(|name| name.encode_utf16().chain([0]))
        .chain([0])
        .collect();
    let info = SERVICE_REQUIRED_PRIVILEGES_INFOW {
        pmszRequiredPrivileges: PWSTR(names.as_mut_ptr()),
    };
    // SAFETY: the handle belongs to `service`, which outlives the call and
    // was opened with the right to change its configuration. `info` points
    // at a list in the documented form, and both live until the call returns.
    unsafe {
        ChangeServiceConfig2W(
            SC_HANDLE(service.raw_handle()),
            SERVICE_CONFIG_REQUIRED_PRIVILEGES_INFO,
            Some(std::ptr::addr_of!(info).cast::<c_void>()),
        )
    }
    .map_err(|e| format!("cannot set the privileges of the service: {e}"))
}

/// The folder exists and can be trusted, or is created so that it can.
fn ensure_private_folder(path: &Path) -> Result<(), String> {
    if fs::symlink_metadata(path).is_ok() {
        check_private_folder(path)
            .map_err(|error| format!("{error}. Remove the folder and run install again"))
    } else {
        create_private_folder(path)
    }
}

fn same_file(first: &Path, second: &Path) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

/// Copies this executable into the program folder. The copy gets a
/// temporary name first, so a failed copy never leaves half a program
/// under the service's name.
fn copy_executable(target_dir: &Path) -> Result<std::path::PathBuf, String> {
    let source =
        std::env::current_exe().map_err(|e| format!("cannot find this executable: {e}"))?;
    fs::create_dir_all(target_dir)
        .map_err(|e| format!("cannot create {}: {e}", target_dir.display()))?;
    let target = target_dir.join(EXECUTABLE);
    if same_file(&source, &target) {
        return Ok(target);
    }
    let temporary = target_dir.join(format!("{EXECUTABLE}.new"));
    fs::copy(&source, &temporary)
        .map_err(|e| format!("cannot copy the service to {}: {e}", target_dir.display()))?;
    fs::rename(&temporary, &target)
        .map_err(|e| format!("cannot replace {}: {e}", target.display()))?;
    Ok(target)
}

/// The process the running service lives in.
fn service_process(service: &Service) -> Result<u32, String> {
    service
        .query_status()
        .map_err(|e| service_error("ask for the service's state", e))?
        .process_id
        .ok_or_else(|| "the service has no process".to_string())
}

/// Reads the provisioning frame from the app's one-time pipe and passes it
/// to the service.
fn provision_from(pipe: &str, program: &str, service: &Service) -> Result<(), String> {
    let frame = read_provisioning_frame(pipe, program)?;
    let line = frame.strip_suffix(b"\n").unwrap_or(&frame);
    match admin::parse(line) {
        Ok(Request::Provision(_)) => {}
        Ok(_) => return Err("the provisioning pipe sent something else than settings".into()),
        Err(refusal) => {
            return Err(format!(
                "the settings from the app are not valid: {}",
                refusal.detail()
            ))
        }
    }
    let mut client = AdminClient::connect(&PipeNames::standard(), Some(service_process(service)?))?;
    match client.request(&frame)? {
        (true, _) => Ok(()),
        (false, detail) => Err(format!("the service refused the settings: {detail}")),
    }
}

/// Installs the service, or updates an installed one, and starts it. `say`
/// receives what was done, one line at a time.
pub fn install(options: &InstallOptions, say: &mut dyn FnMut(&str)) -> Result<(), String> {
    require_elevation("install")?;
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| service_error("reach the service manager", e))?;
    let access = ServiceAccess::QUERY_STATUS
        | ServiceAccess::START
        | ServiceAccess::STOP
        | ServiceAccess::CHANGE_CONFIG;
    let existing = open_service(&manager, access)?;
    if let Some(service) = &existing {
        // A running executable cannot be replaced.
        stop(service)?;
    }

    let executable = copy_executable(&program_dir()?)?;
    ensure_private_folder(&data_parent()?)?;
    ensure_private_folder(&data_dir()?)?;

    let mut launch_arguments = vec![OsString::from("run")];
    launch_arguments.extend(options.run_arguments.iter().cloned());
    let info = ServiceInfo {
        name: SERVICE_NAME.into(),
        display_name: DISPLAY_NAME.into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: executable,
        launch_arguments,
        dependencies: Vec::new(),
        // LocalSystem
        account_name: None,
        account_password: None,
    };
    let service = match existing {
        Some(service) => {
            service
                .change_config(&info)
                .map_err(|e| service_error("update the service", e))?;
            say("Updated the installed service.");
            service
        }
        None => manager
            .create_service(&info, access)
            .map_err(|e| service_error("register the service", e))?,
    };
    service
        .set_description(DESCRIPTION)
        .map_err(|e| service_error("describe the service", e))?;
    // Delayed: the service is not needed for Windows to start.
    service
        .set_delayed_auto_start(true)
        .map_err(|e| service_error("set the start of the service", e))?;
    set_required_privileges(&service)?;
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(24 * 60 * 60)),
            reboot_msg: None,
            command: None,
            actions: Some(vec![
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: Duration::from_secs(5),
                },
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: Duration::from_secs(30),
                },
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: Duration::from_secs(5 * 60),
                },
            ]),
        })
        .map_err(|e| service_error("set what happens when the service fails", e))?;

    start(&service)?;
    say("The Deskmate service is installed and running.");

    if let Some(pipe) = &options.provision_pipe {
        provision_from(pipe, &options.provision_program, &service)?;
        say("The service has its settings.");
    }
    Ok(())
}

/// Makes the service forget its settings and keys. The keys are in the
/// vault of the system account, which only the service itself can reach.
fn forget_setup(service: &Service) -> Result<(), String> {
    let status = service
        .query_status()
        .map_err(|e| service_error("ask for the service's state", e))?;
    if status.current_state != ServiceState::Running {
        start(service)?;
    }
    let mut client = AdminClient::connect(&PipeNames::standard(), Some(service_process(service)?))?;
    match client.request(DEPROVISION_FRAME)? {
        (true, _) => Ok(()),
        (false, detail) => Err(detail),
    }
}

/// Removes a file or an empty folder when Windows starts the next time.
fn remove_at_restart(path: &Path) -> Result<(), String> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is a NUL-terminated path that lives until the call
    // returns; no new name is passed.
    unsafe { MoveFileExW(PCWSTR(wide.as_ptr()), None, MOVEFILE_DELAY_UNTIL_REBOOT) }
        .map_err(|e| format!("cannot mark {} for removal: {e}", path.display()))
}

fn remove_folder(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove {}: {error}", path.display())),
    }
}

/// Removes the service, its settings and keys and its two folders. `say`
/// receives what was done, one line at a time.
pub fn uninstall(say: &mut dyn FnMut(&str)) -> Result<(), String> {
    require_elevation("uninstall")?;
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| service_error("reach the service manager", e))?;
    let access = ServiceAccess::QUERY_STATUS
        | ServiceAccess::START
        | ServiceAccess::STOP
        | ServiceAccess::DELETE;
    if let Some(service) = open_service(&manager, access)? {
        match forget_setup(&service) {
            Ok(()) => say("The service forgot its settings and keys."),
            Err(error) => say(&format!(
                "Warning: the service could not be asked to forget its keys ({error}). If it had keys, they stay in the credential vault of the system account."
            )),
        }
        stop(&service)?;
        service
            .delete()
            .map_err(|e| service_error("remove the service", e))?;
        drop(service);
        // The registration goes away once the last handle to it is closed.
        let deadline = Instant::now() + STATE_WAIT;
        while open_service(&manager, ServiceAccess::QUERY_STATUS)?.is_some() {
            if Instant::now() >= deadline {
                return Err(
                    "the service is marked for removal but still registered; close the Services console and try again"
                        .into(),
                );
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        say("Removed the service.");
    } else {
        say("The service is not registered.");
    }

    remove_folder(&data_dir()?)?;
    // The folder above goes only when nothing else lives in it.
    let _ = fs::remove_dir(data_parent()?);

    let program = program_dir()?;
    let running_from_it = std::env::current_exe()
        .ok()
        .is_some_and(|current| same_file(&current, &program.join(EXECUTABLE)));
    if running_from_it {
        // Windows does not delete a program while it runs.
        remove_at_restart(&program.join(EXECUTABLE))?;
        remove_at_restart(&program)?;
        say(&format!(
            "{} is removed when Windows starts the next time.",
            program.display()
        ));
    } else {
        remove_folder(&program)?;
    }
    say("The Deskmate service is uninstalled.");
    Ok(())
}
