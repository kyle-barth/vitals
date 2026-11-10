//! The set of things Vitals tracks, and the state kept for each one.

use crate::apps::{AppRow, BusyApp};
use crate::history::Series;
use crate::insights::{GapTracker, LimitTracker, cpu_clear, cpu_limit, gpu_clear, gpu_limit};
use crate::severity::{Limits, Severity};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum MetricId {
    CpuTemp,
    IoDieTemp,
    GpuTemp,
    GpuHotspot,
    VramTemp,
    CpuLoad,
    GpuLoad,
    Memory,
    VideoMemory,
    CpuPower,
    GpuPower,
    GpuFan,
    CpuClock,
    DiskRead,
    DiskWrite,
    Download,
    Upload,
    GpuClock,
    /// Fan speed as a share of its maximum, for GPUs that do not report rpm.
    GpuFanPercent,
}

pub const METRIC_COUNT: usize = 19;

pub const ALL: [MetricId; METRIC_COUNT] = [
    MetricId::CpuTemp,
    MetricId::IoDieTemp,
    MetricId::GpuTemp,
    MetricId::GpuHotspot,
    MetricId::VramTemp,
    MetricId::CpuLoad,
    MetricId::GpuLoad,
    MetricId::Memory,
    MetricId::VideoMemory,
    MetricId::CpuPower,
    MetricId::GpuPower,
    MetricId::GpuFan,
    MetricId::CpuClock,
    MetricId::DiskRead,
    MetricId::DiskWrite,
    MetricId::Download,
    MetricId::Upload,
    MetricId::GpuClock,
    MetricId::GpuFanPercent,
];

/// Metrics this PC cannot report at all, as opposed to a reading that is missing
/// for a moment. The layout puts something useful in their place rather than a
/// tile that would only ever show a dash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Absent(u32);

impl Absent {
    pub const fn with(self, id: MetricId) -> Self {
        Self(self.0 | 1 << id as u32)
    }

    pub const fn contains(self, id: MetricId) -> bool {
        self.0 & (1 << id as u32) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    Celsius,
    Percent,
    Gigabytes,
    Watts,
    Rpm,
    Gigahertz,
    MegabytesPerSecond,
}

impl Unit {
    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Celsius => "°",
            Unit::Percent => "%",
            Unit::Gigabytes => "GB",
            Unit::Watts => "W",
            Unit::Rpm => "rpm",
            Unit::Gigahertz => "GHz",
            Unit::MegabytesPerSecond => "MB/s",
        }
    }

    pub fn format(self, value: f32) -> String {
        match self {
            Unit::Gigabytes => format!("{value:.1}"),
            Unit::Gigahertz => format!("{value:.2}"),
            Unit::MegabytesPerSecond if value < 10.0 => format!("{value:.1}"),
            _ => format!("{value:.0}"),
        }
    }
}

/// How a sparkline maps values to height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    /// A fixed range, for values with natural bounds.
    Fixed(f32, f32),
    /// Follows the data, but never spans less than this, so sensor noise is
    /// not magnified into something that looks like a real swing.
    Auto { min_span: f32 },
    /// From zero up to the highest recent value.
    FromZero { min_top: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub label: &'static str,
    pub unit: Unit,
    pub scale: Scale,
    pub limits: Option<Limits>,
}

