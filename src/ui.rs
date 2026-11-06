//! Paints the window from a [`Layout`] and a [`Monitor`].
//!
//! Color follows one rule: in the normal state everything is ink plus a single
//! accent, and amber, orange and red only ever mean a status. Numbers stay ink
//! at every severity except critical, and each status also carries a text
//! label, so it is never signalled by color alone.

use crate::apps::BusyApp;
use crate::gfx::{Align, Canvas, Color, Weight};
use crate::history::{CAPACITY, Series};
use crate::layout::{
    Button, Cell, Layout, Mode, PAD, Rect, StripItem, StripPlan, TITLE_H, confirm_buttons,
    end_button, plan_strip,
};
use crate::metrics::{CoreStatus, MetricId, Monitor, Scale, Unit};
use crate::severity::Severity;

const WARM: Color = Color::rgb(0xfab219);
const HOT: Color = Color::rgb(0xec835a);
const CRITICAL: Color = Color::rgb(0xd03b3b);
const WHITE: Color = Color::rgb(0xffffff);
const HOT_TINT: f32 = 0.25;

const TILE_RADIUS: f32 = 6.0;
const INSET: f32 = 10.0;
const MIN_NUMBER: f32 = 20.0;
const MAX_NUMBER: f32 = 72.0;
const MEMORY_SPARK_MIN_H: f32 = 110.0;

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

/// The text for the title bar and taskbar when a temperature needs attention.
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
    for cell in &v.layout.cells {
        match v.layout.mode {
            Mode::Rows => paint_row(c, v, cell),
            _ => paint_tile(c, v, cell),
        }
    }
    if let Some(area) = v.layout.threads {
        paint_threads(c, v, area);
    }
    paint_app_rows(c, v, false);
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

