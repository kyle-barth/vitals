//! What Vitals works out from readings over time, rather than from one sample:
//! how far a GPU's hotspot runs above its edge, and which parts are held at a
//! limit. Pure logic with no Win32, so the rules can be tested.
//!
//! Each of these only says what the sensors show. A wide gap, for example, is
//! stated as a number and never as a diagnosis, because the data can't prove one.

use crate::history::Series;

/// GPU load from which the gap is worth showing, since an idle GPU's hotspot
/// and edge sit close together whatever the cooling.
pub const GAP_SHOW_LOAD: f32 = 80.0;
/// The gap counts towards "wide" only at this load or more.
const GAP_WIDE_LOAD: f32 = 90.0;
/// Healthy cards reportedly run 15 to 25 degrees apart at full load.
const GAP_WIDE: f32 = 30.0;
/// A wide gap clears below this, so the label doesn't flicker at the line.
const GAP_CLEAR: f32 = 27.0;
/// Samples a wide gap must hold before it's called wide.
const GAP_HOLD: u32 = 60;

/// The gap between a GPU's hotspot and edge temperatures.
#[derive(Default)]
pub struct GapTracker {
    current: Option<f32>,
    held: u32,
    wide: bool,
    history: Series,
}

/// What to show about the gap right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GapView {
    pub degrees: f32,
    pub wide: bool,
    /// Over the samples of the last two minutes that were under load.
    pub average: Option<f32>,
}

impl GapTracker {
    pub fn update(&mut self, edge: Option<f32>, hotspot: Option<f32>, load: Option<f32>) {
        let gap = edge.zip(hotspot).map(|(edge, hot)| hot - edge);
        let load = load.unwrap_or(0.0);
        self.current = gap.filter(|_| load >= GAP_SHOW_LOAD);
        self.history.push(self.current);
        match gap {
            Some(g) if load >= GAP_WIDE_LOAD && g >= GAP_WIDE => self.held += 1,
            _ => self.held = 0,
        }
        if self.held >= GAP_HOLD {
            self.wide = true;
        }
        if gap.is_none_or(|g| g < GAP_CLEAR) {
            self.wide = false;
        }
    }

    pub fn view(&self) -> Option<GapView> {
        let degrees = self.current?;
        Some(GapView {
            degrees,
            wide: self.wide,
            average: self.history.average(),
        })
    }
}

/// What holds a part back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    Temperature,
    Power,
}

/// NVIDIA clock event reasons, from nvml.h.
const NVIDIA_POWER_CAP: u64 = 0x4;
const NVIDIA_THERMAL: u64 = 0x20 | 0x40;
const NVIDIA_POWER_BRAKE: u64 = 0x80;
/// A CPU at this share of its power limit counts as at the limit.
const POWER_LIMIT_SHARE: f32 = 0.97;

/// Whether the GPU is at a limit this second. A hotspot at its rated maximum
/// is one; NVIDIA cards also say why their clocks are held down.
pub fn gpu_limit(
    hotspot: Option<f32>,
    hotspot_critical: Option<f32>,
    nvidia_reasons: Option<u64>,
) -> Option<LimitKind> {
    if hotspot
        .zip(hotspot_critical)
        .is_some_and(|(value, limit)| value >= limit)
    {
        return Some(LimitKind::Temperature);
    }
    let reasons = nvidia_reasons?;
    if reasons & NVIDIA_THERMAL != 0 {
        Some(LimitKind::Temperature)
    } else if reasons & (NVIDIA_POWER_CAP | NVIDIA_POWER_BRAKE) != 0 {
        Some(LimitKind::Power)
    } else {
        None
    }
}

/// Whether the CPU is at a limit this second. Both need the core temperature
/// helper: the I/O die says nothing about the cores' limit.
pub fn cpu_limit(
    core_temp: Option<f32>,
    core_critical: Option<f32>,
    power: Option<f32>,
    power_limit: Option<f32>,
) -> Option<LimitKind> {
    if core_temp
        .zip(core_critical)
        .is_some_and(|(value, limit)| value >= limit)
    {
        return Some(LimitKind::Temperature);
    }
    let (power, limit) = power.zip(power_limit)?;
    (limit > 0.0 && power >= limit * POWER_LIMIT_SHARE).then_some(LimitKind::Power)
}

/// How far under a limit a part must fall for its label to go at once. The
/// usual wait only smooths over a reading that hovers at the limit, not one
/// that has dropped away because the game has ended.
const CLEAR_DEGREES: f32 = 10.0;
const CLEAR_POWER_SHARE: f32 = 0.85;
const CLEAR_GPU_LOAD: f32 = 50.0;

