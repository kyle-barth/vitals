//! Paints the window from a [`Layout`] and a [`Monitor`].
//!
//! Color follows two separate systems. Each part keeps one series color
//! everywhere: the CPU cyan, the GPU violet, its hotspot pink and memory green.
//! State has a ladder of its own, the same on every surface, which grows louder
//! in shape as well as color:
//! - Warm: a tinted amber chip. The number stays as it is.
//! - At limit: an outlined orange chip. A part held at its limit is protecting
//!   itself, which is what it is built to do, so the number and frame stay calm.
//! - Hot: orange digits, a tinted chip and an orange frame on the hero.
//! - Critical: a solid red tile behind the reading, and a red frame.
//!
//! Each state is also spelled out in words, so it is never shown by color alone.

use std::sync::Arc;

use crate::apps::{AppRow, BusyApp, Icon, Sort, SortKey};
use crate::gfx::{Align, Canvas, Color, Weight};
use crate::history::{CAPACITY, Series};
use crate::insights::LimitKind;
use crate::layout::{
    Button, Cell, Compact, Hero, Large, Layout, Mode, Page, Rect, SPECS_FACT_H, SPECS_ITEM_H,
    SPECS_LABEL_W, Section, SpecsPage, StripItem, StripPlan, TABLE_ROW_PAD, TITLE_H, Table,
    confirm_buttons, end_button, stats_metrics,
};
use crate::menu::{Action, Plan};
use crate::metrics::{CoreStatus, MetricId, Monitor, Scale, Unit};
use crate::search::find;
use crate::severity::Severity;
use crate::specs::{Part, PartKind, Specs};

const WHITE: Color = Color::rgb(0xffffff);
/// The corner radius of the page's cards.
const CARD_RADIUS: f32 = 14.0;
/// The corner radius of rows, such as a busy app's.
const ROW_RADIUS: f32 = 10.0;
/// The space inside a card, either side of its content.
const INSET: f32 = 16.0;

pub struct Theme {
    /// Behind the cards.
    pub surface: Color,
    /// Cards, rows and the menu.
    pub raised: Color,
    /// The well the tabs sit in, and the tab that is selected.
    well: Color,
    pill: Color,
    /// The hero's frame while nothing is wrong.
    hair: Color,
    grid: Color,
    pub track: Color,
    pub ink: Color,
    pub ink2: Color,
    pub ink3: Color,
    /// The pin when it is on, ticks in the menu, and a copy that worked.
    pub accent: Color,
    cpu: Color,
    gpu: Color,
    hotspot: Color,
    memory: Color,
    io: Color,
    /// Bars for rates, such as a fan, which are neither good nor bad.
    neutral: Color,
    warm: Color,
    warm_text: Color,
    warm_chip: Color,
    hot: Color,
    hot_text: Color,
    hot_chip: Color,
    hot_chip_text: Color,
    critical: Color,
    critical_text: Color,
    ok: Color,
    tick: Color,
    limit_line: Color,
    divider: Color,
    link: Color,
    tab: Color,
    /// Dark mode marks time at a limit with a bar, and light mode with a band.
    dark: bool,
}

pub const DARK: Theme = Theme {
    surface: Color::rgb(0x0b0e14),
    raised: Color::rgb(0x161d2a),
    well: Color::rgb(0x161c28),
    pill: Color::rgb(0x28324a),
    hair: Color::rgb(0x2b3445),
    grid: Color::rgb(0xffffff).with_alpha(0.09),
    track: Color::rgb(0x222a3a),
    ink: Color::rgb(0xe9eef6),
    ink2: Color::rgb(0xb2bccb),
    ink3: Color::rgb(0x8b97aa),
    accent: Color::rgb(0x4cc2ff),
    cpu: Color::rgb(0x38d0ec),
    gpu: Color::rgb(0xb38bff),
    hotspot: Color::rgb(0xf472b6),
    memory: Color::rgb(0x3fd0a0),
    io: Color::rgb(0x94a3b8),
    neutral: Color::rgb(0x5b6577),
    warm: Color::rgb(0xf6b544),
    warm_text: Color::rgb(0xf6b544),
    warm_chip: Color::rgb(0xf6b544).with_alpha(0.17),
    hot: Color::rgb(0xff8a4d),
    hot_text: Color::rgb(0xff9a63),
    hot_chip: Color::rgb(0xff8a4d).with_alpha(0.18),
    hot_chip_text: Color::rgb(0xff9a63),
    critical: Color::rgb(0xc81e1e),
    critical_text: Color::rgb(0xff6a62),
    ok: Color::rgb(0x8b97aa),
    tick: Color::rgb(0x4a505a),
    limit_line: Color::rgb(0xc8d0dc).with_alpha(0.45),
    divider: Color::rgb(0x1f242f),
    link: Color::rgb(0x7cb4ff),
    tab: Color::rgb(0x8b97aa),
    dark: true,
};

pub const LIGHT: Theme = Theme {
    surface: Color::rgb(0xe8ecf2),
    raised: Color::rgb(0xffffff),
    well: Color::rgb(0xdbe0e8),
    pill: Color::rgb(0xffffff),
    hair: Color::rgb(0xd3dae4),
    grid: Color::rgb(0xe6e9ef),
    track: Color::rgb(0xe6eaf0),
    ink: Color::rgb(0x0b0e14),
    ink2: Color::rgb(0x3d4655),
    ink3: Color::rgb(0x5f6773),
    accent: Color::rgb(0x0067c0),
    cpu: Color::rgb(0x087591),
    gpu: Color::rgb(0x7c4ddb),
    hotspot: Color::rgb(0xb5179e),
    memory: Color::rgb(0x0d8a63),
    io: Color::rgb(0x5b6b80),
    neutral: Color::rgb(0x9aa3af),
    warm: Color::rgb(0xa16207),
    warm_text: Color::rgb(0x8f5b00),
    warm_chip: Color::rgb(0xfdf1c7),
    hot: Color::rgb(0xea580c),
    hot_text: Color::rgb(0xc2410c),
    hot_chip: Color::rgb(0xffe8d6),
    hot_chip_text: Color::rgb(0x9a3412),
    critical: Color::rgb(0xb91c1c),
    critical_text: Color::rgb(0xb91c1c),
    ok: Color::rgb(0x5f6773),
    tick: Color::rgb(0xc3c9d1),
    limit_line: Color::rgb(0x3c4655).with_alpha(0.45),
    divider: Color::rgb(0xe6eaf0),
    link: Color::rgb(0x1f5fd1),
    tab: Color::rgb(0x4b5563),
    dark: false,
};

impl Theme {
    /// The color a part keeps everywhere.
    fn series(&self, metric: MetricId) -> Color {
        use MetricId::*;
        match metric {
            CpuTemp | IoDieTemp | CpuLoad | CpuPower | CpuClock => self.cpu,
            GpuTemp | GpuLoad | GpuPower | GpuClock | VramTemp | VideoMemory => self.gpu,
            GpuHotspot => self.hotspot,
            Memory => self.memory,
            GpuFan | GpuFanPercent => self.neutral,
            DiskRead | DiskWrite | Download | Upload => self.io,
        }
    }

    fn digits(&self, state: State) -> Color {
        match state {
            State::Hot => self.hot_text,
            State::Critical => self.critical_text,
            _ => self.ink,
        }
    }

    /// The color of a state's dot or bar.
    fn level(&self, state: State) -> Option<Color> {
        match state {
            State::Normal => None,
            State::Warm => Some(self.warm),
            State::AtLimit | State::Hot => Some(self.hot),
            State::Critical => Some(self.critical),
        }
    }

    /// The ring on a chart's end dot. A part at its limit is where it should
    /// be, so it has none.
    fn ring(&self, state: State) -> Option<Color> {
        match state {
            State::AtLimit => None,
            other => self.level(other),
        }
    }
}

/// What the pointer is over.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hover {
    pub inside: bool,
    pub button: Option<Button>,
    /// The row of the busy app under the pointer.
    pub app: Option<usize>,
    pub tab: Option<Page>,
    /// The line of the Apps table under the pointer, counted from its first line.
    pub row: Option<usize>,
    pub heading: Option<SortKey>,
    /// The Specs page's copy button.
    pub copy: bool,
    /// The summary's link to everything that needs attention.
    pub more: bool,
    /// The Apps tab's search field, and its clear button.
    pub search: bool,
    pub search_clear: bool,
}

pub struct View<'a> {
    pub layout: &'a Layout,
    pub monitor: &'a Monitor,
    pub theme: &'a Theme,
    pub hover: Hover,
    pub pinned: bool,
    /// The app being asked about before it is ended.
    pub confirm: Option<&'a str>,
    /// A short message on one app's row: its key, then the text.
    pub notice: Option<(&'a str, &'a str)>,
    /// Show only the numbers, rather than their history too.
    pub simple: bool,
    pub sort: Sort,
    /// How many lines of the Apps table, or parts on the Specs page, are
    /// scrolled off the top.
    pub scroll: usize,
    pub menu_open: bool,
    /// What is in this PC, once it has been read.
    pub specs: Option<&'a Specs>,
    /// Whether the last copy worked, while the button is saying so.
    pub copied: Option<bool>,
    pub pc_name: Option<&'a str>,
    /// What has been typed into the Apps tab's search.
    pub search: &'a str,
    /// The search field is focused or holding a search.
    pub search_active: bool,
    /// The field has the keyboard, so it shows its caret.
    pub search_focused: bool,
    pub search_caret: usize,
    /// The selected characters, as an ordered range.
    pub search_selection: Option<(usize, usize)>,
    /// How many apps there are before the search filters them.
    pub apps_total: usize,
}

/// A reading's place on the state ladder, quietest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum State {
    Normal,
    Warm,
    AtLimit,
    Hot,
    Critical,
}

impl State {
    fn word(self) -> &'static str {
        match self {
            State::Normal => "",
            State::Warm => "Warm",
            State::AtLimit => "At limit",
            State::Hot => "Hot",
            State::Critical => "Critical",
        }
    }
}

/// Whether the GPU is held at its temperature limit. A hotspot at its rated
/// maximum is, from the first second, since that is where the card throttles,
/// and many cards run there by design under full load. A CPU at its limit is
/// rarer, and a sign of a cooling problem, so it stays Critical.
fn at_limit(metric: MetricId, monitor: &Monitor) -> bool {
    match metric {
        MetricId::GpuHotspot => {
            limit_on(metric, monitor) == Some(LimitKind::Temperature)
                || monitor.get(metric).severity == Severity::Critical
        }
        MetricId::GpuTemp => limit_on(metric, monitor) == Some(LimitKind::Temperature),
        _ => false,
    }
}

fn state(metric: MetricId, monitor: &Monitor) -> State {
    if at_limit(metric, monitor) {
        return State::AtLimit;
    }
    match monitor.get(metric).severity {
        Severity::Normal => State::Normal,
        Severity::Warm => State::Warm,
        Severity::Hot => State::Hot,
        Severity::Critical => State::Critical,
    }
}

/// The metric a cell actually shows, since the CPU temperature cell falls back
/// to the I/O die until a true core reading is available.
fn resolve(metric: MetricId, monitor: &Monitor) -> MetricId {
    if metric == MetricId::CpuTemp {
        monitor.cpu_temp_metric()
    } else {
        metric
    }
}

/// Whether `metric` is on screen, counting the I/O die's stand-in cell.
pub fn is_visible(layout: &Layout, monitor: &Monitor, metric: MetricId) -> bool {
    layout
        .cells
        .iter()
        .any(|c| resolve(c.metric, monitor) == metric)
}

/// The text for the taskbar, through the window's title, when a temperature
/// needs attention.
pub fn alert_text(layout: &Layout, monitor: &Monitor) -> Option<String> {
    let (metric, _, value) = monitor.alert(|m| is_visible(layout, monitor, m))?;
    Some(format!("{} {:.0}°", metric.alert_name(), value))
}

/// Where painting put the things a click has to find.
#[derive(Debug, Default)]
pub struct Painted {
    /// The summary's link to the full list of what needs attention.
    pub more: Option<Rect>,
    /// Each caret position in the search that is on screen, as (position, x).
    pub search_marks: Vec<(usize, f32)>,
}

/// Paints the window, and says where the things a click has to find went.
pub fn paint(c: &Canvas, v: &View) -> Painted {
    if v.layout.mode == Mode::Strip {
        paint_strip(c, v);
        return Painted::default();
    }
    c.clear(v.theme.surface);
    paint_title_bar(c, v);
    if let Some(table) = &v.layout.table {
        return Painted {
            more: None,
            search_marks: paint_table(c, v, table),
        };
    }
    if let Some(page) = &v.layout.specs {
        paint_specs(c, v, page);
        return Painted::default();
    }
    let mut more = None;
    if let Some(compact) = &v.layout.compact {
        more = paint_compact(c, v, compact);
    } else if let Some(large) = &v.layout.large {
        paint_large(c, v, large);
    } else {
        for cell in &v.layout.cells {
            paint_row(c, v, cell);
        }
    }
    paint_app_rows(c, v, false);
    Painted {
        more,
        search_marks: Vec::new(),
    }
}

/// The limit a reading is at, if it's being shown.
fn limit_on(metric: MetricId, monitor: &Monitor) -> Option<LimitKind> {
    let (gpu, cpu) = (monitor.gpu_limit.shown(), monitor.cpu_limit.shown());
    let temperature = Some(LimitKind::Temperature);
    let power = Some(LimitKind::Power);
    match metric {
        MetricId::GpuHotspot if gpu == temperature => gpu,
        // A GPU without a hotspot sensor shows its limit on the edge instead.
        MetricId::GpuTemp
            if gpu == temperature && monitor.absent.contains(MetricId::GpuHotspot) =>
        {
            gpu
        }
        MetricId::CpuTemp if cpu == temperature => cpu,
        MetricId::GpuPower if gpu == power => gpu,
        MetricId::CpuPower if cpu == power => cpu,
        _ => None,
    }
}

// ---------- small pieces ----------

#[derive(Clone, Copy, PartialEq)]
enum ChipKind {
    Warm,
    Hot,
    Limit,
    Critical,
    /// Outlined in white, on a red tile.
    OnCritical,
}

fn chip_kind(state: State) -> Option<ChipKind> {
    match state {
        State::Normal => None,
        State::Warm => Some(ChipKind::Warm),
        State::AtLimit => Some(ChipKind::Limit),
        State::Hot => Some(ChipKind::Hot),
        State::Critical => Some(ChipKind::Critical),
    }
}

