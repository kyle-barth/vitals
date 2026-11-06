//! Decides which apps are worth showing. Pure logic with no Win32, so that the
//! rules can be tested.
//!
//! The point of Vitals is to stay simple, so the list is empty on a calm PC. An
//! app appears only once it has held a lot of a resource for a few seconds, and
//! leaves a few seconds after dropping back, so a brief spike never moves the
//! window around. A frozen app always appears, however little it is using.

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
    pub frozen: bool,
    pub can_end: bool,
    pub processes: Vec<ProcessId>,
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
        };
        assert_eq!(usage.labels(PC), ["88% GPU", "12.1 GB VRAM", "26% CPU"]);
    }

    #[test]
    fn forgetting_an_app_restarts_its_delay() {
        let mut t = Tracker::default();
        step(&mut t, busy(), APPEAR_AFTER);
        t.forget_all_but(&[]);
        assert_eq!(step(&mut t, busy(), 1), 0);
    }
}
