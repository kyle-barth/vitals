//! Paints the window from a [`Layout`] and a [`Monitor`].
//!
//! Color follows one rule: in the normal state everything is ink plus a single
//! accent, and amber, orange and red only ever mean a status. Numbers stay ink
//! at every severity except critical, and each status also carries a text
//! label, so it is never signalled by color alone.

use std::sync::Arc;

use crate::apps::{AppRow, BusyApp, Icon, Sort, SortKey};
use crate::gfx::{Align, Canvas, Color, Weight};
use crate::history::{CAPACITY, Series};
use crate::insights::LimitKind;
use crate::layout::{
    Button, Cell, Layout, Mode, PAD, Page, Rect, SPECS_FACT_H, SPECS_ITEM_H, SPECS_LABEL_W,
    SpecsPage, StripItem, StripPlan, TABLE_ROW_PAD, Table, confirm_buttons, end_button, plan_strip,
};
use crate::menu::{Action, Plan};
use crate::metrics::{CoreStatus, MetricId, Monitor, Scale, Unit};
use crate::severity::Severity;
use crate::specs::{Part, Specs};

const WARM: Color = Color::rgb(0xfab219);
const HOT: Color = Color::rgb(0xec835a);
const CRITICAL: Color = Color::rgb(0xd03b3b);
const WHITE: Color = Color::rgb(0xffffff);
const HOT_TINT: f32 = 0.25;

const TILE_RADIUS: f32 = 6.0;
const INSET: f32 = 10.0;
const MIN_NUMBER: f32 = 20.0;
const MAX_NUMBER: f32 = 72.0;
const SIMPLE_MAX_NUMBER: f32 = 96.0;
const MEMORY_SPARK_MIN_H: f32 = 110.0;
/// A wide tile this tall has room for a line of history under its number.
const STACKED_MIN_H: f32 = 60.0;
/// A Simple tile this tall has room for its bar under the number.
const SIMPLE_BAR_MIN_H: f32 = 58.0;

pub struct Theme {
    surface: Color,
    raised: Color,
    track: Color,
    ink: Color,
    ink2: Color,
    ink3: Color,
    accent: Color,
}

pub const DARK: Theme = Theme {
    surface: Color::rgb(0x1a1a19),
    raised: Color::rgb(0x232322),
    track: Color::rgb(0x333330),
    ink: Color::rgb(0xf0efec),
    ink2: Color::rgb(0xc3c2b7),
    ink3: Color::rgb(0x8f8e85),
    accent: Color::rgb(0x3987e5),
};

pub const LIGHT: Theme = Theme {
    surface: Color::rgb(0xfcfcfb),
    raised: Color::rgb(0xf3f2ee),
    track: Color::rgb(0xe4e3de),
    ink: Color::rgb(0x0b0b0b),
    ink2: Color::rgb(0x52514e),
    ink3: Color::rgb(0x76756f),
    accent: Color::rgb(0x2a78d6),
};

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
    /// Show only the numbers, rather than the full detail on each tile.
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
}

/// The colors for one tile at a given severity.
struct Look {
    fill: Color,
    ink: Color,
    ink2: Color,
    mark: Color,
    track: Color,
}

fn look(severity: Severity, theme: &Theme) -> Look {
    let base = Look {
        fill: theme.raised,
        ink: theme.ink,
        ink2: theme.ink2,
        mark: theme.accent,
        track: theme.track,
    };
    match severity {
        Severity::Normal => base,
        Severity::Warm => Look { mark: WARM, ..base },
        Severity::Hot => Look {
            fill: theme.raised.mix(HOT, HOT_TINT),
            mark: HOT,
            ..base
        },
        Severity::Critical => Look {
            fill: CRITICAL,
            ink: WHITE,
            ink2: WHITE,
            mark: WHITE,
            track: WHITE.with_alpha(0.3),
        },
    }
}