/// A chip: 18px tall, with 8px either side of its word, centred on `y`.
/// Returns its width.
fn chip(
    c: &Canvas,
    t: &Theme,
    (x, y): (f32, f32),
    word: &str,
    kind: ChipKind,
    align: Align,
) -> f32 {
    let w = c.measure(word, 12.0, Weight::Semibold) + 16.0;
    let left = if align == Align::Right { x - w } else { x };
    let r = Rect::new(left, y - 9.0, w, 18.0);
    let ink = match kind {
        ChipKind::Critical => {
            c.fill_round(r, 9.0, t.critical);
            WHITE
        }
        ChipKind::OnCritical => {
            c.stroke_round(r, 9.0, 1.25, WHITE);
            WHITE
        }
        ChipKind::Limit => {
            c.stroke_round(r, 9.0, 1.5, t.hot);
            t.hot_text
        }
        ChipKind::Hot => {
            c.fill_round(r, 9.0, t.hot_chip);
            t.hot_chip_text
        }
        ChipKind::Warm => {
            c.fill_round(r, 9.0, t.warm_chip);
            t.warm_text
        }
    };
    c.text_mid(
        word,
        (left + 8.0, y),
        12.0,
        Weight::Semibold,
        Align::Left,
        ink,
    );
    w
}

/// A unit's size beside a number: 60% of it, and never under 11px.
fn unit_size(size: f32) -> f32 {
    (size * 0.6).round().max(11.0)
}

/// A number with its unit, set tight for ° and %, and an optional word after.
/// Returns the width.
#[allow(clippy::too_many_arguments)]
fn figure(
    c: &Canvas,
    t: &Theme,
    (x, y): (f32, f32),
    value: &str,
    unit: &str,
    size: f32,
    color: Color,
    word: Option<&str>,
) -> f32 {
    let mut w = c.number(value, (x, y), size, Align::Left, color);
    if !unit.is_empty() {
        let gap = if unit.starts_with(['%', '°']) {
            1.0
        } else {
            3.0
        };
        w += gap
            + c.text(
                unit,
                (x + w + gap, y),
                unit_size(size),
                Weight::Medium,
                Align::Left,
                t.ink3,
            );
    }
    if let Some(word) = word {
        w += 4.0
            + c.text(
                word,
                (x + w + 4.0, y),
                12.0,
                Weight::Regular,
                Align::Left,
                t.ink3,
            );
    }
    w
}

/// A number and its unit, right-aligned together at `right`.
fn figure_right(
    c: &Canvas,
    t: &Theme,
    (right, y): (f32, f32),
    value: &str,
    unit: &str,
    size: f32,
    color: Color,
) -> f32 {
    let mut x = right;
    if !unit.is_empty() {
        let w = c.text(
            unit,
            (right, y),
            unit_size(size),
            Weight::Medium,
            Align::Right,
            t.ink3,
        );
        x -= w + 3.0;
    }
    let w = c.number(value, (x, y), size, Align::Right, color);
    right - x + w
}

/// A reading as its number and unit. Degrees belong to the number.
fn reading_text(metric: MetricId, value: Option<f32>) -> (String, &'static str) {
    let unit = metric.spec().unit;
    match value {
        None => ("–".to_string(), ""),
        Some(v) if unit == Unit::Celsius => (format!("{v:.0}°"), ""),
        Some(v) => (unit.format(v), unit.suffix()),
    }
}

/// A value with just enough of its unit to read on its own.
fn with_unit(metric: MetricId, value: f32) -> String {
    let unit = metric.spec().unit;
    let number = unit.format(value);
    match unit {
        Unit::Celsius | Unit::Percent => format!("{number}{}", unit.suffix()),
        Unit::Watts => format!("{number} W"),
        _ => number,
    }
}

/// A reading's short name, as the hero and the strip use it.
fn short_name(metric: MetricId) -> &'static str {
    match metric {
        MetricId::CpuTemp => "CPU",
        MetricId::IoDieTemp => "CPU I/O die",
        MetricId::GpuTemp => "GPU edge",
        MetricId::GpuHotspot => "Hotspot",
        MetricId::VramTemp | MetricId::VideoMemory => "VRAM",
        other => other.spec().label,
    }
}

fn meter(c: &Canvas, r: Rect, fill: f32, color: Color, track: Color) {
    c.fill_round(r, r.h / 2.0, track);
    if fill > 0.001 {
        let width = (r.w * fill.clamp(0.0, 1.0)).max(r.h);
        c.fill_round(Rect { w: width, ..r }, r.h / 2.0, color);
    }
}

/// The small mark at the end of a bar that stands for a limit.
fn limit_tick(c: &Canvas, t: &Theme, x: f32, bar: Rect) {
    c.fill_round(
        Rect::new(x - 1.0, bar.y - (10.0 - bar.h) / 2.0, 2.0, 10.0),
        1.0,
        t.tick,
    );
}

/// A line sharp on whole pixels, across from `x1` to `x2`.
fn rule(c: &Canvas, (x1, x2): (f32, f32), y: f32, color: Color) {
    let y = y.floor() + 0.5;
    c.line((x1, y), (x2, y), 1.0, color);
}

fn check(c: &Canvas, (x, y): (f32, f32), ink: Color) {
    c.polyline(
        &[(x - 4.0, y), (x - 1.5, y + 3.0), (x + 4.0, y - 3.5)],
        1.5,
        ink,
    );
}

fn chevron(c: &Canvas, (x, y): (f32, f32), ink: Color) {
    c.polyline(
        &[(x - 1.5, y - 3.5), (x + 2.0, y), (x - 1.5, y + 3.5)],
        1.4,
        ink,
    );
}

/// A small arrow, pointing down for download and up for upload.
fn arrow(c: &Canvas, (x, y): (f32, f32), down: bool, ink: Color) {
    let tip = if down { 4.0 } else { -4.0 };
    c.line((x, y - tip), (x, y + tip), 1.2, ink);
    c.polyline(
        &[
            (x - 3.0, y + tip * 0.25),
            (x, y + tip),
            (x + 3.0, y + tip * 0.25),
        ],
        1.2,
        ink,
    );
}

