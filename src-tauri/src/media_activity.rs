//! Camera and microphone "in use" sensors (GitHub issue #4).
//!
//! Deskmate used to read CapabilityAccessManager\ConsentStore, where a
//! LastUsedTimeStop of 0 meant "in use right now". Current Windows 11 builds
//! keep a positive, unchanged LastUsedTimeStop while an app is still
//! capturing, so both sensors stayed off. HASS.Agent hit the same change.
//!
//! Live sources instead:
//! - microphone: an active Core Audio session on any active capture device;
//! - camera: the Media Foundation sensor activity monitor (Windows 10 2004+).
//!   It runs in the background once the camera sensor is enabled and reports
//!   a snapshot of the sensors in use whenever that changes. Monitoring opens
//!   no camera.
//!
//! The registry is only a fallback while a live source is unavailable. A live
//! "nothing in use" answer wins over a stale registry entry.

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Once;
    use windows::core::{implement, Ref, Result};
    use windows::Win32::Media::Audio::{
        eCapture, AudioSessionStateActive, IAudioSessionManager2, IMMDeviceEnumerator,
        MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::Media::MediaFoundation::{
        IMFSensorActivitiesReport, IMFSensorActivitiesReportCallback,
        IMFSensorActivitiesReportCallback_Impl, MFCreateSensorActivityMonitor, MFStartup,
        MFSTARTUP_NOSOCKET, MF_VERSION,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    /// Whether any app is recording from an active capture device.
    pub fn capture_session_active() -> Result<bool> {
        unsafe {
            // Balanced per call: the sensor collector runs on varying blocking
            // threads. S_FALSE (already initialised) also needs the uninit.
            let init = CoInitializeEx(None, COINIT_MULTITHREADED);
            let result = scan_capture_sessions();
            if init.is_ok() {
                CoUninitialize();
            }
            result
        }
    }

    unsafe fn scan_capture_sessions() -> Result<bool> {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
        for i in 0..devices.GetCount()? {
            let manager: IAudioSessionManager2 = devices.Item(i)?.Activate(CLSCTX_ALL, None)?;
            let sessions = manager.GetSessionEnumerator()?;
            for j in 0..sessions.GetCount()? {
                if sessions.GetSession(j)?.GetState()? == AudioSessionStateActive {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    const UNKNOWN: u8 = 0;
    const IDLE: u8 = 1;
    const ACTIVE: u8 = 2;
    /// Last camera state reported by the monitor; UNKNOWN until the first report.
    static CAMERA: AtomicU8 = AtomicU8::new(UNKNOWN);
    static MONITOR: Once = Once::new();

    #[implement(IMFSensorActivitiesReportCallback)]
    struct CameraReports;

    impl IMFSensorActivitiesReportCallback_Impl for CameraReports_Impl {
        fn OnActivitiesReport(&self, report: Ref<'_, IMFSensorActivitiesReport>) -> Result<()> {
            let report = report.ok()?;
            let mut streaming = false;
            unsafe {
                for i in 0..report.GetCount()? {
                    let sensor = report.GetActivityReport(i)?;
                    for p in 0..sensor.GetProcessCount()? {
                        if sensor.GetProcessActivity(p)?.GetStreamingState()?.as_bool() {
                            streaming = true;
                        }
                    }
                }
            }
            CAMERA.store(if streaming { ACTIVE } else { IDLE }, Ordering::Relaxed);
            Ok(())
        }
    }

    /// Some(in use) from the live monitor, None while it has nothing to say.
    pub fn camera_state() -> Option<bool> {
        MONITOR.call_once(|| {
            let spawned = std::thread::Builder::new()
                .name("deskmate-camera-monitor".into())
                .spawn(run_camera_monitor);
            if let Err(e) = spawned {
                log::warn!("camera monitor thread failed to start: {e}");
            }
        });
        match CAMERA.load(Ordering::Relaxed) {
            IDLE => Some(false),
            ACTIVE => Some(true),
            _ => None,
        }
    }

    /// Reports arrive on Media Foundation's own threads. This thread only keeps
    /// the monitor and its apartment alive for the life of the app.
    fn run_camera_monitor() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) {
                log::warn!("camera monitor unavailable (MFStartup: {e}); using the registry");
                return;
            }
            let callback: IMFSensorActivitiesReportCallback = CameraReports.into();
            let monitor = match MFCreateSensorActivityMonitor(&callback) {
                Ok(monitor) => monitor,
                Err(e) => {
                    log::warn!("camera monitor unavailable ({e}); using the registry");
                    return;
                }
            };
            if let Err(e) = monitor.Start() {
                log::warn!("camera monitor did not start ({e}); using the registry");
                return;
            }
            log::info!("camera activity monitor started");
            loop {
                std::thread::park();
            }
        }
    }
}

/// Whether an app is capturing from the microphone right now.
#[cfg(windows)]
pub fn microphone_in_use() -> bool {
    match imp::capture_session_active() {
        Ok(active) => active,
        Err(e) => {
            log::debug!("capture sessions unavailable ({e}); using the registry");
            crate::sensors::win::capability_in_use("microphone")
        }
    }
}

/// Whether an app is streaming from a camera right now.
#[cfg(windows)]
pub fn camera_in_use() -> bool {
    imp::camera_state().unwrap_or_else(|| crate::sensors::win::capability_in_use("webcam"))
}

#[cfg(not(windows))]
pub fn microphone_in_use() -> bool {
    false
}

#[cfg(not(windows))]
pub fn camera_in_use() -> bool {
    false
}
