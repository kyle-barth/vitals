//! Decides which apps are worth showing. Pure logic with no Win32, so that the
//! rules can be tested.
//!
//! The point of Vitals is to stay simple, so the list is empty on a calm PC. An
//! app appears only once it has held a lot of a resource for a few seconds, and
//! leaves a few seconds after dropping back, so a brief spike never moves the
//! window around. A frozen app always appears, however little it is using.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

/// Limits, each as a percentage of the whole resource.
const CPU_LIMIT: f32 = 25.0;
const GPU_LIMIT: f32 = 25.0;
const MEMORY_LIMIT: f32 = 20.0;
const VIDEO_MEMORY_LIMIT: f32 = 25.0;
const GPU_MARGIN: f32 = 5.0;

/// Below this share of a resource, it isn't worth naming on an app's row.
const MENTION_SHARE: f32 = 5.0;

/// Samples an app must stay over a limit before it appears.
const APPEAR_AFTER: u32 = 5;
/// Samples an app must stay under every limit before it leaves.
const LEAVE_AFTER: u32 = 10;
/// Samples an app must stay frozen before it appears.
const FROZEN_AFTER: u32 = 2;

/// What one app is using right now, across all of its processes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    /// Percent of the whole CPU.
    pub cpu: f32,
    /// Percent of the busiest GPU engine.
    pub gpu: f32,
    pub memory_gb: f32,
    pub video_memory_gb: f32,
    /// Everything the app reads and writes, in MB per second.
    pub disk_mb: f32,
}

/// How much of each resource the PC has.
#[derive(Clone, Copy, Debug, Default)]
pub struct Capacity {
    pub memory_gb: f32,
    pub video_memory_gb: f32,
}

/// Whether any app could be over a GPU limit, given the whole PC's use. Per-app
/// GPU data is costly, and no app can use more than the PC as a whole, so it is
/// only worth reading when this is true. Load gets a small margin so that an app
/// climbing towards the limit is already being measured when it arrives.
pub fn gpu_could_be_busy(load_percent: f32, video_memory_percent: f32) -> bool {
    load_percent >= GPU_LIMIT - GPU_MARGIN || video_memory_percent >= VIDEO_MEMORY_LIMIT
}

fn share(used: f32, total: f32) -> f32 {
    if total > 0.0 {
        used / total * 100.0
    } else {
        0.0
    }
}

impl Usage {
    fn memory_share(&self, capacity: Capacity) -> f32 {
        share(self.memory_gb, capacity.memory_gb)
    }

    fn video_memory_share(&self, capacity: Capacity) -> f32 {
        share(self.video_memory_gb, capacity.video_memory_gb)
    }

    fn is_over_a_limit(&self, capacity: Capacity) -> bool {
        self.cpu >= CPU_LIMIT
            || self.gpu >= GPU_LIMIT
            || self.memory_share(capacity) >= MEMORY_LIMIT
            || self.video_memory_share(capacity) >= VIDEO_MEMORY_LIMIT
    }

    /// The resource this app is using the most of. Ties go to the CPU.
    pub fn biggest(&self, capacity: Capacity) -> SortKey {
        let shares = [
            (self.video_memory_share(capacity), SortKey::VideoMemory),
            (self.memory_share(capacity), SortKey::Memory),
            (self.gpu, SortKey::Gpu),
            (self.cpu, SortKey::Cpu),
        ];
        shares
            .into_iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(SortKey::Cpu, |(_, key)| key)
    }

    /// Short descriptions of what the app is using, biggest share first.
    pub fn labels(&self, capacity: Capacity) -> Vec<String> {
        let mut items = [
            (self.gpu, format!("{:.0}% GPU", self.gpu)),
            (
                self.memory_share(capacity),
                format!("{:.1} GB RAM", self.memory_gb),
            ),
            (
                self.video_memory_share(capacity),
                format!("{:.1} GB VRAM", self.video_memory_gb),
            ),
            (self.cpu, format!("{:.0}% CPU", self.cpu)),
        ];
        items.sort_by(|a, b| b.0.total_cmp(&a.0));
        items
            .into_iter()
            .filter(|(amount, _)| *amount >= MENTION_SHARE)
            .map(|(_, label)| label)
            .collect()
    }
}

