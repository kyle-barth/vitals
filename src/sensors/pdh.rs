//! Windows performance counters for CPU, disk and network activity. These are
//! the same counters Task Manager reads, so the numbers agree with it.

use std::mem::{size_of, zeroed};

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Performance::{
    PDH_FMT, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER,
    PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
    PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue, PdhOpenQueryW,
};
use windows::core::{PCWSTR, w};

const BYTES_PER_MB: f32 = 1_000_000.0;
/// Do not cap at 100. Rates and turbo clocks legitimately exceed it.
const PDH_FMT_NOCAP100: u32 = 0x0000_8000;
const FORMAT: PDH_FMT = PDH_FMT(PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100);

#[derive(Clone, Debug, Default)]
pub struct Activity {
    pub cpu_load: Option<f32>,
    /// Effective clock as a percentage of the base clock.
    pub cpu_performance: Option<f32>,
    pub threads: Vec<f32>,
    pub disk_read_mb: Option<f32>,
    pub disk_write_mb: Option<f32>,
    pub download_mb: Option<f32>,
    pub upload_mb: Option<f32>,
}

pub struct Counters {
    query: PDH_HQUERY,
    cpu_load: PDH_HCOUNTER,
    cpu_performance: PDH_HCOUNTER,
    threads: PDH_HCOUNTER,
    disk_read: PDH_HCOUNTER,
    disk_write: PDH_HCOUNTER,
    download: PDH_HCOUNTER,
    upload: PDH_HCOUNTER,
}

// SAFETY: the query handle is owned by the sampler thread and never shared.
unsafe impl Send for Counters {}

impl Counters {
    pub fn open() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: `query` is valid for the call. English counter names are
        // used so this works on every Windows display language.
        unsafe {
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != ERROR_SUCCESS.0 {
                return None;
            }
            let add = |path: PCWSTR| {
                let mut counter = PDH_HCOUNTER::default();
                let status = PdhAddEnglishCounterW(query, path, 0, &mut counter);
                (status == ERROR_SUCCESS.0).then_some(counter)
            };
            let counters = (|| {
                Some(Self {
                    query,
                    cpu_load: add(w!(r"\Processor Information(_Total)\% Processor Utility"))?,
                    cpu_performance: add(w!(
                        r"\Processor Information(_Total)\% Processor Performance"
                    ))?,
                    threads: add(w!(r"\Processor Information(*)\% Processor Utility"))?,
                    disk_read: add(w!(r"\PhysicalDisk(_Total)\Disk Read Bytes/sec"))?,
                    disk_write: add(w!(r"\PhysicalDisk(_Total)\Disk Write Bytes/sec"))?,
                    download: add(w!(r"\Network Interface(*)\Bytes Received/sec"))?,
                    upload: add(w!(r"\Network Interface(*)\Bytes Sent/sec"))?,
                })
            })();
            if counters.is_none() {
                PdhCloseQuery(query);
                return None;
            }
            // Rate counters need two samples, so take the first one now.
            PdhCollectQueryData(query);
            counters
        }
    }

    pub fn sample(&self) -> Activity {
        // SAFETY: the query is open for the life of `self`.
        if unsafe { PdhCollectQueryData(self.query) } != ERROR_SUCCESS.0 {
            return Activity::default();
        }
        let megabytes = |v: f32| v / BYTES_PER_MB;
        Activity {
            cpu_load: value(self.cpu_load).map(|v| v.clamp(0.0, 100.0)),
            cpu_performance: value(self.cpu_performance),
            threads: thread_loads(self.threads),
            disk_read_mb: value(self.disk_read).map(megabytes),
            disk_write_mb: value(self.disk_write).map(megabytes),
            download_mb: total(self.download).map(megabytes),
            upload_mb: total(self.upload).map(megabytes),
        }
    }
}

impl Drop for Counters {
    fn drop(&mut self) {
        // SAFETY: the query was opened in `open` and is not used after this.
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

fn value(counter: PDH_HCOUNTER) -> Option<f32> {
    // SAFETY: an all-zero counter value is valid, and PDH fills it on success.
    // The union's double member is the one requested by PDH_FMT_DOUBLE.
    unsafe {
        let mut out: PDH_FMT_COUNTERVALUE = zeroed();
        let status = PdhGetFormattedCounterValue(counter, FORMAT, None, &mut out);
        (status == ERROR_SUCCESS.0).then_some(out.Anonymous.doubleValue as f32)
    }
}

/// Reads a wildcard counter as (instance name, value) pairs.
fn instances(counter: PDH_HCOUNTER) -> Vec<(String, f32)> {
    let (mut bytes, mut count) = (0u32, 0u32);
    // SAFETY: the first call only reports the required size. The buffer is
    // then aligned for the items, and PDH stores the instance-name strings in
    // the same buffer, so the pointers stay valid while `buffer` lives.
    unsafe {
        let status = PdhGetFormattedCounterArrayW(counter, FORMAT, &mut bytes, &mut count, None);
        if status != PDH_MORE_DATA || bytes == 0 {
            return Vec::new();
        }
        let item_size = size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let mut buffer =
            vec![zeroed::<PDH_FMT_COUNTERVALUE_ITEM_W>(); (bytes as usize).div_ceil(item_size)];
        let status = PdhGetFormattedCounterArrayW(
            counter,
            FORMAT,
            &mut bytes,
            &mut count,
            Some(buffer.as_mut_ptr()),
        );
        if status != ERROR_SUCCESS.0 {
            return Vec::new();
        }
        buffer[..count as usize]
            .iter()
            .filter_map(|item| {
                let name = item.szName.to_string().ok()?;
                Some((name, item.FmtValue.Anonymous.doubleValue as f32))
            })
            .collect()
    }
}

fn total(counter: PDH_HCOUNTER) -> Option<f32> {
    let items = instances(counter);
    (!items.is_empty()).then(|| items.iter().map(|(_, v)| v).sum())
}

fn thread_loads(counter: PDH_HCOUNTER) -> Vec<f32> {
    let mut threads: Vec<_> = instances(counter)
        .into_iter()
        .filter_map(|(name, value)| Some((parse_thread(&name)?, value.clamp(0.0, 100.0))))
        .collect();
    threads.sort_by_key(|(id, _)| *id);
    threads.into_iter().map(|(_, value)| value).collect()
}

/// Instance names look like "0,5" (group, thread). Totals such as "_Total" and
/// "0,_Total" are excluded.
fn parse_thread(name: &str) -> Option<(u32, u32)> {
    let (group, thread) = name.split_once(',')?;
    Some((group.parse().ok()?, thread.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::parse_thread;

    #[test]
    fn parses_thread_instances_and_rejects_totals() {
        assert_eq!(parse_thread("0,5"), Some((0, 5)));
        assert_eq!(parse_thread("1,12"), Some((1, 12)));
        assert_eq!(parse_thread("0,_Total"), None);
        assert_eq!(parse_thread("_Total"), None);
    }
}