impl MetricId {
    pub const fn spec(self) -> Spec {
        // Limits follow each part's rated maximum: 95° for the 9800X3D, and
        // 110° for the RX 7900 XTX hotspot, where the card begins to throttle.
        const fn temp(label: &'static str, warm: f32, hot: f32, critical: f32) -> Spec {
            Spec {
                label,
                unit: Unit::Celsius,
                scale: Scale::Auto { min_span: 20.0 },
                limits: Some(Limits {
                    warm,
                    hot,
                    critical,
                }),
            }
        }
        const fn plain(label: &'static str, unit: Unit, scale: Scale) -> Spec {
            Spec {
                label,
                unit,
                scale,
                limits: None,
            }
        }
        const PERCENT: Scale = Scale::Fixed(0.0, 100.0);
        match self {
            MetricId::CpuTemp => temp("CPU temp", 80.0, 90.0, 95.0),
            MetricId::IoDieTemp => temp("CPU I/O die", 80.0, 90.0, 100.0),
            MetricId::GpuTemp => temp("GPU temp", 80.0, 90.0, 100.0),
            MetricId::GpuHotspot => temp("GPU hotspot", 95.0, 105.0, 110.0),
            MetricId::VramTemp => temp("VRAM temp", 90.0, 100.0, 106.0),
            MetricId::CpuLoad => plain("CPU load", Unit::Percent, PERCENT),
            MetricId::GpuLoad => plain("GPU load", Unit::Percent, PERCENT),
            MetricId::Memory => plain("Memory", Unit::Gigabytes, Scale::FromZero { min_top: 1.0 }),
            MetricId::VideoMemory => plain(
                "Video memory",
                Unit::Gigabytes,
                Scale::FromZero { min_top: 1.0 },
            ),
            MetricId::CpuPower => {
                plain("CPU power", Unit::Watts, Scale::FromZero { min_top: 50.0 })
            }
            MetricId::GpuPower => {
                plain("GPU power", Unit::Watts, Scale::FromZero { min_top: 50.0 })
            }
            MetricId::GpuFan => plain("GPU fan", Unit::Rpm, Scale::FromZero { min_top: 500.0 }),
            MetricId::CpuClock => plain(
                "CPU clock",
                Unit::Gigahertz,
                Scale::FromZero { min_top: 1.0 },
            ),
            MetricId::DiskRead => plain(
                "Disk read",
                Unit::MegabytesPerSecond,
                Scale::FromZero { min_top: 10.0 },
            ),
            MetricId::DiskWrite => plain(
                "Disk write",
                Unit::MegabytesPerSecond,
                Scale::FromZero { min_top: 10.0 },
            ),
            MetricId::Download => plain(
                "Download",
                Unit::MegabytesPerSecond,
                Scale::FromZero { min_top: 1.0 },
            ),
            MetricId::Upload => plain(
                "Upload",
                Unit::MegabytesPerSecond,
                Scale::FromZero { min_top: 1.0 },
            ),
            MetricId::GpuClock => plain(
                "GPU clock",
                Unit::Gigahertz,
                Scale::FromZero { min_top: 1.0 },
            ),
            MetricId::GpuFanPercent => plain("GPU fan", Unit::Percent, PERCENT),
        }
    }

    /// The short name used on the taskbar when this metric raises an alert.
    pub fn alert_name(self) -> &'static str {
        match self {
            MetricId::CpuTemp => "CPU",
            MetricId::IoDieTemp => "CPU I/O die",
            MetricId::GpuTemp => "GPU",
            MetricId::GpuHotspot => "Hotspot",
            MetricId::VramTemp => "VRAM",
            _ => self.spec().label,
        }
    }
}

/// One complete set of sensor values, produced by the sampler thread.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub values: [Option<f32>; METRIC_COUNT],
    pub ram_total_gb: Option<f32>,
    pub vram_total_gb: Option<f32>,
    pub threads: Vec<f32>,
    pub core: CoreStatus,
    pub absent: Absent,
    pub apps: Vec<BusyApp>,
    pub table: Vec<AppRow>,
    /// The CPU's package power limit, from the core temperature helper.
    pub cpu_power_limit: Option<f32>,
    /// NVIDIA's bit mask of why the GPU's clocks are held down.
    pub gpu_limit_reasons: Option<u64>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            values: [None; METRIC_COUNT],
            ram_total_gb: None,
            vram_total_gb: None,
            threads: Vec::new(),
            core: CoreStatus::Off,
            absent: Absent::default(),
            apps: Vec::new(),
            table: Vec::new(),
            cpu_power_limit: None,
            gpu_limit_reasons: None,
        }
    }
}

impl Snapshot {
    pub fn set(&mut self, id: MetricId, value: Option<f32>) {
        self.values[id as usize] = value;
    }
}

/// The state of the optional elevated reader for true CPU core temperature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CoreStatus {
    /// Not started. The user can choose to enable it.
    #[default]
    Off,
    /// Waiting for the UAC prompt to be answered.
    Requesting,
    /// Running and delivering readings.
    Running,
    /// The user declined the UAC prompt.
    Declined,
    /// AMD's library is missing or could not read this CPU.
    Unavailable,
}