/// Whether the GPU is plainly clear of the limit it's shown at.
pub fn gpu_clear(
    kind: LimitKind,
    hotspot: Option<f32>,
    hotspot_critical: Option<f32>,
    load: Option<f32>,
) -> bool {
    load.is_some_and(|l| l < CLEAR_GPU_LOAD)
        || (kind == LimitKind::Temperature
            && hotspot
                .zip(hotspot_critical)
                .is_some_and(|(value, limit)| value <= limit - CLEAR_DEGREES))
}

/// Whether the CPU is plainly clear of the limit it's shown at.
pub fn cpu_clear(
    kind: LimitKind,
    core_temp: Option<f32>,
    core_critical: Option<f32>,
    power: Option<f32>,
    power_limit: Option<f32>,
) -> bool {
    match kind {
        LimitKind::Temperature => core_temp
            .zip(core_critical)
            .is_some_and(|(value, limit)| value <= limit - CLEAR_DEGREES),
        LimitKind::Power => power
            .zip(power_limit)
            .is_some_and(|(power, limit)| power < limit * CLEAR_POWER_SHARE),
    }
}

/// Samples a limit must hold before it's shown, so a brief touch isn't news.
const LIMIT_SHOW_AFTER: u32 = 5;
/// Samples without it before it's no longer shown.
const LIMIT_CLEAR_AFTER: u32 = 10;
/// The share of time at a limit covers this many samples.
const LIMIT_WINDOW: u32 = 120;

/// Whether one part is held at a limit, and for how much of the last two minutes.
#[derive(Clone, Copy, Debug, Default)]
pub struct LimitTracker {
    on: u32,
    off: u32,
    shown: Option<LimitKind>,
    /// One bit per sample, newest lowest.
    history: u128,
    samples: u32,
}

impl LimitTracker {
    pub fn update(&mut self, now: Option<LimitKind>) {
        let mask = (1u128 << LIMIT_WINDOW) - 1;
        self.history = ((self.history << 1) | u128::from(now.is_some())) & mask;
        self.samples = (self.samples + 1).min(LIMIT_WINDOW);
        match now {
            Some(kind) => {
                self.on += 1;
                self.off = 0;
                if self.on >= LIMIT_SHOW_AFTER || self.shown.is_some() {
                    self.shown = Some(kind);
                }
            }
            None => {
                self.off += 1;
                self.on = 0;
                if self.off >= LIMIT_CLEAR_AFTER {
                    self.shown = None;
                }
            }
        }
    }

    pub fn shown(&self) -> Option<LimitKind> {
        self.shown
    }

    /// Stops showing the limit at once, for a part that is well clear of it.
    pub fn release(&mut self) {
        self.shown = None;
        self.on = 0;
    }