fn status_color(severity: Severity) -> Color {
    match severity {
        Severity::Critical => WHITE,
        Severity::Hot => HOT,
        _ => WARM,
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

pub fn paint(c: &Canvas, v: &View) {
    if v.layout.mode == Mode::Strip {
        paint_strip(c, v);
        return;
    }
    c.clear(v.theme.surface);
    paint_title_bar(c, v);
    if let Some(table) = &v.layout.table {
        paint_table(c, v, table);
        return;
    }
    if let Some(page) = &v.layout.specs {
        paint_specs(c, v, page);
        return;
    }
    for cell in &v.layout.cells {
        match v.layout.mode {
            Mode::Rows => paint_row(c, v, cell),
            _ => paint_tile(c, v, cell),
        }
    }
    if let Some(area) = v.layout.threads {
        paint_threads(c, v, area);
    }
    if let Some(area) = v.layout.limits {
        paint_limits(c, v, area);
    }
    paint_app_rows(c, v, false);
}

/// The limit a tile's own reading is at, if it's being shown.
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

/// The word beside a tile's label, with the color of its dot: the limit it is
/// at, or its severity. A power limit is normal under full load, so it stays grey.
fn chip(metric: MetricId, v: &View, severity: Severity) -> Option<(&'static str, Color)> {
    match limit_on(metric, v.monitor) {
        Some(LimitKind::Temperature) => {
            Some(("At limit", status_color(severity.max(Severity::Hot))))
        }
        Some(LimitKind::Power) => Some(("At limit", v.theme.ink3)),
        None => (severity != Severity::Normal).then(|| (severity.label(), status_color(severity))),
    }
}

/// Short facts for the row beside a tile's number, most telling first.
fn details(metric: MetricId, v: &View) -> Vec<String> {
    let monitor = v.monitor;
    let mut out = Vec::new();
    if metric == MetricId::GpuHotspot {
        if let Some(gap) = monitor.gap.view() {
            out.push(if gap.wide {
                format!("Wide gap +{:.0}°", gap.degrees)
            } else {
                format!("+{:.0}° over edge", gap.degrees)
            });
        }
    }
    // Read on from the number: "159 W  of 162 W limit".
    if metric == MetricId::CpuPower {
        if let Some(limit) = monitor.cpu_power_limit {
            out.push(format!("of {limit:.0} W limit"));
        }
    }
    if limit_on(metric, monitor).is_some() {
        let tracker = if matches!(metric, MetricId::CpuTemp | MetricId::CpuPower) {
            &monitor.cpu_limit
        } else {
            &monitor.gpu_limit
        };
        out.push(format!("{:.0}% of last 2 min", tracker.share()));
    }
    if let Some(average) = average_text(metric, monitor) {
        out.push(average);
    }
    out
}

const SEPARATOR: &str = "  ·  ";

/// Joins as many of `parts` as fit in `room`, in order.
fn fit_parts(c: &Canvas, parts: &[String], room: f32) -> Option<String> {
    let mut text: Option<String> = None;
    for part in parts {
        let next = match &text {
            Some(t) => format!("{t}{SEPARATOR}{part}"),
            None => part.clone(),
        };
        if c.measure(&next, 11.0, Weight::Regular) > room {
            break;
        }
        text = Some(next);
    }
    text
}

/// Runs of text drawn one after another: (text, weight, color).
fn paint_runs(c: &Canvas, runs: &[(String, Weight, Color)], (x, y): (f32, f32), size: f32) -> f32 {
    let mut at = x;
    for (text, weight, color) in runs {
        at += c.text_mid(text, (at, y), size, *weight, Align::Left, *color);
    }
    at - x
}

/// What each part's limit row says.
fn limit_runs(v: &View) -> Vec<Vec<(String, Weight, Color)>> {
    let m = v.monitor;
    let (ink, ink2) = (v.theme.ink, v.theme.ink2);
    let marker = |kind: LimitKind| {
        let color = if kind == LimitKind::Temperature {
            HOT
        } else {
            ink2
        };
        (" (limit)".to_string(), Weight::Semibold, color)
    };
    let share = |share: f32| {
        (
            format!(" · {share:.0}% of last 2 min"),
            Weight::Regular,
            ink2,
        )
    };
    let value = |id: MetricId| m.get(id).value;
    let mut out = Vec::new();
    if let Some(kind) = m.gpu_limit.shown() {
        let mut runs = vec![("GPU  ".to_string(), Weight::Semibold, ink)];
        match kind {
            LimitKind::Temperature => {
                let (name, temp) = match value(MetricId::GpuHotspot) {
                    Some(hot) => ("hotspot ", Some(hot)),
                    None => ("", value(MetricId::GpuTemp)),
                };
                runs.push((
                    format!("{name}{}", temp.map_or("–".into(), |t| format!("{t:.0}°"))),
                    Weight::Regular,
                    ink,
                ));
                runs.push(marker(kind));
                if value(MetricId::GpuFanPercent).is_some_and(|f| f >= 99.5) {
                    runs.push((" · fans 100%".into(), Weight::Regular, ink2));
                }
            }
            LimitKind::Power => {
                let power = value(MetricId::GpuPower).map_or("–".into(), |p| format!("{p:.0} W"));
                runs.push((format!("power {power}"), Weight::Regular, ink));
                runs.push(marker(kind));
            }
        }
        runs.push(share(m.gpu_limit.share()));
        out.push(runs);
    }
    if let Some(kind) = m.cpu_limit.shown() {
        let mut runs = vec![("CPU  ".to_string(), Weight::Semibold, ink)];
        let power = value(MetricId::CpuPower)
            .zip(m.cpu_power_limit)
            .map(|(p, l)| format!("{p:.0} of {l:.0} W"));
        match kind {
            LimitKind::Temperature => {
                let temp = value(MetricId::CpuTemp).map_or("–".into(), |t| format!("{t:.0}°"));
                runs.push((temp, Weight::Regular, ink));
                runs.push(marker(kind));
                if let Some(power) = power {
                    runs.push((format!(" · {power}"), Weight::Regular, ink2));
                }
            }
            LimitKind::Power => {
                runs.push((
                    format!("power {}", power.unwrap_or_else(|| "–".into())),
                    Weight::Regular,
                    ink,
                ));
                runs.push(marker(kind));
            }
        }
        runs.push(share(m.cpu_limit.share()));
        out.push(runs);
    }
    out
}

/// The row across a large window that says which parts are at a limit.
fn paint_limits(c: &Canvas, v: &View, r: Rect) {
    c.fill_round(r, TILE_RADIUS, v.theme.raised);
    let y = r.y + r.h / 2.0;
    let mut x = r.x + INSET;
    x += c.text_mid(
        "Limits",
        (x, y),
        12.0,
        Weight::Regular,
        Align::Left,
        v.theme.ink2,
    );
    for runs in limit_runs(v) {
        x += 22.0;
        x += paint_runs(c, &runs, (x, y), 12.0);
    }
}

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
    c.fill_round(r, 4.0, fill);
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
    if v.confirm == Some(app.key.as_str()) {
        paint_confirm(c, &app.name, r);
        return;
    }
    let hovered = v.hover.app == Some(index);
    if app.frozen {
        c.fill_round(r, TILE_RADIUS, CRITICAL);
    } else if hovered {
        c.fill_round(r, TILE_RADIUS, v.theme.track);
    } else if !flat {
        c.fill_round(r, TILE_RADIUS, v.theme.raised);
    }

    let y = r.y + r.h / 2.0;
    let left = r.x + 8.0;
    paint_app_icon(c, v, (&app.key, &app.name), app.icon.as_ref(), (left, y));

    let mut right = r.right() - 6.0;
    let shows_end = app.can_end && (app.frozen || hovered);
    if shows_end {
        let button = end_button(r);
        if app.frozen {
            paint_control(c, button, "End", WHITE, CRITICAL);
        } else {
            paint_control(c, button, "End", CRITICAL, WHITE);
        }
        right = button.x - 6.0;
    }

    let name_x = left + ICON_SIZE + 6.0;
    let name_ink = if app.frozen { WHITE } else { v.theme.ink };
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
            v.theme.ink2,
        );
    }
}