/// Identifies one process across time. Windows reuses process ids, so the
/// creation time is what makes sure End only ever reaches the process it meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessId {
    pub pid: u32,
    pub created: i64,
}

/// One app, as measured in a single sample.
#[derive(Clone, Debug)]
pub struct AppSample {
    /// The lowercase name of its exe, which groups its processes into one app.
    pub key: String,
    pub usage: Usage,
    pub frozen: bool,
    /// False for Windows' own processes, which must not be offered End.
    pub can_end: bool,
    pub processes: Vec<ProcessId>,
}

/// An app's icon: 32-bit premultiplied BGRA pixels, top row first.
#[derive(Debug)]
pub struct Icon {
    pub size: u32,
    pub pixels: Vec<u8>,
}

/// An app that has earned a row in the window.
#[derive(Clone, Debug)]
pub struct BusyApp {
    pub key: String,
    pub name: String,
    pub icon: Option<Arc<Icon>>,
    pub labels: Vec<String>,
    /// Shown on a frozen app, where it separates working hard from truly stuck.
    pub cpu_percent: f32,
    /// What it is using the most of, which clicking its row sorts the Apps view by.
    pub biggest: SortKey,
    pub frozen: bool,
    pub can_end: bool,
    pub processes: Vec<ProcessId>,
}

/// What the Apps view can be sorted by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortKey {
    Name,
    #[default]
    Cpu,
    Memory,
    Gpu,
    VideoMemory,
    Disk,
}

/// The number columns, most useful first. Narrow windows keep the left ones.
pub const COLUMNS: [SortKey; 5] = [
    SortKey::Cpu,
    SortKey::Memory,
    SortKey::Gpu,
    SortKey::VideoMemory,
    SortKey::Disk,
];
/// How many of the columns Simple mode allows.
pub const SIMPLE_COLUMNS: usize = 2;

impl SortKey {
    pub const fn label(self) -> &'static str {
        match self {
            SortKey::Name => "App",
            SortKey::Cpu => "CPU",
            SortKey::Memory => "Memory",
            SortKey::Gpu => "GPU",
            SortKey::VideoMemory => "VRAM",
            SortKey::Disk => "Disk",
        }
    }

    /// The name used in the settings file.
    pub const fn id(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Cpu => "cpu",
            SortKey::Memory => "memory",
            SortKey::Gpu => "gpu",
            SortKey::VideoMemory => "vram",
            SortKey::Disk => "disk",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [SortKey::Name]
            .into_iter()
            .chain(COLUMNS)
            .find(|key| key.id() == id)
    }
}

/// How the Apps view is ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub descending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self {
            key: SortKey::Cpu,
            descending: true,
        }
    }
}

impl Sort {
    /// The order after clicking a column heading: the biggest first for a new
    /// number column, A to Z for names, and the reverse on a second click.
    ///
    /// A narrow window has no heading for the columns it can't fit, so a second
    /// click on its last column moves on to the next one instead, which keeps
    /// every column reachable. `shown` is what is on screen, and `allowed` is how
    /// many of [`COLUMNS`] the current mode offers.
    pub fn after_click(self, clicked: SortKey, shown: &[SortKey], allowed: usize) -> Self {
        if clicked != self.key {
            return Self {
                key: clicked,
                descending: clicked != SortKey::Name,
            };
        }
        let choices = &COLUMNS[..allowed.min(COLUMNS.len())];
        if shown.last() == Some(&clicked) && shown.len() < choices.len() {
            let kept = &shown[..shown.len() - 1];
            let at = choices.iter().position(|k| *k == clicked).unwrap_or(0);
            let next = (1..choices.len())
                .map(|step| choices[(at + step) % choices.len()])
                .find(|key| !kept.contains(key));
            if let Some(key) = next {
                return Self {
                    key,
                    descending: true,
                };
            }
        }
        Self {
            descending: !self.descending,
            ..self
        }
    }
}