/// Wraps `text` to lines no wider than `width`.
fn wrap(c: &Canvas, text: &str, size: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let next = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && c.measure(&next, size, Weight::Regular) > width {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        } else {
            line = next;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

// ---------- what needs attention ----------

struct Issue {
    state: State,
    title: String,
    detail: String,
    /// A shorter title for the one-line summary, where one is needed.
    short: Option<String>,
}

/// Everything that needs attention, worst first, in plain words.
fn issues(monitor: &Monitor) -> Vec<Issue> {
    let mut out = Vec::new();
    let cpu = monitor.cpu_temp_metric();
    for id in [
        cpu,
        MetricId::GpuHotspot,
        MetricId::GpuTemp,
        MetricId::VramTemp,
    ] {
        let Some(value) = monitor.get(id).value else {
            continue;
        };
        let limit = id.spec().limits.map_or(0.0, |l| l.critical);
        let state = state(id, monitor);
        match state {
            State::Normal => {}
            State::AtLimit => {
                let (title, short) = if id == MetricId::GpuHotspot {
                    (
                        format!("GPU hotspot held at its {limit:.0}° limit"),
                        "Hotspot held at its limit",
                    )
                } else {
                    (
                        "GPU held at its temperature limit".to_string(),
                        "GPU held at its limit",
                    )
                };
                let fans = monitor
                    .get(MetricId::GpuFanPercent)
                    .value
                    .filter(|f| *f >= 99.5)
                    .map_or("", |_| ", fans at 100%");
                out.push(Issue {
                    state,
                    title,
                    detail: format!(
                        "{:.0}% of the last 2 minutes{fans}. The card slows itself down to stay there.",
                        monitor.gpu_limit.share()
                    ),
                    short: Some(short.to_string()),
                });
            }
            _ => {
                let name = match id {
                    MetricId::CpuTemp => "CPU",
                    MetricId::IoDieTemp => "CPU I/O die",
                    MetricId::GpuTemp => "GPU edge",
                    MetricId::GpuHotspot => "GPU hotspot",
                    _ => "VRAM",
                };
                let held = id == MetricId::CpuTemp
                    && monitor.cpu_limit.shown() == Some(LimitKind::Temperature);
                let detail = if held {
                    format!(
                        "At its {limit:.0}° limit for {:.0}% of the last 2 minutes. It slows itself down to stay there.",
                        monitor.cpu_limit.share()
                    )
                } else if state == State::Critical {
                    format!("Its limit is {limit:.0}°.")
                } else {
                    format!("Critical at {limit:.0}°.")
                };
                out.push(Issue {
                    state,
                    title: format!("{name} {} at {value:.0}°", state.word().to_lowercase()),
                    detail,
                    short: None,
                });
            }
        }
    }
    if let Some(gap) = monitor.gap.view().filter(|g| g.wide) {
        out.push(Issue {
            state: State::Normal,
            title: format!("Hotspot gap is wide: +{:.0}°", gap.degrees),
            detail: "Most cards run 15–25° apart under load. A wide gap can mean poor \
                     cooler contact."
                .to_string(),
            short: None,
        });
    }
    out.sort_by_key(|issue| std::cmp::Reverse(issue.state));
    out
}

/// A note that a part at its power limit is fine, which it is under full load.
fn power_note(monitor: &Monitor) -> Option<String> {
    if monitor.cpu_limit.shown() == Some(LimitKind::Power) {
        return Some(match monitor.cpu_power_limit {
            Some(limit) => format!("CPU power at its {limit:.0} W limit is normal under load."),
            None => "CPU power at its limit is normal under load.".to_string(),
        });
    }
    (monitor.gpu_limit.shown() == Some(LimitKind::Power))
        .then(|| "GPU power at its limit is normal under load.".to_string())
}

/// The one line under the compact chart: the worst issue, and a link to the
/// rest. Returns where the link went.
fn paint_summary(
    c: &Canvas,
    v: &View,
    issues: &[Issue],
    (x, y): (f32, f32),
    right: f32,
) -> Option<Rect> {
    let t = v.theme;
    let Some(first) = issues.first() else {
        check(c, (x + 5.0, y - 4.0), t.ok);
        c.text(
            "All normal",
            (x + 16.0, y),
            12.0,
            Weight::Semibold,
            Align::Left,
            t.ink2,
        );
        return None;
    };
    c.circle(
        x + 4.0,
        y - 4.0,
        4.0,
        t.level(first.state).unwrap_or(t.ink3),
    );
    let more = (issues.len() > 1).then(|| format!("+{} more", issues.len() - 1));
    let more_w = more
        .as_deref()
        .map_or(0.0, |m| c.measure(m, 12.0, Weight::Semibold) + 23.0);
    let title = first.short.as_deref().unwrap_or(&first.title);
    let title = c.fit(title, 12.0, Weight::Semibold, right - x - 14.0 - more_w);
    let w = c.text(
        &title,
        (x + 14.0, y),
        12.0,
        Weight::Semibold,
        Align::Left,
        t.ink,
    );
    let more = more?;
    let link_x = x + 14.0 + w + 10.0;
    let link_w = c.text(
        &more,
        (link_x, y),
        12.0,
        Weight::Semibold,
        Align::Left,
        t.link,
    );
    chevron(c, (link_x + link_w + 6.0, y - 4.0), t.link);
    if v.hover.more {
        rule(c, (link_x, link_x + link_w), y + 2.0, t.link);
    }
    Some(Rect::new(link_x - 4.0, y - 14.0, link_w + 20.0, 20.0))
}

// ---------- the hero ----------

/// Where a reading's tile goes if it turns critical.
struct Tile {
    left: f32,
    right: f32,
    top: f32,
    /// Against the card's left edge, so its corner follows the card's.
    flush: bool,
}

fn tile_for(hero: &Hero, i: usize, inset: f32, gap: f32) -> Tile {
    let r = hero.rect;
    Tile {
        left: if i == 0 {
            r.x + 3.0
        } else {
            hero.columns[i] - inset
        },
        right: if i < 2 {
            hero.columns[i + 1] - inset - gap
        } else {
            (hero.columns[2] - inset + hero.pitch - gap).min(r.right() - 11.0)
        },
        top: r.y + 3.0,
        flush: i == 0,
    }
}

/// What goes under a reading when its state has nothing to say.
fn caption(metric: MetricId, monitor: &Monitor) -> Option<String> {
    match metric {
        MetricId::IoDieTemp => Some(
            match monitor.core {
                CoreStatus::Requesting => "Waiting for approval",
                CoreStatus::Unavailable => "Core temp unavailable",
                CoreStatus::Running => "Starting core sensor",
                _ => "Click for core temp",
            }
            .to_string(),
        ),
        MetricId::Memory => monitor.ram_total_gb.map(|t| format!("of {t:.0} GB")),
        _ => {
            let peak = monitor.get(metric).peak?;
            Some(format!("Peak {}", with_unit(metric, peak)))
        }
    }
}

/// One of the hero's big readings: its name and series dot, the number, and
/// under it a state chip or its peak.
fn readout(c: &Canvas, v: &View, slot: MetricId, hero: &Hero, i: usize, tile: Tile) {
    let (t, m) = (v.theme, v.monitor);
    let metric = resolve(slot, m);
    let state = state(metric, m);
    let critical = state == State::Critical;
    let (x, label_y, size) = (hero.columns[i], hero.label_y, hero.number);
    let baseline = hero.number_y();
    if critical {
        let left = if tile.flush {
            tile.left - 3.0
        } else {
            tile.left
        };
        let top = tile.top - 3.0;
        let corner = if tile.flush { CARD_RADIUS } else { 8.0 };
        c.fill_corners(
            Rect::new(left, top, tile.right - left, baseline + 33.0 - top),
            [corner, 8.0, 8.0, 8.0],
            t.critical,
        );
    }
    let on = |normal: Color| if critical { WHITE } else { normal };
    c.circle(x + 4.0, label_y - 4.0, 4.0, on(t.series(metric)));
    c.text(
        short_name(metric),
        (x + 13.0, label_y),
        12.0,
        Weight::Medium,
        Align::Left,
        on(t.ink2),
    );
    let (value, unit) = reading_text(metric, m.get(metric).value);
    figure(
        c,
        t,
        (x - 1.0, baseline),
        &value,
        unit,
        size,
        on(t.digits(state)),
        None,
    );
    if let Some(kind) = chip_kind(state) {
        let kind = if critical { ChipKind::OnCritical } else { kind };
        chip(c, t, (x, baseline + 16.0), state.word(), kind, Align::Left);
    } else if let Some(text) = caption(metric, m) {
        // A caption may run up to the next reading's name. One too long for
        // that, such as the prompt for a core reading, steps down to 11px
        // before it is shortened.
        let room = tile.right - x + 10.0;
        let size = if c.measure(&text, 12.0, Weight::Regular) <= room {
            12.0
        } else {
            11.0
        };
        let text = c.fit(&text, size, Weight::Regular, room);
        c.text(
            &text,
            (x, baseline + 20.0),
            size,
            Weight::Regular,
            Align::Left,
            t.ink3,
        );
    }
}

/// The framed card, its readings, and its frame, which tells the worst state.
fn paint_hero(c: &Canvas, v: &View, hero: &Hero, worst: State, inset: f32, gap: f32) {
    let t = v.theme;
    let r = hero.rect;
    c.fill_round(r, CARD_RADIUS, t.raised);
    match worst {
        State::Critical => {}
        State::Hot => c.stroke_round(r, CARD_RADIUS, 2.0, t.hot),
        _ => c.stroke_round(r, CARD_RADIUS, 1.0, t.hair),
    }
    for (i, &metric) in hero.metrics.iter().enumerate() {
        readout(c, v, metric, hero, i, tile_for(hero, i, inset, gap));
    }
    // Drawn last, so no tile covers it.
    if worst == State::Critical {
        c.stroke_round(r, CARD_RADIUS, 3.0, t.critical);
    }
}

// ---------- the chart ----------

/// Maps a series to points, newest at the right edge, each the mean of itself
/// and its neighbours. Each run between gaps becomes its own line, so a lost
/// sensor reads as a break.
fn smooth_runs(series: &Series, area: Rect, (low, high): (f32, f32)) -> Vec<Vec<(f32, f32)>> {
    let values: Vec<f32> = series.iter().collect();
    let count = values.len();
    let step = area.w / (CAPACITY - 1) as f32;
    let span = (high - low).max(f32::EPSILON);
    let mut runs = Vec::new();
    let mut run = Vec::new();
    for (i, &value) in values.iter().enumerate() {
        if value.is_nan() {
            if !run.is_empty() {
                runs.push(std::mem::take(&mut run));
            }
            continue;
        }
        let near = [i.checked_sub(1), Some(i), (i + 1 < count).then_some(i + 1)];
        let (sum, n) = near
            .iter()
            .flatten()
            .map(|&j| values[j])
            .filter(|v| !v.is_nan())
            .fold((0.0, 0.0), |(s, n), v| (s + v, n + 1.0));
        let level = ((sum / n - low) / span).clamp(0.0, 1.0);
        let x = area.right() - (count - 1 - i) as f32 * step;
        run.push((x, area.bottom() - level * area.h));
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}

struct Line {
    metric: MetricId,
    color: Color,
    state: State,
}

fn end_dot(c: &Canvas, t: &Theme, (x, y): (f32, f32), color: Color, ring: Option<Color>) {
    if let Some(ring) = ring {
        c.circle(x, y, 7.5, ring);
        c.circle(x, y, 5.5, t.raised);
    } else {
        c.circle(x, y, 4.5, t.raised);
    }
    c.circle(x, y, 3.5, color);
}

/// Labels level with their lines' ends, at least 14px apart. A label that had
/// to move gets a leader back to its line.
fn end_labels(
    c: &Canvas,
    items: &mut [(f32, f32, String, Color)],
    x: f32,
    (top, bottom): (f32, f32),
) {
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut placed: Vec<f32> = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let floor = if i == 0 { top } else { placed[i - 1] + 14.0 };
        placed.push(item.0.max(floor));
    }
    let over = placed.last().map_or(0.0, |y| (y - bottom).max(0.0));
    for y in &mut placed {
        *y -= over;
    }
    for i in (0..placed.len().saturating_sub(1)).rev() {
        placed[i] = placed[i].min(placed[i + 1] - 14.0);
    }
    for (item, &y) in items.iter().zip(&placed) {
        let (line_y, end_x, label, color) = (item.0, item.1, &item.2, item.3);
        if (y - line_y).abs() > 3.0 {
            let faint = color.with_alpha(0.6);
            c.line((end_x + 9.0, line_y), (x - 9.0, line_y), 1.0, faint);
            c.line((x - 9.0, line_y), (x - 3.0, y), 1.0, faint);
        }
        c.text(
            label,
            (x, y + 4.0),
            12.0,
            Weight::Semibold,
            Align::Left,
            color,
        );
    }
}

/// The temperatures over the last two minutes, on one fixed scale from 30° to
/// 5° past the limit. A large chart adds an axis, labels at the lines' ends,
/// and a mark from when the hotspot reached its limit.
fn paint_chart(c: &Canvas, v: &View, area: Rect, metrics: [MetricId; 3], large: bool) {
    let (t, m) = (v.theme, v.monitor);
    let mut lines: Vec<Line> = metrics
        .iter()
        .map(|&slot| resolve(slot, m))
        .filter(|&metric| metric.spec().unit == Unit::Celsius)
        .map(|metric| Line {
            metric,
            color: t.series(metric),
            state: state(metric, m),
        })
        .collect();
    if lines.is_empty() {
        return;
    }
    let critical = |id: MetricId| id.spec().limits.map_or(100.0, |l| l.critical);
    let limit = [MetricId::GpuHotspot, MetricId::GpuTemp]
        .into_iter()
        .find(|id| lines.iter().any(|l| l.metric == *id))
        .map_or_else(|| critical(lines[0].metric), critical);
    let (axis, ends) = if large { (40.0, 112.0) } else { (0.0, 0.0) };
    let label_size = if large { 12.0 } else { 11.0 };
    let plot = Rect::new(area.x + axis, area.y, area.w - axis - ends, area.h);
    let highest = lines
        .iter()
        .filter_map(|l| m.get(l.metric).series.bounds())
        .map(|(_, high)| high)
        .fold(0.0, f32::max);
    let range = (30.0, (limit + 5.0).max((highest / 5.0).ceil() * 5.0));
    let to_y = |value: f32| {
        let level = ((value - range.0) / (range.1 - range.0)).clamp(0.0, 1.0);
        plot.bottom() - level * plot.h
    };
    let grid_top = if large || plot.h >= 120.0 {
        range.1
    } else {
        90.0
    };
    let mut value = 40.0;
    while value < grid_top {
        rule(c, (plot.x, plot.right()), to_y(value), t.grid);
        if large {
            c.text(
                &format!("{value:.0}°"),
                (plot.x - 8.0, to_y(value) + 4.0),
                12.0,
                Weight::Regular,
                Align::Right,
                t.ink3,
            );
        }
        value += 20.0;
    }
    let limit_y = to_y(limit).floor() + 0.5;
    c.dashed(
        (plot.x, limit_y),
        (plot.right(), limit_y),
        1.0,
        t.limit_line,
        (4.0, 4.0),
    );

    // When the hotspot first reached its limit in these two minutes.
    let hotspot = &m.get(MetricId::GpuHotspot).series;
    let reached = (large && state(MetricId::GpuHotspot, m) == State::AtLimit)
        .then(|| hotspot.iter().position(|v| v >= limit - 0.5))
        .flatten();
    let step = plot.w / (CAPACITY - 1) as f32;
    let mark = reached.map(|i| {
        let x = plot.right() - (hotspot.len() - 1 - i) as f32 * step;
        (x, hotspot.len() - 1 - i)
    });
    if let Some((x, _)) = mark {
        if t.dark {
            c.fill_round(
                Rect::new(x, limit_y - 7.5, plot.right() - x, 3.0),
                1.5,
                t.hot,
            );
        } else {
            c.fill_round(
                Rect::new(x, limit_y - 0.5, plot.right() - x, 10.0),
                0.0,
                t.hot.with_alpha(0.12),
            );
        }
        let x = x.floor() + 0.5;
        c.dashed(
            (x, limit_y - 4.0),
            (x, plot.bottom() + 4.0),
            1.0,
            t.ink3,
            (2.0, 3.0),
        );
    }

    // The worst is drawn last, on top.
    lines.sort_by_key(|l| l.state);
    let runs: Vec<Vec<Vec<(f32, f32)>>> = lines
        .iter()
        .map(|l| smooth_runs(&m.get(l.metric).series, plot, range))
        .collect();
    for (line, runs) in lines.iter().zip(&runs) {
        for run in runs {
            c.polyline(run, 1.75, line.color);
        }
    }
    let ends_at: Vec<(usize, (f32, f32))> = runs
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.last().and_then(|run| run.last()).map(|&p| (i, p)))
        .collect();
    if large {
        let mut labels = Vec::new();
        for &(i, point) in &ends_at {
            let line = &lines[i];
            end_dot(c, t, point, line.color, t.ring(line.state));
            let value = m
                .get(line.metric)
                .value
                .map_or("–".into(), |v| format!("{v:.0}°"));
            labels.push((
                point.1,
                point.0,
                format!("{} {value}", short_name(line.metric)),
                line.color,
            ));
        }
        end_labels(
            c,
            &mut labels,
            plot.right() + 14.0,
            (plot.y + 4.0, plot.bottom() - 2.0),
        );
    } else {
        // Small dots, at least 6px apart.
        let mut dots: Vec<(usize, (f32, f32))> = ends_at.clone();
        dots.sort_by(|a, b| a.1.1.total_cmp(&b.1.1));
        for k in 1..dots.len() {
            if dots[k].1.1 - dots[k - 1].1.1 < 6.0 {
                dots[k].1.1 = dots[k - 1].1.1 + 6.0;
            }
        }
        dots.sort_by_key(|d| d.0);
        for (i, (x, y)) in dots {
            c.circle(x, y, 3.5, t.raised);
            c.circle(x, y, 2.5, lines[i].color);
        }
    }

    // The limit's name sits under the left end of its line, on a plate so no
    // line runs through it.
    let name = format!("{limit:.0}° limit");
    let name_w = c.measure(&name, label_size, Weight::Regular);
    c.fill_round(
        Rect::new(
            plot.x,
            limit_y + 13.0 - label_size * 0.8 - 2.0,
            name_w + 6.0,
            label_size + 4.0,
        ),
        3.0,
        t.raised,
    );
    c.text(
        &name,
        (plot.x + 3.0, limit_y + 13.0),
        label_size,
        Weight::Regular,
        Align::Left,
        t.ink3,
    );

    let times_y = plot.bottom() + 15.0;
    let left_w = c.text(
        "2 min ago",
        (plot.x, times_y),
        11.0,
        Weight::Regular,
        Align::Left,
        t.ink3,
    );
    let now_w = c.text(
        "now",
        (plot.right(), times_y),
        11.0,
        Weight::Regular,
        Align::Right,
        t.ink3,
    );
    if let Some((x, seconds)) = mark {
        let text = format!("reached limit {seconds} s ago");
        let half = c.measure(&text, 11.0, Weight::Regular) / 2.0;
        if x - half > plot.x + left_w + 12.0 && x + half < plot.right() - now_w - 12.0 {
            c.text(
                &text,
                (x, times_y),
                11.0,
                Weight::Regular,
                Align::Center,
                t.ink3,
            );
        }
    }
}

// ---------- the compact page ----------

fn paint_compact(c: &Canvas, v: &View, page: &Compact) -> Option<Rect> {
    let t = v.theme;
    let issues = issues(v.monitor);
    let worst = issues.first().map_or(State::Normal, |i| i.state);
    let hero = &page.hero;
    paint_hero(c, v, hero, worst, 8.0, 8.0);
    if let Some(area) = page.chart {
        paint_chart(c, v, area, hero.metrics, false);
    }
    let link = paint_summary(
        c,
        v,
        &issues,
        (hero.columns[0], page.summary_y),
        hero.rect.right() - 12.0,
    );

    let Some(card) = page.card else {
        return link;
    };
    c.fill_round(card, CARD_RADIUS, t.raised);
    let (left, right) = (hero.columns[0], card.right() - 12.0);
    for (i, &(section, y)) in page.sections.iter().enumerate() {
        if i > 0 {
            rule(c, (left, right), y, t.divider);
        }
        match section {
            Section::Stats => paint_stats(c, v, hero, y, right),
            Section::Details => paint_details(c, v, hero, y, (left, right)),
            Section::Io => paint_io(c, v, y, (left, right)),
        }
    }
    link
}

/// CPU, GPU and memory use, each on the hero's column above it.
fn paint_stats(c: &Canvas, v: &View, hero: &Hero, y: f32, right: f32) {
    let (t, m) = (v.theme, v.monitor);
    let value = |id: MetricId| m.get(id).value;
    for (i, &metric) in stats_metrics(hero.metrics).iter().enumerate() {
        let x = hero.columns[i];
        let w = if i < 2 { hero.pitch - 16.0 } else { right - x };
        let (number, unit) = reading_text(metric, value(metric));
        let power = |id: MetricId, limit: Option<f32>| {
            value(id).map(|p| match limit {
                Some(limit) => format!("{p:.0} / {limit:.0} W"),
                None => format!("{p:.0} W"),
            })
        };
        let (label, word, caption, fill) = match metric {
            MetricId::CpuLoad => (
                "CPU",
                Some("load"),
                power(MetricId::CpuPower, m.cpu_power_limit)
                    .or_else(|| value(MetricId::CpuClock).map(|g| format!("{g:.2} GHz"))),
                value(metric).map(|l| l / 100.0),
            ),
            MetricId::GpuLoad => (
                "GPU",
                Some("load"),
                power(MetricId::GpuPower, None),
                value(metric).map(|l| l / 100.0),
            ),
            MetricId::Memory => (
                "Memory",
                None,
                value(metric)
                    .zip(m.ram_total_gb)
                    .map(|(used, total)| format!("{:.0}% of {total:.0} GB", used / total * 100.0)),
                value(metric)
                    .zip(m.ram_total_gb)
                    .map(|(used, total)| used / total),
            ),
            other => (
                if other == MetricId::CpuClock {
                    "CPU"
                } else {
                    "GPU"
                },
                Some("clock"),
                m.get(other)
                    .peak
                    .map(|p| format!("Peak {}", other.spec().unit.format(p))),
                value(other)
                    .zip(m.get(other).peak)
                    .map(|(now, peak)| now / peak.max(0.01)),
            ),
        };
        c.text(
            label,
            (x, y + 22.0),
            12.0,
            Weight::Medium,
            Align::Left,
            t.ink2,
        );
        let word = word.filter(|_| {
            c.measure_in(crate::gfx::Face::Display, &number, 20.0, Weight::Semibold) + 50.0
                <= w + 8.0
        });
        figure(c, t, (x - 1.0, y + 46.0), &number, unit, 20.0, t.ink, word);
        let color = if metric == MetricId::CpuClock || metric == MetricId::GpuClock {
            t.neutral
        } else {
            t.series(metric)
        };
        meter(
            c,
            Rect::new(x, y + 54.0, w, 4.0),
            fill.unwrap_or(0.0),
            color,
            t.track,
        );
        if let Some(caption) = caption {
            let caption = c.fit(&caption, 12.0, Weight::Regular, w + 6.0);
            c.text(
                &caption,
                (x, y + 74.0),
                12.0,
                Weight::Regular,
                Align::Left,
                t.ink3,
            );
        }
        if i > 0 {
            let line_x = x.floor() - 7.5;
            c.line((line_x, y + 14.0), (line_x, y + 74.0), 1.0, t.divider);
        }
    }
}

/// One detail row: a name, an optional chip and bar, and the value at the right.
struct Detail {
    label: &'static str,
    value: String,
    unit: &'static str,
    state: State,
    /// How full the bar is, and its color, with whether it ends at a limit.
    bar: Option<(f32, Color, bool)>,
}

/// The detail rows worth showing on this PC right now, most telling first.
fn details(v: &View) -> Vec<Detail> {
    let (t, m) = (v.theme, v.monitor);
    let mut out = Vec::new();
    let vram = m.get(MetricId::VramTemp);
    if !m.absent.contains(MetricId::VramTemp) {
        let state = state(MetricId::VramTemp, m);
        out.push(Detail {
            label: "VRAM temp",
            value: vram.value.map_or("–".into(), |v| format!("{v:.0}°")),
            unit: "",
            state,
            bar: vram
                .value
                .map(|v| ((v - 20.0) / 86.0, t.level(state).unwrap_or(t.neutral), true)),
        });
    }
    if let Some(gap) = m.gap.view() {
        let wide = if gap.wide { " · wide" } else { "" };
        out.push(Detail {
            label: "Hotspot gap",
            value: format!("+{:.0}°{wide}", gap.degrees),
            unit: "",
            state: State::Normal,
            bar: None,
        });
    }
    if let (Some(used), Some(total)) = (m.get(MetricId::VideoMemory).value, m.vram_total_gb) {
        out.push(Detail {
            label: "VRAM used",
            value: format!("{used:.1} / {total:.0}"),
            unit: "GB",
            state: State::Normal,
            bar: None,
        });
    }
    let fan_share = m.get(MetricId::GpuFanPercent).value;
    if let Some(rpm) = m.get(MetricId::GpuFan).value {
        out.push(Detail {
            label: "GPU fan",
            value: format!("{rpm:.0}"),
            unit: "rpm",
            state: State::Normal,
            bar: fan_share.map(|f| (f / 100.0, t.neutral, false)),
        });
    } else if let Some(share) = fan_share {
        out.push(Detail {
            label: "GPU fan",
            value: format!("{share:.0}"),
            unit: "%",
            state: State::Normal,
            bar: Some((share / 100.0, t.neutral, false)),
        });
    }
    for (id, label) in [
        (MetricId::CpuClock, "CPU clock"),
        (MetricId::GpuPower, "GPU power"),
    ] {
        if let Some(value) = m.get(id).value {
            let (value, unit) = reading_text(id, Some(value));
            out.push(Detail {
                label,
                value,
                unit,
                state: State::Normal,
                bar: None,
            });
        }
    }
    out
}

fn paint_details(c: &Canvas, v: &View, hero: &Hero, y: f32, (left, right): (f32, f32)) {
    let t = v.theme;
    c.text(
        "Details",
        (left, y + 20.0),
        12.0,
        Weight::Semibold,
        Align::Left,
        t.ink2,
    );
    let bar_x = hero.columns[1] + 18.0;
    let bar_end = hero.columns[2] - 4.0;
    for (i, row) in details(v).iter().take(3).enumerate() {
        let mid = y + 42.0 + i as f32 * 23.0;
        let label_w = c.text_mid(
            row.label,
            (left, mid),
            12.0,
            Weight::Regular,
            Align::Left,
            t.ink2,
        );
        let mut start = bar_x;
        if let Some(kind) = chip_kind(row.state) {
            let w = chip(
                c,
                t,
                (left + label_w + 10.0, mid),
                row.state.word(),
                kind,
                Align::Left,
            );
            start = start.max(left + label_w + 10.0 + w + 6.0);
        }
        if let Some((fill, color, tick)) = row.bar {
            if bar_end - start >= 24.0 {
                let bar = Rect::new(start, mid - 2.0, bar_end - start, 4.0);
                meter(c, bar, fill, color, t.track);
                if tick {
                    limit_tick(c, t, bar.right(), bar);
                }
            }
        }
        figure_right(
            c,
            t,
            (right, mid + 5.0),
            &row.value,
            row.unit,
            14.0,
            t.digits(row.state),
        );
    }
}

/// Network and disk, two readings to a row, each marked by a word or arrow.
fn paint_io(c: &Canvas, v: &View, y: f32, (left, right): (f32, f32)) {
    let (t, m) = (v.theme, v.monitor);
    let rate = |id: MetricId| {
        m.get(id)
            .value
            .map_or("–".into(), |v| Unit::MegabytesPerSecond.format(v))
    };
    let row = |mid: f32, label: &str, parts: [(String, Option<bool>, &str); 2]| {
        c.text_mid(
            label,
            (left, mid),
            12.0,
            Weight::Regular,
            Align::Left,
            t.ink2,
        );
        let unit_w = c.text(
            "MB/s",
            (right, mid + 5.0),
            12.0,
            Weight::Regular,
            Align::Right,
            t.ink3,
        );
        let mut x = right - unit_w - 10.0;
        for (value, down, word) in parts.iter().rev() {
            x -= c.number(value, (x, mid + 5.0), 14.0, Align::Right, t.ink) + 4.0;
            match down {
                Some(down) => {
                    arrow(c, (x - 5.0, mid), *down, t.ink3);
                    x -= 22.0;
                }
                None => {
                    x -= c.text(
                        word,
                        (x, mid + 5.0),
                        12.0,
                        Weight::Regular,
                        Align::Right,
                        t.ink3,
                    ) + 12.0;
                }
            }
        }
    };
    row(
        y + 20.0,
        "Network",
        [
            (rate(MetricId::Download), Some(true), ""),
            (rate(MetricId::Upload), Some(false), ""),
        ],
    );
    row(
        y + 44.0,
        "Disk",
        [
            (rate(MetricId::DiskRead), None, "read"),
            (rate(MetricId::DiskWrite), None, "write"),
        ],
    );
}

// ---------- the large page ----------

fn paint_large(c: &Canvas, v: &View, page: &Large) {
    let (t, m) = (v.theme, v.monitor);
    let issues = issues(m);
    let worst = issues.first().map_or(State::Normal, |i| i.state);
    let hero = &page.hero;
    paint_hero(c, v, hero, worst, 10.0, 12.0);

    // The hotspot's gap from the edge, and how long it has held at its limit.
    if hero.metrics.contains(&MetricId::GpuHotspot) {
        // Level with the readings' names, which Simple mode moves down.
        let (x, top) = (page.side_x, hero.label_y - 30.0);
        c.text(
            "Hotspot gap",
            (x, top + 30.0),
            12.0,
            Weight::Medium,
            Align::Left,
            t.ink2,
        );
        match m.gap.view() {
            Some(gap) => {
                let w = c.number(
                    &format!("+{:.0}°", gap.degrees),
                    (x - 1.0, top + 58.0),
                    22.0,
                    Align::Left,
                    t.ink,
                );
                if gap.wide {
                    c.text(
                        "wide",
                        (x + w + 6.0, top + 58.0),
                        12.0,
                        Weight::Regular,
                        Align::Left,
                        t.ink3,
                    );
                }
            }
            None => {
                c.number("–", (x - 1.0, top + 58.0), 22.0, Align::Left, t.ink);
                c.text(
                    "Shown under load",
                    (x + 18.0, top + 58.0),
                    12.0,
                    Weight::Regular,
                    Align::Left,
                    t.ink3,
                );
            }
        }
        let share = m.gpu_limit.share().round();
        if share > 0.0 {
            c.text(
                "Time at hotspot limit",
                (x, top + 86.0),
                12.0,
                Weight::Medium,
                Align::Left,
                t.ink2,
            );
            figure(
                c,
                t,
                (x - 1.0, top + 114.0),
                &format!("{share:.0}"),
                "%",
                22.0,
                t.ink,
                Some("of 2 min"),
            );
        }
    }
    if let Some(area) = page.chart {
        paint_chart(c, v, area, hero.metrics, true);
    }
    for &(metric, r) in &page.tiles {
        paint_tile(c, v, metric, r);
    }
    paint_attention(c, v, &issues, worst, page.attention);
    paint_gpu(c, v, page.gpu);
    paint_cpu(c, v, page.cpu);
}

/// The scale a tile's history is drawn on, and the limit marked on it.
fn tile_scale(metric: MetricId, m: &Monitor) -> ((f32, f32), Option<f32>) {
    let bounds = m.get(metric).series.bounds();
    match metric {
        MetricId::CpuLoad | MetricId::GpuLoad | MetricId::GpuFanPercent => ((0.0, 100.0), None),
        MetricId::CpuPower if m.cpu_power_limit.is_some() => {
            let limit = m.cpu_power_limit.unwrap_or(0.0);
            ((0.0, limit * 1.08), Some(limit))
        }
        MetricId::Memory if m.ram_total_gb.is_some() => {
            ((0.0, m.ram_total_gb.unwrap_or(1.0)), None)
        }
        MetricId::VideoMemory if m.vram_total_gb.is_some() => {
            ((0.0, m.vram_total_gb.unwrap_or(1.0)), None)
        }
        _ => (domain(metric.spec().scale, bounds), None),
    }
}

/// The note at a tile's top right, with a shorter wording for a narrow tile.
fn aside(metric: MetricId, m: &Monitor) -> Option<(String, String)> {
    let peak = m.get(metric).peak;
    let pair = |word: &str, value: String| (format!("{word} {value}"), value);
    match metric {
        MetricId::Memory => m.ram_total_gb.map(|t| pair("of", format!("{t:.0} GB"))),
        MetricId::VideoMemory => m.vram_total_gb.map(|t| pair("of", format!("{t:.0} GB"))),
        MetricId::CpuPower if m.cpu_power_limit.is_some() => m
            .cpu_power_limit
            .map(|l| pair("limit", format!("{l:.0} W"))),
        _ => peak.map(|p| pair("peak", with_unit(metric, p))),
    }
}

/// One of the six readings beside the large chart, with its history.
fn paint_tile(c: &Canvas, v: &View, metric: MetricId, r: Rect) {
    let (t, m) = (v.theme, v.monitor);
    c.fill_round(r, CARD_RADIUS, t.raised);
    let name_w = c.text(
        short_or_label(metric),
        (r.x + INSET, r.y + 22.0),
        12.0,
        Weight::Medium,
        Align::Left,
        t.ink2,
    );
    let room = r.w - INSET * 2.0 - name_w - 10.0;
    let aside = aside(metric, m).and_then(|(long, short)| {
        [long, short]
            .into_iter()
            .find(|text| c.measure(text, 12.0, Weight::Regular) <= room)
    });
    if let Some(aside) = aside {
        c.text(
            &aside,
            (r.right() - INSET, r.y + 22.0),
            12.0,
            Weight::Regular,
            Align::Right,
            t.ink3,
        );
    }
    let reading = m.get(metric);
    let (value, unit) = reading_text(metric, reading.value);
    if v.simple {
        let y = r.y + 22.0 + (r.h - 22.0) / 2.0 + 12.0;
        figure(
            c,
            t,
            (r.x + INSET - 1.0, y),
            &value,
            unit,
            34.0,
            t.ink,
            None,
        );
        return;
    }
    figure(
        c,
        t,
        (r.x + INSET - 1.0, r.y + 50.0),
        &value,
        unit,
        22.0,
        t.ink,
        None,
    );
    let area = Rect::new(r.x + INSET, r.y + 62.0, r.w - INSET * 2.0, r.h - 76.0);
    let (range, limit) = tile_scale(metric, m);
    let rate = metric.spec().unit == Unit::MegabytesPerSecond;
    spark(
        c,
        t,
        &reading.series,
        area,
        t.series(metric),
        range,
        limit,
        !rate,
    );
}

/// A tile's name. VRAM is plain "VRAM" here, as its temperature is elsewhere.
fn short_or_label(metric: MetricId) -> &'static str {
    match metric {
        MetricId::VideoMemory => "VRAM",
        other => other.spec().label,
    }
}