/// Asks once more, in place, before ending an app.
fn paint_confirm(c: &Canvas, name: &str, r: Rect) {
    c.fill_round(r, TILE_RADIUS, CRITICAL);
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
    paint_control(c, end, "End", WHITE, CRITICAL);
    paint_control(c, keep, "Keep", WHITE.with_alpha(0.22), WHITE);
}

fn paint_tabs(c: &Canvas, v: &View) {
    for &(page, r) in &v.layout.tabs {
        let label = match page {
            Page::Vitals => "Vitals",
            Page::Apps => "Apps",
            Page::Specs => "Specs",
        };
        let ink = if page == v.layout.page {
            v.theme.ink
        } else if v.hover.tab == Some(page) {
            v.theme.ink2
        } else {
            v.theme.ink3
        };
        c.text_mid(
            label,
            (r.x + r.w / 2.0, r.y + r.h / 2.0),
            12.0,
            Weight::Semibold,
            Align::Center,
            ink,
        );
    }
}

/// The tabs and window buttons. A warning goes to the taskbar instead, through
/// the window's title, and the tiles already show it, so the tabs always stay.
fn paint_title_bar(c: &Canvas, v: &View) {
    paint_tabs(c, v);
    for &(button, rect) in &v.layout.buttons {
        paint_button(c, v, button, rect);
    }
}

fn paint_button(c: &Canvas, v: &View, button: Button, r: Rect) {
    let hovered = v.hover.button == Some(button);
    if hovered {
        let fill = if button == Button::Close {
            CRITICAL
        } else {
            v.theme.track
        };
        c.fill_round(r, 0.0, fill);
    }
    let color = if hovered && button == Button::Close {
        WHITE
    } else {
        v.theme.ink2
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
            let mark = if v.menu_open { v.theme.accent } else { color };
            c.outline_rect(Rect::new(x - 5.5, y - 4.5, 11.0, 9.0), 1.1, mark);
            c.outline_rect(Rect::new(x - 3.0, y - 1.0, 3.5, 3.0), 1.1, mark);
        }
        Button::Pin => {
            let mark = if v.pinned { v.theme.accent } else { color };
            if v.pinned {
                c.circle(x, y - 2.0, 3.2, mark);
            } else {
                c.outline_circle(x, y - 2.0, 2.8, 1.2, mark);
            }
            c.line((x, y + 1.0), (x, y + 6.0), 1.2, mark);
        }
    }
}