#[derive(Default)]
pub struct Reading {
    pub series: Series,
    pub value: Option<f32>,
    pub severity: Severity,
    pub peak: Option<f32>,
}

/// All tracked state, owned by the UI thread.
#[derive(Default)]
pub struct Monitor {
    readings: [Reading; METRIC_COUNT],
    pub ram_total_gb: Option<f32>,
    pub vram_total_gb: Option<f32>,
    pub threads: Vec<f32>,
    pub core: CoreStatus,
    /// What this PC cannot report, so the layout can leave it out.
    pub absent: Absent,
    pub apps: Vec<BusyApp>,
    /// Every app, while the Apps view is open.
    pub table: Vec<AppRow>,
    /// How far the GPU's hotspot runs above its edge.
    pub gap: GapTracker,
    pub gpu_limit: LimitTracker,
    pub cpu_limit: LimitTracker,
    pub cpu_power_limit: Option<f32>,
}

impl Monitor {
    pub fn get(&self, id: MetricId) -> &Reading {
        &self.readings[id as usize]
    }

    pub fn update(&mut self, snapshot: Snapshot) {
        for id in ALL {
            let spec = id.spec();
            let value = snapshot.values[id as usize];
            let reading = &mut self.readings[id as usize];
            reading.series.push(value);
            reading.value = value;
            if let Some(v) = value {
                reading.peak = Some(reading.peak.map_or(v, |p| p.max(v)));
                if let Some(limits) = spec.limits {
                    reading.severity = limits.classify(v, reading.severity);
                }
            } else {
                reading.severity = Severity::Normal;
            }
        }
        self.ram_total_gb = snapshot.ram_total_gb.or(self.ram_total_gb);
        self.vram_total_gb = snapshot.vram_total_gb.or(self.vram_total_gb);
        self.threads = snapshot.threads;
        self.core = snapshot.core;
        self.absent = snapshot.absent;
        self.apps = snapshot.apps;
        self.table = snapshot.table;
        self.cpu_power_limit = snapshot.cpu_power_limit;

        let value = |id: MetricId| self.readings[id as usize].value;
        let critical = |id: MetricId| id.spec().limits.map(|l| l.critical);
        self.gap.update(
            value(MetricId::GpuTemp),
            value(MetricId::GpuHotspot),
            value(MetricId::GpuLoad),
        );
        let gpu = gpu_limit(
            value(MetricId::GpuHotspot),
            critical(MetricId::GpuHotspot),
            snapshot.gpu_limit_reasons,
        );
        let cpu = cpu_limit(
            value(MetricId::CpuTemp),
            critical(MetricId::CpuTemp),
            value(MetricId::CpuPower),
            self.cpu_power_limit,
        );
        self.gpu_limit.update(gpu);
        self.cpu_limit.update(cpu);
        if let (None, Some(kind)) = (gpu, self.gpu_limit.shown()) {
            if gpu_clear(
                kind,
                value(MetricId::GpuHotspot),
                critical(MetricId::GpuHotspot),
                value(MetricId::GpuLoad),
            ) {
                self.gpu_limit.release();
            }
        }
        if let (None, Some(kind)) = (cpu, self.cpu_limit.shown()) {
            if cpu_clear(
                kind,
                value(MetricId::CpuTemp),
                critical(MetricId::CpuTemp),
                value(MetricId::CpuPower),
                self.cpu_power_limit,
            ) {
                self.cpu_limit.release();
            }
        }
    }

    pub fn reset_peak(&mut self, id: MetricId) {
        let reading = &mut self.readings[id as usize];
        reading.peak = reading.value;
    }

    /// The metric shown as "the CPU temperature". The I/O die is only a
    /// stand-in, and is always labelled as such: it does not follow core load.
    pub fn cpu_temp_metric(&self) -> MetricId {
        if self.get(MetricId::CpuTemp).value.is_some() {
            MetricId::CpuTemp
        } else {
            MetricId::IoDieTemp
        }
    }