/// One line of the Apps view.
#[derive(Clone, Debug)]
pub struct AppRow {
    pub key: String,
    pub name: String,
    pub icon: Option<Arc<Icon>>,
    pub usage: Usage,
    pub frozen: bool,
    pub can_end: bool,
    pub processes: Vec<ProcessId>,
}

fn percent_text(value: f32) -> String {
    if value < 9.95 {
        format!("{value:.1}%")
    } else {
        format!("{value:.0}%")
    }
}

fn size_text(gb: f32) -> String {
    if gb < 1.0 {
        format!("{:.0} MB", gb * 1024.0)
    } else {
        format!("{gb:.1} GB")
    }
}

fn rate_text(mb: f32) -> String {
    if mb < 9.95 {
        format!("{mb:.1} MB/s")
    } else {
        format!("{mb:.0} MB/s")
    }
}

impl AppRow {
    fn amount(&self, key: SortKey) -> f32 {
        match key {
            SortKey::Name => 0.0,
            SortKey::Cpu => self.usage.cpu,
            SortKey::Memory => self.usage.memory_gb,
            SortKey::Gpu => self.usage.gpu,
            SortKey::VideoMemory => self.usage.video_memory_gb,
            SortKey::Disk => self.usage.disk_mb,
        }
    }

    /// The text for one number column.
    pub fn cell(&self, key: SortKey) -> String {
        let amount = self.amount(key);
        match key {
            SortKey::Name => self.name.clone(),
            SortKey::Cpu | SortKey::Gpu => percent_text(amount),
            SortKey::Memory | SortKey::VideoMemory => size_text(amount),
            SortKey::Disk => rate_text(amount),
        }
    }

    /// Whether a number is too small to show as anything but zero, so it can
    /// fade back and let the numbers that matter stand out.
    pub fn is_idle(&self, key: SortKey) -> bool {
        let amount = self.amount(key);
        match key {
            SortKey::Memory | SortKey::VideoMemory => amount * 1024.0 < 0.5,
            _ => amount < 0.05,
        }
    }
}

fn compare(a: &AppRow, b: &AppRow, sort: Sort) -> Ordering {
    let order = match sort.key {
        SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        key => a.amount(key).total_cmp(&b.amount(key)),
    };
    let order = if sort.descending {
        order.reverse()
    } else {
        order
    };
    order.then_with(|| a.key.cmp(&b.key))
}

/// Puts the rows in display order, with frozen apps first.
///
/// `held` is the order to keep while the pointer is over the table, because a
/// row that moves could put a different app's End button under it. Apps that
/// appear in the meantime wait at the bottom.
pub fn arrange(mut rows: Vec<AppRow>, sort: Sort, held: Option<&[String]>) -> Vec<AppRow> {
    rows.sort_by(|a, b| b.frozen.cmp(&a.frozen).then_with(|| compare(a, b, sort)));
    if let Some(order) = held {
        let last = order.len();
        rows.sort_by_cached_key(|row| order.iter().position(|k| *k == row.key).unwrap_or(last));
    }
    rows
}

#[derive(Default)]
struct Track {
    over: u32,
    under: u32,
    frozen: u32,
    shown: bool,
    /// When this app last appeared. It sets the row order.
    since: u64,
}

/// Remembers each app between samples, to apply the appear and leave delays.
#[derive(Default)]
pub struct Tracker {
    tracks: HashMap<String, Track>,
    clock: u64,
}