/// The value as (number, unit), or a dash when the sensor has no reading.
fn value_parts(metric: MetricId, value: Option<f32>) -> (String, &'static str) {
    let unit = metric.spec().unit;
    match value {
        Some(v) => (unit.format(v), unit.suffix()),
        None => ("–".to_string(), ""),
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

/// The average over the history a tile holds, which is two minutes when full.
fn average_text(metric: MetricId, monitor: &Monitor) -> Option<String> {
    let average = monitor.get(metric).series.average()?;
    Some(format!("Avg {}", with_unit(metric, average)))
}

/// Short text beside the number: the session peak, the installed capacity, or
/// a prompt explaining how to get a true CPU core reading.
fn note(metric: MetricId, monitor: &Monitor) -> Option<String> {
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
        MetricId::VideoMemory => monitor.vram_total_gb.map(|t| format!("of {t:.0} GB")),
        _ => {
            let peak = monitor.get(metric).peak?;
            Some(format!("Peak {}", with_unit(metric, peak)))
        }
    }
}

fn capacity(metric: MetricId, monitor: &Monitor) -> Option<f32> {
    match metric {
        MetricId::Memory => monitor.ram_total_gb,
        MetricId::VideoMemory => monitor.vram_total_gb,
        _ => None,
    }
}

fn paint_tile(c: &Canvas, v: &View, cell: &Cell) {
    let metric = resolve(cell.metric, v.monitor);
    let reading = v.monitor.get(metric);
    let spec = metric.spec();
    let style = look(reading.severity, v.theme);
    let r = cell.rect;
    c.fill_round(r, TILE_RADIUS, style.fill);

    let (left, right) = (r.x + INSET, r.right() - INSET);
    let title_y = r.y + 13.0;
    c.text_mid(
        spec.label,
        (left, title_y),
        12.0,
        Weight::Regular,
        Align::Left,
        style.ink2,
    );

    // A narrow tile cannot hold the label and the chip on one row. The dot then
    // stays on the title row, and the chip's word takes the note's place, so
    // the status is still spelled out rather than shown by color alone.
    let label_w = c.measure(spec.label, 12.0, Weight::Regular);
    let chip = chip(metric, v, reading.severity);
    let chip_w = chip_width(c, chip);
    let chip_fits = chip_w == 0.0 || left + label_w + 10.0 + chip_w <= right;
    if chip_fits {
        paint_chip(c, chip, right, title_y, &style);
    } else if let Some((_, color)) = chip {
        c.circle(right - 4.0, title_y, 4.0, color);
    }
    let chip_word = chip.map(|(word, _)| word.to_string());
    if v.simple {
        paint_simple_tile(c, v, metric, r, chip_fits, chip_word, &style);
        return;
    }

    let size = (r.h * 0.34).min(r.w * 0.22).clamp(MIN_NUMBER, MAX_NUMBER);
    let baseline = r.y + 24.0 + size * 0.86;
    let number_right = paint_number(c, metric, reading.value, left, baseline, size, &style);

    let wide = r.w > r.h * 2.0;
    let bar = capacity(metric, v.monitor);
    let bar_top = r.bottom() - 15.0;
    let details = details(metric, v);
    // With the height for it, the history goes under the number and the row
    // beside the number is free for the details.
    let stacked = wide && bar.is_none() && r.h >= STACKED_MIN_H;
    let note_text = if chip_fits {
        note(metric, v.monitor)
    } else {
        chip_word
    };
    let note_w = note_text
        .as_deref()
        .map_or(0.0, |t| c.measure(t, 11.0, Weight::Regular));

    // A wide tile keeps its note on the title row and gives the row beside the
    // number to the sparkline. Otherwise the note sits level with the number.
    let mut plot_right = right;
    let stacked_details = stacked
        .then(|| fit_parts(c, &details, right - number_right - 8.0))
        .flatten();
    let beside_number = |text: &str| {
        c.text(
            text,
            (right, baseline),
            11.0,
            Weight::Regular,
            Align::Right,
            style.ink2,
        );
    };
    if let Some(text) = &note_text {
        let chip_gap = if chip_fits && chip_w > 0.0 {
            chip_w + 10.0
        } else {
            0.0
        };
        if wide && chip_fits && left + label_w + 10.0 + chip_gap + note_w <= right {
            let x = right - chip_gap;
            c.text_mid(
                text,
                (x, title_y),
                11.0,
                Weight::Regular,
                Align::Right,
                style.ink2,
            );
            if let Some(details) = &stacked_details {
                beside_number(details);
            }
        } else if let Some(details) = stacked_details.as_deref().filter(|_| chip_fits) {
            // The chip has taken the note's place on the title row. The row
            // beside the number still gives the details, as in any stacked
            // tile: the gap, or how long a limit held, says more than the peak.
            beside_number(details);
        } else if number_right + 8.0 + note_w <= right {
            // A tall tile says as much as fits on the one line, the note last.
            let both = (!wide && bar.is_none())
                .then(|| {
                    let mut parts = details.clone();
                    parts.push(text.clone());
                    fit_parts(c, &parts, right - number_right - 8.0)
                })
                .flatten()
                .filter(|both| both.ends_with(text.as_str()));
            beside_number(both.as_deref().unwrap_or(text));
            if wide {
                plot_right = right - note_w - 12.0;
            }
        }
    }

    let plot = if stacked {
        let top = baseline + 4.0;
        Rect::new(left, top, right - left, r.bottom() - 6.0 - top)
    } else if wide {
        let bottom = if bar.is_some() {
            bar_top - 6.0
        } else {
            r.bottom() - 9.0
        };
        Rect::new(
            number_right + 14.0,
            r.y + 24.0,
            plot_right - number_right - 14.0,
            bottom - r.y - 24.0,
        )
    } else {
        let top = baseline + 8.0;
        let bottom = if bar.is_some() {
            bar_top - 6.0
        } else {
            r.bottom() - 9.0
        };
        Rect::new(left, top, right - left, bottom - top)
    };

    // A short tile has no room for a bar under the number, so it sits beside it.
    let short = r.h < baseline - r.y + 24.0;
    if let Some(total) = bar.filter(|t| *t > 0.0) {
        let fill = reading.value.unwrap_or(0.0) / total;
        let beside = Rect::new(
            number_right + 14.0,
            baseline - size * 0.35 - 3.0,
            plot_right - number_right - 14.0,
            6.0,
        );
        if !short {
            let below = Rect::new(left, bar_top, right - left, 6.0);
            meter(c, below, fill, style.mark, style.track);
        } else if beside.w > 24.0 {
            meter(c, beside, fill, style.mark, style.track);
        }
    }
    let has_room = bar.is_none() || wide || r.h > MEMORY_SPARK_MIN_H;
    if (has_room && !short) || (bar.is_none() && wide) {
        paint_spark(c, v.monitor, metric, plot, &style);
    }
}

/// A tile in Simple mode: the number, and only what can't be left out.
fn paint_simple_tile(
    c: &Canvas,
    v: &View,
    metric: MetricId,
    r: Rect,
    chip_fits: bool,
    chip_word: Option<String>,
    style: &Look,
) {
    const TITLE_SPACE: f32 = 22.0;
    const BAR_SPACE: f32 = 15.0;
    let reading = v.monitor.get(metric);
    let (left, right) = (r.x + INSET, r.right() - INSET);
    let size = ((r.h - 28.0) * 0.8)
        .min(r.w * 0.24)
        .clamp(MIN_NUMBER, SIMPLE_MAX_NUMBER);
    let space = r.h - TITLE_SPACE - BAR_SPACE;
    let baseline = r.y + TITLE_SPACE + (space + size * 0.7) / 2.0;
    let number_right = paint_number(c, metric, reading.value, left, baseline, size, style);

    if let Some(total) = capacity(metric, v.monitor).filter(|t| *t > 0.0) {
        let fill = reading.value.unwrap_or(0.0) / total;
        let area = if r.h >= SIMPLE_BAR_MIN_H {
            Rect::new(left, r.bottom() - BAR_SPACE, right - left, 6.0)
        } else {
            let x = number_right + 14.0;
            Rect::new(x, baseline - size * 0.35 - 3.0, right - x, 6.0)
        };
        if area.w > 24.0 {
            meter(c, area, fill, style.mark, style.track);
        }
    }

    // The way to a true core reading stays, as nothing else offers it, and so
    // does a status word that didn't fit on the title row.
    let wide_gap = metric == MetricId::GpuHotspot && v.monitor.gap.view().is_some_and(|g| g.wide);
    let text = if !chip_fits {
        chip_word
    } else if metric == MetricId::IoDieTemp {
        note(metric, v.monitor)
    } else if wide_gap {
        Some("Wide gap".to_string())
    } else {
        None
    };
    if let Some(text) = text {
        if number_right + 8.0 + c.measure(&text, 11.0, Weight::Regular) <= right {
            c.text(
                &text,
                (right, baseline),
                11.0,
                Weight::Regular,
                Align::Right,
                style.ink2,
            );
        }
    }
}

const CHIP_DOT_SPACE: f32 = 12.0;

/// The width of a chip, which is zero when there is nothing to report.
fn chip_width(c: &Canvas, chip: Option<(&str, Color)>) -> f32 {
    chip.map_or(0.0, |(word, _)| {
        c.measure(word, 11.0, Weight::Semibold) + CHIP_DOT_SPACE
    })
}

/// Draws a chip right-aligned at `right`.
fn paint_chip(c: &Canvas, chip: Option<(&str, Color)>, right: f32, y: f32, style: &Look) {
    let Some((word, color)) = chip else {
        return;
    };
    let width = c.text_mid(
        word,
        (right, y),
        11.0,
        Weight::Semibold,
        Align::Right,
        style.ink,
    );
    c.circle(right - width - 8.0, y, 4.0, color);
}

/// Draws a number and its unit, returning the x position after them.
fn paint_number(
    c: &Canvas,
    metric: MetricId,
    value: Option<f32>,
    x: f32,
    baseline: f32,
    size: f32,
    style: &Look,
) -> f32 {
    let (number, suffix) = value_parts(metric, value);
    let number_w = c.text(
        &number,
        (x, baseline),
        size,
        Weight::Semibold,
        Align::Left,
        style.ink,
    );
    if suffix == Unit::Celsius.suffix() {
        let w = c.text(
            suffix,
            (x + number_w, baseline),
            size,
            Weight::Regular,
            Align::Left,
            style.ink2,
        );
        x + number_w + w
    } else if suffix.is_empty() {
        x + number_w
    } else {
        let unit_size = (size * 0.42).max(12.0);
        let unit_x = x + number_w + 3.0;
        let w = c.text(
            suffix,
            (unit_x, baseline),
            unit_size,
            Weight::Regular,
            Align::Left,
            style.ink2,
        );
        unit_x + w
    }
}

fn meter(c: &Canvas, r: Rect, fill: f32, color: Color, track: Color) {
    c.fill_round(r, r.h / 2.0, track);
    let width = (r.w * fill.clamp(0.0, 1.0)).max(r.h);
    c.fill_round(Rect { w: width, ..r }, r.h / 2.0, color);
}

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

fn paint_spark(c: &Canvas, monitor: &Monitor, metric: MetricId, area: Rect, style: &Look) {
    const END_DOT: f32 = 6.0;
    if area.w < 24.0 || area.h < 8.0 {
        return;
    }
    let area = Rect {
        w: area.w - END_DOT,
        y: area.y + 2.0,
        h: area.h - 4.0,
        ..area
    };
    let reading = monitor.get(metric);
    let scale = metric.spec().scale;

    // The hotspot's distance from the edge temperature is the useful signal,
    // so the edge is drawn behind it on the same scale.
    let companion =
        (metric == MetricId::GpuHotspot).then(|| &monitor.get(MetricId::GpuTemp).series);
    let mut bounds = reading.series.bounds();
    if let (Some(other), Some((low, high))) = (companion.and_then(Series::bounds), bounds) {
        bounds = Some((low.min(other.0), high.max(other.1)));
    }
    let mut range = domain(scale, bounds);
    // A wide gap shades the band between hotspot and edge, where there's room.
    // Space under the edge line keeps that band from filling the whole plot.
    let wide = companion.is_some() && monitor.gap.view().is_some_and(|g| g.wide);
    if wide {
        range.0 -= (range.1 - range.0) * 0.6;
    }

    let runs = spark_runs(&reading.series, area, range);
    if let Some(series) = companion {
        let under = spark_runs(series, area, range);
        if let (true, true, Some(top), Some(bottom)) =
            (wide, area.h >= 30.0, runs.last(), under.last())
        {
            if top.len() == bottom.len() && top.len() >= 2 {
                let mut band = top.clone();
                band.extend(bottom.iter().rev());
                c.fill_polygon(&band, style.mark.with_alpha(0.16));
            }
        }
        for run in under {
            c.polyline(&run, 1.2, style.ink2.with_alpha(0.6));
        }
    }
    for run in &runs {
        c.polyline(run, 2.0, style.mark);
    }
    if let Some(&(x, y)) = runs.last().and_then(|run| run.last()) {
        c.circle(x, y, 6.0, style.fill);
        c.circle(x, y, 4.0, style.mark);
    }
}

fn paint_row(c: &Canvas, v: &View, cell: &Cell) {
    let metric = resolve(cell.metric, v.monitor);
    let reading = v.monitor.get(metric);
    let style = look(reading.severity, v.theme);
    let r = cell.rect;
    c.fill_round(r, TILE_RADIUS, style.fill);

    let (left, right) = (r.x + 8.0, r.right() - 8.0);
    let total = capacity(metric, v.monitor).filter(|t| *t > 0.0);
    let centered = v.simple && total.is_none();
    let y = if centered {
        r.y + r.h / 2.0
    } else {
        r.y + 14.0
    };
    c.text_mid(
        metric.spec().label,
        (left, y),
        12.0,
        Weight::Regular,
        Align::Left,
        style.ink2,
    );

    let size = if centered {
        (r.h * 0.5).clamp(15.0, 30.0)
    } else {
        (r.h * 0.36).clamp(15.0, 22.0)
    };
    let (number, suffix) = value_parts(metric, reading.value);
    let unit_size = if suffix == "°" { size } else { 11.0 };
    let unit_w = c.text(
        suffix,
        (right, y + size * 0.35),
        unit_size,
        Weight::Regular,
        Align::Right,
        style.ink2,
    );
    let gap = if suffix == "°" || suffix.is_empty() {
        0.0
    } else {
        3.0
    };
    c.text(
        &number,
        (right - unit_w - gap, y + size * 0.35),
        size,
        Weight::Semibold,
        Align::Right,
        style.ink,
    );

    let top = r.y + 27.0;
    let area = Rect::new(left, top, right - left, r.bottom() - top - 6.0);
    match total {
        Some(total) => {
            let bar = Rect::new(left, r.bottom() - 12.0, right - left, 5.0);
            meter(
                c,
                bar,
                reading.value.unwrap_or(0.0) / total,
                style.mark,
                style.track,
            );
        }
        None if v.simple => {}
        None => paint_spark(c, v.monitor, metric, area, &style),
    }
}

fn paint_threads(c: &Canvas, v: &View, area: Rect) {
    c.fill_round(area, TILE_RADIUS, v.theme.raised);
    let (left, right) = (area.x + INSET, area.right() - INSET);
    c.text_mid(
        "Thread load",
        (left, area.y + 13.0),
        12.0,
        Weight::Regular,
        Align::Left,
        v.theme.ink2,
    );
    let count = v.monitor.threads.len();
    if count == 0 {
        return;
    }
    const GAP: f32 = 3.0;
    let width = (right - left - GAP * (count - 1) as f32) / count as f32;
    let top = area.y + 26.0;
    let height = area.bottom() - top - 10.0;
    for (i, load) in v.monitor.threads.iter().enumerate() {
        let x = left + i as f32 * (width + GAP);
        c.fill_round(Rect::new(x, top, width, height), 2.0, v.theme.track);
        let fill = (height * load / 100.0).max(2.0);
        c.fill_round(
            Rect::new(x, top + height - fill, width, fill),
            2.0,
            v.theme.accent,
        );
    }
}

fn strip_entry(item: StripItem, monitor: &Monitor) -> (&'static str, String, Severity) {
    let percent = |metric: MetricId, total: Option<f32>| {
        let share = monitor
            .get(metric)
            .value
            .zip(total)
            .map(|(used, t)| used / t * 100.0);
        share.map_or("–".to_string(), |s| format!("{s:.0}%"))
    };
    let degrees = |metric: MetricId| {
        let reading = monitor.get(metric);
        let text = reading
            .value
            .map_or("–".to_string(), |v| format!("{v:.0}°"));
        (text, reading.severity)
    };
    let at_limit = |metric: MetricId| {
        if limit_on(metric, monitor) == Some(LimitKind::Temperature) {
            "▾"
        } else {
            ""
        }
    };
    match item {
        // Without a temperature, the CPU's load is the next most telling number.
        StripItem::Cpu if monitor.absent.contains(MetricId::CpuTemp) => (
            "CPU",
            percent(MetricId::CpuLoad, Some(100.0)),
            Severity::Normal,
        ),
        StripItem::Cpu => {
            let metric = monitor.cpu_temp_metric();
            let label = if metric == MetricId::CpuTemp {
                "CPU"
            } else {
                "I/O"
            };
            let (text, severity) = degrees(metric);
            (label, format!("{text}{}", at_limit(metric)), severity)
        }
        StripItem::Gpu => {
            let (text, severity) = degrees(MetricId::GpuTemp);
            ("GPU", text, severity)
        }
        StripItem::Hotspot => {
            let (text, severity) = degrees(MetricId::GpuHotspot);
            let gap = monitor
                .gap
                .view()
                .filter(|g| g.wide)
                .map(|g| format!(" +{:.0}°", g.degrees))
                .unwrap_or_default();
            (
                "Hot",
                format!("{text}{}{gap}", at_limit(MetricId::GpuHotspot)),
                severity,
            )
        }
        StripItem::Ram => (
            "RAM",
            percent(MetricId::Memory, monitor.ram_total_gb),
            Severity::Normal,
        ),
        StripItem::Vram => (
            "VRAM",
            percent(MetricId::VideoMemory, monitor.vram_total_gb),
            Severity::Normal,
        ),
    }
}

/// The key numbers above the Apps table, which is all of the sensors it leaves
/// room for. It shows as many as fit, in the same order as the strip.
fn paint_summary(c: &Canvas, v: &View, area: Rect) {
    use StripItem::*;
    const LABEL: f32 = 11.0;
    const VALUE: f32 = 13.0;
    const ITEM_GAP: f32 = 12.0;
    let y = area.y + area.h / 2.0;
    let mut x = area.x + TABLE_ROW_PAD;
    let items = [Cpu, Gpu, Hotspot, Ram, Vram]
        .into_iter()
        .filter(|i| *i != Hotspot || !v.monitor.absent.contains(MetricId::GpuHotspot));
    for item in items {
        let (label, value, severity) = strip_entry(item, v.monitor);
        let dot = if severity == Severity::Normal {
            0.0
        } else {
            STRIP_DOT_SPACE
        };
        let width = c.measure(label, LABEL, Weight::Regular)
            + STRIP_LABEL_GAP
            + c.measure(&value, VALUE, Weight::Semibold)
            + dot;
        if x + width > area.right() {
            break;
        }
        x += c.text_mid(
            label,
            (x, y),
            LABEL,
            Weight::Regular,
            Align::Left,
            v.theme.ink3,
        );
        x += STRIP_LABEL_GAP;
        x += c.text_mid(
            &value,
            (x, y),
            VALUE,
            Weight::Semibold,
            Align::Left,
            v.theme.ink,
        );
        if dot > 0.0 {
            c.circle(x + 7.0, y, 3.5, status_color(severity));
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

fn paint_table(c: &Canvas, v: &View, table: &Table) {
    if let Some(area) = table.summary {
        paint_summary(c, v, area);
    }
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
    c.fill_round(
        Rect::new(header.x, header.bottom() - 1.0, header.w, 1.0),
        0.0,
        v.theme.track,
    );

    let rows = &v.monitor.table;
    if rows.is_empty() {
        c.text_mid(
            "Reading apps",
            (table.body.x + table.body.w / 2.0, table.body.y + 20.0),
            12.0,
            Weight::Regular,
            Align::Center,
            v.theme.ink3,
        );
        return;
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
}

fn paint_table_row(c: &Canvas, v: &View, table: &Table, row: &AppRow, index: usize, r: Rect) {
    if v.confirm == Some(row.key.as_str()) {
        paint_confirm(c, &row.name, r);
        return;
    }
    let hovered = v.hover.row == Some(index);
    if row.frozen {
        c.fill_round(r, 4.0, CRITICAL);
    } else if hovered {
        c.fill_round(r, 4.0, v.theme.raised);
    }
    let ink = if row.frozen { WHITE } else { v.theme.ink };
    let faded = if row.frozen { WHITE } else { v.theme.ink3 };

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
            paint_control(c, button, "End", WHITE, CRITICAL);
        } else {
            paint_control(c, button, "End", CRITICAL, WHITE);
        }
    }
}

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
    let (fill, ink, glyph_ink, label) = match v.copied {
        Some(true) => (v.theme.accent, WHITE, WHITE, "Copied"),
        Some(false) => (v.theme.raised, v.theme.ink, v.theme.ink2, "Couldn't copy"),
        None if v.hover.copy => (v.theme.track, v.theme.ink, v.theme.ink2, "Copy specs"),
        None => (v.theme.raised, v.theme.ink, v.theme.ink2, "Copy specs"),
    };
    c.fill_round(r, 5.0, fill);
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
    let theme = v.theme;
    c.fill_round(r, TILE_RADIUS, theme.raised);
    let left = r.x + INSET;
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
    let room = r.right() - INSET - x;
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

const STRIP_LABEL_RATIO: f32 = 0.74;
const STRIP_LABEL_GAP: f32 = 4.0;
const STRIP_DOT_SPACE: f32 = 13.0;

fn strip_gap(size: f32) -> f32 {
    (size * 0.8).max(12.0)
}

fn strip_item_width(c: &Canvas, item: StripItem, monitor: &Monitor, size: f32) -> f32 {
    let (label, value, severity) = strip_entry(item, monitor);
    let dot = if severity == Severity::Normal {
        0.0
    } else {
        STRIP_DOT_SPACE
    };
    c.measure(label, size * STRIP_LABEL_RATIO, Weight::Regular)
        + STRIP_LABEL_GAP
        + c.measure(&value, size, Weight::Semibold)
        + dot
}

fn plan(c: &Canvas, v: &View) -> StripPlan {
    plan_strip(
        v.layout.width,
        v.layout.height,
        v.monitor.absent,
        |items, size| {
            let widths: f32 = items
                .iter()
                .map(|i| strip_item_width(c, *i, v.monitor, size))
                .sum();
            widths + strip_gap(size) * items.len().saturating_sub(1) as f32
        },
    )
}

fn paint_strip(c: &Canvas, v: &View) {
    let severity = v.monitor.worst_severity();
    let surface = match severity {
        Severity::Critical => CRITICAL,
        Severity::Hot => v.theme.surface.mix(HOT, HOT_TINT),
        _ => v.theme.surface,
    };
    c.clear(surface);
    if v.layout.strip_is_replaced() {
        paint_app_rows(c, v, true);
        return;
    }
    let critical = severity == Severity::Critical;
    let ink = if critical { WHITE } else { v.theme.ink };
    let muted = if critical { WHITE } else { v.theme.ink3 };

    // An app on the second line takes the place of RAM and VRAM.
    let mut strip = plan(c, v);
    let app_row = v.layout.app_rows.first().copied();
    if app_row.is_some() {
        strip.lines.truncate(1);
    }
    let count = strip.lines.len() as f32;
    let height = app_row.map_or(v.layout.height, |row| row.y);
    let pitch = (strip.size * 1.8).min(height / count);
    let first = (height - pitch * (count - 1.0)) / 2.0;
    for (row, line) in strip.lines.iter().enumerate() {
        let y = first + row as f32 * pitch;
        let mut x = PAD + 4.0;
        for &item in line {
            let (label, value, item_severity) = strip_entry(item, v.monitor);
            let label_size = strip.size * STRIP_LABEL_RATIO;
            x += c.text_mid(
                label,
                (x, y),
                label_size,
                Weight::Regular,
                Align::Left,
                muted,
            );
            x += STRIP_LABEL_GAP;
            x += c.text_mid(
                &value,
                (x, y),
                strip.size,
                Weight::Semibold,
                Align::Left,
                ink,
            );
            if item_severity != Severity::Normal {
                c.circle(x + 7.0, y, 3.5, status_color(item_severity));
                x += STRIP_DOT_SPACE;
            }
            x += strip_gap(strip.size);
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
            c.fill_round(full_height, 0.0, surface);
        }
        for &(button, rect) in &v.layout.buttons {
            paint_button(c, v, button, rect);
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