/// A history line with a fading fill beneath it.
#[allow(clippy::too_many_arguments)]
fn spark(
    c: &Canvas,
    t: &Theme,
    series: &Series,
    area: Rect,
    color: Color,
    range: (f32, f32),
    limit: Option<f32>,
    smooth: bool,
) {
    if area.w < 24.0 || area.h < 8.0 {
        return;
    }
    let runs = if smooth {
        smooth_runs(series, area, range)
    } else {
        spark_runs(series, area, range)
    };
    for run in &runs {
        c.fade(run, area.bottom(), color, 0.14, 30.0);
    }
    if let Some(limit) = limit {
        let level = ((limit - range.0) / (range.1 - range.0).max(f32::EPSILON)).clamp(0.0, 1.0);
        let y = (area.bottom() - level * area.h).floor() + 0.5;
        c.dashed(
            (area.x, y),
            (area.right(), y),
            1.0,
            t.limit_line,
            (3.0, 3.0),
        );
    }
    for run in &runs {
        c.polyline(run, 1.6, color);
    }
}

fn card_title(c: &Canvas, t: &Theme, text: &str, (x, y): (f32, f32)) -> f32 {
    c.text_in(
        crate::gfx::Face::Display,
        text,
        (x, y),
        14.0,
        Weight::Semibold,
        Align::Left,
        t.ink,
    )
}

