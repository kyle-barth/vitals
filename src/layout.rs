//! Window layout. Pure geometry with no drawing or Win32, so that every
//! window size can be tested. All values are device-independent pixels.
//!
//! The window moves between these modes as it is resized:
//! - `Strip`: one or two lines of key numbers, with no title bar.
//! - `Rows`: a single column of metric rows, for tall, thin windows.
//! - `Tiles`: a grid that adds tiles, and enlarges them, as space grows.
//! - `Table`: the Apps page, which shows more columns as width grows.
//!
//! Window size decides how many things are shown. Simple mode decides how much
//! is said about each one, so it never changes which tiles appear.

use crate::apps::{COLUMNS, SIMPLE_COLUMNS, SortKey};
use crate::metrics::Absent;
use crate::metrics::MetricId::{self, *};

pub const TITLE_H: f32 = 28.0;
pub const PAD: f32 = 8.0;
pub const GAP: f32 = 6.0;
const ROW_GAP: f32 = 4.0;
pub const MIN_TILE_H: f32 = 52.0;
pub const MIN_ROW_H: f32 = 50.0;
const ROWS_BELOW_WIDTH: f32 = 240.0;
const LARGE_W: f32 = 820.0;
const LARGE_H: f32 = 600.0;
const THREADS_H: f32 = 76.0;
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
/// The grid must show at least this many tiles, otherwise the strip is better.
const MIN_TILES: usize = 4;

/// Below this width, minimize and maximize step aside for the tabs, and the
/// tabs and buttons that remain draw closer together.
const ALL_BUTTONS_W: f32 = 280.0;
const TABS: [(Page, f32); 3] = [
    (Page::Vitals, 46.0),
    (Page::Apps, 40.0),
    (Page::Specs, 44.0),
];
const COMPACT_TABS: [(Page, f32); 3] = [
    (Page::Vitals, 40.0),
    (Page::Apps, 34.0),
    (Page::Specs, 38.0),
];
const TABS_X: f32 = 4.0;

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

/// Reading order for a two-column grid: CPU on the left, GPU on the right.
const TILES_2: [MetricId; 12] = [
    CpuTemp,
    GpuTemp,
    Memory,
    GpuHotspot,
    CpuLoad,
    GpuLoad,
    VideoMemory,
    VramTemp,
    CpuPower,
    GpuPower,
    Download,
    GpuFan,
];
const TILES_3: [MetricId; 12] = [
    CpuTemp,
    GpuTemp,
    GpuHotspot,
    Memory,
    CpuLoad,
    GpuLoad,
    VideoMemory,
    VramTemp,
    GpuFan,
    CpuPower,
    GpuPower,
    Download,
];
const TILES_4: [MetricId; 16] = [
    CpuTemp,
    GpuTemp,
    GpuHotspot,
    Memory,
    CpuLoad,
    GpuLoad,
    VramTemp,
    VideoMemory,
    CpuPower,
    GpuPower,
    GpuFan,
    Download,
    DiskRead,
    DiskWrite,
    CpuClock,
    Upload,
];
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
    Tiles,
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
    /// Something is at a limit, which a large window gives a row of its own.
    pub limits: bool,
}

const LIMITS_H: f32 = 30.0;

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

#[derive(Clone, Debug)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub mode: Mode,
    pub cells: Vec<Cell>,
    pub threads: Option<Rect>,
    pub buttons: Vec<(Button, Rect)>,
    /// One row per busy app shown, in the same order as the apps.
    pub app_rows: Vec<Rect>,
    pub tabs: Vec<(Page, Rect)>,
    /// The page actually showing. A strip has no room for Apps, whatever is chosen.
    pub page: Page,
    pub table: Option<Table>,
    pub specs: Option<SpecsPage>,
    /// The row that says what is at a limit, in a large window.
    pub limits: Option<Rect>,
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

