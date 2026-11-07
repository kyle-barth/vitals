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

/// Below this width, minimize and maximize step aside for the tabs.
const ALL_BUTTONS_W: f32 = 230.0;
const TABS: [(Page, f32); 2] = [(Page::Vitals, 46.0), (Page::Apps, 40.0)];
const TABS_X: f32 = 4.0;

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
}

/// Which of the two tabs is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Vitals,
    Apps,
}

/// Everything besides the window's size that changes its layout.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// How many busy apps want a row.
    pub apps: usize,
    /// Whether the first of them is frozen.
    pub frozen: bool,
    pub page: Page,
    pub simple: bool,
    pub sort: SortKey,
}

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
        layout.table = Some(table(width, height, options));
        layout.buttons = buttons(width, Mode::Table);
        layout.tabs = tabs();
        return layout;
    }
    vitals(width, height, options)
}

fn table(width: f32, height: f32, options: Options) -> Table {
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
        ..
    } = options;
    let columns = if width >= APP_COLUMNS_W && apps >= 2 {
        2
    } else {
        1
    };
    for count in (1..=apps.min(MAX_APP_ROWS * columns)).rev() {
        let rows = count.div_ceil(columns);
        let band = rows as f32 * APP_ROW_H + (rows - 1) as f32 * APP_ROW_GAP + GAP;
        let mut layout = arrange(width, height, band, simple);
        if layout.mode != Mode::Strip {
            let cell_w = (width - PAD * 2.0 - GAP * (columns - 1) as f32) / columns as f32;
            let top = height - PAD - (rows as f32 * (APP_ROW_H + APP_ROW_GAP) - APP_ROW_GAP);
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
            return layout;
        }
    }
    let mut layout = arrange(width, height, 0.0, simple);
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
    }
}

fn tabs() -> Vec<(Page, Rect)> {
    let mut x = TABS_X;
    TABS.iter()
        .map(|&(page, width)| {
            let rect = Rect::new(x, 0.0, width, TITLE_H);
            x += width;
            (page, rect)
        })
        .collect()
}

fn arrange(width: f32, height: f32, band: f32, simple: bool) -> Layout {
    let mut layout = blank(width, height);
    let body = (height - TITLE_H - PAD - band).max(0.0);
    if width < ROWS_BELOW_WIDTH {
        let count = (((body + ROW_GAP) / (MIN_ROW_H + ROW_GAP)).floor() as usize).min(ROWS.len());
        if count >= MIN_TILES {
            layout.mode = Mode::Rows;
            grid(&mut layout, &ROWS[..count], 1, body, ROW_GAP);
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
        let grid_h = if threads {
            body - THREADS_H - GAP
        } else {
            body
        };
        let rows = ((grid_h + GAP) / (MIN_TILE_H + GAP)).floor().max(0.0) as usize;
        let count = order.len().min(rows * cols);
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
        layout.tabs = tabs();
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
/// VRAM goes first, then RAM. The temperatures are never dropped.
/// `line_width` measures one line of items at a given text size.
pub fn plan_strip(
    width: f32,
    height: f32,
    line_width: impl Fn(&[StripItem], f32) -> f32,
) -> StripPlan {
    use StripItem::*;
    let candidates: [&[StripItem]; 3] = [
        &[Cpu, Gpu, Hotspot, Ram, Vram],
        &[Cpu, Gpu, Hotspot, Ram],
        &[Cpu, Gpu, Hotspot],
    ];
    let two_lines = height >= STRIP_TWO_LINES_H;
    let line_h = if two_lines { height / 2.0 } else { height };
    let start = (line_h * 0.46).clamp(STRIP_MIN_SIZE, 26.0);
    let split = |items: &[StripItem]| -> Vec<Vec<StripItem>> {
        if two_lines {
            let (a, b) = items.split_at(3);
            [a.to_vec(), b.to_vec()]
                .into_iter()
                .filter(|l| !l.is_empty())
                .collect()
        } else {
            vec![items.to_vec()]
        }
    };
    let available = width - PAD * 2.0;
    for items in candidates {
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
        lines: split(candidates[2]),
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
        let plan = plan_strip(320.0, 46.0, measured(60.5));
        assert_eq!(plan.size, STRIP_MIN_SIZE);
        assert_eq!(plan.lines[0].len(), 5);
    }

    #[test]
    fn strip_drops_vram_then_ram_but_never_temperatures() {
        use StripItem::*;
        let dropped_one = plan_strip(320.0, 46.0, measured(70.0));
        assert_eq!(dropped_one.lines[0], vec![Cpu, Gpu, Hotspot, Ram]);
        let dropped_two = plan_strip(240.0, 46.0, measured(70.0));
        assert_eq!(dropped_two.lines[0], vec![Cpu, Gpu, Hotspot]);
        let too_tight = plan_strip(100.0, 46.0, measured(70.0));
        assert_eq!(too_tight.lines[0], vec![Cpu, Gpu, Hotspot]);
    }

    #[test]
    fn strip_wraps_to_two_lines_when_tall_enough() {
        let plan = plan_strip(320.0, 70.0, measured(58.0));
        assert_eq!(plan.lines.len(), 2);
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
            let again = arrange(w, h, 0.0, false);
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
            let (full, simple) = (plain(w, h), compute(w, h, options));
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

    #[test]
    fn hit_testing_finds_the_right_app() {
        let l = with_apps(340.0, 470.0, 2, false);
        let second = l.app_rows[1];
        assert_eq!(l.app_at(second.x + 5.0, second.y + 5.0), Some(1));
        assert_eq!(l.app_at(20.0, 40.0), None);
    }
}