/// What needs attention, in plain words, or the session's peaks when nothing does.
fn paint_attention(c: &Canvas, v: &View, issues: &[Issue], worst: State, r: Rect) {
    let (t, m) = (v.theme, v.monitor);
    c.fill_round(r, CARD_RADIUS, t.raised);
    let (left, right) = (r.x + INSET, r.right() - INSET);
    let note = power_note(m).map(|note| wrap(c, &note, 12.0, right - left));
    let note_h = note.as_ref().map_or(0.0, |lines| lines.len() as f32 * 16.0);
    if !issues.is_empty() {
        let w = card_title(c, t, "Needs attention", (left, r.y + 30.0));
        let count = issues.len().to_string();
        if worst >= State::Hot {
            let kind = if worst == State::Critical {
                ChipKind::Critical
            } else {
                ChipKind::Hot
            };
            chip(
                c,
                t,
                (left + w + 8.0, r.y + 25.0),
                &count,
                kind,
                Align::Left,
            );
        } else {
            c.text_in(
                crate::gfx::Face::Display,
                &count,
                (left + w + 8.0, r.y + 30.0),
                14.0,
                Weight::Semibold,
                Align::Left,
                t.ink3,
            );
        }
        let floor = r.bottom() - 14.0 - if note_h > 0.0 { note_h + 6.0 } else { 0.0 };
        let mut y = r.y + 64.0;
        for issue in issues {
            let lines = wrap(c, &issue.detail, 12.0, right - left - 16.0);
            if y + lines.len() as f32 * 16.0 + 2.0 > floor {
                break;
            }
            c.circle(
                left + 4.0,
                y - 4.0,
                4.0,
                t.level(issue.state).unwrap_or(t.ink3),
            );
            let title = c.fit(&issue.title, 12.0, Weight::Semibold, right - left - 16.0);
            c.text(
                &title,
                (left + 16.0, y),
                12.0,
                Weight::Semibold,
                Align::Left,
                t.ink,
            );
            let mut line_y = y + 18.0;
            for line in &lines {
                c.text(
                    line,
                    (left + 16.0, line_y),
                    12.0,
                    Weight::Regular,
                    Align::Left,
                    t.ink2,
                );
                line_y += 16.0;
            }
            y = line_y + 12.0;
        }
    } else {
        card_title(c, t, "Status", (left, r.y + 30.0));
        check(c, (left + 5.0, r.y + 58.0), t.ok);
        c.text(
            "All normal",
            (left + 18.0, r.y + 62.0),
            12.0,
            Weight::Semibold,
            Align::Left,
            t.ink,
        );
        c.text(
            "Nothing is warm or held at a limit.",
            (left + 16.0, r.y + 80.0),
            12.0,
            Weight::Regular,
            Align::Left,
            t.ink3,
        );
        c.text(
            "Peaks this session",
            (left, r.y + 114.0),
            12.0,
            Weight::Semibold,
            Align::Left,
            t.ink2,
        );
        let peaks = [
            (m.cpu_temp_metric(), "CPU"),
            (MetricId::GpuTemp, "GPU edge"),
            (MetricId::GpuHotspot, "Hotspot"),
            (MetricId::VramTemp, "VRAM"),
        ];
        let mut y = r.y + 142.0;
        for (id, name) in peaks {
            let Some(peak) = m.get(id).peak else { continue };
            if y + 14.0 > r.bottom() - 8.0 - note_h {
                break;
            }
            let limit = id.spec().limits.map_or(100.0, |l| l.critical);
            let w = c.text(name, (left, y), 12.0, Weight::Regular, Align::Left, t.ink2);
            c.text(
                &format!("  · limit {limit:.0}°"),
                (left + w, y),
                12.0,
                Weight::Regular,
                Align::Left,
                t.ink3,
            );
            figure_right(c, t, (right, y), &format!("{peak:.0}°"), "", 14.0, t.ink);
            let bar = Rect::new(left, y + 10.0, right - left, 4.0);
            meter(c, bar, (peak - 30.0) / (limit - 30.0), t.ink3, t.track);
            limit_tick(c, t, right, bar);
            y += 40.0;
        }
    }
    if let Some(lines) = note {
        let mut y = r.bottom() - 16.0 - (lines.len() as f32 - 1.0) * 16.0;
        for line in &lines {
            c.text(line, (left, y), 12.0, Weight::Regular, Align::Left, t.ink3);
            y += 16.0;
        }
    }
}

/// The GPU's own readings, each with a bar.
fn paint_gpu(c: &Canvas, v: &View, r: Rect) {
    let (t, m) = (v.theme, v.monitor);
    c.fill_round(r, CARD_RADIUS, t.raised);
    let (left, right) = (r.x + INSET, r.right() - INSET);
    let w = card_title(c, t, "GPU details", (left, r.y + 30.0));
    let name = v
        .specs
        .and_then(|s| s.part(PartKind::Gpu))
        .and_then(|p| p.items.first())
        .map(|i| i.name.as_str());
    if let Some(name) = name {
        let name = c.fit(name, 12.0, Weight::Regular, right - left - w - 16.0);
        c.text(
            &name,
            (right, r.y + 30.0),
            12.0,
            Weight::Regular,
            Align::Right,
            t.ink3,
        );
    }
    let value = |id: MetricId| m.get(id).value;
    let mut rows: Vec<Detail> = Vec::new();
    if !m.absent.contains(MetricId::VramTemp) {
        let state = state(MetricId::VramTemp, m);
        rows.push(Detail {
            label: "VRAM temp",
            value: value(MetricId::VramTemp).map_or("–".into(), |v| format!("{v:.0}°")),
            unit: "",
            state,
            bar: value(MetricId::VramTemp)
                .map(|v| ((v - 20.0) / 86.0, t.level(state).unwrap_or(t.neutral), true)),
        });
    }
    let share = value(MetricId::GpuFanPercent);
    if let Some(rpm) = value(MetricId::GpuFan) {
        rows.push(Detail {
            label: "Fan",
            value: format!("{rpm:.0}"),
            unit: "rpm",
            state: State::Normal,
            bar: share.map(|s| (s / 100.0, t.neutral, false)),
        });
    } else if let Some(share) = share {
        rows.push(Detail {
            label: "Fan",
            value: format!("{share:.0}"),
            unit: "%",
            state: State::Normal,
            bar: Some((share / 100.0, t.neutral, false)),
        });
    }
    // Power and clock have no known maximum to measure a bar against, and one
    // measured against this session's peak would look full at idle.
    if let Some(power) = value(MetricId::GpuPower) {
        rows.push(Detail {
            label: "Power",
            value: format!("{power:.0}"),
            unit: "W",
            state: State::Normal,
            bar: None,
        });
    }
    if let Some(clock) = value(MetricId::GpuClock) {
        rows.push(Detail {
            label: "Clock",
            value: format!("{clock:.2}"),
            unit: "GHz",
            state: State::Normal,
            bar: None,
        });
    }
    if let (Some(used), Some(total)) = (value(MetricId::VideoMemory), m.vram_total_gb) {
        rows.push(Detail {
            label: "VRAM used",
            value: format!("{used:.1}"),
            unit: "",
            state: State::Normal,
            bar: Some((used / total, t.gpu, false)),
        });
    }
    let count = rows.len();
    if count == 0 {
        return;
    }
    let pitch = if count > 1 {
        ((r.h - 66.0 - 26.0) / (count - 1) as f32).min(48.0)
    } else {
        0.0
    };
    for (i, row) in rows.iter().enumerate() {
        let y = r.y + 66.0 + i as f32 * pitch;
        let label_w = c.text(
            row.label,
            (left, y),
            12.0,
            Weight::Regular,
            Align::Left,
            t.ink2,
        );
        if let Some(kind) = chip_kind(row.state) {
            chip(
                c,
                t,
                (left + label_w + 8.0, y - 4.0),
                row.state.word(),
                kind,
                Align::Left,
            );
        }
        if row.label == "VRAM used" {
            let total = m.vram_total_gb.unwrap_or(0.0);
            figure_right(
                c,
                t,
                (right, y),
                &row.value,
                &format!("/ {total:.0} GB"),
                16.0,
                t.ink,
            );
        } else {
            figure_right(
                c,
                t,
                (right, y),
                &row.value,
                row.unit,
                16.0,
                t.digits(row.state),
            );
        }
        if let Some((fill, color, tick)) = row.bar {
            let bar = Rect::new(left, y + 11.0, right - left, 4.0);
            meter(c, bar, fill, color, t.track);
            if tick {
                limit_tick(c, t, right, bar);
            }
        }
    }
}

/// The CPU's threads, then network and disk.
fn paint_cpu(c: &Canvas, v: &View, r: Rect) {
    let (t, m) = (v.theme, v.monitor);
    c.fill_round(r, CARD_RADIUS, t.raised);
    let (left, right) = (r.x + INSET, r.right() - INSET);
    let w = card_title(c, t, "CPU", (left, r.y + 30.0));
    let mut facts = Vec::new();
    if let Some(clock) = m.get(MetricId::CpuClock).value {
        facts.push(format!("{clock:.2} GHz"));
    }
    if m.cpu_temp_metric() == MetricId::CpuTemp {
        if let Some(io) = m.get(MetricId::IoDieTemp).value {
            facts.push(format!("I/O die {io:.0}°"));
        }
    }
    if !facts.is_empty() {
        let text = c.fit(
            &facts.join(" · "),
            12.0,
            Weight::Regular,
            right - left - w - 16.0,
        );
        c.text(
            &text,
            (right, r.y + 30.0),
            12.0,
            Weight::Regular,
            Align::Right,
            t.ink3,
        );
    }
    let mut y = r.y + 52.0;
    let threads = &m.threads;
    if !v.simple && !threads.is_empty() {
        let count = threads.len() as f32;
        let gap = if count > 24.0 { 3.0 } else { 6.0 };
        let bar_w = ((right - left - gap * (count - 1.0)) / count).max(1.0);
        for (i, load) in threads.iter().enumerate() {
            let x = left + i as f32 * (bar_w + gap);
            c.fill_round(Rect::new(x, y, bar_w, 36.0), 3.0, t.track);
            let h = (36.0 * load / 100.0).max(3.0);
            c.fill_round(
                Rect::new(x, y + 36.0 - h, bar_w, h),
                3.0,
                t.cpu.with_alpha(0.75),
            );
        }
        c.text(
            &format!("Load on each of {} threads", threads.len()),
            (left, y + 56.0),
            12.0,
            Weight::Regular,
            Align::Left,
            t.ink3,
        );
        rule(c, (left, right), y + 72.0, t.grid);
        y += 98.0;
    } else {
        y += 20.0;
    }
    card_title(c, t, "Network and disk", (left, y));
    let half = (right - left - 16.0) / 2.0;
    let with_history = !v.simple && r.bottom() - (y + 28.0) >= 118.0;
    let pitch = if with_history { 72.0 } else { 26.0 };
    let items = [
        (MetricId::Download, "Download", 0.0, 0.0),
        (MetricId::Upload, "Upload", 1.0, 0.0),
        (MetricId::DiskRead, "Read", 0.0, 1.0),
        (MetricId::DiskWrite, "Write", 1.0, 1.0),
    ];
    for (id, label, column, row) in items {
        let x = left + column * (half + 16.0);
        let line_y = y + 28.0 + row * pitch;
        if line_y > r.bottom() - 10.0 {
            break;
        }
        c.text(
            label,
            (x, line_y),
            12.0,
            Weight::Regular,
            Align::Left,
            t.ink2,
        );
        let reading = m.get(id);
        let value = reading
            .value
            .map_or("–".into(), |v| Unit::MegabytesPerSecond.format(v));
        figure_right(c, t, (x + half, line_y), &value, "MB/s", 14.0, t.ink);
        if with_history {
            let range = domain(id.spec().scale, reading.series.bounds());
            spark(
                c,
                t,
                &reading.series,
                Rect::new(x, line_y + 10.0, half, 36.0),
                t.io,
                range,
                None,
                false,
            );
        }
    }
}

// ---------- a narrow window's rows ----------

/// One row in a tall, thin window: its name, its number, and its history.
fn paint_row(c: &Canvas, v: &View, cell: &Cell) {
    let (t, m) = (v.theme, v.monitor);
    let metric = resolve(cell.metric, m);
    let reading = m.get(metric);
    let state = state(metric, m);
    let critical = state == State::Critical;
    let r = cell.rect;
    c.fill_round(r, ROW_RADIUS, if critical { t.critical } else { t.raised });
    if state == State::Hot {
        c.stroke_round(r, ROW_RADIUS, 1.0, t.hot);
    }
    let on = |normal: Color| if critical { WHITE } else { normal };
    let (left, right) = (r.x + 10.0, r.right() - 10.0);
    let y = r.y + 15.0;
    c.circle(left + 3.0, y, 3.0, on(t.series(metric)));
    let label_w = c.text_mid(
        short_or_label(metric),
        (left + 11.0, y),
        12.0,
        Weight::Medium,
        Align::Left,
        on(t.ink2),
    );
    let size = (r.h * 0.36).clamp(15.0, 22.0);
    let (value, unit) = reading_text(metric, reading.value);
    let value_w = figure_right(
        c,
        t,
        (right, y + size * 0.35),
        &value,
        unit,
        size,
        on(t.digits(state)),
    );
    // The state's word, where it fits between the name and the number.
    let word = state.word();
    if !word.is_empty()
        && left + 11.0 + label_w + 8.0 + c.measure(word, 11.0, Weight::Semibold)
            < right - value_w - 6.0
    {
        let ink = if critical {
            WHITE
        } else {
            t.level(state).unwrap_or(t.ink3)
        };
        c.text_mid(
            word,
            (left + 11.0 + label_w + 8.0, y),
            11.0,
            Weight::Semibold,
            Align::Left,
            ink,
        );
    }
    let bottom = r.bottom() - 6.0;
    match state {
        State::Warm => c.fill_round(Rect::new(r.x + 6.0, bottom, r.w - 12.0, 2.0), 1.0, t.warm),
        State::AtLimit => c.dashed(
            (r.x + 6.0, bottom + 1.0),
            (r.right() - 6.0, bottom + 1.0),
            2.0,
            t.hot,
            (4.0, 2.0),
        ),
        _ => {}
    }
    let total = match metric {
        MetricId::Memory => m.ram_total_gb,
        MetricId::VideoMemory => m.vram_total_gb,
        _ => None,
    };
    if let Some(total) = total.filter(|t| *t > 0.0) {
        let bar = Rect::new(left, r.bottom() - 13.0, right - left, 4.0);
        meter(
            c,
            bar,
            reading.value.unwrap_or(0.0) / total,
            on(t.series(metric)),
            t.track,
        );
    } else if !v.simple {
        let top = r.y + 27.0;
        let area = Rect::new(left, top, right - left, r.bottom() - 10.0 - top);
        let (range, _) = tile_scale(metric, m);
        let color = on(t.series(metric));
        if area.w >= 24.0 && area.h >= 8.0 {
            for run in smooth_runs(&reading.series, area, range) {
                c.polyline(&run, 1.5, color);
            }
        }
    }
}

// ---------- the strip ----------

/// What a strip slot says: its name, its number and unit, and its state.
fn strip_entry(item: StripItem, monitor: &Monitor) -> (&'static str, String, &'static str, State) {
    let share = |metric: MetricId, total: Option<f32>| {
        let share = monitor
            .get(metric)
            .value
            .zip(total)
            .map(|(used, t)| used / t * 100.0);
        share.map_or("–".to_string(), |s| format!("{s:.0}"))
    };
    let degrees = |metric: MetricId| {
        let value = monitor.get(metric).value;
        (
            value.map_or("–".to_string(), |v| format!("{v:.0}°")),
            state(metric, monitor),
        )
    };
    let load = |metric: MetricId| {
        monitor
            .get(metric)
            .value
            .map_or("–".to_string(), |v| format!("{v:.0}"))
    };
    match item {
        // Without a temperature, the CPU's load is the next most telling number.
        StripItem::Cpu if monitor.absent.contains(MetricId::CpuTemp) => {
            ("CPU load", load(MetricId::CpuLoad), "%", State::Normal)
        }
        StripItem::Cpu => {
            let metric = monitor.cpu_temp_metric();
            let label = if metric == MetricId::CpuTemp {
                "CPU"
            } else {
                "I/O die"
            };
            let (text, state) = degrees(metric);
            (label, text, "", state)
        }
        StripItem::Gpu => {
            let (text, state) = degrees(MetricId::GpuTemp);
            ("GPU edge", text, "", state)
        }
        StripItem::Hotspot => {
            let (text, state) = degrees(MetricId::GpuHotspot);
            ("Hotspot", text, "", state)
        }
        StripItem::GpuLoad => ("GPU load", load(MetricId::GpuLoad), "%", State::Normal),
        StripItem::CpuLoad => ("CPU load", load(MetricId::CpuLoad), "%", State::Normal),
        StripItem::Ram => (
            "RAM",
            share(MetricId::Memory, monitor.ram_total_gb),
            "%",
            State::Normal,
        ),
        StripItem::Vram => (
            "VRAM",
            share(MetricId::VideoMemory, monitor.vram_total_gb),
            "%",
            State::Normal,
        ),
    }
}