    /// The most severe temperature, if it needs to reach the taskbar:
    /// anything critical, or anything hot that is not currently on screen.
    pub fn alert(
        &self,
        is_visible: impl Fn(MetricId) -> bool,
    ) -> Option<(MetricId, Severity, f32)> {
        const TEMPS: [MetricId; 5] = [
            MetricId::GpuHotspot,
            MetricId::CpuTemp,
            MetricId::GpuTemp,
            MetricId::VramTemp,
            MetricId::IoDieTemp,
        ];
        TEMPS
            .iter()
            .filter_map(|&id| {
                let r = self.get(id);
                let raised = r.severity == Severity::Critical
                    || (r.severity == Severity::Hot && !is_visible(id));
                r.value.filter(|_| raised).map(|v| (id, r.severity, v))
            })
            .max_by_key(|&(_, severity, _)| severity)
    }

    pub fn worst_severity(&self) -> Severity {
        self.readings
            .iter()
            .map(|r| r.severity)
            .max()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(id: MetricId, value: f32) -> Snapshot {
        let mut s = Snapshot::default();
        s.set(id, Some(value));
        s
    }

    #[test]
    fn all_is_indexed_by_discriminant() {
        for (i, id) in ALL.iter().enumerate() {
            assert_eq!(*id as usize, i);
        }
    }

    #[test]
    fn peak_tracks_and_resets() {
        let mut m = Monitor::default();
        m.update(snapshot(MetricId::GpuHotspot, 90.0));
        m.update(snapshot(MetricId::GpuHotspot, 110.0));
        m.update(snapshot(MetricId::GpuHotspot, 95.0));
        assert_eq!(m.get(MetricId::GpuHotspot).peak, Some(110.0));
        m.reset_peak(MetricId::GpuHotspot);
        assert_eq!(m.get(MetricId::GpuHotspot).peak, Some(95.0));
    }

    #[test]
    fn every_metric_keeps_a_peak_not_only_temperatures() {
        let mut m = Monitor::default();
        m.update(snapshot(MetricId::CpuLoad, 54.0));
        m.update(snapshot(MetricId::CpuLoad, 19.0));
        assert_eq!(m.get(MetricId::CpuLoad).peak, Some(54.0));
    }

    #[test]
    fn hotspot_at_rated_limit_is_critical() {
        let mut m = Monitor::default();
        m.update(snapshot(MetricId::GpuHotspot, 110.0));
        assert_eq!(m.get(MetricId::GpuHotspot).severity, Severity::Critical);
    }

    #[test]
    fn io_die_stands_in_only_without_a_core_reading() {
        let mut m = Monitor::default();
        m.update(snapshot(MetricId::IoDieTemp, 70.0));
        assert_eq!(m.cpu_temp_metric(), MetricId::IoDieTemp);
        m.update(snapshot(MetricId::CpuTemp, 88.0));
        assert_eq!(m.cpu_temp_metric(), MetricId::CpuTemp);
    }

    #[test]
    fn hot_alerts_only_when_hidden_but_critical_always_alerts() {
        let mut m = Monitor::default();
        m.update(snapshot(MetricId::GpuHotspot, 106.0));
        assert!(m.alert(|_| true).is_none());
        assert!(m.alert(|_| false).is_some());
        m.update(snapshot(MetricId::GpuHotspot, 111.0));
        assert_eq!(m.alert(|_| true).map(|a| a.0), Some(MetricId::GpuHotspot));
    }

    #[test]
    fn lost_sensor_clears_severity_but_keeps_peak() {
        let mut m = Monitor::default();
        m.update(snapshot(MetricId::GpuTemp, 105.0));
        m.update(Snapshot::default());
        assert_eq!(m.get(MetricId::GpuTemp).severity, Severity::Normal);
        assert_eq!(m.get(MetricId::GpuTemp).peak, Some(105.0));
    }

    #[test]
    fn unit_formatting() {
        assert_eq!(Unit::Gigabytes.format(18.44), "18.4");
        assert_eq!(Unit::Gigahertz.format(5.184), "5.18");
        assert_eq!(Unit::Celsius.format(87.6), "88");
        assert_eq!(Unit::MegabytesPerSecond.format(6.26), "6.3");
        assert_eq!(Unit::MegabytesPerSecond.format(120.4), "120");
    }
}
