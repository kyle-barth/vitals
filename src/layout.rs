//! Window layout. Pure geometry with no drawing or Win32, so that every
//! window size can be tested. All values are device-independent pixels.
//!
//! The window moves between these modes as it is resized:
//! - `Strip`: a row of fixed slots for the key numbers, with no title bar.
//! - `Rows`: a single column of metric rows, for tall, thin windows.
//! - `Compact`: the three temperatures and their history in a framed card,
//!   then one card of readings, which gives up sections as height runs out.
//! - `Large`: the same framed card with a full chart, six readings beside it,
//!   and cards below for what needs attention, the GPU and the CPU.
//! - `Table`: the Apps page, which shows more columns as width grows.
//!
//! Window size decides how many things are shown. Simple mode decides how much
//! is said about each one: it gives up the history, never a reading.

use crate::apps::{COLUMNS, SIMPLE_COLUMNS, SortKey};
use crate::metrics::Absent;
use crate::metrics::MetricId::{self, *};

pub const TITLE_H: f32 = 38.0;
/// The margin around each page.
pub const PAD: f32 = 12.0;
pub const GAP: f32 = 6.0;
/// The space between the Vitals page's cards.
pub const CARD_GAP: f32 = 8.0;
const ROW_GAP: f32 = 4.0;
pub const MIN_ROW_H: f32 = 50.0;
const ROWS_BELOW_WIDTH: f32 = 240.0;
const LARGE_W: f32 = 820.0;
const LARGE_H: f32 = 600.0;
const APP_ROW_H: f32 = 30.0;
const APP_ROW_GAP: f32 = 4.0;
/// From this width, busy apps sit side by side.
const APP_COLUMNS_W: f32 = 500.0;
/// The most rows the list may take, however many apps are busy.
const MAX_APP_ROWS: usize = 3;
const STRIP_INSET: f32 = 4.0;
const CONTROL_H: f32 = 20.0;
const CONTROL_INSET: f32 = 6.0;
const END_W: f32 = 40.0;
const CONFIRM_W: f32 = 46.0;
const KEEP_W: f32 = 42.0;
/// A column of rows must show at least this many, otherwise the strip is better.
const MIN_ROWS: usize = 4;

/// Below this width, minimize and maximize step aside for the tabs, and the
/// tabs and buttons that remain draw closer together.
const ALL_BUTTONS_W: f32 = 312.0;
/// Each tab fits its name in 12px semibold, with 11px either side.
const TABS: [(Page, f32); 3] = [
    (Page::Vitals, 52.0),
    (Page::Apps, 50.0),
    (Page::Specs, 53.0),
];
const COMPACT_TABS: [(Page, f32); 3] = [
    (Page::Vitals, 37.0),
    (Page::Apps, 34.0),
    (Page::Specs, 39.0),
];
/// Where the first tab starts, 2px inside the control that holds them.
const TABS_X: f32 = 14.0;
const COMPACT_TABS_X: f32 = 6.0;
/// The Specs page needs its title bar, its copy band and at least one row.
const SPECS_MIN_H: f32 = 130.0;
const SPECS_BAND_H: f32 = 30.0;
/// Wide enough for "Couldn't copy" beside its glyph, so the button keeps one
/// size whatever it says.
const COPY_W: f32 = 112.0;
const COPY_H: f32 = 24.0;
/// From this width, the CPU side and the GPU side of the PC get a column each.
const SPECS_COLUMNS_W: f32 = 670.0;
/// Narrower than this, a part's label goes above its name rather than beside it.
const SPECS_STACK_BELOW: f32 = 250.0;
/// The column of part labels, which fits "Motherboard".
pub const SPECS_LABEL_W: f32 = 80.0;
pub const SPECS_ITEM_H: f32 = 38.0;
pub const SPECS_FACT_H: f32 = 20.0;
/// Details need room for a label and a value beside the part's own label.
const SPECS_DETAIL_MIN_W: f32 = 400.0;

const TABLE_MIN_H: f32 = 110.0;
const TABLE_SUMMARY_MIN_H: f32 = 180.0;
const TABLE_SUMMARY_H: f32 = 24.0;
const TABLE_HEADER_H: f32 = 24.0;
const TABLE_ROW_H: f32 = 26.0;
/// Space inside a row, either side of its content.
pub const TABLE_ROW_PAD: f32 = 6.0;
const MIN_COLUMN_W: f32 = 60.0;
const MAX_COLUMN_W: f32 = 120.0;
/// The name column never gets narrower than this.
const MIN_NAME_W: f32 = 90.0;
/// Columns widen once the name column has this much.
const COMFORTABLE_NAME_W: f32 = 220.0;

/// The sizes the menu offers, in device-independent pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Small,
    Medium,
    Large,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Small, Preset::Medium, Preset::Large];

    pub const fn size(self) -> (i32, i32) {
        match self {
            Preset::Small => (320, 72),
            Preset::Medium => (340, 520),
            Preset::Large => (1240, 760),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Preset::Small => "Small",
            Preset::Medium => "Medium",
            Preset::Large => "Large",
        }
    }

    pub fn matching(size: (i32, i32)) -> Option<Preset> {
        Self::ALL.into_iter().find(|preset| preset.size() == size)
    }
}

/// Where a window of `size` should go, given where it is now. All rectangles are
/// left, top, right, bottom in pixels.
///
/// The corner nearest the edge of the screen stays put, so a window tucked into a
/// corner stays in that corner. It never ends up off screen or larger than it.
pub fn fit_window(
    now: (i32, i32, i32, i32),
    work: (i32, i32, i32, i32),
    size: (i32, i32),
) -> (i32, i32, i32, i32) {
    let width = size.0.min(work.2 - work.0);
    let height = size.1.min(work.3 - work.1);
    let keep_right = now.0 + now.2 > work.0 + work.2;
    let keep_bottom = now.1 + now.3 > work.1 + work.3;
    let left = if keep_right { now.2 - width } else { now.0 };
    let top = if keep_bottom { now.3 - height } else { now.1 };
    let left = left.clamp(work.0, work.2 - width);
    let top = top.clamp(work.1, work.3 - height);
    (left, top, left + width, top + height)
}

/// How many columns a mode offers, however wide the window is.
pub fn allowed_columns(simple: bool) -> usize {
    if simple {
        SIMPLE_COLUMNS
    } else {
        COLUMNS.len()
    }
}

/// The number columns to show at a width. The sorted column is always among them,
/// since the order would otherwise make no sense.
pub fn table_columns(width: f32, simple: bool, sort: SortKey) -> Vec<SortKey> {
    let room = (width - PAD * 2.0 - TABLE_ROW_PAD * 2.0 - MIN_NAME_W) / MIN_COLUMN_W;
    let room = (room.floor().max(1.0) as usize).min(allowed_columns(simple));
    let mut shown = COLUMNS[..room].to_vec();
    if sort != SortKey::Name && !shown.contains(&sort) {
        shown[room - 1] = sort;
    }
    shown
}

const ROWS: [MetricId; 11] = [
    CpuTemp,
    GpuTemp,
    GpuHotspot,
    Memory,
    VideoMemory,
    CpuLoad,
    GpuLoad,
    VramTemp,
    CpuPower,
    GpuPower,
    GpuFan,
];

/// The three readings at the top of the Vitals page: the CPU, the GPU's edge
/// and its hotspot. A PC that can't report one gets its nearest stand-in.
pub fn hero_metrics(absent: Absent) -> [MetricId; 3] {
    let cpu = if absent.contains(CpuTemp) && absent.contains(IoDieTemp) {
        CpuLoad
    } else {
        CpuTemp
    };
    let gpu = if absent.contains(GpuTemp) {
        GpuLoad
    } else {
        GpuTemp
    };
    let third = if !absent.contains(GpuHotspot) {
        GpuHotspot
    } else if gpu != GpuLoad {
        GpuLoad
    } else {
        Memory
    };
    [cpu, gpu, third]
}

/// The six readings beside a large window's chart, CPU on the left and GPU on
/// the right, with stand-ins for anything absent or already in the hero.
fn large_tiles(hero: [MetricId; 3], absent: Absent) -> [MetricId; 6] {
    const WANTED: [MetricId; 6] = [CpuLoad, GpuLoad, CpuPower, GpuPower, Memory, VideoMemory];
    const CPU_SPARES: [MetricId; 4] = [CpuClock, DiskRead, Download, DiskWrite];
    const GPU_SPARES: [MetricId; 5] = [GpuClock, GpuFan, GpuFanPercent, Upload, DiskWrite];
    let mut shown: Vec<MetricId> = Vec::with_capacity(6);
    for (i, &metric) in WANTED.iter().enumerate() {
        let free = |m: &MetricId| !absent.contains(*m) && !hero.contains(m) && !shown.contains(m);
        let spares: &[MetricId] = if i % 2 == 0 { &CPU_SPARES } else { &GPU_SPARES };
        let pick = std::iter::once(metric)
            .chain(spares.iter().copied())
            .find(free)
            .unwrap_or(metric);
        shown.push(pick);
    }
    [shown[0], shown[1], shown[2], shown[3], shown[4], shown[5]]
}