/// Lays out the Vitals page. The list of busy apps takes room from the tiles, and
/// gives up rows before tiles would be lost altogether. With no apps, this is the
/// window as it always is.
fn vitals(width: f32, height: f32, options: Options) -> Layout {
    let Options {
        apps,
        frozen,
        simple,
        absent,
        limits,
        ..
    } = options;
    let columns = if width >= APP_COLUMNS_W && apps >= 2 {
        2
    } else {
        1
    };
    let large = width >= LARGE_W && height >= LARGE_H;
    // When room runs short, busy apps keep their rows before the limits row,
    // because only the busy list can be acted on.
    let extras: &[bool] = if limits && large {
        &[true, false]
    } else {
        &[false]
    };
    for count in (0..=apps.min(MAX_APP_ROWS * columns)).rev() {
        let rows = count.div_ceil(columns);
        let busy_band = if count > 0 {
            rows as f32 * APP_ROW_H + (rows - 1) as f32 * APP_ROW_GAP + GAP
        } else {
            0.0
        };
        for &with_limits in extras {
            let band = busy_band + if with_limits { LIMITS_H + GAP } else { 0.0 };
            let mut layout = arrange(width, height, band, simple, absent);
            if layout.mode == Mode::Strip {
                continue;
            }
            let mut bottom = height - PAD;
            if count > 0 {
                let cell_w = (width - PAD * 2.0 - GAP * (columns - 1) as f32) / columns as f32;
                let top = bottom - (rows as f32 * (APP_ROW_H + APP_ROW_GAP) - APP_ROW_GAP);
                layout.app_rows = (0..count)
                    .map(|i| {
                        let (col, row) = ((i % columns) as f32, (i / columns) as f32);
                        Rect::new(
                            PAD + col * (cell_w + GAP),
                            top + row * (APP_ROW_H + APP_ROW_GAP),
                            cell_w,
                            APP_ROW_H,
                        )
                    })
                    .collect();
                bottom = top - GAP;
            }
            if with_limits {
                layout.limits = Some(Rect::new(
                    PAD,
                    bottom - LIMITS_H,
                    width - PAD * 2.0,
                    LIMITS_H,
                ));
            }
            return layout;
        }
    }
    let mut layout = arrange(width, height, 0.0, simple, absent);
    if layout.mode == Mode::Strip && apps > 0 {
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
        threads: None,
        buttons: Vec::new(),
        app_rows: Vec::new(),
        tabs: Vec::new(),
        page: Page::Vitals,
        table: None,
        specs: None,
        limits: None,
    }
}

fn tabs(window_w: f32) -> Vec<(Page, Rect)> {
    let set = if window_w < ALL_BUTTONS_W {
        &COMPACT_TABS
    } else {
        &TABS
    };
    let mut x = TABS_X;
    set.iter()
        .map(|&(page, width)| {
            let rect = Rect::new(x, 0.0, width, TITLE_H);
            x += width;
            (page, rect)
        })
        .collect()
}

fn arrange(width: f32, height: f32, band: f32, simple: bool, absent: Absent) -> Layout {
    let mut layout = blank(width, height);
    let body = (height - TITLE_H - PAD - band).max(0.0);
    if width < ROWS_BELOW_WIDTH {
        let order = fill(&ROWS, absent);
        let count = (((body + ROW_GAP) / (MIN_ROW_H + ROW_GAP)).floor() as usize).min(order.len());
        if count >= MIN_TILES {
            layout.mode = Mode::Rows;
            grid(&mut layout, &order[..count], 1, body, ROW_GAP);
        }
    } else {
        let large = width >= LARGE_W && height >= LARGE_H;
        let threads = large && !simple;
        let (order, cols): (&[MetricId], usize) = if width >= LARGE_W {
            (if large { &TILES_4 } else { &TILES_4[..12] }, 4)
        } else if width >= 500.0 {
            (&TILES_3, 3)
        } else {
            (&TILES_2, 2)
        };
        let order = fill(order, absent);
        let grid_h = if threads {
            body - THREADS_H - GAP
        } else {
            body
        };
        let rows = ((grid_h + GAP) / (MIN_TILE_H + GAP)).floor().max(0.0) as usize;
        let mut count = order.len().min(rows * cols);
        // A short last row reads as tiles that failed to load.
        if count > cols {
            count -= count % cols;
        }
        if count >= MIN_TILES {
            layout.mode = Mode::Tiles;
            grid(&mut layout, &order[..count], cols, grid_h, GAP);
            if threads {
                let y = TITLE_H + grid_h + GAP;
                layout.threads = Some(Rect::new(PAD, y, width - PAD * 2.0, THREADS_H));
            }
        }
    }
    layout.buttons = buttons(width, layout.mode);
    if layout.mode != Mode::Strip {
        layout.tabs = tabs(width);
    }
    layout
}

