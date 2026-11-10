//! Sensor collection. A background thread takes one sample per second and hands
//! the latest to the UI, so a slow driver call can never stall the window.

pub mod adl;
pub mod hardware;
pub mod io;
pub mod nvml;
pub mod pdh;
pub mod processes;
pub mod ryzen;
pub mod system;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::apps::Capacity;
use crate::helper::CoreLink;
use crate::metrics::{Absent, CoreStatus, MetricId, Snapshot};
use system::CpuVendor;

/// One reading from whichever GPU library this PC has.
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuReading {
    pub temp_edge: Option<f32>,
    pub temp_hotspot: Option<f32>,
    pub temp_memory: Option<f32>,
    pub load: Option<f32>,
    pub power_w: Option<f32>,
    pub fan_rpm: Option<f32>,
    pub fan_percent: Option<f32>,
    pub vram_used_mb: Option<f32>,
    pub clock_mhz: Option<f32>,
    /// NVIDIA's bit mask of why the clocks are held down.
    pub limit_reasons: Option<u64>,
}

/// Where GPU readings come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuSource {
    Amd,
    Nvidia,
    None,
}

/// What this PC cannot report at all, judged from its hardware at startup.
pub fn absent(cpu: CpuVendor, gpu: GpuSource) -> Absent {
    use MetricId::*;
    let mut absent = Absent::default();
    if cpu == CpuVendor::Intel {
        // Intel CPUs only report temperature and power through a kernel
        // driver, and Vitals installs nothing.
        absent = absent.with(CpuTemp).with(IoDieTemp).with(CpuPower);
    }
    match gpu {
        GpuSource::Nvidia => absent.with(GpuHotspot).with(VramTemp).with(GpuFan),
        GpuSource::Amd | GpuSource::None => absent.with(GpuClock).with(GpuFanPercent),
    }
}

const INTERVAL: Duration = Duration::from_secs(1);
const MB_PER_GB: f32 = 1024.0;
/// Package power from the integrated GPU is very noisy from second to second.
const POWER_SMOOTHING: f32 = 0.25;

/// The most recent snapshot, waiting for the UI to collect it.
pub type Latest = Arc<Mutex<Option<Snapshot>>>;