/// Readings that fill the gap left by something this PC cannot report, in the
/// order they are used.
const SPARES: [MetricId; 6] = [GpuClock, CpuClock, DiskRead, DiskWrite, Download, Upload];

/// Close relatives of a metric, tried before the spares, so that a CPU tile is
/// replaced by a CPU tile and a GPU tile by a GPU tile where possible.
fn stand_ins(metric: MetricId) -> &'static [MetricId] {
    match metric {
        CpuTemp | CpuPower => &[CpuClock],
        GpuHotspot | VramTemp => &[GpuClock],
        GpuFan => &[GpuFanPercent],
        _ => &[],
    }
}

/// `order` with each metric this PC cannot report swapped, in its place, for a
/// stand-in or a spare. A stand-in that was further down moves up rather than
/// appearing twice. With nothing absent, this is `order` unchanged.
pub fn fill(order: &[MetricId], absent: Absent) -> Vec<MetricId> {
    let mut shown: Vec<MetricId> = Vec::with_capacity(order.len());
    for &metric in order {
        if shown.contains(&metric) {
            continue;
        }
        if !absent.contains(metric) {
            shown.push(metric);
            continue;
        }
        let pick = stand_ins(metric)
            .iter()
            .chain(&SPARES)
            .copied()
            .find(|m| !absent.contains(*m) && !shown.contains(m));
        shown.extend(pick);
    }
    for spare in SPARES {
        if shown.len() >= order.len() {
            break;
        }
        if !absent.contains(spare) && !shown.contains(&spare) {
            shown.push(spare);
        }
    }
    shown
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Strip,
    Rows,
    Compact,
    Large,
    Table,
    Specs,
}

/// Which tab is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Vitals,
    Apps,
    Specs,
}

/// What the Specs page needs to know about a part to make room for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PartShape {
    /// How many devices it lists, such as drives.
    pub items: usize,
    /// How many label and value pairs it has.
    pub facts: usize,
    /// Whether it belongs on the GPU's side when there are two columns.
    pub right: bool,
}

/// Everything besides the window's size that changes its layout.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// How many busy apps want a row.
    pub apps: usize,
    /// Whether the first of them is frozen.
    pub frozen: bool,
    pub page: Page,
    pub simple: bool,
    pub sort: SortKey,
    /// What this PC cannot report, which the tiles leave out.
    pub absent: Absent,
    /// The parts on the Specs page, once they have been read.
    pub parts: Vec<PartShape>,
}

/// A reading that can be clicked, and that counts as on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub metric: MetricId,
    pub rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Menu,
    Pin,
    Minimize,
    Maximize,
    Close,
}

/// The framed card at the top of the Vitals page, with its three temperatures.
#[derive(Clone, Debug, PartialEq)]
pub struct Hero {
    pub rect: Rect,
    pub metrics: [MetricId; 3],
    /// The left edge of each reading. The readings below line up with them.
    pub columns: [f32; 3],
    /// The distance from one reading to the next.
    pub pitch: f32,
    /// The size of the big numbers.
    pub number: f32,
    /// The baseline of the readings' names.
    pub label_y: f32,
}

impl Hero {
    /// The baseline of the big numbers.
    pub fn number_y(&self) -> f32 {
        (self.label_y + 8.0 + self.number * 0.74).round()
    }
}

/// A part of the readings card in a compact window, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// CPU, GPU and memory use, side by side.
    Stats,
    /// Three detail rows, such as the VRAM's temperature.
    Details,
    /// Network and disk.
    Io,
}

impl Section {
    pub const fn height(self) -> f32 {
        match self {
            Section::Stats => 86.0,
            Section::Details => 102.0,
            Section::Io => 64.0,
        }
    }
}

/// The Vitals page below Large.
#[derive(Clone, Debug, PartialEq)]
pub struct Compact {
    pub hero: Hero,
    /// The plot of the last two minutes. Its times go in the 15px below it.
    pub chart: Option<Rect>,
    /// The baseline of the one-line summary under the chart.
    pub summary_y: f32,
    pub card: Option<Rect>,
    /// The sections that fit in the card, and where each one starts.
    pub sections: Vec<(Section, f32)>,
}

/// The Vitals page in a large window.
#[derive(Clone, Debug, PartialEq)]
pub struct Large {
    pub hero: Hero,
    /// The chart, from its axis labels on the left to its end labels on the
    /// right. Its times go in the 15px below it.
    pub chart: Option<Rect>,
    /// Where the hotspot's gap and time at its limit go, right of the readings.
    pub side_x: f32,
    pub tiles: [(MetricId, Rect); 6],
    pub attention: Rect,
    pub gpu: Rect,
    pub cpu: Rect,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub mode: Mode,
    pub cells: Vec<Cell>,
    pub buttons: Vec<(Button, Rect)>,
    /// One row per busy app shown, in the same order as the apps.
    pub app_rows: Vec<Rect>,
    pub tabs: Vec<(Page, Rect)>,
    /// The page actually showing. A strip has no room for Apps, whatever is chosen.
    pub page: Page,
    pub table: Option<Table>,
    pub specs: Option<SpecsPage>,
    pub compact: Option<Compact>,
    pub large: Option<Large>,
    /// The strip's slots, when the window is a strip.
    pub strip: Option<StripPlan>,
}

/// The Specs page: a band with the copy button, then a row for each part.
#[derive(Clone, Debug, PartialEq)]
pub struct SpecsPage {
    pub band: Rect,
    pub copy: Rect,
    /// Where the rows go.
    pub view: Rect,
    pub column_w: f32,
    /// Each column's left edge, and its parts as (index, height).
    pub columns: Vec<(f32, Vec<(usize, f32)>)>,
    /// Labels sit above names, in a narrow window.
    pub stacked: bool,
    /// Every part shows its details, because they all fit.
    pub detailed: bool,
}

impl SpecsPage {
    /// The parts on screen, and where they go, with `scroll` rows hidden at
    /// the top of each column. Only whole rows are shown, so no text is cut.
    pub fn visible(&self, scroll: usize) -> Vec<(usize, Rect)> {
        let mut shown = Vec::new();
        for (x, rows) in &self.columns {
            let mut y = self.view.y;
            for &(index, h) in rows.iter().skip(scroll) {
                let room = self.view.bottom() - y;
                if h > room + 0.01 {
                    // A part taller than the whole view, such as a long list
                    // of drives in a short window, still shows when it is at
                    // the top, cut to the devices that fit.
                    if y == self.view.y && room > 0.0 {
                        shown.push((index, Rect::new(*x, y, self.column_w, room)));
                    }
                    break;
                }
                shown.push((index, Rect::new(*x, y, self.column_w, h)));
                y += h + GAP;
            }
        }
        shown
    }

    /// The most rows that can be scrolled away before the last one shows.
    pub fn max_scroll(&self) -> usize {
        self.columns
            .iter()
            .map(|(_, rows)| {
                (0..rows.len())
                    .find(|&skip| column_height(&rows[skip..]) <= self.view.h + 0.01)
                    .unwrap_or(rows.len().saturating_sub(1))
            })
            .max()
            .unwrap_or(0)
    }

    /// Rows in the longest column, for the mark that shows the scroll position.
    pub fn longest(&self) -> usize {
        self.columns
            .iter()
            .map(|(_, rows)| rows.len())
            .max()
            .unwrap_or(0)
    }
}

fn column_height(rows: &[(usize, f32)]) -> f32 {
    let gaps = rows.len().saturating_sub(1) as f32 * GAP;
    rows.iter().map(|(_, h)| h).sum::<f32>() + gaps
}

/// A part's height: a line pair per device, or its name and details.
fn part_height(shape: PartShape, stacked: bool, detailed: bool) -> f32 {
    if detailed && shape.items == 1 && shape.facts > 0 {
        return 44.0 + shape.facts as f32 * SPECS_FACT_H;
    }
    let items = shape.items.max(1) as f32 * SPECS_ITEM_H;
    if stacked { 22.0 + items } else { 10.0 + items }
}

fn specs_page(width: f32, height: f32, parts: &[PartShape], simple: bool) -> SpecsPage {
    let inner = width - PAD * 2.0;
    let band = Rect::new(PAD, TITLE_H, inner, SPECS_BAND_H);
    let copy = Rect::new(
        width - PAD - COPY_W,
        TITLE_H + (SPECS_BAND_H - COPY_H) / 2.0,
        COPY_W,
        COPY_H,
    );
    let top = band.bottom();
    let view = Rect::new(PAD, top, inner, (height - PAD - top).max(0.0));
    let two = width >= SPECS_COLUMNS_W;
    let column_w = if two { (inner - GAP) / 2.0 } else { inner };
    let stacked = column_w < SPECS_STACK_BELOW;
    // Each column stacks on its own, so a long list of drives only makes its
    // own column longer.
    let arrange = |detailed: bool| {
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for (index, shape) in parts.iter().enumerate() {
            let h = part_height(*shape, stacked, detailed);
            if two && shape.right {
                right.push((index, h));
            } else {
                left.push((index, h));
            }
        }
        let mut columns = vec![(PAD, left)];
        if two {
            columns.push((PAD + column_w + GAP, right));
        }
        columns
    };
    let can_detail = !simple && !stacked && column_w >= SPECS_DETAIL_MIN_W;
    let full = arrange(true);
    let detailed = can_detail && full.iter().all(|(_, rows)| column_height(rows) <= view.h);
    SpecsPage {
        band,
        copy,
        view,
        column_w,
        columns: if detailed { full } else { arrange(false) },
        stacked,
        detailed,
    }
}