/// The temperatures the strip has no slot for, which need saying when they
/// need attention: the worst of them, and how many there are.
fn hidden_alarm(plan: &StripPlan, monitor: &Monitor) -> Option<(MetricId, State, usize)> {
    let shown = |item: StripItem| plan.slots.iter().any(|(i, _)| *i == item);
    let mut hidden: Vec<(MetricId, State)> = Vec::new();
    if !shown(StripItem::Hotspot) && !monitor.absent.contains(MetricId::GpuHotspot) {
        hidden.push((MetricId::GpuHotspot, state(MetricId::GpuHotspot, monitor)));
    }
    if !monitor.absent.contains(MetricId::VramTemp) {
        hidden.push((MetricId::VramTemp, state(MetricId::VramTemp, monitor)));
    }
    hidden.retain(|(_, s)| *s != State::Normal);
    let count = hidden.len();
    hidden
        .into_iter()
        .max_by_key(|(_, s)| *s)
        .map(|(metric, state)| (metric, state, count))
}

/// One slot's treatment: an underline for Warm, a dashed one at a limit, an
/// outline for Hot, and solid red for Critical.
fn slot_state(c: &Canvas, t: &Theme, r: Rect, state: State) {
    let bottom = r.bottom();
    match state {
        State::Critical => c.fill_round(Rect::new(r.x, r.y + 3.0, r.w, r.h - 6.0), 6.0, t.critical),
        State::Hot => c.stroke_round(Rect::new(r.x, r.y + 3.0, r.w, r.h - 6.0), 6.0, 1.0, t.hot),
        State::AtLimit => c.dashed(
            (r.x + 6.0, bottom - 5.0),
            (r.right() - 6.0, bottom - 5.0),
            2.0,
            t.hot,
            (4.0, 2.0),
        ),
        State::Warm => c.fill_round(
            Rect::new(r.x + 6.0, bottom - 6.0, r.w - 12.0, 2.0),
            1.0,
            t.warm,
        ),
        State::Normal => {}
    }
}

fn paint_strip(c: &Canvas, v: &View) {
    let t = v.theme;
    c.clear(t.raised);
    if v.layout.strip_is_replaced() {
        paint_app_rows(c, v, true);
        return;
    }
    if let Some(plan) = &v.layout.strip {
        let size = plan.size;
        // The name and number are centred together in the slot.
        let block = 11.0 * 0.7 + 3.8 + size * 0.7;
        for &(item, r) in &plan.slots {
            let (label, value, unit, state) = strip_entry(item, v.monitor);
            slot_state(c, t, r, state);
            let solid = state == State::Critical;
            let top = r.y + (r.h - block) / 2.0;
            let room = r.w - 9.0;
            let label = c.fit(label, 11.0, Weight::Semibold, room);
            c.text(
                &label,
                (r.x + 7.0, top + 7.7),
                11.0,
                Weight::Semibold,
                Align::Left,
                if solid { WHITE } else { t.ink2 },
            );
            let ink = if solid { WHITE } else { t.digits(state) };
            figure(
                c,
                t,
                (r.x + 6.0, top + block),
                &value,
                unit,
                size,
                ink,
                None,
            );
        }
        if let Some((metric, state, count)) = hidden_alarm(plan, v.monitor) {
            let r = plan.alarm;
            let x = (r.x - 3.5).floor() + 0.5;
            c.line((x, r.y + 10.0), (x, r.bottom() - 10.0), 1.0, t.hair);
            let slot = Rect::new(r.x, r.y, r.w, r.h);
            match state {
                State::Critical | State::Hot => slot_state(c, t, slot, state),
                State::AtLimit => {
                    slot_state(c, t, Rect::new(r.x + 2.0, r.y, r.w - 4.0, r.h), state)
                }
                _ => c.fill_round(
                    Rect::new(r.x + 4.0, r.bottom() - 6.0, r.w - 8.0, 2.0),
                    1.0,
                    t.warm,
                ),
            }
            let solid = state == State::Critical;
            let value_size = (size * 0.7).round();
            let block = 11.0 * 0.7 + 3.8 + value_size * 0.7;
            let top = r.y + (r.h - block) / 2.0;
            let name = if count > 1 {
                format!("+{count}")
            } else {
                short_name(metric).to_string()
            };
            let name = c.fit(&name, 11.0, Weight::Semibold, r.w - 6.0);
            c.text(
                &name,
                (r.x + 5.0, top + 7.7),
                11.0,
                Weight::Semibold,
                Align::Left,
                if solid { WHITE } else { t.ink2 },
            );
            let value = v
                .monitor
                .get(metric)
                .value
                .map_or("–".into(), |v| format!("{v:.0}°"));
            let ink = if solid { WHITE } else { t.digits(state) };
            c.number(
                &value,
                (r.x + 4.0, top + block),
                value_size,
                Align::Left,
                ink,
            );
        }
    }

    paint_app_rows(c, v, true);

    // The strip has no title bar, so its controls appear only on hover.
    if v.hover.inside {
        let backdrop = v.layout.buttons.iter().fold(None::<Rect>, |area, (_, r)| {
            Some(area.map_or(*r, |a| Rect {
                x: a.x.min(r.x),
                w: a.w + r.w,
                ..a
            }))
        });
        if let Some(area) = backdrop {
            let full_height = Rect {
                h: v.layout.height,
                ..area
            };
            c.fill_round(full_height, 0.0, t.raised);
        }
        for &(button, rect) in &v.layout.buttons {
            paint_button(c, v, button, rect);
        }
    }
}

// ---------- busy apps ----------

fn paint_app_rows(c: &Canvas, v: &View, flat: bool) {
    for (i, &row) in v.layout.app_rows.iter().enumerate() {
        if let Some(app) = v.monitor.apps.get(i) {
            paint_app_row(c, v, app, i, row, flat);
        }
    }
}

const ICON_SIZE: f32 = 16.0;
const FROZEN_CPU_GAP: f32 = 12.0;
/// How far each of two stacked lines sits from a row's center. Equal offsets keep
/// the pair centered on the icon, with even space above and below.
const STACK_OFFSET: f32 = 5.5;

/// The status line of a frozen app in a narrow row: the longest wording that
/// fits. "Not responding" comes first in all of them, so it is never cut off.
fn frozen_status(cpu_percent: f32, room: f32, width: impl Fn(&str) -> f32) -> String {
    let long = format!("Not responding · {cpu_percent:.0}% CPU");
    let short = format!("Not responding · {cpu_percent:.0}%");
    [long, short]
        .into_iter()
        .find(|text| width(text) <= room)
        .unwrap_or_else(|| "Not responding".to_string())
}

/// Spells out a frozen app's status on two lines. Stacked rather than dropped, so
/// the status is never left to the color alone.
fn paint_frozen_lines(c: &Canvas, name: &str, cpu: f32, (x, y): (f32, f32), room: f32, ink: Color) {
    let name = c.fit(name, 11.0, Weight::Semibold, room);
    let status = frozen_status(cpu, room, |text| c.measure(text, 10.0, Weight::Regular));
    c.text_mid(
        &name,
        (x, y - STACK_OFFSET),
        11.0,
        Weight::Semibold,
        Align::Left,
        ink,
    );
    c.text_mid(
        &status,
        (x, y + STACK_OFFSET),
        10.0,
        Weight::Regular,
        Align::Left,
        ink,
    );
}

fn paint_app_icon(
    c: &Canvas,
    v: &View,
    (key, name): (&str, &str),
    icon: Option<&Arc<Icon>>,
    (x, y): (f32, f32),
) {
    let area = Rect::new(x, y - ICON_SIZE / 2.0, ICON_SIZE, ICON_SIZE);
    if let Some(icon) = icon {
        c.icon(key, icon, area);
        return;
    }
    c.fill_round(area, 3.0, v.theme.track);
    let letter: String = name.chars().take(1).collect();
    c.text_mid(
        &letter.to_uppercase(),
        (x + ICON_SIZE / 2.0, y),
        11.0,
        Weight::Semibold,
        Align::Center,
        v.theme.ink,
    );
}

/// Draws a small labelled button on a row.
fn paint_control(c: &Canvas, r: Rect, label: &str, fill: Color, ink: Color) {
    c.fill_round(r, 5.0, fill);
    c.text_mid(
        label,
        (r.x + r.w / 2.0, r.y + r.h / 2.0),
        11.0,
        Weight::Semibold,
        Align::Center,
        ink,
    );
}

fn paint_app_row(c: &Canvas, v: &View, app: &BusyApp, index: usize, r: Rect, flat: bool) {
    let t = v.theme;
    if v.confirm == Some(app.key.as_str()) {
        paint_confirm(c, t, &app.name, r);
        return;
    }
    let hovered = v.hover.app == Some(index);
    if app.frozen {
        c.fill_round(r, ROW_RADIUS, t.critical);
    } else if hovered {
        c.fill_round(r, ROW_RADIUS, t.track);
    } else if !flat {
        c.fill_round(r, ROW_RADIUS, t.raised);
    }

    let y = r.y + r.h / 2.0;
    let left = r.x + 10.0;
    paint_app_icon(c, v, (&app.key, &app.name), app.icon.as_ref(), (left, y));

    let mut right = r.right() - 8.0;
    let shows_end = app.can_end && (app.frozen || hovered);
    if shows_end {
        let button = end_button(r);
        if app.frozen {
            paint_control(c, button, "End", WHITE, t.critical);
        } else {
            paint_control(c, button, "End", t.critical, WHITE);
        }
        right = button.x - 6.0;
    }

    let name_x = left + ICON_SIZE + 8.0;
    let name_ink = if app.frozen { WHITE } else { t.ink };
    let name_room = right - name_x;
    let notice = v
        .notice
        .filter(|(key, _)| *key == app.key)
        .map(|(_, text)| text);

    if app.frozen {
        // CPU use tells an app that is hard at work from one that is truly
        // stuck, which Windows can't do, and they call for opposite decisions.
        let full = format!("{} not responding", app.name);
        let cpu = format!("{:.0}% CPU", app.cpu_percent);
        let needed = c.measure(&full, 12.0, Weight::Semibold)
            + FROZEN_CPU_GAP
            + c.measure(&cpu, 11.0, Weight::Regular);
        if needed <= name_room {
            c.text_mid(
                &full,
                (name_x, y),
                12.0,
                Weight::Semibold,
                Align::Left,
                name_ink,
            );
            c.text_mid(
                &cpu,
                (right - 2.0, y),
                11.0,
                Weight::Regular,
                Align::Right,
                name_ink,
            );
        } else {
            paint_frozen_lines(
                c,
                &app.name,
                app.cpu_percent,
                (name_x, y),
                name_room,
                name_ink,
            );
        }
        return;
    }

    let name_w = c.measure(&app.name, 12.0, Weight::Semibold).min(name_room);
    let name = c.fit(&app.name, 12.0, Weight::Semibold, name_room);
    c.text_mid(
        &name,
        (name_x, y),
        12.0,
        Weight::Semibold,
        Align::Left,
        name_ink,
    );

    // Name as many of its readings as fit, biggest first.
    let room = right - name_x - name_w - 12.0;
    let mut detail = notice.map_or(String::new(), str::to_string);
    if notice.is_none() {
        for label in &app.labels {
            let next = if detail.is_empty() {
                label.clone()
            } else {
                format!("{detail}  ·  {label}")
            };
            if c.measure(&next, 11.0, Weight::Regular) > room {
                break;
            }
            detail = next;
        }
    }
    if !detail.is_empty() {
        let text = c.fit(&detail, 11.0, Weight::Regular, room);
        c.text_mid(
            &text,
            (right - 2.0, y),
            11.0,
            Weight::Regular,
            Align::Right,
            t.ink2,
        );
    }
}

/// Asks once more, in place, before ending an app.
fn paint_confirm(c: &Canvas, t: &Theme, name: &str, r: Rect) {
    c.fill_round(r, ROW_RADIUS, t.critical);
    let (end, keep) = confirm_buttons(r);
    let room = end.x - r.x - 16.0;
    let asks = [
        format!("End {name}? Unsaved work will be lost."),
        format!("End {name}? Unsaved work is lost."),
        format!("End {name}?"),
    ];
    let question = asks
        .iter()
        .find(|text| c.measure(text, 12.0, Weight::Semibold) <= room)
        .cloned()
        .unwrap_or_else(|| c.fit(&asks[2], 12.0, Weight::Semibold, room));
    c.text_mid(
        &question,
        (r.x + 10.0, r.y + r.h / 2.0),
        12.0,
        Weight::Semibold,
        Align::Left,
        WHITE,
    );
    paint_control(c, end, "End", WHITE, t.critical);
    paint_control(c, keep, "Keep", WHITE.with_alpha(0.22), WHITE);
}

// ---------- the title bar ----------

/// The tabs, as a segmented control: a well, with the open page on a pill.
fn paint_tabs(c: &Canvas, v: &View) {
    let t = v.theme;
    let tabs = &v.layout.tabs;
    let (Some(first), Some(last)) = (tabs.first(), tabs.last()) else {
        return;
    };
    let y = ((TITLE_H - 26.0) / 2.0).round();
    let well = Rect::new(first.1.x - 2.0, y, last.1.right() - first.1.x + 4.0, 26.0);
    c.fill_round(well, 9.0, t.well);
    for &(page, r) in tabs {
        let label = match page {
            Page::Vitals => "Vitals",
            Page::Apps => "Apps",
            Page::Specs => "Specs",
        };
        let selected = page == v.layout.page;
        if selected {
            let pill = Rect::new(r.x, y + 2.0, r.w, 22.0);
            c.fill_round(pill, 7.0, t.pill);
            if !t.dark {
                c.stroke_round(pill, 7.0, 1.0, t.hair);
            }
        }
        let ink = if selected {
            t.ink
        } else if v.hover.tab == Some(page) {
            t.ink2
        } else {
            t.tab
        };
        c.text_mid(
            label,
            (r.x + r.w / 2.0, y + 13.0),
            12.0,
            Weight::Semibold,
            Align::Center,
            ink,
        );
    }
}