fn paint_app_icon(c: &Canvas, v: &View, app: &BusyApp, x: f32, y: f32) {
    let area = Rect::new(x, y - ICON_SIZE / 2.0, ICON_SIZE, ICON_SIZE);
    if let Some(icon) = &app.icon {
        c.icon(&app.key, icon, area);
        return;
    }
    c.fill_round(area, 3.0, v.theme.track);
    let letter: String = app.name.chars().take(1).collect();
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
        paint_confirm(c, app, r);
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
    paint_app_icon(c, v, app, left, y);

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
            // Stacked rather than dropped, so the status is always spelled out
            // and never left to the color alone.
            let name = c.fit(&app.name, 11.0, Weight::Semibold, name_room);
            let status = frozen_status(app.cpu_percent, name_room, |text| {
                c.measure(text, 10.0, Weight::Regular)
            });
            c.text_mid(
                &name,
                (name_x, y - STACK_OFFSET),
                11.0,
                Weight::Semibold,
                Align::Left,
                name_ink,
            );
            c.text_mid(
                &status,
                (name_x, y + STACK_OFFSET),
                10.0,
                Weight::Regular,
                Align::Left,
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
fn paint_confirm(c: &Canvas, app: &BusyApp, r: Rect) {
    c.fill_round(r, TILE_RADIUS, CRITICAL);
    let (end, keep) = confirm_buttons(r);
    let room = end.x - r.x - 16.0;
    let asks = [
        format!("End {}? Unsaved work will be lost.", app.name),
        format!("End {}? Unsaved work is lost.", app.name),
        format!("End {}?", app.name),
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

fn paint_title_bar(c: &Canvas, v: &View) {
    let y = TITLE_H / 2.0;
    match alert_text(v.layout, v.monitor) {
        Some(alert) => {
            let severity = v.monitor.worst_severity();
            c.circle(
                PAD + 6.0,
                y,
                4.0,
                if severity == Severity::Critical {
                    CRITICAL
                } else {
                    HOT
                },
            );
            c.text_mid(
                &alert,
                (PAD + 16.0, y),
                12.0,
                Weight::Semibold,
                Align::Left,
                v.theme.ink,
            );
        }
        None => {
            c.text_mid(
                "Vitals",
                (PAD + 2.0, y),
                12.0,
                Weight::Semibold,
                Align::Left,
                v.theme.ink2,
            );
        }
    }
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
            Some(format!("Peak {peak:.0}°"))
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
    // stays on the title row, and the severity word takes the note's place, so
    // the status is still spelled out rather than shown by color alone.
    let label_w = c.measure(spec.label, 12.0, Weight::Regular);
    let chip_w = chip_width(c, reading.severity);
    let chip_fits = chip_w == 0.0 || left + label_w + 10.0 + chip_w <= right;
    if chip_fits {
        paint_chip(c, reading.severity, right, title_y, &style);
    } else {
        c.circle(right - 4.0, title_y, 4.0, status_color(reading.severity));
    }

    let size = (r.h * 0.34).min(r.w * 0.22).clamp(MIN_NUMBER, MAX_NUMBER);
    let baseline = r.y + 24.0 + size * 0.86;
    let number_right = paint_number(c, metric, reading.value, left, baseline, size, &style);

    let wide = r.w > r.h * 2.0;
    let bar = capacity(metric, v.monitor);
    let bar_top = r.bottom() - 15.0;
    let note_text = if chip_fits {
        note(metric, v.monitor)
    } else {
        Some(reading.severity.label().to_string())
    };
    let note_w = note_text
        .as_deref()
        .map_or(0.0, |t| c.measure(t, 11.0, Weight::Regular));

    // A wide tile keeps its note on the title row and gives the row beside the
    // number to the sparkline. Otherwise the note sits level with the number.
    let mut plot_right = right;
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
        } else if number_right + 8.0 + note_w <= right {
            c.text(
                text,
                (right, baseline),
                11.0,
                Weight::Regular,
                Align::Right,
                style.ink2,
            );
            if wide {
                plot_right = right - note_w - 12.0;
            }
        }
    }

    let plot = if wide {
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

const CHIP_DOT_SPACE: f32 = 12.0;

/// The width of a severity chip, which is zero when there is nothing to report.
fn chip_width(c: &Canvas, severity: Severity) -> f32 {
    if severity == Severity::Normal {
        return 0.0;
    }
    c.measure(severity.label(), 11.0, Weight::Semibold) + CHIP_DOT_SPACE
}

/// Draws a severity chip right-aligned at `right`.
fn paint_chip(c: &Canvas, severity: Severity, right: f32, y: f32, style: &Look) {
    if severity == Severity::Normal {
        return;
    }
    let width = c.text_mid(
        severity.label(),
        (right, y),
        11.0,
        Weight::Semibold,
        Align::Right,
        style.ink,
    );
    c.circle(right - width - 8.0, y, 4.0, status_color(severity));
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
    let range = domain(scale, bounds);

    if let Some(series) = companion {
        for run in spark_runs(series, area, range) {
            c.polyline(&run, 1.2, style.ink2.with_alpha(0.6));
        }
    }
    let runs = spark_runs(&reading.series, area, range);
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
    let y = r.y + 14.0;
    c.text_mid(
        metric.spec().label,
        (left, y),
        12.0,
        Weight::Regular,
        Align::Left,
        style.ink2,
    );

    let size = (r.h * 0.36).clamp(15.0, 22.0);
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
    match capacity(metric, v.monitor).filter(|t| *t > 0.0) {
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
    match item {
        StripItem::Cpu => {
            let metric = monitor.cpu_temp_metric();
            let label = if metric == MetricId::CpuTemp {
                "CPU"
            } else {
                "I/O"
            };
            let (text, severity) = degrees(metric);
            (label, text, severity)
        }
        StripItem::Gpu => {
            let (text, severity) = degrees(MetricId::GpuTemp);
            ("GPU", text, severity)
        }
        StripItem::Hotspot => {
            let (text, severity) = degrees(MetricId::GpuHotspot);
            ("Hot", text, severity)
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
    plan_strip(v.layout.width, v.layout.height, |items, size| {
        let widths: f32 = items
            .iter()
            .map(|i| strip_item_width(c, *i, v.monitor, size))
            .sum();
        widths + strip_gap(size) * items.len().saturating_sub(1) as f32
    })
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