/// The Apps page.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    /// The row of key numbers, when there is room for one.
    pub summary: Option<Rect>,
    pub header: Rect,
    pub body: Rect,
    /// Each number column, with the x of its right edge.
    pub columns: Vec<(SortKey, f32)>,
    pub column_w: f32,
    /// How many whole rows fit.
    pub visible: usize,
}

impl Table {
    /// Where the `slot`th row on screen goes.
    pub fn row(&self, slot: usize) -> Rect {
        Rect::new(
            self.body.x,
            self.body.y + slot as f32 * TABLE_ROW_H,
            self.body.w,
            TABLE_ROW_H,
        )
    }

    /// The row on screen at a point.
    pub fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.visible).find(|&slot| self.row(slot).contains(x, y))
    }

    /// The left edge of the number columns, which is where names must stop.
    pub fn names_end(&self) -> f32 {
        self.columns
            .first()
            .map_or(self.body.right(), |(_, right)| right - self.column_w)
    }

    /// The column heading at a point.
    pub fn header_at(&self, x: f32, y: f32) -> Option<SortKey> {
        if !self.header.contains(x, y) {
            return None;
        }
        let column = self
            .columns
            .iter()
            .find(|(_, right)| x >= right - self.column_w && x < right + TABLE_ROW_PAD);
        Some(column.map_or(SortKey::Name, |(key, _)| *key))
    }
}

impl Layout {
    #[cfg(test)]
    pub fn shows(&self, metric: MetricId) -> bool {
        self.cells.iter().any(|c| c.metric == metric)
    }

    /// Whether a frozen app has taken over a one-line strip entirely.
    pub fn strip_is_replaced(&self) -> bool {
        self.mode == Mode::Strip && self.app_rows.first().is_some_and(|r| r.y <= STRIP_INSET)
    }

    pub fn app_at(&self, x: f32, y: f32) -> Option<usize> {
        self.app_rows.iter().position(|r| r.contains(x, y))
    }

    pub fn tab_at(&self, x: f32, y: f32) -> Option<Page> {
        self.tabs
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(page, _)| *page)
    }

    pub fn button_at(&self, x: f32, y: f32) -> Option<Button> {
        self.buttons
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(b, _)| *b)
    }

    pub fn cell_at(&self, x: f32, y: f32) -> Option<MetricId> {
        self.cells
            .iter()
            .find(|c| c.rect.contains(x, y))
            .map(|c| c.metric)
    }
}

/// Lays out the window. The Apps page needs a title bar and some rows, so a very
/// short window shows the strip whichever page is open.
pub fn compute(width: f32, height: f32, options: Options) -> Layout {
    if options.page == Page::Apps && height >= TABLE_MIN_H {
        let mut layout = blank(width, height);
        layout.mode = Mode::Table;
        layout.page = Page::Apps;
        layout.table = Some(table(width, height, &options));
        layout.buttons = buttons(width, Mode::Table);
        layout.tabs = tabs(width);
        return layout;
    }
    if options.page == Page::Specs && height >= SPECS_MIN_H {
        let mut layout = blank(width, height);
        layout.mode = Mode::Specs;
        layout.page = Page::Specs;
        layout.specs = Some(specs_page(width, height, &options.parts, options.simple));
        layout.buttons = buttons(width, Mode::Specs);
        layout.tabs = tabs(width);
        return layout;
    }
    vitals(width, height, options)
}

fn table(width: f32, height: f32, options: &Options) -> Table {
    let tall = height >= TABLE_SUMMARY_MIN_H;
    let (left, inner) = (PAD, width - PAD * 2.0);
    let summary = tall.then(|| Rect::new(left, TITLE_H, inner, TABLE_SUMMARY_H));
    let header_y = TITLE_H + if tall { TABLE_SUMMARY_H } else { 0.0 };
    let header = Rect::new(left, header_y, inner, TABLE_HEADER_H);
    let top = header.bottom() + 2.0;
    let body = Rect::new(left, top, inner, (height - PAD - top).max(0.0));

    let shown = table_columns(width, options.simple, options.sort);
    let count = shown.len() as f32;
    let spare = inner - TABLE_ROW_PAD * 2.0 - COMFORTABLE_NAME_W;
    let column_w = (spare / count).clamp(MIN_COLUMN_W, MAX_COLUMN_W);
    let last = body.right() - TABLE_ROW_PAD;
    let columns = shown
        .iter()
        .enumerate()
        .map(|(i, key)| (*key, last - (count - 1.0 - i as f32) * column_w))
        .collect();
    Table {
        summary,
        header,
        body,
        columns,
        column_w,
        visible: (body.h / TABLE_ROW_H).floor() as usize,
    }
}

/// Lays out the Vitals page. The list of busy apps takes room from the cards,
/// and gives up rows before the page would turn into the strip, or a large
/// window would lose its layout. With no apps, this is the window as it always is.
fn vitals(width: f32, height: f32, options: Options) -> Layout {
    let Options {
        apps,
        frozen,
        simple,
        absent,
        ..
    } = options;
    let columns = if width >= APP_COLUMNS_W && apps >= 2 {
        2
    } else {
        1
    };
    let wants_large = width >= LARGE_W && height >= LARGE_H;
    for count in (1..=apps.min(MAX_APP_ROWS * columns)).rev() {
        let rows = count.div_ceil(columns);
        let list_h = rows as f32 * APP_ROW_H + (rows - 1) as f32 * APP_ROW_GAP;
        let mut layout = arrange(width, height, list_h + CARD_GAP, simple, absent);
        if layout.mode == Mode::Strip || (wants_large && layout.mode != Mode::Large) {
            continue;
        }
        let cell_w = (width - PAD * 2.0 - CARD_GAP * (columns - 1) as f32) / columns as f32;
        let top = height - PAD - list_h;
        layout.app_rows = (0..count)
            .map(|i| {
                let (col, row) = ((i % columns) as f32, (i / columns) as f32);
                Rect::new(
                    PAD + col * (cell_w + CARD_GAP),
                    top + row * (APP_ROW_H + APP_ROW_GAP),
                    cell_w,
                    APP_ROW_H,
                )
            })
            .collect();
        return layout;
    }
    let mut layout = arrange(width, height, 0.0, simple, absent);
    if layout.mode != Mode::Strip {
        return layout;
    }
    if apps > 0 {
        if height >= STRIP_TWO_LINES_H {
            let row_h = 26.0;
            layout.app_rows = vec![Rect::new(
                STRIP_INSET,
                height - row_h - STRIP_INSET,
                width - STRIP_INSET * 2.0,
                row_h,
            )];
        } else if frozen {
            // One line has no room for both, and a frozen app needs acting on.
            layout.app_rows = vec![Rect::new(
                STRIP_INSET,
                STRIP_INSET,
                width - STRIP_INSET * 2.0,
                height - STRIP_INSET * 2.0,
            )];
            layout.buttons.clear();
        }
    }
    if !layout.strip_is_replaced() {
        let slots_h = layout.app_rows.first().map_or(height, |row| row.y);
        layout.strip = Some(plan_strip(width, slots_h, absent));
    }
    layout
}

/// The End button on an app's row.
pub fn end_button(row: Rect) -> Rect {
    Rect::new(
        row.right() - CONTROL_INSET - END_W,
        row.y + (row.h - CONTROL_H) / 2.0,
        END_W,
        CONTROL_H,
    )
}

/// The End and Keep buttons shown while confirming.
pub fn confirm_buttons(row: Rect) -> (Rect, Rect) {
    let y = row.y + (row.h - CONTROL_H) / 2.0;
    let keep = Rect::new(row.right() - CONTROL_INSET - KEEP_W, y, KEEP_W, CONTROL_H);
    let end = Rect::new(keep.x - CONFIRM_W - 4.0, y, CONFIRM_W, CONTROL_H);
    (end, keep)
}

fn blank(width: f32, height: f32) -> Layout {
    Layout {
        width,
        height,
        mode: Mode::Strip,
        cells: Vec::new(),
        buttons: Vec::new(),
        app_rows: Vec::new(),
        tabs: Vec::new(),
        page: Page::Vitals,
        table: None,
        specs: None,
        compact: None,
        large: None,
        strip: None,
    }
}

fn tabs(window_w: f32) -> Vec<(Page, Rect)> {
    let (set, mut x) = if window_w < ALL_BUTTONS_W {
        (&COMPACT_TABS, COMPACT_TABS_X)
    } else {
        (&TABS, TABS_X)
    };
    set.iter()
        .map(|&(page, width)| {
            let rect = Rect::new(x, 0.0, width, TITLE_H);
            x += width;
            (page, rect)
        })
        .collect()
}