/// The tabs and window buttons. A warning goes to the taskbar instead, through
/// the window's title, and the page already shows it, so the tabs always stay.
fn paint_title_bar(c: &Canvas, v: &View) {
    paint_tabs(c, v);
    for &(button, rect) in &v.layout.buttons {
        paint_button(c, v, button, rect);
    }
}

fn paint_button(c: &Canvas, v: &View, button: Button, r: Rect) {
    let t = v.theme;
    let hovered = v.hover.button == Some(button);
    if hovered {
        let fill = if button == Button::Close {
            t.critical
        } else {
            t.track
        };
        c.fill_round(r, 0.0, fill);
    }
    let color = if hovered && button == Button::Close {
        WHITE
    } else {
        t.ink2
    };
    let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    const S: f32 = 4.5;
    match button {
        Button::Close => {
            c.line((x - S, y - S), (x + S, y + S), 1.2, color);
            c.line((x + S, y - S), (x - S, y + S), 1.2, color);
        }
        Button::Maximize => c.outline_rect(Rect::new(x - S, y - S, S * 2.0, S * 2.0), 1.1, color),
        Button::Minimize => c.line((x - S, y), (x + S, y), 1.2, color),
        Button::Menu => {
            let mark = if v.menu_open { t.accent } else { color };
            c.outline_rect(Rect::new(x - 5.5, y - 4.5, 11.0, 9.0), 1.1, mark);
            c.outline_rect(Rect::new(x - 3.0, y - 1.0, 3.5, 3.0), 1.1, mark);
        }
        Button::Pin => {
            let mark = if v.pinned { t.accent } else { color };
            if v.pinned {
                c.circle(x, y - 2.0, 3.2, mark);
            } else {
                c.outline_circle(x, y - 2.0, 2.8, 1.2, mark);
            }
            c.line((x, y + 1.0), (x, y + 6.0), 1.2, mark);
        }
    }
}

// ---------- history helpers ----------

/// The value range a sparkline should span, given the values in its series.
pub fn domain(scale: Scale, bounds: Option<(f32, f32)>) -> (f32, f32) {
    match scale {
        Scale::Fixed(low, high) => (low, high),
        Scale::Auto { min_span } => {
            let (low, high) = bounds.unwrap_or((0.0, min_span));
            let span = (high - low).max(min_span);
            let middle = (low + high) / 2.0;
            (middle - span / 2.0, middle + span / 2.0)
        }
        Scale::FromZero { min_top } => (0.0, bounds.map_or(min_top, |b| b.1).max(min_top) * 1.1),
    }
}

