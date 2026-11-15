//! The displays behind the brightness number: the built-in panel through WMI
//! and external monitors through DDC/CI. Every handle and COM interface lives
//! only for one call of `with_displays`.

use super::brightness::Display;

/// Runs `action` on the displays found right now, built-in panels first.
#[cfg(windows)]
pub(super) fn with_displays<T>(action: impl FnOnce(&mut [Box<dyn Display>]) -> T) -> T {
    let _com = win::Com::initialize();
    let mut displays = win::wmi_panels();
    displays.extend(win::ddc_monitors());
    // The displays, and the COM interfaces and handles they hold, are dropped
    // before COM is uninitialized.
    action(&mut displays)
}

#[cfg(not(windows))]
pub(super) fn with_displays<T>(action: impl FnOnce(&mut [Box<dyn Display>]) -> T) -> T {
    action(&mut [])
}

#[cfg(windows)]
mod win {
    use super::super::brightness::{
        ddc_from_percent, percent_from_ddc, readings_by_instance, Display,
    };
    use windows::core::BSTR;
    use windows::Win32::Devices::Display::{
        DestroyPhysicalMonitor, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
    };
    use windows::Win32::Foundation::{HANDLE, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IDispatch, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::System::Wmi::{ISWbemLocator, ISWbemObject, ISWbemServices, SWbemLocator};

    /// The namespace of the monitor classes.
    const WMI_NAMESPACE: &str = "ROOT\\WMI";
    const WMI_READ_QUERY: &str =
        "SELECT InstanceName, CurrentBrightness FROM WmiMonitorBrightness WHERE Active = TRUE";
    const WMI_METHODS_QUERY: &str = "SELECT * FROM WmiMonitorBrightnessMethods WHERE Active = TRUE";
    const WMI_SET_METHOD: &str = "WmiSetBrightness";
    /// The Timeout argument of WmiSetBrightness, in seconds. 1 is the value
    /// Microsoft's examples pass.
    const WMI_SET_TIMEOUT_SECONDS: i32 = 1;
    /// More physical monitors behind one display than this is not real.
    const MAX_PHYSICAL_MONITORS: u32 = 16;
    /// More WMI objects than this is not real either.
    const MAX_WMI_OBJECTS: i32 = 16;

    /// COM for this thread, for as long as the guard lives.
    pub struct Com {
        initialized: bool,
    }

    impl Com {
        pub fn initialize() -> Self {
            // SAFETY: the reserved argument is None as required; a successful
            // call (S_OK or S_FALSE) is balanced by CoUninitialize in Drop.
            let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            Self {
                initialized: result.is_ok(),
            }
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            if self.initialized {
                // SAFETY: balances the successful CoInitializeEx of this
                // guard, on the same thread.
                unsafe { CoUninitialize() };
            }
        }
    }

    // ------------------------------------------------------------------ WMI

    /// A built-in panel: its WmiMonitorBrightnessMethods object and the
    /// brightness WmiMonitorBrightness reported for it.
    struct WmiPanel {
        methods: ISWbemObject,
        current: Option<u8>,
    }

    impl Display for WmiPanel {
        fn read(&mut self) -> Option<u8> {
            self.current
        }

        fn write(&mut self, percent: u8) -> Result<(), String> {
            set_wmi_brightness(&self.methods, percent)
                .map_err(|error| format!("the built-in panel refused the brightness: {error}"))?;
            self.current = Some(percent);
            Ok(())
        }
    }

    fn connect() -> Option<ISWbemServices> {
        // SAFETY: COM is initialized on this thread (Com guard); the BSTRs
        // live for the duration of the calls.
        unsafe {
            let locator: ISWbemLocator =
                CoCreateInstance(&SWbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
            let empty = BSTR::new();
            locator
                .ConnectServer(
                    &empty,
                    &BSTR::from(WMI_NAMESPACE),
                    &empty,
                    &empty,
                    &empty,
                    &empty,
                    0,
                    None,
                )
                .ok()
        }
    }

    fn objects(services: &ISWbemServices, query: &str) -> Vec<ISWbemObject> {
        // SAFETY: COM calls on interfaces owned by this thread; the BSTRs
        // live for the duration of the calls.
        unsafe {
            let Ok(set) = services.ExecQuery(&BSTR::from(query), &BSTR::from("WQL"), 0, None)
            else {
                return Vec::new();
            };
            let count = set.Count().unwrap_or(0).clamp(0, MAX_WMI_OBJECTS);
            (0..count)
                .filter_map(|index| set.ItemIndex(index).ok())
                .collect()
        }
    }

    fn property(object: &ISWbemObject, name: &str) -> Option<VARIANT> {
        // SAFETY: COM calls on an interface owned by this thread; the BSTR
        // lives for the duration of the call.
        unsafe {
            object
                .Properties_()
                .ok()?
                .Item(&BSTR::from(name), 0)
                .ok()?
                .Value()
                .ok()
        }
    }

    fn text(object: &ISWbemObject, name: &str) -> Option<String> {
        BSTR::try_from(&property(object, name)?)
            .ok()
            .map(|value| value.to_string())
    }

    fn number(object: &ISWbemObject, name: &str) -> Option<u32> {
        u32::try_from(&property(object, name)?).ok()
    }

    /// The active built-in panels with their methods object. A panel whose
    /// brightness cannot be read is kept: it can still be set.
    pub fn wmi_panels() -> Vec<Box<dyn Display>> {
        let Some(services) = connect() else {
            return Vec::new();
        };
        let readings = readings_by_instance(
            objects(&services, WMI_READ_QUERY)
                .iter()
                .filter_map(|object| {
                    Some((
                        text(object, "InstanceName")?,
                        number(object, "CurrentBrightness"),
                    ))
                })
                .collect(),
        );
        objects(&services, WMI_METHODS_QUERY)
            .into_iter()
            .map(|methods| {
                let current = text(&methods, "InstanceName")
                    .and_then(|instance| readings.get(&instance).copied());
                Box::new(WmiPanel { methods, current }) as Box<dyn Display>
            })
            .collect()
    }

    fn set_wmi_brightness(methods: &ISWbemObject, percent: u8) -> windows::core::Result<()> {
        let method = BSTR::from(WMI_SET_METHOD);
        let timeout = VARIANT::from(WMI_SET_TIMEOUT_SECONDS);
        let brightness = VARIANT::from(i32::from(percent));
        // SAFETY: COM calls on interfaces owned by this thread; the BSTRs and
        // VARIANTs live until the calls return.
        unsafe {
            let input = methods
                .Methods_()?
                .Item(&method, 0)?
                .InParameters()?
                .SpawnInstance_(0)?;
            let arguments = input.Properties_()?;
            arguments
                .Item(&BSTR::from("Timeout"), 0)?
                .SetValue(&timeout)?;
            arguments
                .Item(&BSTR::from("Brightness"), 0)?
                .SetValue(&brightness)?;
            methods.ExecMethod_(&method, &input, 0, None::<&IDispatch>)?;
        }
        Ok(())
    }

    // --------------------------------------------------------------- DDC/CI

    /// A physical monitor handle, destroyed on drop.
    struct DdcMonitor {
        handle: HANDLE,
        /// The monitor's own brightness range, once it answered.
        range: Option<(u32, u32)>,
    }

    impl Display for DdcMonitor {
        fn read(&mut self) -> Option<u8> {
            let (mut min, mut current, mut max) = (0u32, 0u32, 0u32);
            // SAFETY: `handle` is a physical monitor handle this value owns,
            // and the three pointers are valid for writes during the call.
            let answered =
                unsafe { GetMonitorBrightness(self.handle, &mut min, &mut current, &mut max) } != 0;
            let percent = percent_from_ddc(answered, min, current, max)?;
            self.range = Some((min, max));
            Some(percent)
        }

        fn write(&mut self, percent: u8) -> Result<(), String> {
            if self.range.is_none() {
                // Learns the range; the reading itself is not needed.
                let _ = self.read();
            }
            let (min, max) = self
                .range
                .ok_or_else(|| "a monitor does not answer over DDC/CI".to_string())?;
            // SAFETY: `handle` is a physical monitor handle this value owns.
            let accepted =
                unsafe { SetMonitorBrightness(self.handle, ddc_from_percent(min, max, percent)) }
                    != 0;
            if accepted {
                Ok(())
            } else {
                Err("a monitor refused the brightness over DDC/CI".into())
            }
        }
    }

    impl Drop for DdcMonitor {
        fn drop(&mut self) {
            // SAFETY: the handle came from GetPhysicalMonitorsFromHMONITOR,
            // is owned by this value alone and destroyed only here.
            if let Err(error) = unsafe { DestroyPhysicalMonitor(self.handle) } {
                log::warn!("cannot release a monitor handle: {error}");
            }
        }
    }

    unsafe extern "system" fn add_monitor(
        monitor: HMONITOR,
        _context: HDC,
        _area: *mut RECT,
        data: LPARAM,
    ) -> windows::core::BOOL {
        // SAFETY: `data` is the address of the Vec that `display_monitors`
        // passes, alive and borrowed by nothing else while
        // EnumDisplayMonitors runs, which calls this on the same thread.
        let monitors = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        monitors.push(monitor);
        true.into()
    }

    fn display_monitors() -> Vec<HMONITOR> {
        let mut monitors: Vec<HMONITOR> = Vec::new();
        let data = LPARAM(std::ptr::from_mut(&mut monitors) as isize);
        // SAFETY: the callback only pushes into `monitors`, which outlives
        // the call; no device context or clip area is passed.
        let enumerated = unsafe { EnumDisplayMonitors(None, None, Some(add_monitor), data) };
        if !enumerated.as_bool() {
            log::warn!("cannot list the displays for their brightness");
        }
        monitors
    }

    fn physical_monitors(monitor: HMONITOR) -> Vec<DdcMonitor> {
        let mut count = 0u32;
        // SAFETY: `monitor` came from EnumDisplayMonitors and `count` is valid
        // for the write.
        if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut count) }.is_err()
            || count == 0
        {
            return Vec::new();
        }
        let mut physical =
            vec![PHYSICAL_MONITOR::default(); count.min(MAX_PHYSICAL_MONITORS) as usize];
        // SAFETY: `physical` is a writable array of the length passed; on
        // success every handle in it is owned by us and wrapped right below.
        if unsafe { GetPhysicalMonitorsFromHMONITOR(monitor, &mut physical) }.is_err() {
            return Vec::new();
        }
        physical
            .into_iter()
            .map(|entry| DdcMonitor {
                handle: entry.hPhysicalMonitor,
                range: None,
            })
            .collect()
    }

    /// Every physical monitor; the ones without DDC/CI answer nothing.
    pub fn ddc_monitors() -> Vec<Box<dyn Display>> {
        display_monitors()
            .into_iter()
            .flat_map(physical_monitors)
            .map(|monitor| Box::new(monitor) as Box<dyn Display>)
            .collect()
    }
}