/// `band` is kept free at the bottom, for the list of busy apps.
fn arrange(width: f32, height: f32, band: f32, simple: bool, absent: Absent) -> Layout {
    let mut layout = blank(width, height);
    let bottom = height - PAD - band;
    if width < ROWS_BELOW_WIDTH {
        let body = (bottom - TITLE_H).max(0.0);
        let order = fill(&ROWS, absent);
        let count = (((body + ROW_GAP) / (MIN_ROW_H + ROW_GAP)).floor() as usize).min(order.len());
        if count >= MIN_ROWS {
            layout.mode = Mode::Rows;
            rows(&mut layout, &order[..count], body);
        }
    } else if let Some((large, cells)) =
        large(width, bottom, simple, absent).filter(|_| width >= LARGE_W && height >= LARGE_H)
    {
        layout.mode = Mode::Large;
        layout.cells = cells;
        layout.large = Some(large);
    } else if let Some((compact, cells)) = compact(width, bottom, simple, absent) {
        layout.mode = Mode::Compact;
        layout.cells = cells;
        layout.compact = Some(compact);
    }
    layout.buttons = buttons(width, layout.mode);
    if layout.mode != Mode::Strip {
        layout.tabs = tabs(width);
    }
    layout
}

fn rows(layout: &mut Layout, order: &[MetricId], body: f32) {
    let count = order.len() as f32;
    let row_h = (body - ROW_GAP * (count - 1.0)) / count;
    layout.cells = order
        .iter()
        .enumerate()
        .map(|(i, &metric)| {
            let rect = Rect::new(
                PAD,
                TITLE_H + i as f32 * (row_h + ROW_GAP),
                layout.width - PAD * 2.0,
                row_h,
            );
            Cell { metric, rect }
        })
        .collect();
}

/// The chart's height in the Medium window. A taller window gives it the extra.
const CHART_H: f32 = 66.0;
/// Shorter than this, a chart says too little to be worth its room.
const CHART_MIN_H: f32 = 40.0;
/// From the numbers' baseline to the chart.
const CHART_OFFSET: f32 = 39.0;
/// From the chart's bottom to the card's: its times, then the summary.
const CHART_FOOT: f32 = 48.0;
/// From the numbers' baseline to the card's bottom, with no chart.
const PLAIN_FOOT: f32 = 58.0;

/// The readings in the compact card's top section: CPU, GPU and memory use,
/// or a clock where the load is already a big number above.
pub fn stats_metrics(hero: [MetricId; 3]) -> [MetricId; 3] {
    let cpu = if hero.contains(&CpuLoad) {
        CpuClock
    } else {
        CpuLoad
    };
    let gpu = if hero.contains(&GpuLoad) {
        GpuClock
    } else {
        GpuLoad
    };
    [cpu, gpu, Memory]
}

/// The compact Vitals page, filling down to `bottom`, if there is room for at
/// least its three temperatures. The readings card gives up its sections from
/// the bottom, then the chart shrinks, then goes.
fn compact(width: f32, bottom: f32, simple: bool, absent: Absent) -> Option<(Compact, Vec<Cell>)> {
    let top = TITLE_H + 4.0;
    // The readings sit 108px apart in the 340px Medium window, with the last
    // one 32px narrower, beside the card's edge.
    let pitch = (width - 16.0) / 3.0;
    let first = PAD + 12.0;
    let hero = Hero {
        rect: Rect::new(PAD, top, width - PAD * 2.0, 0.0),
        metrics: hero_metrics(absent),
        columns: [first, first + pitch, first + pitch * 2.0],
        pitch,
        number: (pitch * 0.315).round().clamp(24.0, 48.0),
        label_y: top + 20.0,
    };
    let head = hero.number_y() - top;
    let with_chart = |chart_h: f32| head + CHART_OFFSET + chart_h + CHART_FOOT;
    let plain = head + PLAIN_FOOT;
    let room = bottom - top;
    const ALL: [Section; 3] = [Section::Stats, Section::Details, Section::Io];
    let card_part = |shown: usize| {
        let h: f32 = ALL[..shown].iter().map(|s| s.height()).sum();
        if shown > 0 { CARD_GAP + h } else { 0.0 }
    };
    let mut pick: Option<(Option<f32>, usize)> = None;
    if !simple {
        pick = (0..=ALL.len()).rev().find_map(|shown| {
            let spare = room - with_chart(CHART_H) - card_part(shown);
            (spare >= -0.01).then(|| (Some(CHART_H + spare.max(0.0)), shown))
        });
        if pick.is_none() && with_chart(CHART_MIN_H) <= room {
            pick = Some((Some(room - with_chart(0.0)), 0));
        }
    }
    if pick.is_none() {
        pick = (0..=ALL.len())
            .rev()
            .find(|&shown| plain + card_part(shown) <= room + 0.01)
            .map(|shown| (None, shown));
    }
    let (chart_h, shown) = pick?;

    let hero_h = chart_h.map_or(plain, with_chart);
    let hero = Hero {
        rect: Rect {
            h: hero_h,
            ..hero.rect
        },
        ..hero
    };
    let right = hero.rect.right();
    let chart = chart_h.map(|h| {
        let x = hero.columns[0];
        Rect::new(x, hero.number_y() + CHART_OFFSET, right - 20.0 - x, h)
    });
    let card_y = hero.rect.bottom() + CARD_GAP;
    let card =
        (shown > 0).then(|| Rect::new(PAD, card_y, hero.rect.w, card_part(shown) - CARD_GAP));
    let mut sections = Vec::new();
    let mut y = card_y;
    for section in &ALL[..shown] {
        sections.push((*section, y));
        y += section.height();
    }

    // What can be clicked, and what counts as on screen.
    let column = |i: usize, y: f32, h: f32, metric: MetricId| {
        let x = hero.columns[i] - 8.0;
        Cell {
            metric,
            rect: Rect::new(x, y, hero.pitch.min(right - x), h),
        }
    };
    let mut cells: Vec<Cell> = (0..3)
        .map(|i| column(i, top, hero.number_y() + 30.0 - top, hero.metrics[i]))
        .collect();
    for &(section, y) in &sections {
        match section {
            Section::Stats => {
                let stats = stats_metrics(hero.metrics);
                cells.extend((0..3).map(|i| column(i, y, section.height(), stats[i])));
            }
            Section::Details if !absent.contains(VramTemp) => cells.push(Cell {
                metric: VramTemp,
                rect: Rect::new(PAD, y + 42.0 - 11.5, hero.rect.w, 23.0),
            }),
            _ => {}
        }
    }
    let compact = Compact {
        summary_y: hero.rect.bottom() - 12.0,
        hero,
        chart,
        card,
        sections,
    };
    Some((compact, cells))
}

/// The large Vitals page, on a grid of 12 columns with 8px gutters, filling
/// down to `bottom`. The hero takes 8 columns and the readings beside it 4,
/// over three cards of 4 columns each.
fn large(width: f32, bottom: f32, simple: bool, absent: Absent) -> Option<(Large, Vec<Cell>)> {
    const HERO_MIN_H: f32 = 232.0;
    let top = TITLE_H + 10.0;
    let column_w = (width - PAD * 2.0 - CARD_GAP * 11.0) / 12.0;
    let span = |n: f32| n * column_w + (n - 1.0) * CARD_GAP;
    let column_x = |i: f32| PAD + i * (column_w + CARD_GAP);
    let room = bottom - top - CARD_GAP;
    let lower = (room * 0.468).round().clamp(220.0, 400.0);
    let hero_h = room - lower;
    if hero_h < HERO_MIN_H {
        return None;
    }
    // The design is set at 1240px, where the hero is 808px wide.
    let hero_w = span(8.0);
    let scale = hero_w / 808.0;
    let first = PAD + 16.0;
    let pitch = 180.0 * scale;
    // Simple mode has no chart, so its numbers grow to fill the card, centred.
    let (number, label_y) = if simple {
        let number = (hero_h * 0.3).min(pitch * 0.5).clamp(48.0, 112.0).floor();
        let block = 45.0 + number * 0.74;
        (number, (top + (hero_h - block) / 2.0 + 12.0).round())
    } else {
        ((48.0 * scale).round().clamp(48.0, 64.0), top + 30.0)
    };
    let hero = Hero {
        rect: Rect::new(PAD, top, hero_w, hero_h),
        metrics: hero_metrics(absent),
        columns: [first, first + pitch, first + pitch * 2.0],
        pitch,
        number,
        label_y,
    };
    let chart = (!simple).then(|| Rect::new(first, top + 132.0, hero_w - 32.0, hero_h - 172.0));
    let tile_w = span(2.0);
    let tile_h = (hero_h - CARD_GAP * 2.0) / 3.0;
    let metrics = large_tiles(hero.metrics, absent);
    let tiles: [(MetricId, Rect); 6] = std::array::from_fn(|i| {
        let x = column_x(8.0) + (i % 2) as f32 * (tile_w + CARD_GAP);
        let y = top + (i / 2) as f32 * (tile_h + CARD_GAP);
        (metrics[i], Rect::new(x, y, tile_w, tile_h))
    });
    let lower_y = top + hero_h + CARD_GAP;
    let card = |i: f32| Rect::new(column_x(i), lower_y, span(4.0), lower);
    let large = Large {
        side_x: first + 560.0 * scale,
        chart,
        tiles,
        attention: card(0.0),
        gpu: card(4.0),
        cpu: card(8.0),
        hero,
    };
    let mut cells: Vec<Cell> = (0..3)
        .map(|i| Cell {
            metric: large.hero.metrics[i],
            rect: Rect::new(
                large.hero.columns[i] - 10.0,
                top,
                pitch,
                large.hero.number_y() + 30.0 - top,
            ),
        })
        .collect();
    cells.extend(tiles.iter().map(|&(metric, rect)| Cell { metric, rect }));
    if !absent.contains(VramTemp) {
        let gpu = large.gpu;
        cells.push(Cell {
            metric: VramTemp,
            rect: Rect::new(gpu.x, gpu.y + 48.0, gpu.w, 32.0),
        });
    }
    Some((large, cells))
}

