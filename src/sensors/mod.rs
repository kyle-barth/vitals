//! Sensor collection. A background thread takes one sample per second and hands
//! the latest to the UI, so a slow driver call can never stall the window.

pub mod adl;
pub mod pdh;
pub mod ryzen;
pub mod system;

use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::helper::CoreLink;
use crate::metrics::{CoreStatus, MetricId, Snapshot};

const INTERVAL: Duration = Duration::from_secs(1);
const MB_PER_GB: f32 = 1024.0;
/// Package power from the integrated GPU is very noisy from second to second.
const POWER_SMOOTHING: f32 = 0.25;

/// The most recent snapshot, waiting for the UI to collect it.
pub type Latest = Arc<Mutex<Option<Snapshot>>>;

pub struct Sampler {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Sampler {
    /// Starts sampling. `notify` runs after each new snapshot is stored.
    pub fn start(latest: Latest, core: CoreLink, notify: impl Fn() + Send + 'static) -> Self {
        let (stop, stopped) = channel();
        let thread = thread::spawn(move || {
            let mut sources = Sources::open();
            loop {
                let started = Instant::now();
                let snapshot = sources.sample(&core);
                if let Ok(mut slot) = latest.lock() {
                    *slot = Some(snapshot);
                }
                notify();
                let wait = INTERVAL.saturating_sub(started.elapsed());
                match stopped.recv_timeout(wait) {
                    Err(RecvTimeoutError::Timeout) => {}
                    _ => break,
                }
            }
        });
        Self {
            stop: Some(stop),
            thread: Some(thread),
        }
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Sources {
    counters: Option<pdh::Counters>,
    adl: Option<adl::Adl>,
    base_mhz: Option<f32>,
    vram_total_gb: Option<f32>,
    smoothed_power: Option<f32>,
}

impl Sources {
    fn open() -> Self {
        Self {
            counters: pdh::Counters::open(),
            adl: adl::Adl::load(),
            base_mhz: system::base_clock_mhz(),
            vram_total_gb: system::video_memory_gb(),
            smoothed_power: None,
        }
    }

    fn sample(&mut self, core: &CoreLink) -> Snapshot {
        let mut s = Snapshot {
            vram_total_gb: self.vram_total_gb,
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
        s.set(MetricId::DiskRead, activity.disk_read_mb);
        s.set(MetricId::DiskWrite, activity.disk_write_mb);
        s.set(MetricId::Download, activity.download_mb);
        s.set(MetricId::Upload, activity.upload_mb);
        s.threads = activity.threads;

        if let Some((used, total)) = system::memory_gb() {
            s.set(MetricId::Memory, Some(used));
            s.ram_total_gb = Some(total);
        }

        let gpu = self.adl.as_ref().map(|a| a.gpu()).unwrap_or_default();
        s.set(MetricId::GpuTemp, gpu.temp_edge);
        s.set(MetricId::GpuHotspot, gpu.temp_hotspot);
        s.set(MetricId::VramTemp, gpu.temp_memory);
        s.set(MetricId::GpuLoad, gpu.load);
        s.set(MetricId::GpuPower, gpu.power_w);
        s.set(MetricId::GpuFan, gpu.fan_rpm);
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
            }
            None => s.set(MetricId::CpuPower, self.smoothed_power),
        }
        s
    }
}