    /// Percent of the last two minutes spent at a limit.
    pub fn share(&self) -> f32 {
        if self.samples == 0 {
            return 0.0;
        }
        self.history.count_ones() as f32 / self.samples as f32 * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gap_for(t: &mut GapTracker, seconds: u32, edge: f32, hot: f32, load: f32) {
        for _ in 0..seconds {
            t.update(Some(edge), Some(hot), Some(load));
        }
    }

    #[test]
    fn the_gap_only_shows_under_load() {
        let mut t = GapTracker::default();
        gap_for(&mut t, 1, 40.0, 45.0, 10.0);
        assert_eq!(t.view(), None);
        gap_for(&mut t, 1, 78.0, 110.0, 99.0);
        assert_eq!(t.view().map(|v| v.degrees), Some(32.0));
    }

    #[test]
    fn a_gap_is_wide_only_after_holding_for_a_minute() {
        let mut t = GapTracker::default();
        gap_for(&mut t, 59, 78.0, 110.0, 99.0);
        assert!(!t.view().unwrap().wide);
        gap_for(&mut t, 1, 78.0, 110.0, 99.0);
        assert!(t.view().unwrap().wide);
    }

    #[test]
    fn a_healthy_card_is_never_wide() {
        let mut t = GapTracker::default();
        gap_for(&mut t, 600, 75.0, 97.0, 99.0);
        assert!(!t.view().unwrap().wide);
    }

    #[test]
    fn a_wide_gap_holds_until_it_clearly_narrows() {
        let mut t = GapTracker::default();
        gap_for(&mut t, 60, 78.0, 110.0, 99.0);
        gap_for(&mut t, 5, 80.0, 108.0, 99.0);
        assert!(t.view().unwrap().wide, "28 degrees is still wide");
        gap_for(&mut t, 1, 82.0, 108.0, 99.0);
        assert!(!t.view().unwrap().wide, "26 degrees clears it");
    }

    #[test]
    fn a_gpu_without_a_hotspot_has_no_gap() {
        let mut t = GapTracker::default();
        t.update(Some(70.0), None, Some(99.0));
        assert_eq!(t.view(), None);
    }

    fn limit_for(t: &mut LimitTracker, seconds: u32, now: Option<LimitKind>) {
        for _ in 0..seconds {
            t.update(now);
        }
    }

    #[test]
    fn a_limit_shows_after_five_seconds_and_clears_after_ten() {
        let mut t = LimitTracker::default();
        limit_for(&mut t, 4, Some(LimitKind::Temperature));
        assert_eq!(t.shown(), None);
        limit_for(&mut t, 1, Some(LimitKind::Temperature));
        assert_eq!(t.shown(), Some(LimitKind::Temperature));
        limit_for(&mut t, 9, None);
        assert_eq!(t.shown(), Some(LimitKind::Temperature));
        limit_for(&mut t, 1, None);
        assert_eq!(t.shown(), None);
    }

    #[test]
    fn the_share_covers_the_last_two_minutes() {
        let mut t = LimitTracker::default();
        limit_for(&mut t, 90, Some(LimitKind::Power));
        limit_for(&mut t, 30, None);
        assert_eq!(t.share().round(), 75.0);
        limit_for(&mut t, 120, None);
        assert_eq!(t.share(), 0.0);
    }

    #[test]
    fn the_gpu_is_at_its_limit_only_at_its_rated_hotspot() {
        assert_eq!(
            gpu_limit(Some(110.0), Some(110.0), None),
            Some(LimitKind::Temperature)
        );
        assert_eq!(gpu_limit(Some(109.0), Some(110.0), None), None);
        assert_eq!(gpu_limit(None, Some(110.0), None), None);
    }

    #[test]
    fn nvidia_reasons_say_which_limit() {
        assert_eq!(gpu_limit(None, None, Some(0x4)), Some(LimitKind::Power));
        assert_eq!(
            gpu_limit(None, None, Some(0x40)),
            Some(LimitKind::Temperature)
        );
        assert_eq!(
            gpu_limit(None, None, Some(0x1)),
            None,
            "idle is not a limit"
        );
        assert_eq!(gpu_limit(None, None, Some(0x0)), None);
    }

    #[test]
    fn the_cpu_limit_needs_the_core_reading() {
        assert_eq!(
            cpu_limit(Some(95.0), Some(95.0), Some(107.0), Some(162.0)),
            Some(LimitKind::Temperature)
        );
        assert_eq!(
            cpu_limit(Some(90.0), Some(95.0), Some(107.0), Some(162.0)),
            None
        );
        assert_eq!(
            cpu_limit(Some(80.0), Some(95.0), Some(160.0), Some(162.0)),
            Some(LimitKind::Power)
        );
        assert_eq!(cpu_limit(None, Some(95.0), Some(160.0), None), None);
    }

    #[test]
    fn a_shown_limit_follows_a_change_of_kind() {
        let mut t = LimitTracker::default();
        limit_for(&mut t, 5, Some(LimitKind::Power));
        limit_for(&mut t, 1, Some(LimitKind::Temperature));
        assert_eq!(t.shown(), Some(LimitKind::Temperature));
    }

    #[test]
    fn a_part_well_clear_of_its_limit_lets_the_label_go() {
        let hot = LimitKind::Temperature;
        assert!(
            !gpu_clear(hot, Some(108.0), Some(110.0), Some(99.0)),
            "hovering"
        );
        assert!(gpu_clear(hot, Some(100.0), Some(110.0), Some(99.0)));
        assert!(
            gpu_clear(hot, Some(108.0), Some(110.0), Some(20.0)),
            "the game ended"
        );
        assert!(!gpu_clear(LimitKind::Power, None, None, Some(99.0)));
        assert!(!cpu_clear(
            hot,
            Some(90.0),
            Some(95.0),
            Some(40.0),
            Some(162.0)
        ));
        assert!(cpu_clear(
            hot,
            Some(85.0),
            Some(95.0),
            Some(160.0),
            Some(162.0)
        ));
        assert!(!cpu_clear(
            LimitKind::Power,
            Some(60.0),
            Some(95.0),
            Some(150.0),
            Some(162.0)
        ));
        assert!(cpu_clear(
            LimitKind::Power,
            Some(60.0),
            Some(95.0),
            Some(52.0),
            Some(162.0)
        ));

        let mut t = LimitTracker::default();
        limit_for(&mut t, 5, Some(hot));
        t.release();
        assert_eq!(t.shown(), None);
        limit_for(&mut t, 4, Some(hot));
        assert_eq!(
            t.shown(),
            None,
            "it must hold again before it's shown again"
        );
    }
}