fn buttons(width: f32, mode: Mode) -> Vec<(Button, Rect)> {
    let (size, height) = if mode == Mode::Strip {
        (24.0, 24.0)
    } else if width < ALL_BUTTONS_W {
        (24.0, TITLE_H)
    } else {
        (28.0, TITLE_H)
    };
    // Close, pin and the menu are kept at every width, because the menu is the
    // way back from the strip. The others step aside when narrow.
    let order: &[Button] = if mode == Mode::Strip || width < ALL_BUTTONS_W {
        &[Button::Close, Button::Pin, Button::Menu]
    } else {
        &[
            Button::Close,
            Button::Maximize,
            Button::Minimize,
            Button::Pin,
            Button::Menu,
        ]
    };
    order
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            (
                b,
                Rect::new(width - size * (i + 1) as f32, 0.0, size, height),
            )
        })
        .collect()
}

/// One slot in the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripItem {
    Cpu,
    Gpu,
    Hotspot,
    GpuLoad,
    CpuLoad,
    Ram,
    Vram,
}

/// The strip: slots of one width that never move, and one kept free at the
/// right for an alarm about something the strip has no slot for.
#[derive(Clone, Debug, PartialEq)]
pub struct StripPlan {
    /// The size of the numbers.
    pub size: f32,
    pub slots: Vec<(StripItem, Rect)>,
    pub alarm: Rect,
}

const STRIP_MIN_SIZE: f32 = 12.0;
const STRIP_MAX_SIZE: f32 = 30.0;
/// Numbers smaller than this give up the fourth slot before shrinking further.
const STRIP_FOUR_MIN_SIZE: f32 = 20.0;
const STRIP_TWO_LINES_H: f32 = 64.0;
const STRIP_SLOT_GAP: f32 = 4.0;
/// A slot needs this many times its number's size, to fit "110°".
const SLOT_PER_SIZE: f32 = 2.4;
const ALARM_PER_SIZE: f32 = 1.55;
/// The alarm slot is never narrower than this, so "VRAM" always fits.
const ALARM_MIN_W: f32 = 40.0;

