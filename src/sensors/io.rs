//! Disk and network totals read straight from Windows, for PCs whose disk or
//! network performance counters are missing.
//!
//! A damaged performance counter registry can lose whole counter sets while the
//! kernel still keeps count, so these totals are turned into rates here instead.
//! Nothing here needs elevation.

use std::mem::size_of;
use std::time::Instant;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{DISK_PERFORMANCE, IOCTL_DISK_PERFORMANCE};
use windows::core::HSTRING;

const BYTES_PER_MB: f64 = 1_000_000.0;
/// Drive numbers checked at startup. Windows numbers them from zero with no
/// gaps in practice, but a removed drive can leave one.
const MAX_DRIVES: u32 = 16;
/// `MIB_IF_ROW2` flag bits.
const HARDWARE_INTERFACE: u8 = 1 << 0;
const FILTER_INTERFACE: u8 = 1 << 1;

/// Running totals in bytes since Windows started.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Totals {
    pub disk_read: Option<u64>,
    pub disk_written: Option<u64>,
    pub received: Option<u64>,
    pub sent: Option<u64>,
}

/// Rates in MB per second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rates {
    pub disk_read_mb: Option<f32>,
    pub disk_write_mb: Option<f32>,
    pub download_mb: Option<f32>,
    pub upload_mb: Option<f32>,
}

pub struct IoTotals {
    disks: Vec<HANDLE>,
    last: Option<(Instant, Totals)>,
}

// SAFETY: the handles are owned by the sampler thread and never shared.
unsafe impl Send for IoTotals {}

impl IoTotals {
    pub fn open() -> Self {
        let disks = (0..MAX_DRIVES)
            .filter_map(|n| {
                let path = HSTRING::from(format!(r"\\.\PhysicalDrive{n}"));
                // SAFETY: the path is a valid string. No access rights are
                // asked for, which is enough for the statistics query and
                // works without elevation.
                unsafe {
                    CreateFileW(
                        &path,
                        0,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        None,
                        OPEN_EXISTING,
                        FILE_FLAGS_AND_ATTRIBUTES(0),
                        None,
                    )
                }
                .ok()
            })
            .collect();
        Self { disks, last: None }
    }

    fn totals(&self) -> Totals {
        let (read, written) = self
            .disks
            .iter()
            .filter_map(|&disk| disk_bytes(disk))
            .fold(None, |sum: Option<(u64, u64)>, (r, w)| {
                let (sr, sw) = sum.unwrap_or_default();
                Some((sr + r, sw + w))
            })
            .unzip();
        let (received, sent) = network_bytes().unzip();
        Totals {
            disk_read: read,
            disk_written: written,
            received,
            sent,
        }
    }

    /// Rates since the last call. The first call only takes a starting point.
    pub fn sample(&mut self) -> Rates {
        let now = (Instant::now(), self.totals());
        let rates = self
            .last
            .map(|(at, before)| rates(before, now.1, now.0.duration_since(at).as_secs_f64()))
            .unwrap_or_default();
        self.last = Some(now);
        rates
    }
}

impl Drop for IoTotals {
    fn drop(&mut self) {
        for disk in self.disks.drain(..) {
            // SAFETY: each handle was opened in `open` and is closed once.
            let _ = unsafe { CloseHandle(disk) };
        }
    }
}

/// Turns two sets of totals into rates. A total that went down means its
/// device was removed or reset, which is reported as no activity.
pub fn rates(before: Totals, now: Totals, seconds: f64) -> Rates {
    let rate = |a: Option<u64>, b: Option<u64>| -> Option<f32> {
        let (a, b) = (a?, b?);
        (seconds > 0.0).then(|| (b.saturating_sub(a) as f64 / seconds / BYTES_PER_MB) as f32)
    };
    Rates {
        disk_read_mb: rate(before.disk_read, now.disk_read),
        disk_write_mb: rate(before.disk_written, now.disk_written),
        download_mb: rate(before.received, now.received),
        upload_mb: rate(before.sent, now.sent),
    }
}

fn disk_bytes(disk: HANDLE) -> Option<(u64, u64)> {
    let mut stats = DISK_PERFORMANCE::default();
    let mut returned = 0u32;
    // SAFETY: the output buffer is a DISK_PERFORMANCE of the size given, and
    // the handle stays open for the life of `IoTotals`.
    unsafe {
        DeviceIoControl(
            disk,
            IOCTL_DISK_PERFORMANCE,
            None,
            0,
            Some((&raw mut stats).cast()),
            size_of::<DISK_PERFORMANCE>() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    Some((
        stats.BytesRead.max(0) as u64,
        stats.BytesWritten.max(0) as u64,
    ))
}

/// Bytes received and sent by physical network adapters that are up. Virtual
/// adapters such as VPN tunnels are left out, because their traffic also
/// crosses a physical adapter and would be counted twice.
fn network_bytes() -> Option<(u64, u64)> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: Windows allocates the table, which is read within its stated
    // length and freed once.
    unsafe {
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return None;
        }
        let count = (*table).NumEntries as usize;
        // The rows run on past the one-element array the type declares.
        let first = (&raw const (*table).Table).cast::<MIB_IF_ROW2>();
        let rows = std::slice::from_raw_parts(first, count);
        let totals = rows
            .iter()
            .filter(|row| {
                let flags = row.InterfaceAndOperStatusFlags._bitfield;
                flags & HARDWARE_INTERFACE != 0
                    && flags & FILTER_INTERFACE == 0
                    && row.OperStatus == IfOperStatusUp
            })
            .fold((0u64, 0u64), |(r, s), row| {
                (r + row.InOctets, s + row.OutOctets)
            });
        FreeMibTable(table.cast());
        Some(totals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn totals(read: u64, written: u64, received: u64, sent: u64) -> Totals {
        Totals {
            disk_read: Some(read),
            disk_written: Some(written),
            received: Some(received),
            sent: Some(sent),
        }
    }

    #[test]
    fn rates_are_megabytes_per_second() {
        let r = rates(
            totals(0, 0, 0, 0),
            totals(4_000_000, 2_000_000, 1_000_000, 0),
            2.0,
        );
        assert_eq!(r.disk_read_mb, Some(2.0));
        assert_eq!(r.disk_write_mb, Some(1.0));
        assert_eq!(r.download_mb, Some(0.5));
        assert_eq!(r.upload_mb, Some(0.0));
    }

    #[test]
    fn a_total_that_goes_down_reads_as_idle_not_negative() {
        let r = rates(totals(9_000_000, 0, 0, 0), totals(1_000_000, 0, 0, 0), 1.0);
        assert_eq!(r.disk_read_mb, Some(0.0));
    }

    #[test]
    fn a_missing_total_gives_no_rate() {
        let before = Totals {
            received: None,
            ..totals(0, 0, 0, 0)
        };
        let r = rates(before, totals(0, 0, 5_000_000, 0), 1.0);
        assert_eq!(r.download_mb, None);
        assert_eq!(r.disk_read_mb, Some(0.0));
    }

    #[test]
    fn no_time_passing_gives_no_rate() {
        assert_eq!(
            rates(totals(0, 0, 0, 0), totals(1, 1, 1, 1), 0.0),
            Rates::default()
        );
    }
}
