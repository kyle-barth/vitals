//! Windows performance counters for CPU, disk and network activity. These are
//! the same counters Task Manager reads, so the numbers agree with it.

use std::collections::HashMap;
use std::mem::{size_of, zeroed};

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Performance::{
    PDH_FMT, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER,
    PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
    PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue, PdhOpenQueryW,
};
use windows::core::{PCWSTR, w};

const BYTES_PER_MB: f32 = 1_000_000.0;
const BYTES_PER_GB: f32 = 1024.0 * 1024.0 * 1024.0;
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

/// Each counter is optional. A damaged counter registry can lose whole counter
/// sets, such as disk or network, and that must not cost the CPU readings too.
pub struct Counters {
    query: PDH_HQUERY,
    cpu_load: Option<PDH_HCOUNTER>,
    cpu_performance: Option<PDH_HCOUNTER>,
    threads: Option<PDH_HCOUNTER>,
    disk_read: Option<PDH_HCOUNTER>,
    disk_write: Option<PDH_HCOUNTER>,
    download: Option<PDH_HCOUNTER>,
    upload: Option<PDH_HCOUNTER>,
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
            let counters = Self {
                query,
                cpu_load: add(w!(r"\Processor Information(_Total)\% Processor Utility")),
                cpu_performance: add(w!(
                    r"\Processor Information(_Total)\% Processor Performance"
                )),
                threads: add(w!(r"\Processor Information(*)\% Processor Utility")),
                disk_read: add(w!(r"\PhysicalDisk(_Total)\Disk Read Bytes/sec")),
                disk_write: add(w!(r"\PhysicalDisk(_Total)\Disk Write Bytes/sec")),
                download: add(w!(r"\Network Interface(*)\Bytes Received/sec")),
                upload: add(w!(r"\Network Interface(*)\Bytes Sent/sec")),
            };
            // Rate counters need two samples, so take the first one now.
            PdhCollectQueryData(query);
            Some(counters)
        }
    }

    /// Whether disk and network both have counters, so no other source is needed.
    pub fn has_io(&self) -> bool {
        [self.disk_read, self.disk_write, self.download, self.upload]
            .iter()
            .all(Option::is_some)
    }

    pub fn sample(&self) -> Activity {
        // SAFETY: the query is open for the life of `self`.
        if unsafe { PdhCollectQueryData(self.query) } != ERROR_SUCCESS.0 {
            return Activity::default();
        }
        let megabytes = |v: f32| v / BYTES_PER_MB;
        Activity {
            cpu_load: self.cpu_load.and_then(value).map(|v| v.clamp(0.0, 100.0)),
            cpu_performance: self.cpu_performance.and_then(value),
            threads: self.threads.map(thread_loads).unwrap_or_default(),
            disk_read_mb: self.disk_read.and_then(value).map(megabytes),
            disk_write_mb: self.disk_write.and_then(value).map(megabytes),
            download_mb: self.download.and_then(total).map(megabytes),
            upload_mb: self.upload.and_then(total).map(megabytes),
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

/// One process's share of the GPU.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuUse {
    /// How busy its busiest GPU engine is, which is what Task Manager shows.
    pub engine_percent: f32,
    pub dedicated_gb: f32,
}

/// Per-process GPU counters. These have an instance for every engine of every
/// process using the GPU, so they get their own query, read only while the GPU
/// is busy.
pub struct GpuApps {
    query: PDH_HQUERY,
    engines: PDH_HCOUNTER,
    memory: PDH_HCOUNTER,
}

impl GpuApps {
    pub fn open() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: `query` is valid for the call, and is closed on failure or drop.
        unsafe {
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != ERROR_SUCCESS.0 {
                return None;
            }
            let add = |path: PCWSTR| {
                let mut counter = PDH_HCOUNTER::default();
                let status = PdhAddEnglishCounterW(query, path, 0, &mut counter);
                (status == ERROR_SUCCESS.0).then_some(counter)
            };
            let engines = add(w!(r"\GPU Engine(*)\Utilization Percentage"));
            let memory = add(w!(r"\GPU Process Memory(*)\Dedicated Usage"));
            let (Some(engines), Some(memory)) = (engines, memory) else {
                PdhCloseQuery(query);
                return None;
            };
            // Rate counters need two samples, so take the first one now.
            PdhCollectQueryData(query);
            Some(Self {
                query,
                engines,
                memory,
            })
        }
    }

    pub fn sample(&self) -> HashMap<u32, GpuUse> {
        let mut apps: HashMap<u32, GpuUse> = HashMap::new();
        // SAFETY: the query is open for the life of `self`.
        if unsafe { PdhCollectQueryData(self.query) } != ERROR_SUCCESS.0 {
            return apps;
        }
        for (name, value) in instances(self.engines) {
            if let Some(pid) = parse_process(&name) {
                let app = apps.entry(pid).or_default();
                app.engine_percent = app.engine_percent.max(value);
            }
        }
        for (name, value) in instances(self.memory) {
            if let Some(pid) = parse_process(&name) {
                apps.entry(pid).or_default().dedicated_gb += value / BYTES_PER_GB;
            }
        }
        apps
    }
}

impl Drop for GpuApps {
    fn drop(&mut self) {
        // SAFETY: the query was opened in `open` and is not used after this.
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

/// Instance names look like "pid_1234_luid_0x0_0xA1B2_phys_0_eng_0_engtype_3D".
fn parse_process(name: &str) -> Option<u32> {
    name.strip_prefix("pid_")?.split('_').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{parse_process, parse_thread};

    #[test]
    fn parses_thread_instances_and_rejects_totals() {
        assert_eq!(parse_thread("0,5"), Some((0, 5)));
        assert_eq!(parse_thread("1,12"), Some((1, 12)));
        assert_eq!(parse_thread("0,_Total"), None);
        assert_eq!(parse_thread("_Total"), None);
    }

    #[test]
    fn parses_the_process_from_gpu_instances() {
        let engine = "pid_1234_luid_0x00000000_0x0000A1B2_phys_0_eng_0_engtype_3D";
        assert_eq!(parse_process(engine), Some(1234));
        assert_eq!(parse_process("pid_77_luid_0x0_0x1_phys_0"), Some(77));
        assert_eq!(parse_process("_Total"), None);
        assert_eq!(parse_process("pid_x_luid"), None);
    }
}
