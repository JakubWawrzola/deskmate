//! A WQL query against WMI, for the readings Windows offers nowhere else.
//! One call opens COM, runs one query and returns plain values, so callers
//! hold no COM object and need no unsafe code.

use windows::core::BSTR;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Wmi::{ISWbemLocator, SWbemLocator};

/// Most rows read from one query. A class with more instances than this is
/// not one of the small lists this module is for.
const MAX_ROWS: i32 = 512;

/// One property of one row.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Text(String),
    Number(f64),
    /// The property is missing, empty, or of a type that is neither.
    Empty,
}

impl Field {
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            // WMI hands out 64-bit integers as text.
            Self::Text(text) => text.trim().parse().ok(),
            Self::Empty => None,
        }
    }
}

/// Runs `wql` in `namespace` and returns, for every row, the properties in
/// `names`, in that order. A namespace or class that does not exist, and a
/// query the account may not run, give no rows.
pub fn query(namespace: &str, wql: &str, names: &[&str]) -> Vec<Vec<Field>> {
    // SAFETY: CoInitializeEx is balanced by CoUninitialize below, and only
    // when this call is the one that initialized COM on the thread. Every
    // COM object made in between is dropped inside `rows` before that.
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let found = rows(namespace, wql, names).unwrap_or_default();
        if initialized {
            CoUninitialize();
        }
        found
    }
}

/// Unsafe: COM must be initialized on the calling thread.
unsafe fn rows(
    namespace: &str,
    wql: &str,
    names: &[&str],
) -> windows::core::Result<Vec<Vec<Field>>> {
    let locator: ISWbemLocator = CoCreateInstance(&SWbemLocator, None, CLSCTX_INPROC_SERVER)?;
    let empty = BSTR::new();
    let services = locator.ConnectServer(
        &empty,
        &BSTR::from(namespace),
        &empty,
        &empty,
        &empty,
        &empty,
        0,
        None,
    )?;
    let set = services.ExecQuery(&BSTR::from(wql), &BSTR::from("WQL"), 0, None)?;
    let count = set.Count()?.clamp(0, MAX_ROWS);
    let mut found = Vec::new();
    for index in 0..count {
        let Ok(properties) = set.ItemIndex(index).and_then(|object| object.Properties_()) else {
            continue;
        };
        found.push(
            names
                .iter()
                .map(|name| {
                    properties
                        .Item(&BSTR::from(*name), 0)
                        .and_then(|property| property.Value())
                        .map(|value| field(&value))
                        .unwrap_or(Field::Empty)
                })
                .collect(),
        );
    }
    Ok(found)
}

fn field(value: &VARIANT) -> Field {
    if let Ok(text) = BSTR::try_from(value) {
        return Field::Text(text.to_string());
    }
    f64::try_from(value)
        .ok()
        .or_else(|| i64::try_from(value).ok().map(|number| number as f64))
        .or_else(|| u64::try_from(value).ok().map(|number| number as f64))
        .or_else(|| i32::try_from(value).ok().map(f64::from))
        .or_else(|| u32::try_from(value).ok().map(f64::from))
        .map_or(Field::Empty, Field::Number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_reads_as_text_or_as_a_number() {
        assert_eq!(Field::Text("abc".into()).text(), Some("abc"));
        assert_eq!(Field::Text("abc".into()).number(), None);
        assert_eq!(Field::Text(" 42 ".into()).number(), Some(42.0));
        assert_eq!(Field::Number(1.5).number(), Some(1.5));
        assert_eq!(Field::Number(1.5).text(), None);
        assert_eq!(Field::Empty.number(), None);
        assert_eq!(Field::Empty.text(), None);
    }

    /// Runs a real query on the computer the test runs on: every Windows has
    /// exactly one operating system instance.
    #[test]
    fn a_real_query_returns_its_rows() {
        let rows = query(
            "ROOT\\CIMV2",
            "SELECT Caption, NumberOfProcesses FROM Win32_OperatingSystem",
            &["Caption", "NumberOfProcesses", "NoSuchProperty"],
        );
        assert_eq!(rows.len(), 1);
        assert!(rows[0][0].text().is_some_and(|caption| !caption.is_empty()));
        assert!(rows[0][1].number().is_some_and(|count| count > 0.0));
        assert_eq!(rows[0][2], Field::Empty);
    }

    #[test]
    fn a_namespace_or_class_that_does_not_exist_gives_no_rows() {
        assert!(query("ROOT\\NoSuchNamespace", "SELECT Name FROM Thing", &["Name"]).is_empty());
        assert!(query("ROOT\\CIMV2", "SELECT Name FROM NoSuchClass", &["Name"]).is_empty());
    }
}
