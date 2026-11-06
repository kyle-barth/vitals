//! True CPU core temperature, read through the AMD Ryzen Master library that
//! ships with AMD Software.
//!
//! This only works elevated, because AMD's driver refuses unelevated callers,
//! so it runs in the helper process and never in the UI. `GetRmCpuParameters`
//! is not a documented export. The output layout below was worked out on a
//! 9800X3D by watching each field during a CPU load, and it matches the field
//! order of AMD's published `CPUParameters`. The temperature field was picked
//! because it responds within two seconds of load starting, and the fields
//! around it were confirmed by cross-checking them against each other.
//!
//! A later AMD Software update could change the layout, so [`parse`] rejects
//! anything out of range. A bad layout then means no reading, not a wrong one.

use std::mem::transmute;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::UI::Shell::{FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::{HSTRING, s};

const OFFSET_PACKAGE_LIMIT: usize = 0x10;
const OFFSET_PACKAGE_POWER: usize = 0x14;
const OFFSET_PEAK_VOLTAGE: usize = 0x80;
const OFFSET_TEMPERATURE: usize = 0x90;
const REPORT_LEN: usize = 0x98;

/// Larger than the report, in case a later version writes more than this one.
const BUFFER_LEN: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoreReading {
    pub temp_c: f32,
    pub package_w: f32,
}

/// Decodes AMD's report, rejecting anything that is not physically plausible.
pub fn parse(report: &[u8]) -> Option<CoreReading> {
    let bytes = report.get(..REPORT_LEN)?;
    let f32_at = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let f64_at = |o: usize| f64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());

    let limit = f32_at(OFFSET_PACKAGE_LIMIT);
    let package_w = f32_at(OFFSET_PACKAGE_POWER);
    let voltage = f64_at(OFFSET_PEAK_VOLTAGE);
    let temp_c = f64_at(OFFSET_TEMPERATURE) as f32;

    let plausible = (15.0..=1000.0).contains(&limit)
        && (0.0..=limit * 1.5).contains(&package_w)
        && (0.2..=2.0).contains(&voltage)
        && (5.0..=125.0).contains(&temp_c);
    plausible.then_some(CoreReading { temp_c, package_w })
}

type ReadParameters = unsafe extern "C" fn(*mut u8) -> i32;

pub struct RyzenMaster {
    read: ReadParameters,
    _module: HMODULE,
}

impl RyzenMaster {
    pub fn load() -> Option<Self> {
        // SAFETY: the path comes from the known-folder API rather than the
        // environment, and Program Files is writable only by administrators,
        // so an unprivileged process cannot plant a DLL for the elevated
        // helper. Dependencies resolve only from that folder and System32.
        unsafe {
            let folder =
                SHGetKnownFolderPath(&FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, None).ok()?;
            let root = folder.to_string();
            CoTaskMemFree(Some(folder.0 as *const _));
            let path = HSTRING::from(format!(r"{}\AMD\CNext\CNext\Platform.dll", root.ok()?));
            let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;
            let module = LoadLibraryExW(&path, None, flags).ok()?;
            let read: ReadParameters = transmute(GetProcAddress(module, s!("GetRmCpuParameters"))?);
            Some(Self {
                read,
                _module: module,
            })
        }
    }

    pub fn read(&self) -> Option<CoreReading> {
        let mut report = vec![0u8; BUFFER_LEN];
        // SAFETY: the buffer is far larger than the report AMD writes, and the
        // function only reads sensors. It fails cleanly when not elevated.
        let status = unsafe { (self.read)(report.as_mut_ptr()) };
        if status == 0 { parse(&report) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A report shaped like the one captured from a 9800X3D under load.
    fn report(limit: f32, power: f32, volts: f64, temp: f64) -> Vec<u8> {
        let mut r = vec![0u8; BUFFER_LEN];
        r[OFFSET_PACKAGE_LIMIT..][..4].copy_from_slice(&limit.to_le_bytes());
        r[OFFSET_PACKAGE_POWER..][..4].copy_from_slice(&power.to_le_bytes());
        r[OFFSET_PEAK_VOLTAGE..][..8].copy_from_slice(&volts.to_le_bytes());
        r[OFFSET_TEMPERATURE..][..8].copy_from_slice(&temp.to_le_bytes());
        r
    }

    #[test]
    fn parses_a_real_shaped_report() {
        let reading = parse(&report(162.0, 100.07, 1.296, 88.381)).unwrap();
        assert!((reading.temp_c - 88.381).abs() < 0.001);
        assert!((reading.package_w - 100.07).abs() < 0.001);
    }

    #[test]
    fn rejects_an_all_zero_report() {
        assert_eq!(parse(&vec![0u8; BUFFER_LEN]), None);
    }

    #[test]
    fn rejects_a_shifted_layout() {
        let mut shifted = vec![0u8; 8];
        shifted.extend(report(162.0, 100.07, 1.296, 88.381));
        assert_eq!(parse(&shifted), None);
    }

    #[test]
    fn rejects_implausible_values() {
        assert_eq!(parse(&report(162.0, 100.0, 1.3, 400.0)), None);
        assert_eq!(parse(&report(162.0, 100.0, 1.3, -1.0)), None);
        assert_eq!(parse(&report(162.0, 100.0, 9.0, 80.0)), None);
        assert_eq!(parse(&report(162.0, 900.0, 1.3, 80.0)), None);
    }

    #[test]
    fn rejects_a_short_report() {
        assert_eq!(parse(&[0u8; 16]), None);
    }
}