/// Fits the strip's slots into `width` and `height`. The temperatures come
/// first and are never dropped, though the hotspot is left out on a GPU that
/// cannot report it. A wider strip adds slots; a narrower one shrinks its
/// numbers to keep four, then keeps three.
pub fn plan_strip(width: f32, height: f32, absent: Absent) -> StripPlan {
    use StripItem::*;
    let items: Vec<StripItem> = [Cpu, Gpu, Hotspot, GpuLoad, CpuLoad, Ram, Vram]
        .into_iter()
        .filter(|i| *i != Hotspot || !absent.contains(GpuHotspot))
        .collect();
    let inner = width - STRIP_INSET * 2.0;
    let fit = |size: f32| {
        let room = inner - (size * ALARM_PER_SIZE).max(ALARM_MIN_W) - STRIP_SLOT_GAP * 2.0;
        let count = ((room + STRIP_SLOT_GAP) / (size * SLOT_PER_SIZE + STRIP_SLOT_GAP))
            .floor()
            .max(0.0) as usize;
        (count.min(items.len()), room)
    };
    let start = (height * 0.565)
        .round()
        .clamp(STRIP_MIN_SIZE, STRIP_MAX_SIZE);
    let sizes = || (0..=(start - STRIP_MIN_SIZE) as usize).map(|i| start - i as f32);
    let size = sizes()
        .find(|&s| s >= STRIP_FOUR_MIN_SIZE && fit(s).0 >= 4)
        .or_else(|| sizes().find(|&s| fit(s).0 >= 3))
        .unwrap_or(STRIP_MIN_SIZE);
    let (count, room) = fit(size);
    let count = count.max(3).min(items.len());
    let slot_w = (room - STRIP_SLOT_GAP * (count - 1) as f32) / count as f32;
    let slots = items[..count]
        .iter()
        .enumerate()
        .map(|(i, &item)| {
            let x = STRIP_INSET + i as f32 * (slot_w + STRIP_SLOT_GAP);
            (item, Rect::new(x, 0.0, slot_w, height))
        })
        .collect();
    let alarm_x = STRIP_INSET + room + STRIP_SLOT_GAP * 2.0;
    StripPlan {
        size,
        slots,
        alarm: Rect::new(alarm_x, 0.0, width - STRIP_INSET - alarm_x, height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Vitals page with `apps` busy apps.
    fn with_apps(width: f32, height: f32, apps: usize, frozen: bool) -> Layout {
        let options = Options {
            apps,
            frozen,
            ..Default::default()
        };
        compute(width, height, options)
    }

    fn plain(width: f32, height: f32) -> Layout {
        with_apps(width, height, 0, false)
    }

    fn apps_page(width: f32, height: f32, simple: bool, sort: SortKey) -> Layout {
        let options = Options {
            page: Page::Apps,
            simple,
            sort,
            ..Default::default()
        };
        compute(width, height, options)
    }

    fn sizes() -> impl Iterator<Item = (f32, f32)> {
        (140..=1600)
            .step_by(23)
            .flat_map(|w| (36..=1000).step_by(17).map(move |h| (w as f32, h as f32)))
    }

    #[test]
    fn every_cell_stays_inside_the_window() {
        for (w, h) in sizes() {
            for c in plain(w, h).cells {
                assert!(
                    c.rect.x >= 0.0 && c.rect.right() <= w + 0.01,
                    "{w}x{h} {c:?}"
                );
                assert!(
                    c.rect.y >= TITLE_H && c.rect.bottom() <= h + 0.01,
                    "{w}x{h} {c:?}"
                );
            }
        }
    }

    #[test]
    fn cells_never_overlap() {
        for (w, h) in sizes() {
            let l = plain(w, h);
            for (i, a) in l.cells.iter().enumerate() {
                for b in &l.cells[i + 1..] {
                    let apart = a.rect.right() <= b.rect.x + 0.01
                        || b.rect.right() <= a.rect.x + 0.01
                        || a.rect.bottom() <= b.rect.y + 0.01
                        || b.rect.bottom() <= a.rect.y + 0.01;
                    assert!(apart, "{w}x{h}: {a:?} overlaps {b:?}");
                }
            }
        }
    }

    #[test]
    fn non_strip_modes_always_lead_with_both_temperatures() {
        for (w, h) in sizes() {
            let l = plain(w, h);
            if l.mode != Mode::Strip {
                assert!(l.cells.len() >= 3, "{w}x{h}");
                assert!(l.shows(CpuTemp) && l.shows(GpuTemp), "{w}x{h}");
            }
        }
    }

    #[test]
    fn default_window_shows_hotspot_and_memory() {
        let l = plain(340.0, 470.0);
        assert!(l.shows(GpuHotspot) && l.shows(Memory) && l.shows(VramTemp));
    }

    #[test]
    fn tall_thin_window_uses_rows_and_shows_eleven_metrics() {
        let l = plain(190.0, 640.0);
        assert_eq!(l.mode, Mode::Rows);
        assert_eq!(l.cells.len(), 11);
    }

    #[test]
    fn pin_and_close_exist_at_every_size() {
        for (w, h) in sizes() {
            let l = plain(w, h);
            assert!(l.buttons.iter().any(|(b, _)| *b == Button::Pin), "{w}x{h}");
            assert!(
                l.buttons.iter().any(|(b, _)| *b == Button::Close),
                "{w}x{h}"
            );
        }
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.right() - 0.01
            && b.x < a.right() - 0.01
            && a.y < b.bottom() - 0.01
            && b.y < a.bottom() - 0.01
    }

    #[test]
    fn no_busy_apps_means_the_window_is_exactly_as_before() {
        for (w, h) in sizes() {
            let quiet = with_apps(w, h, 0, false);
            assert!(quiet.app_rows.is_empty(), "{w}x{h}");
            let again = arrange(w, h, 0.0, false, Absent::default());
            assert_eq!(quiet.mode, again.mode, "{w}x{h}");
            assert_eq!(quiet.cells, again.cells, "{w}x{h}");
        }
    }

    #[test]
    fn app_rows_stay_inside_the_window_and_clear_of_everything() {
        for (w, h) in sizes() {
            for apps in 1..=6 {
                for frozen in [false, true] {
                    let l = with_apps(w, h, apps, frozen);
                    for (i, row) in l.app_rows.iter().enumerate() {
                        assert!(row.x >= 0.0 && row.right() <= w + 0.01, "{w}x{h} {row:?}");
                        assert!(row.y >= 0.0 && row.bottom() <= h + 0.01, "{w}x{h} {row:?}");
                        for c in &l.cells {
                            assert!(!overlaps(*row, c.rect), "{w}x{h} {row:?} {c:?}");
                        }
                        for other in &l.app_rows[i + 1..] {
                            assert!(!overlaps(*row, *other), "{w}x{h} {row:?} {other:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_list_never_costs_the_temperatures_when_tiles_are_showing() {
        for (w, h) in sizes() {
            let l = with_apps(w, h, 3, false);
            if l.mode != Mode::Strip {
                assert!(l.shows(CpuTemp) && l.shows(GpuTemp), "{w}x{h}");
                assert!(l.cells.len() >= 3, "{w}x{h}");
            }
        }
    }

    #[test]
    fn the_default_window_shows_two_apps_in_a_single_column() {
        let l = with_apps(340.0, 470.0, 2, false);
        assert_eq!(l.mode, Mode::Compact);
        assert_eq!(l.app_rows.len(), 2);
        assert_eq!(l.app_rows[0].x, l.app_rows[1].x);
    }

    #[test]
    fn wide_windows_put_apps_side_by_side() {
        let l = with_apps(720.0, 380.0, 2, false);
        assert_eq!(l.app_rows.len(), 2);
        assert_eq!(l.app_rows[0].y, l.app_rows[1].y);
        assert!(l.app_rows[0].right() <= l.app_rows[1].x);
    }

    #[test]
    fn one_app_uses_the_full_width_of_a_wide_window() {
        let l = with_apps(720.0, 380.0, 1, false);
        assert_eq!(l.app_rows[0].w, 720.0 - PAD * 2.0);
    }

    #[test]
    fn the_list_stops_growing_at_its_limit() {
        assert_eq!(
            with_apps(340.0, 800.0, 9, false).app_rows.len(),
            MAX_APP_ROWS
        );
        assert_eq!(
            with_apps(1240.0, 760.0, 9, false).app_rows.len(),
            MAX_APP_ROWS * 2
        );
    }

    #[test]
    fn a_one_line_strip_only_gives_way_to_a_frozen_app() {
        let busy = with_apps(320.0, 46.0, 1, false);
        assert_eq!(busy.mode, Mode::Strip);
        assert!(busy.app_rows.is_empty());
        let frozen = with_apps(320.0, 46.0, 1, true);
        assert!(frozen.strip_is_replaced());
        assert_eq!(frozen.app_rows.len(), 1);
    }

    #[test]
    fn a_taller_strip_keeps_its_numbers_above_the_app() {
        let l = with_apps(320.0, 72.0, 1, false);
        assert_eq!(l.mode, Mode::Strip);
        assert_eq!(l.app_rows.len(), 1);
        assert!(!l.strip_is_replaced());
        assert!(l.app_rows[0].y > 30.0);
    }

    #[test]
    fn controls_stay_inside_their_row() {
        let row = Rect::new(8.0, 400.0, 174.0, 30.0);
        let inside = |r: Rect| {
            r.x >= row.x && r.right() <= row.right() && r.y >= row.y && r.bottom() <= row.bottom()
        };
        assert!(inside(end_button(row)));
        let (end, keep) = confirm_buttons(row);
        assert!(inside(end) && inside(keep));
        assert!(end.right() <= keep.x);
    }

    #[test]
    fn the_menu_exists_at_every_size_so_a_strip_can_grow_again() {
        for (w, h) in sizes() {
            let l = plain(w, h);
            assert!(l.buttons.iter().any(|(b, _)| *b == Button::Menu), "{w}x{h}");
        }
    }

    #[test]
    fn the_tabs_never_reach_the_buttons() {
        for w in (190..=1600).step_by(5) {
            let l = plain(w as f32, 470.0);
            let tabs_end = l.tabs.last().map_or(0.0, |(_, r)| r.right());
            for (button, r) in &l.buttons {
                assert!(tabs_end <= r.x, "{button:?} at {w}");
            }
        }
    }

    #[test]
    fn a_strip_has_no_tabs() {
        assert!(plain(320.0, 46.0).tabs.is_empty());
    }

    #[test]
    fn the_apps_page_fits_every_size_it_appears_at() {
        for (w, h) in sizes().filter(|(w, _)| *w >= 190.0) {
            let l = apps_page(w, h, false, SortKey::Cpu);
            let Some(table) = &l.table else {
                assert_ne!(l.mode, Mode::Table, "{w}x{h}");
                continue;
            };
            assert_eq!(l.mode, Mode::Table);
            assert_eq!(l.page, Page::Apps);
            assert!(table.visible >= 1, "{w}x{h}");
            let last = table.row(table.visible - 1);
            assert!(last.bottom() <= h - PAD + 0.01, "{w}x{h}");
            assert!(table.names_end() - table.body.x >= MIN_NAME_W, "{w}x{h}");
        }
    }

    #[test]
    fn a_short_window_shows_the_strip_on_either_page() {
        let l = apps_page(320.0, 72.0, false, SortKey::Cpu);
        assert_eq!(l.mode, Mode::Strip);
        assert_eq!(l.page, Page::Vitals);
    }

    #[test]
    fn columns_appear_as_the_window_widens() {
        let count = |w| table_columns(w, false, SortKey::Cpu).len();
        assert_eq!(count(190.0), 1);
        assert_eq!(count(254.0), 2);
        assert_eq!(count(340.0), 3);
        assert_eq!(count(460.0), 5);
        assert_eq!(count(1240.0), 5);
    }

    #[test]
    fn simple_mode_stops_at_two_columns() {
        assert_eq!(table_columns(1240.0, true, SortKey::Cpu).len(), 2);
    }

    #[test]
    fn the_sorted_column_is_always_shown() {
        assert_eq!(
            table_columns(190.0, false, SortKey::Memory),
            [SortKey::Memory]
        );
        let shown = table_columns(254.0, false, SortKey::Disk);
        assert_eq!(shown, [SortKey::Cpu, SortKey::Disk]);
    }

    #[test]
    fn sorting_by_name_needs_no_extra_column() {
        assert_eq!(table_columns(190.0, false, SortKey::Name), [SortKey::Cpu]);
    }

    #[test]
    fn hit_testing_finds_headings_and_rows() {
        let l = apps_page(340.0, 520.0, false, SortKey::Cpu);
        let t = l.table.as_ref().unwrap();
        let (key, right) = t.columns[1];
        let y = t.header.y + 5.0;
        assert_eq!(t.header_at(right - 5.0, y), Some(key));
        assert_eq!(t.header_at(t.body.x + 20.0, y), Some(SortKey::Name));
        assert_eq!(t.header_at(right - 5.0, y + 200.0), None);
        let row = t.row(3);
        assert_eq!(t.row_at(row.x + 5.0, row.y + 5.0), Some(3));
        assert_eq!(t.row_at(row.x + 5.0, 2.0), None);
    }

    #[test]
    fn every_preset_is_recognised_and_other_sizes_are_not() {
        for preset in Preset::ALL {
            assert_eq!(Preset::matching(preset.size()), Some(preset));
        }
        assert_eq!(Preset::matching((254, 661)), None);
    }

    const SCREEN: (i32, i32, i32, i32) = (0, 0, 1920, 1040);

    #[test]
    fn a_window_stays_in_the_corner_it_is_in() {
        let top_left = (20, 20, 360, 540);
        assert_eq!(fit_window(top_left, SCREEN, (320, 72)), (20, 20, 340, 92));
        let bottom_right = (1560, 500, 1900, 1020);
        assert_eq!(
            fit_window(bottom_right, SCREEN, (320, 72)),
            (1580, 948, 1900, 1020)
        );
    }

    #[test]
    fn a_window_never_leaves_the_screen() {
        let r = fit_window((1500, 400, 1840, 900), SCREEN, (1240, 760));
        assert!(r.0 >= 0 && r.1 >= 0 && r.2 <= 1920 && r.3 <= 1040, "{r:?}");
        assert_eq!((r.2 - r.0, r.3 - r.1), (1240, 760));
    }

    #[test]
    fn a_window_is_never_larger_than_the_screen() {
        let small = (0, 0, 1280, 680);
        let r = fit_window((100, 100, 440, 620), small, (1240, 760));
        assert_eq!((r.1, r.3), (0, 680));
    }

    #[test]
    fn hit_testing_finds_the_tabs() {
        let l = plain(340.0, 470.0);
        assert_eq!(l.tab_at(20.0, 14.0), Some(Page::Vitals));
        assert_eq!(l.tab_at(70.0, 14.0), Some(Page::Apps));
        assert_eq!(l.tab_at(200.0, 14.0), None);
    }

    /// What an Intel CPU with an NVIDIA GPU cannot report.
    fn intel_nvidia() -> Absent {
        [CpuTemp, IoDieTemp, CpuPower, GpuHotspot, VramTemp, GpuFan]
            .into_iter()
            .fold(Absent::default(), Absent::with)
    }

    fn on(absent: Absent, width: f32, height: f32) -> Layout {
        let options = Options {
            absent,
            ..Default::default()
        };
        compute(width, height, options)
    }

    #[test]
    fn nothing_absent_leaves_every_order_alone() {
        assert_eq!(fill(&ROWS, Absent::default()), ROWS);
    }

    #[test]
    fn absent_metrics_never_get_a_tile_at_any_size() {
        let absent = intel_nvidia();
        for (w, h) in sizes() {
            for c in on(absent, w, h).cells {
                assert!(!absent.contains(c.metric), "{w}x{h} {c:?}");
            }
        }
    }

    #[test]
    fn a_stand_in_further_down_moves_up_rather_than_appearing_twice() {
        let order = fill(&ROWS, intel_nvidia());
        assert_eq!(order[0], CpuClock);
        let mut seen = order.clone();
        seen.sort_by_key(|m| *m as usize);
        seen.dedup();
        assert_eq!(seen.len(), order.len());
    }

    #[test]
    fn cells_stay_apart_and_inside_with_metrics_missing() {
        let absent = intel_nvidia();
        for (w, h) in sizes() {
            let l = on(absent, w, h);
            for (i, a) in l.cells.iter().enumerate() {
                assert!(a.rect.right() <= w + 0.01 && a.rect.bottom() <= h + 0.01);
                for b in &l.cells[i + 1..] {
                    assert!(!overlaps(a.rect, b.rect), "{w}x{h}: {a:?} {b:?}");
                }
            }
        }
    }

    /// The parts of the PC these were written on: an i7 with an RTX card,
    /// two drives and one display.
    fn this_pc_parts() -> Vec<PartShape> {
        let part = |items, facts, right| PartShape {
            items,
            facts,
            right,
        };
        vec![
            part(1, 4, false),
            part(1, 6, true),
            part(1, 3, false),
            part(1, 4, false),
            part(2, 2, false),
            part(1, 2, true),
            part(1, 3, true),
        ]
    }

    fn specs(width: f32, height: f32, simple: bool) -> Layout {
        let options = Options {
            page: Page::Specs,
            simple,
            parts: this_pc_parts(),
            ..Default::default()
        };
        compute(width, height, options)
    }

    #[test]
    fn the_specs_page_keeps_every_row_whole_and_inside_at_every_size_and_scroll() {
        for (w, h) in sizes().filter(|(w, _)| *w >= 190.0) {
            let l = specs(w, h, false);
            let Some(page) = &l.specs else {
                assert_ne!(l.mode, Mode::Specs, "{w}x{h}");
                continue;
            };
            assert!(
                page.copy.x >= page.band.x && page.copy.right() <= w - PAD + 0.01,
                "{w}x{h}"
            );
            for scroll in 0..=page.max_scroll() {
                let rows = page.visible(scroll);
                assert!(!rows.is_empty(), "{w}x{h} scrolled {scroll}");
                for (i, (_, a)) in rows.iter().enumerate() {
                    assert!(
                        a.y >= page.view.y && a.bottom() <= page.view.bottom() + 0.01,
                        "{w}x{h}"
                    );
                    assert!(a.x >= 0.0 && a.right() <= w + 0.01, "{w}x{h}");
                    for (_, b) in &rows[i + 1..] {
                        assert!(!overlaps(*a, *b), "{w}x{h}: {a:?} {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_default_window_shows_every_part_in_one_column() {
        let l = specs(340.0, 520.0, false);
        let page = l.specs.as_ref().unwrap();
        assert_eq!(page.columns.len(), 1);
        assert_eq!(page.visible(0).len(), 7);
        assert_eq!(page.max_scroll(), 0);
        assert!(!page.stacked && !page.detailed);
    }

    #[test]
    fn a_large_window_puts_the_gpu_side_on_the_right_with_details() {
        let page = specs(1240.0, 760.0, false).specs.unwrap();
        assert_eq!(page.columns.len(), 2);
        assert!(page.detailed);
        let right: Vec<usize> = page.columns[1].1.iter().map(|(i, _)| *i).collect();
        assert_eq!(right, [1, 5, 6]);
        assert!(
            !specs(1240.0, 760.0, true).specs.unwrap().detailed,
            "Simple keeps it brief"
        );
    }

    #[test]
    fn a_narrow_window_puts_labels_above_names() {
        assert!(specs(200.0, 520.0, false).specs.unwrap().stacked);
        assert!(!specs(280.0, 520.0, false).specs.unwrap().stacked);
    }

    #[test]
    fn a_short_window_scrolls_by_whole_rows_down_to_the_last() {
        let page = specs(340.0, 220.0, false).specs.unwrap();
        assert!(page.max_scroll() > 0);
        let last = page.visible(page.max_scroll());
        assert_eq!(last.last().map(|(i, _)| *i), Some(6));
        assert!(page.visible(0).len() < 7);
    }

    #[test]
    fn a_part_taller_than_the_window_still_shows_when_it_reaches_the_top() {
        let mut parts = this_pc_parts();
        parts[4].items = 6;
        let options = Options {
            page: Page::Specs,
            parts,
            ..Default::default()
        };
        let page = compute(209.0, 140.0, options).specs.unwrap();
        for scroll in 0..=page.max_scroll() {
            assert!(!page.visible(scroll).is_empty(), "scrolled {scroll}");
        }
    }

    #[test]
    fn a_very_short_window_shows_the_strip_on_the_specs_page_too() {
        let l = specs(340.0, 100.0, false);
        assert_eq!(l.mode, Mode::Strip);
        assert!(l.specs.is_none());
    }

    #[test]
    fn three_tabs_never_reach_the_buttons_on_any_page() {
        for page in [Page::Vitals, Page::Apps, Page::Specs] {
            for w in (190..=1600).step_by(1) {
                let options = Options {
                    page,
                    ..Default::default()
                };
                let l = compute(w as f32, 470.0, options);
                let tabs_end = l.tabs.last().map_or(0.0, |(_, r)| r.right());
                for (button, r) in &l.buttons {
                    assert!(tabs_end <= r.x, "{page:?} {button:?} at {w}");
                }
            }
        }
    }

    #[test]
    fn hit_testing_finds_the_right_app() {
        let l = with_apps(340.0, 470.0, 2, false);
        let second = l.app_rows[1];
        assert_eq!(l.app_at(second.x + 5.0, second.y + 5.0), Some(1));
        assert_eq!(l.app_at(20.0, 40.0), None);
    }

    #[test]
    fn rows_respect_their_minimum_height() {
        for (w, h) in sizes() {
            let l = plain(w, h);
            if l.mode == Mode::Rows {
                for c in &l.cells {
                    assert!(c.rect.h >= MIN_ROW_H - 0.01, "{w}x{h} {c:?}");
                }
            }
        }
    }

    #[test]
    fn the_compact_page_fits_from_about_165_tall() {
        assert_eq!(plain(340.0, 166.0).mode, Mode::Compact);
        assert_eq!(plain(340.0, 150.0).mode, Mode::Strip);
    }

    #[test]
    fn the_hero_reads_cpu_then_gpu_edge_then_hotspot() {
        let l = plain(340.0, 520.0);
        let compact = l.compact.as_ref().unwrap();
        assert_eq!(compact.hero.metrics, [CpuTemp, GpuTemp, GpuHotspot]);
        let x = |m: MetricId| l.cells.iter().find(|c| c.metric == m).unwrap().rect.x;
        assert!(x(CpuTemp) < x(GpuTemp) && x(GpuTemp) < x(GpuHotspot));
    }

    #[test]
    fn the_medium_window_matches_the_design() {
        let l = plain(340.0, 520.0);
        assert_eq!(l.mode, Mode::Compact);
        let c = l.compact.unwrap();
        assert_eq!(c.hero.rect, Rect::new(12.0, 42.0, 316.0, 206.0));
        assert_eq!(c.hero.columns, [24.0, 132.0, 240.0]);
        assert_eq!(c.hero.number, 34.0);
        assert_eq!(c.chart, Some(Rect::new(24.0, 134.0, 284.0, 66.0)));
        assert_eq!(c.summary_y, 236.0);
        assert_eq!(c.card, Some(Rect::new(12.0, 256.0, 316.0, 252.0)));
        let sections: Vec<Section> = c.sections.iter().map(|(s, _)| *s).collect();
        assert_eq!(sections, [Section::Stats, Section::Details, Section::Io]);
        assert!(l.cells.iter().any(|c| c.metric == VramTemp));
    }

    #[test]
    fn a_taller_window_gives_the_extra_height_to_the_chart() {
        let tall = plain(340.0, 700.0).compact.unwrap();
        assert_eq!(tall.chart.unwrap().h, 66.0 + 180.0);
        assert_eq!(tall.sections.len(), 3);
    }

    #[test]
    fn a_shorter_window_gives_up_sections_from_the_bottom_before_the_chart() {
        let shown = |h: f32| {
            let c = plain(340.0, h).compact.unwrap();
            (
                c.sections.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
                c.chart.is_some(),
            )
        };
        assert_eq!(shown(500.0), (vec![Section::Stats, Section::Details], true));
        assert_eq!(shown(360.0), (vec![Section::Stats], true));
        assert_eq!(shown(280.0), (vec![], true));
        assert_eq!(shown(200.0), (vec![], false));
    }

    #[test]
    fn simple_mode_keeps_every_reading_and_gives_up_the_history() {
        let options = Options {
            simple: true,
            ..Default::default()
        };
        for (w, h) in sizes() {
            let simple = compute(w, h, options.clone());
            if let Some(c) = &simple.compact {
                assert!(c.chart.is_none(), "{w}x{h}");
            }
            if let Some(l) = &simple.large {
                assert!(l.chart.is_none(), "{w}x{h}");
            }
            if simple.mode != Mode::Strip {
                assert!(simple.cells.iter().any(|c| c.metric == CpuTemp), "{w}x{h}");
            }
        }
    }

    #[test]
    fn compact_cards_stay_inside_the_window_and_apart() {
        for (w, h) in sizes() {
            for apps in [0, 2] {
                let l = with_apps(w, h, apps, false);
                let Some(c) = &l.compact else { continue };
                let hero = c.hero.rect;
                assert!(
                    hero.y >= TITLE_H && hero.right() <= w - PAD + 0.01,
                    "{w}x{h}"
                );
                if let Some(chart) = c.chart {
                    assert!(
                        chart.x >= hero.x && chart.right() <= hero.right(),
                        "{w}x{h}"
                    );
                    assert!(chart.bottom() + 15.0 < c.summary_y, "{w}x{h}");
                }
                assert!(c.summary_y < hero.bottom(), "{w}x{h}");
                let mut boxes = vec![hero];
                boxes.extend(c.card);
                boxes.extend(l.app_rows.iter().copied());
                for (i, a) in boxes.iter().enumerate() {
                    assert!(a.bottom() <= h - PAD + 0.01, "{w}x{h}: {a:?}");
                    for b in &boxes[i + 1..] {
                        assert!(!overlaps(*a, *b), "{w}x{h}: {a:?} and {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_large_window_matches_the_design() {
        let l = plain(1240.0, 760.0);
        assert_eq!(l.mode, Mode::Large);
        let large = l.large.unwrap();
        assert_eq!(large.hero.rect, Rect::new(12.0, 48.0, 808.0, 368.0));
        assert_eq!(large.hero.columns, [28.0, 208.0, 388.0]);
        assert_eq!(large.side_x, 588.0);
        assert_eq!(large.tiles[0].1.x, 828.0);
        assert_eq!(large.tiles[1].1.x, 1032.0);
        assert!((large.tiles[5].1.bottom() - 416.0).abs() < 0.01);
        assert_eq!(large.attention, Rect::new(12.0, 424.0, 400.0, 324.0));
        assert_eq!(large.gpu.x, 420.0);
        assert_eq!(large.cpu.right(), 1228.0);
        let metrics: Vec<MetricId> = large.tiles.iter().map(|(m, _)| *m).collect();
        assert_eq!(
            metrics,
            [CpuLoad, GpuLoad, CpuPower, GpuPower, Memory, VideoMemory]
        );
    }

    #[test]
    fn large_cards_stay_inside_the_window_and_apart() {
        for w in (820..=1920).step_by(37) {
            for h in (600..=1200).step_by(29) {
                for apps in [0, 3, 6] {
                    let l = with_apps(w as f32, h as f32, apps, false);
                    let Some(large) = &l.large else {
                        panic!("{w}x{h} with {apps} apps is not large");
                    };
                    let mut boxes = vec![large.hero.rect, large.attention, large.gpu, large.cpu];
                    boxes.extend(large.tiles.iter().map(|(_, r)| *r));
                    boxes.extend(l.app_rows.iter().copied());
                    for (i, a) in boxes.iter().enumerate() {
                        assert!(a.x >= PAD - 0.01 && a.right() <= w as f32 - PAD + 0.01);
                        assert!(a.y >= TITLE_H && a.bottom() <= h as f32 - PAD + 0.01);
                        for b in &boxes[i + 1..] {
                            assert!(!overlaps(*a, *b), "{w}x{h}: {a:?} and {b:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn hit_testing_finds_cells_and_buttons() {
        let l = plain(340.0, 520.0);
        assert_eq!(l.button_at(335.0, 10.0), Some(Button::Close));
        assert_eq!(l.cell_at(30.0, 70.0), Some(CpuTemp));
        assert_eq!(l.cell_at(250.0, 70.0), Some(GpuHotspot));
        assert_eq!(l.cell_at(20.0, 10.0), None);
    }

    #[test]
    fn the_default_strip_has_four_slots_and_room_for_an_alarm() {
        use StripItem::*;
        let plan = plan_strip(320.0, 46.0, Absent::default());
        assert_eq!(plan.size, 26.0);
        let items: Vec<StripItem> = plan.slots.iter().map(|(i, _)| *i).collect();
        assert_eq!(items, [Cpu, Gpu, Hotspot, GpuLoad]);
        assert!(plan.alarm.w >= 40.0);
        let last = plan.slots.last().unwrap().1;
        assert!(last.right() < plan.alarm.x && plan.alarm.right() <= 316.01);
    }

    #[test]
    fn the_small_preset_keeps_the_same_slots() {
        let short = plan_strip(320.0, 46.0, Absent::default());
        let small = plan_strip(320.0, 72.0, Absent::default());
        assert_eq!(small.size, short.size);
        assert_eq!(small.slots.len(), 4);
    }

    #[test]
    fn a_narrow_strip_keeps_the_three_temperatures() {
        use StripItem::*;
        let plan = plan_strip(190.0, 46.0, Absent::default());
        let items: Vec<StripItem> = plan.slots.iter().map(|(i, _)| *i).collect();
        assert_eq!(items, [Cpu, Gpu, Hotspot]);
        assert!(plan.size >= 16.0);
    }

    #[test]
    fn a_wide_strip_adds_slots() {
        assert_eq!(plan_strip(800.0, 46.0, Absent::default()).slots.len(), 7);
    }

    #[test]
    fn a_gpu_without_a_hotspot_sensor_leaves_it_out_of_the_strip() {
        use StripItem::*;
        let absent = Absent::default().with(GpuHotspot);
        let plan = plan_strip(320.0, 46.0, absent);
        let items: Vec<StripItem> = plan.slots.iter().map(|(i, _)| *i).collect();
        assert_eq!(items, [Cpu, Gpu, GpuLoad, CpuLoad]);
    }

    #[test]
    fn strip_slots_stay_inside_and_apart_at_every_width() {
        for w in 190..=1600 {
            for h in [36.0, 46.0, 63.0] {
                let plan = plan_strip(w as f32, h, Absent::default());
                assert!(plan.slots.len() >= 3, "{w}");
                let mut edge = 0.0;
                for (_, r) in &plan.slots {
                    assert!(r.x >= edge && r.w > 0.0, "{w}");
                    edge = r.right();
                }
                assert!(
                    edge <= plan.alarm.x && plan.alarm.right() <= w as f32,
                    "{w}"
                );
            }
        }
    }

    #[test]
    fn presets_show_the_strip_the_compact_page_and_the_large_page() {
        let at = |preset: Preset| {
            let (w, h) = preset.size();
            plain(w as f32, h as f32)
        };
        assert_eq!(at(Preset::Small).mode, Mode::Strip);
        assert!(at(Preset::Small).strip.is_some());
        assert_eq!(at(Preset::Medium).mode, Mode::Compact);
        assert_eq!(at(Preset::Large).mode, Mode::Large);
    }

    #[test]
    fn a_large_window_gives_up_busy_rows_before_its_layout() {
        let l = with_apps(1240.0, 600.0, 6, false);
        assert_eq!(l.mode, Mode::Large);
        assert!(!l.app_rows.is_empty());
    }

    #[test]
    fn a_pc_without_a_cpu_temperature_leads_with_its_load() {
        let l = on(intel_nvidia(), 340.0, 520.0);
        let c = l.compact.as_ref().unwrap();
        assert_eq!(c.hero.metrics, [CpuLoad, GpuTemp, GpuLoad]);
        assert_eq!(stats_metrics(c.hero.metrics), [CpuClock, GpuClock, Memory]);
        let large = on(intel_nvidia(), 1240.0, 760.0).large.unwrap();
        for (metric, _) in large.tiles {
            assert!(!intel_nvidia().contains(metric), "{metric:?}");
            assert!(!large.hero.metrics.contains(&metric), "{metric:?}");
        }
    }

    #[test]
    fn simple_mode_grows_the_large_numbers_into_the_room_the_chart_leaves() {
        let options = Options {
            simple: true,
            ..Default::default()
        };
        for (w, h) in [(1240.0, 760.0), (820.0, 600.0), (1920.0, 1040.0)] {
            let large = compute(w, h, options.clone()).large.unwrap();
            let hero = &large.hero;
            assert!(hero.number > 48.0, "{w}x{h}");
            assert!(hero.label_y - 12.0 > hero.rect.y, "{w}x{h}");
            assert!(hero.number_y() + 25.0 < hero.rect.bottom(), "{w}x{h}");
            assert!(hero.number * 2.0 <= hero.pitch + 0.01, "{w}x{h}");
        }
    }
}