fn grid(layout: &mut Layout, order: &[MetricId], cols: usize, grid_h: f32, gap: f32) {
    let rows = order.len().div_ceil(cols);
    let cell_w = (layout.width - PAD * 2.0 - gap * (cols - 1) as f32) / cols as f32;
    let cell_h = (grid_h - gap * (rows - 1) as f32) / rows as f32;
    layout.cells = order
        .iter()
        .enumerate()
        .map(|(i, &metric)| {
            let (col, row) = ((i % cols) as f32, (i / cols) as f32);
            let rect = Rect::new(
                PAD + col * (cell_w + gap),
                TITLE_H + row * (cell_h + gap),
                cell_w,
                cell_h,
            );
            Cell { metric, rect }
        })
        .collect();
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

/// One item in the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripItem {
    Cpu,
    Gpu,
    Hotspot,
    Ram,
    Vram,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StripPlan {
    pub size: f32,
    pub lines: Vec<Vec<StripItem>>,
}

const STRIP_MIN_SIZE: f32 = 14.0;
const STRIP_TWO_LINES_H: f32 = 64.0;

/// Fits the strip into `width`, shrinking the text before dropping anything.
/// VRAM goes first, then RAM. The temperatures are never dropped, though the
/// hotspot is left out on a GPU that cannot report it.
/// `line_width` measures one line of items at a given text size.
pub fn plan_strip(
    width: f32,
    height: f32,
    absent: Absent,
    line_width: impl Fn(&[StripItem], f32) -> f32,
) -> StripPlan {
    use StripItem::*;
    let kept = |items: &[StripItem]| -> Vec<StripItem> {
        items
            .iter()
            .copied()
            .filter(|i| *i != Hotspot || !absent.contains(GpuHotspot))
            .collect()
    };
    let candidates = [
        kept(&[Cpu, Gpu, Hotspot, Ram, Vram]),
        kept(&[Cpu, Gpu, Hotspot, Ram]),
        kept(&[Cpu, Gpu, Hotspot]),
    ];
    let two_lines = height >= STRIP_TWO_LINES_H;
    let line_h = if two_lines { height / 2.0 } else { height };
    let start = (line_h * 0.46).clamp(STRIP_MIN_SIZE, 26.0);
    let split = |items: &[StripItem]| -> Vec<Vec<StripItem>> {
        if two_lines {
            let processors = items
                .iter()
                .take_while(|i| matches!(i, Cpu | Gpu | Hotspot))
                .count();
            let (a, b) = items.split_at(processors);
            [a.to_vec(), b.to_vec()]
                .into_iter()
                .filter(|l| !l.is_empty())
                .collect()
        } else {
            vec![items.to_vec()]
        }
    };
    let available = width - PAD * 2.0;
    for items in &candidates {
        let lines = split(items);
        let mut size = start;
        loop {
            if lines.iter().all(|l| line_width(l, size) <= available) {
                return StripPlan { size, lines };
            }
            if size <= STRIP_MIN_SIZE {
                break;
            }
            size = (size - 1.0).max(STRIP_MIN_SIZE);
        }
    }
    StripPlan {
        size: STRIP_MIN_SIZE,
        lines: split(&candidates[2]),
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
    fn cells_respect_minimum_height() {
        for (w, h) in sizes() {
            let l = plain(w, h);
            let min = if l.mode == Mode::Rows {
                MIN_ROW_H
            } else {
                MIN_TILE_H
            };
            for c in &l.cells {
                assert!(c.rect.h >= min - 0.01, "{w}x{h} {c:?}");
            }
        }
    }

    #[test]
    fn non_strip_modes_always_lead_with_both_temperatures() {
        for (w, h) in sizes() {
            let l = plain(w, h);
            if l.mode != Mode::Strip {
                assert!(l.cells.len() >= MIN_TILES, "{w}x{h}");
                assert!(l.shows(CpuTemp) && l.shows(GpuTemp), "{w}x{h}");
            }
        }
    }

    #[test]
    fn four_tiles_fit_from_about_146_tall() {
        assert_eq!(plain(340.0, 146.0).mode, Mode::Tiles);
        assert_eq!(plain(340.0, 146.0).cells.len(), 4);
        assert_eq!(plain(340.0, 120.0).mode, Mode::Strip);
    }

    #[test]
    fn two_column_grid_keeps_cpu_left_and_gpu_right() {
        let l = plain(340.0, 470.0);
        let x = |m: MetricId| l.cells.iter().find(|c| c.metric == m).unwrap().rect.x;
        assert!(x(CpuTemp) < x(GpuTemp));
        assert!(x(CpuLoad) < x(GpuLoad));
        assert!(x(CpuPower) < x(GpuPower));
    }

    #[test]
    fn default_window_shows_hotspot_and_memory() {
        let l = plain(340.0, 470.0);
        assert!(l.shows(GpuHotspot) && l.shows(Memory) && l.shows(VideoMemory));
    }

    #[test]
    fn small_window_shows_hotspot_and_memory_in_its_first_four() {
        let l = plain(250.0, 160.0);
        assert_eq!(l.cells.len(), 4);
        assert!(l.shows(GpuHotspot) && l.shows(Memory));
    }

    #[test]
    fn tall_thin_window_uses_rows_and_shows_eleven_metrics() {
        let l = plain(190.0, 640.0);
        assert_eq!(l.mode, Mode::Rows);
        assert_eq!(l.cells.len(), 11);
    }

    #[test]
    fn large_window_adds_extra_tiles_and_thread_bars() {
        let l = plain(1240.0, 760.0);
        assert_eq!(l.cells.len(), 16);
        assert!(l.shows(Upload) && l.shows(DiskRead));
        assert!(l.threads.is_some());
        assert!(plain(1240.0, 400.0).threads.is_none());
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

    #[test]
    fn hit_testing_finds_cells_and_buttons() {
        let l = plain(340.0, 470.0);
        assert_eq!(l.button_at(335.0, 10.0), Some(Button::Close));
        assert_eq!(l.cell_at(20.0, 40.0), Some(CpuTemp));
        assert_eq!(l.cell_at(20.0, 10.0), None);
    }

    // A stand-in for text measurement: each item is `advance` wide at size 14.
    fn measured(advance: f32) -> impl Fn(&[StripItem], f32) -> f32 {
        move |items, size| items.len() as f32 * advance * size / 14.0
    }

    #[test]
    fn strip_shrinks_text_before_dropping_anything() {
        let plan = plan_strip(320.0, 46.0, Absent::default(), measured(60.5));
        assert_eq!(plan.size, STRIP_MIN_SIZE);
        assert_eq!(plan.lines[0].len(), 5);
    }

    #[test]
    fn strip_drops_vram_then_ram_but_never_temperatures() {
        use StripItem::*;
        let dropped_one = plan_strip(320.0, 46.0, Absent::default(), measured(70.0));
        assert_eq!(dropped_one.lines[0], vec![Cpu, Gpu, Hotspot, Ram]);
        let dropped_two = plan_strip(240.0, 46.0, Absent::default(), measured(70.0));
        assert_eq!(dropped_two.lines[0], vec![Cpu, Gpu, Hotspot]);
        let too_tight = plan_strip(100.0, 46.0, Absent::default(), measured(70.0));
        assert_eq!(too_tight.lines[0], vec![Cpu, Gpu, Hotspot]);
    }

    #[test]
    fn strip_wraps_to_two_lines_when_tall_enough() {
        let plan = plan_strip(320.0, 70.0, Absent::default(), measured(58.0));
        assert_eq!(plan.lines.len(), 2);
    }

    #[test]
    fn a_gpu_without_a_hotspot_sensor_leaves_it_out_of_the_strip() {
        use StripItem::*;
        let absent = Absent::default().with(GpuHotspot);
        let plan = plan_strip(320.0, 46.0, absent, measured(40.0));
        assert_eq!(plan.lines[0], vec![Cpu, Gpu, Ram, Vram]);
        let two = plan_strip(320.0, 70.0, absent, measured(40.0));
        assert_eq!(two.lines, vec![vec![Cpu, Gpu], vec![Ram, Vram]]);
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
            assert_eq!(quiet.threads, again.threads, "{w}x{h}");
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
                        if let Some(threads) = l.threads {
                            assert!(!overlaps(*row, threads), "{w}x{h} {row:?}");
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
    fn tiles_keep_their_minimum_height_beside_the_list() {
        for (w, h) in sizes() {
            let l = with_apps(w, h, 3, false);
            let min = if l.mode == Mode::Rows {
                MIN_ROW_H
            } else {
                MIN_TILE_H
            };
            for c in &l.cells {
                assert!(c.rect.h >= min - 0.01, "{w}x{h} {c:?}");
            }
        }
    }

    #[test]
    fn the_list_never_costs_the_temperatures_when_tiles_are_showing() {
        for (w, h) in sizes() {
            let l = with_apps(w, h, 3, false);
            if l.mode != Mode::Strip {
                assert!(l.shows(CpuTemp) && l.shows(GpuTemp), "{w}x{h}");
                assert!(l.cells.len() >= MIN_TILES, "{w}x{h}");
            }
        }
    }

    #[test]
    fn the_default_window_shows_two_apps_in_a_single_column() {
        let l = with_apps(340.0, 470.0, 2, false);
        assert_eq!(l.mode, Mode::Tiles);
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
    fn simple_mode_keeps_every_tile_and_gives_up_the_thread_bars() {
        let options = Options {
            simple: true,
            ..Default::default()
        };
        for (w, h) in sizes() {
            let (full, simple) = (plain(w, h), compute(w, h, options.clone()));
            if full.threads.is_none() {
                assert_eq!(full.cells.len(), simple.cells.len(), "{w}x{h}");
            }
            assert!(simple.threads.is_none(), "{w}x{h}");
        }
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

    #[test]
    fn small_is_a_strip_and_large_has_the_thread_bars() {
        let at = |preset: Preset| {
            let (w, h) = preset.size();
            plain(w as f32, h as f32)
        };
        assert_eq!(at(Preset::Small).mode, Mode::Strip);
        assert_eq!(at(Preset::Medium).mode, Mode::Tiles);
        assert!(at(Preset::Large).threads.is_some());
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
        for order in [&TILES_2[..], &TILES_3, &TILES_4, &ROWS] {
            assert_eq!(fill(order, Absent::default()), order);
        }
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
    fn stand_ins_keep_the_cpu_left_and_the_gpu_right() {
        let l = on(intel_nvidia(), 340.0, 520.0);
        let x = |m: MetricId| l.cells.iter().find(|c| c.metric == m).unwrap().rect.x;
        assert_eq!(l.cells[0].metric, CpuClock);
        assert!(x(CpuClock) < x(GpuTemp));
        assert!(x(CpuLoad) < x(GpuLoad));
        assert!(x(GpuClock) > x(Memory));
        assert!(l.shows(GpuFanPercent));
    }

    #[test]
    fn a_stand_in_further_down_moves_up_rather_than_appearing_twice() {
        let order = fill(&TILES_4, intel_nvidia());
        assert_eq!(order[0], CpuClock);
        let mut seen = order.clone();
        seen.sort_by_key(|m| *m as usize);
        seen.dedup();
        assert_eq!(seen.len(), order.len());
    }

    #[test]
    fn a_grid_short_of_metrics_never_leaves_a_ragged_last_row() {
        let absent = intel_nvidia();
        for (w, h) in sizes() {
            let l = on(absent, w, h);
            if l.mode != Mode::Tiles {
                continue;
            }
            let columns = {
                let mut xs: Vec<i32> = l.cells.iter().map(|c| c.rect.x as i32).collect();
                xs.sort_unstable();
                xs.dedup();
                xs.len()
            };
            assert_eq!(l.cells.len() % columns, 0, "{w}x{h}");
        }
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

    fn with_limits(width: f32, height: f32, apps: usize) -> Layout {
        compute(
            width,
            height,
            Options {
                apps,
                limits: true,
                ..Default::default()
            },
        )
    }

    #[test]
    fn the_limits_row_only_appears_in_a_large_window() {
        assert!(with_limits(340.0, 520.0, 0).limits.is_none());
        let large = with_limits(1240.0, 760.0, 0);
        assert!(large.limits.is_some());
        assert!(large.threads.is_some(), "the thread bars stay");
    }

    #[test]
    fn the_limits_row_never_overlaps_anything() {
        for (w, h) in sizes() {
            for apps in [0, 2] {
                let l = with_limits(w, h, apps);
                let mut boxes: Vec<Rect> = l.cells.iter().map(|c| c.rect).collect();
                boxes.extend(l.threads);
                boxes.extend(l.app_rows.iter().copied());
                boxes.extend(l.limits);
                for (i, a) in boxes.iter().enumerate() {
                    assert!(a.x >= 0.0 && a.right() <= w + 0.01, "{w}x{h}: {a:?}");
                    assert!(a.y >= 0.0 && a.bottom() <= h + 0.01, "{w}x{h}: {a:?}");
                    for b in &boxes[i + 1..] {
                        assert!(!overlaps(*a, *b), "{w}x{h}: {a:?} and {b:?}");
                    }
                }
            }
        }
    }
}