impl Tracker {
    /// Takes every app in this sample, and returns the ones to show.
    ///
    /// Frozen apps come first, then apps in the order they appeared. Rows must
    /// not swap places on their own, because that could move an End button
    /// under the pointer just as it is clicked.
    pub fn update(&mut self, samples: Vec<AppSample>, capacity: Capacity) -> Vec<AppSample> {
        self.clock += 1;
        let clock = self.clock;
        let mut shown = Vec::new();
        for mut sample in samples {
            let track = self.tracks.entry(sample.key.clone()).or_default();
            if sample.usage.is_over_a_limit(capacity) {
                track.over += 1;
                track.under = 0;
            } else {
                track.under += 1;
                track.over = 0;
            }
            track.frozen = if sample.frozen { track.frozen + 1 } else { 0 };
            if track.over >= APPEAR_AFTER && !track.shown {
                track.shown = true;
                track.since = clock;
            }
            if track.under >= LEAVE_AFTER {
                track.shown = false;
            }
            sample.frozen = track.frozen >= FROZEN_AFTER;
            if track.shown || sample.frozen {
                shown.push(sample);
            }
        }
        let tracks = &self.tracks;
        let order = |key: &str| tracks.get(key).map_or(0, |t| t.since);
        shown.sort_by_key(|s| (!s.frozen, order(&s.key), s.key.clone()));
        shown
    }

    /// Whether an app has been frozen for long enough to say so.
    pub fn is_frozen(&self, key: &str) -> bool {
        self.tracks
            .get(key)
            .is_some_and(|track| track.frozen >= FROZEN_AFTER)
    }