/// Maps a series to points, newest at the right edge. Each run between gaps
/// becomes its own line, so a lost sensor reads as a break.
fn spark_runs(series: &Series, area: Rect, range: (f32, f32)) -> Vec<Vec<(f32, f32)>> {
    let step = area.w / (CAPACITY - 1) as f32;
    let count = series.len();
    let span = (range.1 - range.0).max(f32::EPSILON);
    let mut runs = Vec::new();
    let mut run = Vec::new();
    for (i, value) in series.iter().enumerate() {
        if value.is_nan() {
            if !run.is_empty() {
                runs.push(std::mem::take(&mut run));
            }
            continue;
        }
        let x = area.right() - (count - 1 - i) as f32 * step;
        let level = ((value - range.0) / span).clamp(0.0, 1.0);
        run.push((x, area.bottom() - level * area.h));
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}

// ---------- the Apps page ----------

/// The key numbers above the Apps table, which is all of the sensors it leaves
/// room for. It shows as many as fit, in the strip's order.
fn paint_summary_row(c: &Canvas, v: &View, area: Rect) {
    use StripItem::*;
    const LABEL: f32 = 11.0;
    const VALUE: f32 = 13.0;
    const ITEM_GAP: f32 = 14.0;
    let t = v.theme;
    let y = area.y + area.h / 2.0;
    let mut x = area.x + TABLE_ROW_PAD;
    let items = [Cpu, Gpu, Hotspot, Ram, Vram]
        .into_iter()
        .filter(|i| *i != Hotspot || !v.monitor.absent.contains(MetricId::GpuHotspot));
    for item in items {
        let (label, value, unit, state) = strip_entry(item, v.monitor);
        let value = format!("{value}{unit}");
        let dot = if state == State::Normal { 0.0 } else { 13.0 };
        let width = c.measure(label, LABEL, Weight::Medium)
            + 4.0
            + c.measure(&value, VALUE, Weight::Semibold)
            + dot;
        if x + width > area.right() {
            break;
        }
        x += c.text_mid(label, (x, y), LABEL, Weight::Medium, Align::Left, t.ink3);
        x += 4.0;
        x += c.text_mid(
            &value,
            (x, y),
            VALUE,
            Weight::Semibold,
            Align::Left,
            t.digits(state),
        );
        if let Some(color) = t.level(state) {
            c.circle(x + 7.0, y, 3.5, color);
            x += dot;
        }
        x += ITEM_GAP;
    }
}

fn paint_heading(c: &Canvas, v: &View, key: SortKey, (x, y): (f32, f32), align: Align) {
    let sorted = v.sort.key == key;
    let ink = if sorted {
        v.theme.ink
    } else if v.hover.heading == Some(key) {
        v.theme.ink2
    } else {
        v.theme.ink3
    };
    let width = c.text_mid(key.label(), (x, y), 11.0, Weight::Semibold, align, ink);
    if sorted {
        let arrow_x = match align {
            Align::Right => x - width - 7.0,
            _ => x + width + 7.0,
        };
        let tip = if v.sort.descending { 1.5 } else { -1.5 };
        c.polyline(
            &[
                (arrow_x - 3.0, y - tip),
                (arrow_x, y + tip),
                (arrow_x + 3.0, y - tip),
            ],
            1.2,
            ink,
        );
    }
}

/// A magnifying glass: a ring and its handle, centred on `(x, y)`.
fn magnifier(c: &Canvas, (x, y): (f32, f32), ink: Color) {
    c.outline_circle(x - 1.0, y - 1.0, 4.5, 1.4, ink);
    c.line((x + 2.3, y + 2.3), (x + 5.0, y + 5.0), 1.6, ink);
}

/// The search's text, between `left` and `right`, with its selection and
/// caret. Text too long for the field scrolls to keep the caret in view,
/// showing the end where it can. Returns each caret position on screen and its x.
fn paint_search_text(c: &Canvas, v: &View, (left, right): (f32, f32), y: f32) -> Vec<(usize, f32)> {
    let t = v.theme;
    let chars: Vec<char> = v.search.chars().collect();
    let count = chars.len();
    let width = |from: usize, to: usize| {
        let part: String = chars[from..to].iter().collect();
        c.measure(&part, 12.0, Weight::Regular)
    };
    let room = right - left;
    let caret = v.search_caret.min(count);
    let first = if width(0, count) <= room {
        0
    } else {
        let tail = (0..count)
            .find(|&from| width(from, count) <= room)
            .unwrap_or(count);
        let to_caret = (0..=caret)
            .find(|&from| width(from, caret) <= room)
            .unwrap_or(caret);
        tail.min(to_caret)
    };
    let mut last = count;
    while last > first && width(first, last) > room {
        last -= 1;
    }
    let marks: Vec<(usize, f32)> = (first..=last)
        .map(|i| (i, left + width(first, i)))
        .collect();
    let x_of = |i: usize| left + width(first, i.clamp(first, last));
    if let Some((from, to)) = v.search_selection {
        let (a, b) = (x_of(from), x_of(to));
        if b > a {
            let tint = t.accent.with_alpha(if t.dark { 0.38 } else { 0.24 });
            c.fill_round(Rect::new(a, y - 9.0, b - a, 18.0), 2.0, tint);
        }
    }
    let shown: String = chars[first..last].iter().collect();
    c.text_mid(&shown, (left, y), 12.0, Weight::Regular, Align::Left, t.ink);
    if v.search_focused && v.search_selection.is_none() && (first..=last).contains(&caret) {
        let x = x_of(caret).round() + 0.5;
        c.line((x, y - 7.0), (x, y + 7.0), 1.0, t.accent);
    }
    marks
}

/// The Apps tab's search field. Before a search it says what it is for and
/// how to reach it from the keyboard. During one it shows the text, a caret,
/// how many apps match and a button that clears it. Returns each caret
/// position on screen and its x.
fn paint_search(c: &Canvas, v: &View, table: &Table) -> Vec<(usize, f32)> {
    let t = v.theme;
    let r = table.search;
    let y = r.y + r.h / 2.0;
    c.fill_round(r, 8.0, t.raised);
    if v.search_active {
        c.stroke_round(r, 8.0, 1.5, t.accent);
    } else {
        c.stroke_round(r, 8.0, 1.0, if v.hover.search { t.ink3 } else { t.hair });
    }
    let text_x = r.x + 28.0;
    let typed = !v.search.is_empty();
    magnifier(c, (r.x + 14.0, y), if typed { t.ink2 } else { t.ink3 });
    let mut right = r.right() - 8.0;
    if typed {
        let clear = table.search_clear();
        let (cx, cy) = (clear.x + clear.w / 2.0, y);
        c.circle(
            cx,
            cy,
            8.0,
            if v.hover.search_clear {
                t.ink3.with_alpha(0.45)
            } else {
                t.track
            },
        );
        c.line((cx - 3.0, cy - 3.0), (cx + 3.0, cy + 3.0), 1.3, t.ink2);
        c.line((cx + 3.0, cy - 3.0), (cx - 3.0, cy + 3.0), 1.3, t.ink2);
        let count = format!("{} of {}", v.monitor.table.len(), v.apps_total);
        let count_w = c.measure(&count, 11.0, Weight::Regular);
        right = clear.x - 2.0;
        if right - count_w - 12.0 > text_x + 40.0 {
            c.text_mid(
                &count,
                (right, y),
                11.0,
                Weight::Regular,
                Align::Right,
                t.ink3,
            );
            right -= count_w + 12.0;
        }
        return paint_search_text(c, v, (text_x, right - 4.0), y);
    }
    if v.search_focused {
        let x = text_x.round() - 0.5;
        c.line((x, y - 7.0), (x, y + 7.0), 1.0, t.accent);
    }
    let hint_w = c.measure("Ctrl+F", 10.0, Weight::Semibold) + 10.0;
    let placeholder_w = c.text_mid(
        "Search apps",
        (text_x, y),
        12.0,
        Weight::Regular,
        Align::Left,
        t.ink3,
    );
    if !v.search_active && text_x + placeholder_w + 12.0 < right - hint_w {
        let key = Rect::new(right - hint_w, y - 8.0, hint_w, 16.0);
        c.stroke_round(key, 4.0, 1.0, t.hair);
        c.text_mid(
            "Ctrl+F",
            (key.x + key.w / 2.0, y),
            10.0,
            Weight::Semibold,
            Align::Center,
            t.ink3,
        );
    }
    vec![(0, text_x)]
}

fn paint_table(c: &Canvas, v: &View, table: &Table) -> Vec<(usize, f32)> {
    if let Some(area) = table.summary {
        paint_summary_row(c, v, area);
    }
    let marks = paint_search(c, v, table);
    let header = table.header;
    let y = header.y + header.h / 2.0;
    paint_heading(
        c,
        v,
        SortKey::Name,
        (header.x + TABLE_ROW_PAD, y),
        Align::Left,
    );
    for &(key, right) in &table.columns {
        paint_heading(c, v, key, (right, y), Align::Right);
    }
    rule(
        c,
        (header.x, header.right()),
        header.bottom() - 1.0,
        v.theme.track,
    );

    let rows = &v.monitor.table;
    if rows.is_empty() && !v.search.is_empty() && v.apps_total > 0 {
        let middle = table.body.x + table.body.w / 2.0;
        let query = c.fit(v.search, 12.0, Weight::Semibold, table.body.w - 140.0);
        let message = c.fit(
            &format!("No apps match \u{201c}{query}\u{201d}"),
            12.0,
            Weight::Semibold,
            table.body.w - 16.0,
        );
        c.text_mid(
            &message,
            (middle, table.body.y + 36.0),
            12.0,
            Weight::Semibold,
            Align::Center,
            v.theme.ink2,
        );
        c.text_mid(
            "Esc clears the search",
            (middle, table.body.y + 56.0),
            11.0,
            Weight::Regular,
            Align::Center,
            v.theme.ink3,
        );
        return marks;
    }
    if rows.is_empty() {
        c.text_mid(
            "Reading apps",
            (table.body.x + table.body.w / 2.0, table.body.y + 20.0),
            12.0,
            Weight::Regular,
            Align::Center,
            v.theme.ink3,
        );
        return marks;
    }
    for (slot, row) in rows.iter().skip(v.scroll).take(table.visible).enumerate() {
        paint_table_row(c, v, table, row, v.scroll + slot, table.row(slot));
    }

    // A thin mark shows where the view is in the list. The wheel moves it.
    if rows.len() > table.visible {
        let body = table.body;
        let share = table.visible as f32 / rows.len() as f32;
        let height = (body.h * share).max(20.0);
        let span = (rows.len() - table.visible) as f32;
        let top = body.y + (body.h - height) * (v.scroll as f32 / span);
        c.fill_round(
            Rect::new(body.right() + 2.0, top, 3.0, height),
            1.5,
            v.theme.track,
        );
    }
    marks
}

fn paint_table_row(c: &Canvas, v: &View, table: &Table, row: &AppRow, index: usize, r: Rect) {
    let t = v.theme;
    if v.confirm == Some(row.key.as_str()) {
        paint_confirm(c, t, &row.name, r);
        return;
    }
    let hovered = v.hover.row == Some(index);
    if row.frozen {
        c.fill_round(r, 6.0, t.critical);
    } else if hovered {
        c.fill_round(r, 6.0, t.raised);
    }
    let ink = if row.frozen { WHITE } else { t.ink };
    let faded = if row.frozen { WHITE } else { t.ink3 };

    let y = r.y + r.h / 2.0;
    let left = r.x + TABLE_ROW_PAD;
    paint_app_icon(c, v, (&row.key, &row.name), row.icon.as_ref(), (left, y));

    let shows_end = row.can_end && (row.frozen || hovered);
    let button = end_button(r);
    let cells_start = if shows_end && table.columns.len() == 1 {
        button.x
    } else {
        table.names_end()
    };
    let name_x = left + ICON_SIZE + 6.0;
    let room = cells_start - name_x - 6.0;

    if row.frozen {
        let full = format!("{} not responding", row.name);
        if c.measure(&full, 12.0, Weight::Semibold) <= room {
            c.text_mid(&full, (name_x, y), 12.0, Weight::Semibold, Align::Left, ink);
        } else {
            paint_frozen_lines(c, &row.name, row.usage.cpu, (name_x, y), room, ink);
        }
    } else {
        let count = row.processes.len();
        let suffix = (!v.simple && count > 1).then(|| count.to_string());
        let suffix_w = suffix
            .as_deref()
            .map_or(0.0, |s| c.measure(s, 10.0, Weight::Regular) + 5.0);
        let name = c.fit(&row.name, 12.0, Weight::Regular, room - suffix_w);
        if let Some((start, end)) = find(&name, v.search.trim()) {
            let width = |chars: usize| {
                let part: String = name.chars().take(chars).collect();
                c.measure(&part, 12.0, Weight::Regular)
            };
            let (left, right) = (width(start), width(end));
            let tint = if row.frozen {
                WHITE.with_alpha(0.28)
            } else {
                t.accent.with_alpha(if t.dark { 0.24 } else { 0.16 })
            };
            c.fill_round(
                Rect::new(name_x + left - 1.0, y - 8.0, right - left + 2.0, 16.0),
                3.0,
                tint,
            );
        }
        let name_w = c.text_mid(&name, (name_x, y), 12.0, Weight::Regular, Align::Left, ink);
        if let Some(suffix) = suffix {
            c.text_mid(
                &suffix,
                (name_x + name_w + 5.0, y),
                10.0,
                Weight::Regular,
                Align::Left,
                faded,
            );
        }
    }

    let notice = v
        .notice
        .filter(|(key, _)| *key == row.key)
        .map(|(_, text)| text);
    let end_edge = if shows_end { button.x } else { r.right() };
    if let Some(text) = notice {
        c.text_mid(
            text,
            (end_edge - 4.0, y),
            11.0,
            Weight::Regular,
            Align::Right,
            ink,
        );
    } else {
        for &(key, right) in &table.columns {
            if right > end_edge {
                continue;
            }
            let weight = if v.sort.key == key {
                Weight::Semibold
            } else {
                Weight::Regular
            };
            let color = if row.is_idle(key) { faded } else { ink };
            c.text_mid(
                &row.cell(key),
                (right, y),
                12.0,
                weight,
                Align::Right,
                color,
            );
        }
    }
    if shows_end {
        if row.frozen {
            paint_control(c, button, "End", WHITE, t.critical);
        } else {
            paint_control(c, button, "End", t.critical, WHITE);
        }
    }
}

// ---------- the Specs page ----------

/// Two overlapping squares, the usual sign for copy. The front one is filled
/// with the button's own color so it hides the back one where they cross.
fn copy_glyph(c: &Canvas, (x, y): (f32, f32), fill: Color, ink: Color) {
    c.outline_rect(Rect::new(x - 2.0, y - 5.5, 7.5, 7.5), 1.1, ink);
    c.fill_round(Rect::new(x - 6.0, y - 2.5, 8.5, 8.5), 0.0, fill);
    c.outline_rect(Rect::new(x - 5.5, y - 2.0, 7.5, 7.5), 1.1, ink);
}

fn tick(c: &Canvas, (x, y): (f32, f32), ink: Color) {
    c.polyline(
        &[(x - 4.0, y), (x - 1.0, y + 3.0), (x + 4.0, y - 3.5)],
        1.6,
        ink,
    );
}

/// The copy button keeps one size, and says what happened for a moment after.
fn paint_copy_button(c: &Canvas, v: &View, r: Rect) {
    const GLYPH: f32 = 12.0;
    const GAP: f32 = 7.0;
    let t = v.theme;
    let (fill, ink, glyph_ink, label) = match v.copied {
        Some(true) => (t.accent, WHITE, WHITE, "Copied"),
        Some(false) => (t.raised, t.ink, t.ink2, "Couldn't copy"),
        None if v.hover.copy => (t.track, t.ink, t.ink2, "Copy specs"),
        None => (t.raised, t.ink, t.ink2, "Copy specs"),
    };
    c.fill_round(r, 8.0, fill);
    let width = GLYPH + GAP + c.measure(label, 12.0, Weight::Semibold);
    let x = r.x + (r.w - width) / 2.0;
    let y = r.y + r.h / 2.0;
    if v.copied == Some(true) {
        tick(c, (x + GLYPH / 2.0, y), glyph_ink);
    } else {
        copy_glyph(c, (x + GLYPH / 2.0, y), fill, glyph_ink);
    }
    c.text_mid(
        label,
        (x + GLYPH + GAP, y),
        12.0,
        Weight::Semibold,
        Align::Left,
        ink,
    );
}

fn paint_specs(c: &Canvas, v: &View, page: &SpecsPage) {
    paint_copy_button(c, v, page.copy);
    // The PC's name is shown whole or not at all, since a cut name is no name.
    let band_y = page.band.y + page.band.h / 2.0;
    if let Some(name) = v.pc_name {
        let room = page.copy.x - page.band.x - 12.0;
        if c.measure(name, 12.0, Weight::Regular) <= room {
            c.text_mid(
                name,
                (page.band.x + 2.0, band_y),
                12.0,
                Weight::Regular,
                Align::Left,
                v.theme.ink3,
            );
        }
    }
    let Some(specs) = v.specs else {
        c.text_mid(
            "Reading this PC's parts",
            (page.view.x + page.view.w / 2.0, page.view.y + 20.0),
            12.0,
            Weight::Regular,
            Align::Center,
            v.theme.ink3,
        );
        return;
    };
    for (index, r) in page.visible(v.scroll) {
        if let Some(part) = specs.parts.get(index) {
            paint_part(c, v, page, part, r);
        }
    }

    // A thin mark shows where the view is, as on the Apps page.
    let hidden = page.max_scroll();
    if hidden > 0 {
        let body = page.view;
        let share = 1.0 - hidden as f32 / page.longest().max(1) as f32;
        let height = (body.h * share).max(20.0);
        let top = body.y + (body.h - height) * (v.scroll as f32 / hidden as f32);
        c.fill_round(
            Rect::new(body.right() + 2.0, top, 3.0, height),
            1.5,
            v.theme.track,
        );
    }
}

fn paint_part(c: &Canvas, v: &View, page: &SpecsPage, part: &Part, r: Rect) {
    const KEY_W: f32 = 110.0;
    const PART_INSET: f32 = 12.0;
    let theme = v.theme;
    c.fill_round(r, ROW_RADIUS, theme.raised);
    let left = r.x + PART_INSET;
    let label = part.kind.label();
    let (x, first) = if page.stacked {
        c.text_mid(
            label,
            (left, r.y + 14.0),
            11.0,
            Weight::Regular,
            Align::Left,
            theme.ink3,
        );
        (left, r.y + 34.0)
    } else {
        c.text_mid(
            label,
            (left, r.y + 20.0),
            12.0,
            Weight::Regular,
            Align::Left,
            theme.ink3,
        );
        (left + SPECS_LABEL_W, r.y + 20.0)
    };
    let room = r.right() - PART_INSET - x;
    let name = |text: &str, y: f32| {
        let text = c.fit(text, 13.0, Weight::Semibold, room);
        c.text_mid(
            &text,
            (x, y),
            13.0,
            Weight::Semibold,
            Align::Left,
            theme.ink,
        );
    };

    if page.detailed && part.items.len() == 1 && !part.facts.is_empty() {
        name(&part.items[0].name, first);
        let mut y = first + 26.0;
        for (key, value) in &part.facts {
            if y + 8.0 > r.bottom() {
                break;
            }
            let key = c.fit(key, 12.0, Weight::Regular, KEY_W - 8.0);
            c.text_mid(&key, (x, y), 12.0, Weight::Regular, Align::Left, theme.ink3);
            let value = c.fit(value, 12.0, Weight::Regular, room - KEY_W);
            c.text_mid(
                &value,
                (x + KEY_W, y),
                12.0,
                Weight::Regular,
                Align::Left,
                theme.ink,
            );
            y += SPECS_FACT_H;
        }
        return;
    }
    let mut y = first;
    for item in &part.items {
        // A part cut short by a short window shows only whole devices.
        if y + 22.0 > r.bottom() {
            break;
        }
        name(&item.name, y);
        if !item.line.is_empty() {
            let line = c.fit(&item.line, 11.0, Weight::Regular, room);
            c.text_mid(
                &line,
                (x, y + 16.0),
                11.0,
                Weight::Regular,
                Align::Left,
                theme.ink2,
            );
        }
        y += SPECS_ITEM_H;
    }
}

// ---------- the menu ----------

/// Paints the menu beside the pin, which is a small window of its own.
pub fn paint_menu(c: &Canvas, theme: &Theme, plan: &Plan, hover: Option<usize>) {
    c.clear(theme.raised);
    let heading = |text: &str, y: f32| {
        c.text_mid(
            text,
            (12.0, y + 9.0),
            11.0,
            Weight::Regular,
            Align::Left,
            theme.ink3,
        );
    };
    heading("Size", plan.size_heading_y);
    heading("Show", plan.show_heading_y);
    c.fill_round(
        Rect::new(6.0, plan.rule_y, plan.width - 12.0, 1.0),
        0.0,
        theme.track,
    );
    c.fill_round(plan.switch, 6.0, theme.surface);
    c.fill_round(
        Rect::new(6.0, plan.ticks_rule_y, plan.width - 12.0, 1.0),
        0.0,
        theme.track,
    );

    for (index, item) in plan.items.iter().enumerate() {
        let r = item.rect;
        let y = r.y + r.h / 2.0;
        let hovered = hover == Some(index);
        if item.action.is_tick() {
            if hovered {
                c.fill_round(r, 4.0, theme.track);
            }
            let check = Rect::new(r.x + 10.0, y - 7.0, 14.0, 14.0);
            if item.ticked {
                c.fill_round(check, 3.0, theme.accent);
                c.polyline(
                    &[
                        (check.x + 3.5, y),
                        (check.x + 6.0, y + 2.5),
                        (check.x + 10.5, y - 3.0),
                    ],
                    1.6,
                    WHITE,
                );
            } else {
                c.outline_rect(check, 1.1, theme.ink3);
            }
            c.text_mid(
                item.label,
                (r.x + 38.0, y),
                12.0,
                Weight::Semibold,
                Align::Left,
                theme.ink,
            );
            continue;
        }
        if matches!(item.action, Action::Simple | Action::Advanced) {
            if item.ticked {
                c.fill_round(r, 4.0, theme.track);
            }
            let ink = if item.ticked || hovered {
                theme.ink
            } else {
                theme.ink3
            };
            c.text_mid(
                item.label,
                (r.x + r.w / 2.0, y),
                12.0,
                Weight::Semibold,
                Align::Center,
                ink,
            );
            continue;
        }
        if hovered {
            c.fill_round(r, 4.0, theme.track);
        }
        if let Some((width, height)) = item.glyph {
            let middle = r.x + 17.0;
            c.outline_rect(
                Rect::new(middle - width / 2.0, y - height / 2.0, width, height),
                1.1,
                theme.ink2,
            );
        }
        c.text_mid(
            item.label,
            (r.x + 38.0, y),
            12.0,
            Weight::Semibold,
            Align::Left,
            theme.ink,
        );
        c.text_mid(
            &item.detail,
            (r.right() - 22.0, y),
            11.0,
            Weight::Regular,
            Align::Right,
            theme.ink3,
        );
        if item.ticked {
            let (tick_x, size) = (r.right() - 14.0, 3.5);
            c.polyline(
                &[
                    (tick_x - size, y),
                    (tick_x - 1.0, y + size - 1.0),
                    (tick_x + size, y - size),
                ],
                1.5,
                theme.accent,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for text measurement: every character is 5 wide.
    fn chars(text: &str) -> f32 {
        text.chars().count() as f32 * 5.0
    }

    #[test]
    fn a_frozen_app_shows_its_cpu_use_when_there_is_room() {
        assert_eq!(
            frozen_status(16.4, 200.0, chars),
            "Not responding · 16% CPU"
        );
    }

    #[test]
    fn a_stuck_app_shows_zero_rather_than_nothing() {
        assert_eq!(frozen_status(0.2, 200.0, chars), "Not responding · 0% CPU");
    }

    #[test]
    fn a_narrow_row_shortens_the_figure_before_dropping_it() {
        assert_eq!(frozen_status(16.0, 100.0, chars), "Not responding · 16%");
    }

    #[test]
    fn the_status_itself_survives_any_width() {
        assert_eq!(frozen_status(16.0, 1.0, chars), "Not responding");
    }

    #[test]
    fn values_carry_just_enough_of_their_unit() {
        assert_eq!(with_unit(MetricId::GpuTemp, 71.6), "72°");
        assert_eq!(with_unit(MetricId::CpuLoad, 54.0), "54%");
        assert_eq!(with_unit(MetricId::CpuPower, 81.2), "81 W");
        assert_eq!(with_unit(MetricId::Download, 6.26), "6.3");
    }

    #[test]
    fn auto_scale_never_spans_less_than_its_minimum() {
        let (low, high) = domain(Scale::Auto { min_span: 20.0 }, Some((70.0, 71.0)));
        assert_eq!(high - low, 20.0);
        assert!(low < 70.0 && high > 71.0);
    }

    #[test]
    fn auto_scale_expands_for_a_wide_range() {
        assert_eq!(
            domain(Scale::Auto { min_span: 20.0 }, Some((40.0, 90.0))),
            (40.0, 90.0)
        );
    }

    #[test]
    fn fixed_scale_ignores_the_data() {
        assert_eq!(
            domain(Scale::Fixed(0.0, 100.0), Some((40.0, 50.0))),
            (0.0, 100.0)
        );
    }

    #[test]
    fn from_zero_scale_starts_at_zero_with_headroom() {
        let (low, high) = domain(Scale::FromZero { min_top: 50.0 }, Some((10.0, 300.0)));
        assert_eq!(low, 0.0);
        assert!(high > 300.0);
        assert_eq!(domain(Scale::FromZero { min_top: 50.0 }, None).0, 0.0);
    }

    #[test]
    fn gaps_split_the_line_and_newest_sits_at_the_right_edge() {
        let mut s = Series::default();
        for v in [Some(10.0), Some(20.0), None, Some(30.0)] {
            s.push(v);
        }
        let area = Rect::new(0.0, 0.0, 119.0, 100.0);
        let runs = spark_runs(&s, area, (0.0, 100.0));
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len(), 2);
        assert_eq!(runs[1][0], (119.0, 70.0));
    }

    #[test]
    fn values_outside_the_range_stay_inside_the_plot() {
        let mut s = Series::default();
        s.push(Some(500.0));
        let runs = spark_runs(&s, Rect::new(0.0, 10.0, 50.0, 20.0), (0.0, 100.0));
        assert_eq!(runs[0][0].1, 10.0);
    }
}
