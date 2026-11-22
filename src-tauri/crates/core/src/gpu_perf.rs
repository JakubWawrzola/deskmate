//! The GPU temperature as the graphics driver reports it to Windows: the
//! reading Task Manager shows. It needs no driver of ours, no administrator
//! and no LibreHardwareMonitor. A driver that does not report one answers
//! zero, which stands for "no sensor" here.

/// The driver counts in tenths of a degree Celsius.
const DECI_PER_DEGREE: f64 = 10.0;
/// Above this the number is not a temperature of a working graphics card.
const MAX_PLAUSIBLE_CELSIUS: f64 = 150.0;

/// The driver's number as degrees Celsius; none for zero and for nonsense.
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "only Windows asks the driver")
)]
fn celsius(deci_degrees: u32) -> Option<f64> {
    let degrees = f64::from(deci_degrees) / DECI_PER_DEGREE;
    (degrees > 0.0 && degrees <= MAX_PLAUSIBLE_CELSIUS).then_some(degrees)
}

/// The temperature of the hottest graphics card, or none when no driver
/// reports one.
#[cfg(windows)]
pub fn temperature_celsius() -> Option<f64> {
    adapter_luids()
        .into_iter()
        .filter_map(driver_temperature)
        .filter_map(celsius)
        .fold(None, |hottest: Option<f64>, value| {
            Some(hottest.map_or(value, |so_far| so_far.max(value)))
        })
}

/// The graphics cards of this computer, without the software renderer.
#[cfg(windows)]
fn adapter_luids() -> Vec<windows::Win32::Foundation::LUID> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
    };
    let mut luids = Vec::new();
    // SAFETY: the DXGI calls take no pointers from here; the factory and the
    // adapters are COM objects that windows-rs releases when they go out of
    // scope.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return luids;
        };
        let mut index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            if let Ok(description) = adapter.GetDesc1() {
                if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 {
                    luids.push(description.AdapterLuid);
                }
            }
            index += 1;
        }
    }
    luids
}

/// What the driver of one graphics card reports as its temperature, in
/// tenths of a degree.
#[cfg(windows)]
fn driver_temperature(luid: windows::Win32::Foundation::LUID) -> Option<u32> {
    use windows::Wdk::Graphics::Direct3D::{
        D3DKMTCloseAdapter, D3DKMTOpenAdapterFromLuid, D3DKMTQueryAdapterInfo,
        D3DKMT_ADAPTER_PERFDATA, D3DKMT_CLOSEADAPTER, D3DKMT_OPENADAPTERFROMLUID,
        D3DKMT_QUERYADAPTERINFO, KMTQAITYPE_ADAPTERPERFDATA,
    };

    // SAFETY: every pointer handed to the three calls points at a local of
    // the type the call expects and lives until the call returns. The query
    // names `performance` as its buffer together with that struct's size, so
    // the kernel writes no further. The adapter handle is closed once, on
    // every path after it was opened.
    unsafe {
        let mut open = D3DKMT_OPENADAPTERFROMLUID {
            AdapterLuid: luid,
            hAdapter: 0,
        };
        if D3DKMTOpenAdapterFromLuid(&mut open).is_err() {
            return None;
        }
        let mut performance = D3DKMT_ADAPTER_PERFDATA::default();
        let mut query = D3DKMT_QUERYADAPTERINFO {
            hAdapter: open.hAdapter,
            Type: KMTQAITYPE_ADAPTERPERFDATA,
            pPrivateDriverData: std::ptr::from_mut(&mut performance).cast(),
            PrivateDriverDataSize: std::mem::size_of::<D3DKMT_ADAPTER_PERFDATA>() as u32,
        };
        let status = D3DKMTQueryAdapterInfo(&mut query);
        let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER {
            hAdapter: open.hAdapter,
        });
        status.is_ok().then_some(performance.Temperature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenths_of_a_degree_become_degrees() {
        assert_eq!(celsius(455), Some(45.5));
        assert_eq!(celsius(1), Some(0.1));
        assert_eq!(celsius(1500), Some(150.0));
    }

    #[test]
    fn zero_and_nonsense_mean_no_sensor() {
        assert_eq!(celsius(0), None);
        assert_eq!(celsius(1501), None);
        assert_eq!(celsius(u32::MAX), None);
    }

    /// Asks the driver of the computer the test runs on. Whether it has a
    /// sensor depends on the graphics card, so only the range is checked.
    #[cfg(windows)]
    #[test]
    fn a_reported_temperature_is_plausible() {
        if let Some(degrees) = temperature_celsius() {
            assert!(degrees > 0.0 && degrees <= MAX_PLAUSIBLE_CELSIUS);
        }
    }
}