pub struct Sampler {
    /// Sending wakes the thread early, and dropping this stops it.
    wake: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Sampler {
    /// Starts sampling. `notify` runs after each new snapshot is stored.
    /// `wants_table` asks for every app, which costs more, so it is only set
    /// while the Apps page is showing.
    pub fn start(
        latest: Latest,
        core: CoreLink,
        wants_table: Arc<AtomicBool>,
        notify: impl Fn() + Send + 'static,
    ) -> Self {
        let (wake, woken) = channel();
        let thread = thread::spawn(move || {
            let mut sources = Sources::open();
            loop {
                let started = Instant::now();
                let snapshot = sources.sample(&core, wants_table.load(Ordering::Relaxed));
                if let Ok(mut slot) = latest.lock() {
                    *slot = Some(snapshot);
                }
                notify();
                let wait = INTERVAL.saturating_sub(started.elapsed());
                if let Err(RecvTimeoutError::Disconnected) = woken.recv_timeout(wait) {
                    break;
                }
            }
        });
        Self {
            wake: Some(wake),
            thread: Some(thread),
        }
    }

    /// Takes the next sample now, so the Apps page fills as soon as it opens.
    pub fn wake(&self) {
        if let Some(wake) = &self.wake {
            let _ = wake.send(());
        }
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.wake.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Sources {
    counters: Option<pdh::Counters>,
    /// Kernel totals, only kept when disk or network counters are missing.
    io: Option<io::IoTotals>,
    adl: Option<adl::Adl>,
    nvml: Option<nvml::Nvml>,
    absent: Absent,
    base_mhz: Option<f32>,
    vram_total_gb: Option<f32>,
    smoothed_power: Option<f32>,
    processes: processes::ProcessSource,
}

impl Sources {
    fn open() -> Self {
        let adl = adl::Adl::load();
        // A discrete AMD GPU is read through ADL. Otherwise NVIDIA's library is
        // tried, which also covers a Ryzen with an NVIDIA card, where ADL only
        // finds the integrated GPU.
        let nvml = if adl.as_ref().is_some_and(adl::Adl::has_discrete) {
            None
        } else {
            nvml::Nvml::load()
        };
        let gpu = if nvml.is_some() {
            GpuSource::Nvidia
        } else if adl.as_ref().is_some_and(adl::Adl::has_discrete) {
            GpuSource::Amd
        } else {
            GpuSource::None
        };
        let counters = pdh::Counters::open();
        let io = (!counters.as_ref().is_some_and(pdh::Counters::has_io)).then(io::IoTotals::open);
        Self {
            counters,
            io,
            adl,
            nvml,
            absent: absent(system::cpu_vendor(), gpu),
            base_mhz: system::base_clock_mhz(),
            vram_total_gb: system::video_memory_gb(),
            smoothed_power: None,
            processes: processes::ProcessSource::new(),
        }
    }

    fn gpu(&self) -> GpuReading {
        match (&self.nvml, &self.adl) {
            (Some(nvml), _) => nvml.gpu(),
            (None, Some(adl)) => adl.gpu(),
            (None, None) => GpuReading::default(),
        }
    }

    fn sample(&mut self, core: &CoreLink, wants_table: bool) -> Snapshot {
        let mut s = Snapshot {
            vram_total_gb: self.vram_total_gb,
            absent: self.absent,
            ..Default::default()
        };

        let activity = self
            .counters
            .as_ref()
            .map(|c| c.sample())
            .unwrap_or_default();
        s.set(MetricId::CpuLoad, activity.cpu_load);
        let clock = activity.cpu_performance.zip(self.base_mhz);
        s.set(
            MetricId::CpuClock,
            clock.map(|(percent, mhz)| percent / 100.0 * mhz / 1000.0),
        );
        let kernel = self
            .io
            .as_mut()
            .map(io::IoTotals::sample)
            .unwrap_or_default();
        s.set(
            MetricId::DiskRead,
            activity.disk_read_mb.or(kernel.disk_read_mb),
        );
        s.set(
            MetricId::DiskWrite,
            activity.disk_write_mb.or(kernel.disk_write_mb),
        );
        s.set(
            MetricId::Download,
            activity.download_mb.or(kernel.download_mb),
        );
        s.set(MetricId::Upload, activity.upload_mb.or(kernel.upload_mb));
        s.threads = activity.threads;

        if let Some((used, total)) = system::memory_gb() {
            s.set(MetricId::Memory, Some(used));
            s.ram_total_gb = Some(total);
        }

        let gpu = self.gpu();
        s.set(MetricId::GpuTemp, gpu.temp_edge);
        s.set(MetricId::GpuHotspot, gpu.temp_hotspot);
        s.set(MetricId::VramTemp, gpu.temp_memory);
        s.set(MetricId::GpuLoad, gpu.load);
        s.set(MetricId::GpuPower, gpu.power_w);
        s.set(MetricId::GpuFan, gpu.fan_rpm);
        s.set(MetricId::GpuFanPercent, gpu.fan_percent);
        s.set(MetricId::GpuClock, gpu.clock_mhz.map(|mhz| mhz / 1000.0));
        s.gpu_limit_reasons = gpu.limit_reasons;
        s.set(
            MetricId::VideoMemory,
            gpu.vram_used_mb.map(|mb| mb / MB_PER_GB),
        );

        let package = self.adl.as_ref().map(|a| a.package()).unwrap_or_default();
        s.set(MetricId::IoDieTemp, package.io_die_temp);
        self.smoothed_power = match (package.package_power_w, self.smoothed_power) {
            (Some(now), Some(previous)) => Some(previous + (now - previous) * POWER_SMOOTHING),
            (now, _) => now,
        };

        let (status, reading) = core.latest();
        s.core = status;
        match reading.filter(|_| status == CoreStatus::Running) {
            Some(r) => {
                s.set(MetricId::CpuTemp, Some(r.temp_c));
                s.set(MetricId::CpuPower, Some(r.package_w));
                s.cpu_power_limit = Some(r.limit_w);
            }
            None => s.set(MetricId::CpuPower, self.smoothed_power),
        }

        let capacity = Capacity {
            memory_gb: s.ram_total_gb.unwrap_or(0.0),
            video_memory_gb: self.vram_total_gb.unwrap_or(0.0),
        };
        let vram_percent = gpu
            .vram_used_mb
            .zip(self.vram_total_gb)
            .map_or(0.0, |(used, total)| used / MB_PER_GB / total * 100.0);
        let gpu_busy = crate::apps::gpu_could_be_busy(gpu.load.unwrap_or(0.0), vram_percent);
        (s.apps, s.table) = self.processes.apps(gpu_busy, capacity, wants_table);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MetricId::*;

    #[test]
    fn an_amd_pc_is_missing_nothing_it_shows_by_default() {
        let a = absent(CpuVendor::Amd, GpuSource::Amd);
        for id in [CpuTemp, IoDieTemp, CpuPower, GpuHotspot, VramTemp, GpuFan] {
            assert!(!a.contains(id), "{id:?}");
        }
    }

    #[test]
    fn intel_has_no_cpu_temperature_or_power_without_a_driver() {
        let a = absent(CpuVendor::Intel, GpuSource::Nvidia);
        assert!(a.contains(CpuTemp) && a.contains(IoDieTemp) && a.contains(CpuPower));
        assert!(!a.contains(CpuLoad) && !a.contains(CpuClock));
    }

    #[test]
    fn nvidia_reports_fan_share_and_clock_but_not_hotspot_or_vram_temperature() {
        let a = absent(CpuVendor::Amd, GpuSource::Nvidia);
        assert!(a.contains(GpuHotspot) && a.contains(VramTemp) && a.contains(GpuFan));
        assert!(!a.contains(GpuFanPercent) && !a.contains(GpuClock));
        assert!(!a.contains(GpuTemp) && !a.contains(GpuPower));
    }

    #[test]
    fn without_a_gpu_library_the_gpu_tiles_stay_as_they_were() {
        let a = absent(CpuVendor::Other, GpuSource::None);
        assert!(!a.contains(GpuTemp) && !a.contains(GpuHotspot));
        assert!(a.contains(GpuClock) && a.contains(GpuFanPercent));
    }
}