    /// Forgets apps that are no longer running. `running` holds every app key
    /// from the latest sample.
    pub fn forget_all_but(&mut self, running: &[&str]) {
        self.tracks.retain(|key, _| running.contains(&key.as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PC: Capacity = Capacity {
        memory_gb: 31.0,
        video_memory_gb: 24.0,
    };

    fn app(key: &str, usage: Usage) -> AppSample {
        AppSample {
            key: key.to_string(),
            usage,
            frozen: false,
            can_end: true,
            processes: Vec::new(),
        }
    }

    fn busy() -> Usage {
        Usage {
            cpu: 40.0,
            ..Default::default()
        }
    }

    fn calm() -> Usage {
        Usage {
            cpu: 3.0,
            ..Default::default()
        }
    }

    fn step(t: &mut Tracker, usage: Usage, times: u32) -> usize {
        (0..times)
            .map(|_| t.update(vec![app("a", usage)], PC).len())
            .last()
            .unwrap_or(0)
    }

    #[test]
    fn each_limit_counts_as_busy_on_its_own() {
        let cases = [
            Usage {
                cpu: 25.0,
                ..Default::default()
            },
            Usage {
                gpu: 25.0,
                ..Default::default()
            },
            Usage {
                memory_gb: 6.3,
                ..Default::default()
            },
            Usage {
                video_memory_gb: 6.0,
                ..Default::default()
            },
        ];
        for usage in cases {
            assert!(usage.is_over_a_limit(PC), "{usage:?}");
        }
    }

    #[test]
    fn just_under_every_limit_is_not_busy() {
        let usage = Usage {
            cpu: 24.9,
            gpu: 24.9,
            memory_gb: 6.1,
            video_memory_gb: 5.9,
            ..Default::default()
        };
        assert!(!usage.is_over_a_limit(PC));
    }

    #[test]
    fn a_pc_with_no_gpu_data_never_divides_by_zero() {
        let usage = Usage {
            video_memory_gb: 4.0,
            ..Default::default()
        };
        assert!(!usage.is_over_a_limit(Capacity::default()));
    }

    #[test]
    fn gpu_data_is_only_read_when_an_app_could_be_over_a_limit() {
        // An idle desktop with some video memory in use.
        assert!(!gpu_could_be_busy(4.0, 20.0));
        assert!(gpu_could_be_busy(92.0, 20.0));
        assert!(gpu_could_be_busy(4.0, 25.0));
        assert!(gpu_could_be_busy(GPU_LIMIT - GPU_MARGIN, 0.0));
    }

    #[test]
    fn a_calm_pc_shows_nothing() {
        let mut t = Tracker::default();
        assert_eq!(step(&mut t, calm(), 60), 0);
    }

    #[test]
    fn an_app_appears_only_after_holding_a_limit() {
        let mut t = Tracker::default();
        assert_eq!(step(&mut t, busy(), APPEAR_AFTER - 1), 0);
        assert_eq!(step(&mut t, busy(), 1), 1);
    }

    #[test]
    fn a_brief_spike_never_appears() {
        let mut t = Tracker::default();
        for _ in 0..10 {
            step(&mut t, busy(), APPEAR_AFTER - 1);
            assert_eq!(step(&mut t, calm(), 1), 0);
        }
    }

    #[test]
    fn an_app_stays_through_a_dip_then_leaves() {
        let mut t = Tracker::default();
        step(&mut t, busy(), APPEAR_AFTER);
        assert_eq!(step(&mut t, calm(), LEAVE_AFTER - 1), 1);
        assert_eq!(step(&mut t, busy(), 1), 1);
        assert_eq!(step(&mut t, calm(), LEAVE_AFTER - 1), 1);
        assert_eq!(step(&mut t, calm(), 1), 0);
    }

    #[test]
    fn a_frozen_app_appears_however_little_it_uses() {
        let mut t = Tracker::default();
        let mut sample = app("a", Usage::default());
        sample.frozen = true;
        assert!(t.update(vec![sample.clone()], PC).is_empty());
        let shown = t.update(vec![sample], PC);
        assert_eq!(shown.len(), 1);
        assert!(shown[0].frozen);
    }

    #[test]
    fn a_frozen_blip_is_ignored() {
        let mut t = Tracker::default();
        let mut sample = app("a", Usage::default());
        sample.frozen = true;
        t.update(vec![sample.clone()], PC);
        sample.frozen = false;
        assert!(t.update(vec![sample.clone()], PC).is_empty());
        sample.frozen = true;
        assert!(t.update(vec![sample], PC).is_empty());
    }

    #[test]
    fn frozen_apps_lead_and_the_rest_keep_their_order() {
        let mut t = Tracker::default();
        for _ in 0..APPEAR_AFTER {
            t.update(vec![app("first", busy())], PC);
        }
        for _ in 0..APPEAR_AFTER {
            t.update(vec![app("first", busy()), app("second", busy())], PC);
        }
        let keys = |list: Vec<AppSample>| list.into_iter().map(|a| a.key).collect::<Vec<_>>();
        assert_eq!(
            keys(t.update(vec![app("second", busy()), app("first", busy())], PC)),
            ["first", "second"]
        );

        // Rows never trade places, even if the later app becomes the busiest.
        let heavy = Usage {
            cpu: 90.0,
            ..Default::default()
        };
        assert_eq!(
            keys(t.update(vec![app("second", heavy), app("first", busy())], PC)),
            ["first", "second"]
        );

        let mut frozen = app("frozen", Usage::default());
        frozen.frozen = true;
        t.update(
            vec![app("first", busy()), app("second", busy()), frozen.clone()],
            PC,
        );
        assert_eq!(
            keys(t.update(
                vec![app("first", busy()), app("second", busy()), frozen],
                PC
            )),
            ["frozen", "first", "second"]
        );
    }

    #[test]
    fn labels_lead_with_the_biggest_share_and_skip_tiny_ones() {
        let usage = Usage {
            cpu: 26.0,
            gpu: 88.0,
            memory_gb: 0.4,
            video_memory_gb: 12.1,
            ..Default::default()
        };
        assert_eq!(usage.labels(PC), ["88% GPU", "12.1 GB VRAM", "26% CPU"]);
    }

    fn row(name: &str, cpu: f32, memory_gb: f32) -> AppRow {
        AppRow {
            key: name.to_lowercase(),
            name: name.to_string(),
            icon: None,
            usage: Usage {
                cpu,
                memory_gb,
                ..Default::default()
            },
            frozen: false,
            can_end: true,
            processes: Vec::new(),
        }
    }

    fn names(rows: &[AppRow]) -> Vec<&str> {
        rows.iter().map(|r| r.name.as_str()).collect()
    }

    fn three() -> Vec<AppRow> {
        vec![
            row("beta", 5.0, 3.0),
            row("Alpha", 1.0, 9.0),
            row("gamma", 40.0, 1.0),
        ]
    }

    #[test]
    fn rows_sort_by_the_chosen_column() {
        let by_memory = Sort {
            key: SortKey::Memory,
            descending: true,
        };
        assert_eq!(
            names(&arrange(three(), Sort::default(), None)),
            ["gamma", "beta", "Alpha"]
        );
        assert_eq!(
            names(&arrange(three(), by_memory, None)),
            ["Alpha", "beta", "gamma"]
        );
    }

    #[test]
    fn names_sort_without_regard_to_case() {
        let by_name = Sort {
            key: SortKey::Name,
            descending: false,
        };
        assert_eq!(
            names(&arrange(three(), by_name, None)),
            ["Alpha", "beta", "gamma"]
        );
    }

    #[test]
    fn a_frozen_app_comes_first_in_any_order() {
        let mut rows = three();
        rows[1].frozen = true;
        let ascending = Sort {
            key: SortKey::Cpu,
            descending: false,
        };
        assert_eq!(arrange(rows.clone(), ascending, None)[0].name, "Alpha");
        assert_eq!(arrange(rows, Sort::default(), None)[0].name, "Alpha");
    }

    #[test]
    fn a_held_order_ignores_new_numbers() {
        let held = ["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        assert_eq!(
            names(&arrange(three(), Sort::default(), Some(&held))),
            ["Alpha", "beta", "gamma"]
        );
    }

    #[test]
    fn apps_that_appear_during_a_hold_wait_at_the_bottom() {
        let held = ["gamma".to_string(), "alpha".to_string()];
        assert_eq!(
            names(&arrange(three(), Sort::default(), Some(&held))),
            ["gamma", "Alpha", "beta"]
        );
    }

    #[test]
    fn apps_that_leave_during_a_hold_close_up_the_gap() {
        let held = ["alpha".to_string(), "gone".to_string(), "beta".to_string()];
        let rows = vec![row("beta", 5.0, 3.0), row("Alpha", 1.0, 9.0)];
        assert_eq!(
            names(&arrange(rows, Sort::default(), Some(&held))),
            ["Alpha", "beta"]
        );
    }

    const ALL_SHOWN: [SortKey; 5] = COLUMNS;

    #[test]
    fn a_new_number_column_starts_biggest_first_and_names_from_a() {
        let sort = Sort::default();
        let memory = sort.after_click(SortKey::Memory, &ALL_SHOWN, 5);
        assert_eq!((memory.key, memory.descending), (SortKey::Memory, true));
        let name = sort.after_click(SortKey::Name, &ALL_SHOWN, 5);
        assert_eq!((name.key, name.descending), (SortKey::Name, false));
    }

    #[test]
    fn a_second_click_reverses_the_order() {
        let sort = Sort::default().after_click(SortKey::Cpu, &ALL_SHOWN, 5);
        assert_eq!((sort.key, sort.descending), (SortKey::Cpu, false));
    }

    #[test]
    fn a_narrow_window_can_still_reach_every_column() {
        let mut sort = Sort::default();
        let mut seen = vec![sort.key];
        for _ in 0..4 {
            sort = sort.after_click(sort.key, &[sort.key], 5);
            seen.push(sort.key);
        }
        assert_eq!(seen, COLUMNS);
        assert_eq!(sort.after_click(sort.key, &[sort.key], 5).key, SortKey::Cpu);
    }

    #[test]
    fn stepping_keeps_clear_of_the_columns_still_showing() {
        let sort = Sort {
            key: SortKey::Memory,
            descending: true,
        };
        let shown = [SortKey::Cpu, SortKey::Memory];
        let next = sort.after_click(SortKey::Memory, &shown, 5);
        assert_eq!(next.key, SortKey::Gpu);
        let last = Sort {
            key: SortKey::Disk,
            descending: true,
        };
        let shown = [SortKey::Cpu, SortKey::Disk];
        assert_eq!(
            last.after_click(SortKey::Disk, &shown, 5).key,
            SortKey::Memory
        );
    }

    #[test]
    fn simple_mode_steps_between_its_own_columns_only() {
        let sort = Sort::default();
        let next = sort.after_click(SortKey::Cpu, &[SortKey::Cpu], SIMPLE_COLUMNS);
        assert_eq!(next.key, SortKey::Memory);
    }

    #[test]
    fn a_column_not_at_the_end_only_reverses() {
        let shown = [SortKey::Cpu, SortKey::Memory];
        let sort = Sort::default().after_click(SortKey::Cpu, &shown, 5);
        assert_eq!((sort.key, sort.descending), (SortKey::Cpu, false));
    }

    #[test]
    fn numbers_read_well_at_every_scale() {
        let mut app = row("a", 0.42, 0.5);
        app.usage.gpu = 37.4;
        app.usage.video_memory_gb = 2.14;
        app.usage.disk_mb = 120.4;
        assert_eq!(app.cell(SortKey::Cpu), "0.4%");
        assert_eq!(app.cell(SortKey::Gpu), "37%");
        assert_eq!(app.cell(SortKey::Memory), "512 MB");
        assert_eq!(app.cell(SortKey::VideoMemory), "2.1 GB");
        assert_eq!(app.cell(SortKey::Disk), "120 MB/s");
    }

    #[test]
    fn only_nothing_counts_as_idle() {
        let app = row("a", 0.01, 0.2);
        assert!(app.is_idle(SortKey::Cpu));
        assert!(!app.is_idle(SortKey::Memory));
        assert!(app.is_idle(SortKey::VideoMemory));
    }

    #[test]
    fn every_sort_key_survives_the_settings_file() {
        for key in [SortKey::Name].into_iter().chain(COLUMNS) {
            assert_eq!(SortKey::from_id(key.id()), Some(key));
        }
        assert_eq!(SortKey::from_id("nonsense"), None);
    }

    #[test]
    fn the_biggest_share_decides_which_column_a_busy_row_opens() {
        let games = Usage {
            cpu: 30.0,
            gpu: 90.0,
            ..Default::default()
        };
        assert_eq!(games.biggest(PC), SortKey::Gpu);
        let hoarder = Usage {
            cpu: 2.0,
            memory_gb: 15.5,
            ..Default::default()
        };
        assert_eq!(hoarder.biggest(PC), SortKey::Memory);
        assert_eq!(Usage::default().biggest(PC), SortKey::Cpu);
    }

    #[test]
    fn frozen_is_reported_for_every_app_not_only_shown_ones() {
        let mut t = Tracker::default();
        let mut sample = app("a", calm());
        sample.frozen = true;
        assert!(!t.is_frozen("a"));
        t.update(vec![sample.clone()], PC);
        assert!(!t.is_frozen("a"));
        t.update(vec![sample], PC);
        assert!(t.is_frozen("a"));
        assert!(!t.is_frozen("unknown"));
    }

    #[test]
    fn forgetting_an_app_restarts_its_delay() {
        let mut t = Tracker::default();
        step(&mut t, busy(), APPEAR_AFTER);
        t.forget_all_but(&[]);
        assert_eq!(step(&mut t, busy(), 1), 0);
    }
}
