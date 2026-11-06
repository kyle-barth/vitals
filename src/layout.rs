//! Window layout. Pure geometry with no drawing or Win32, so that every
//! window size can be tested. All values are device-independent pixels.
//!
//! The window moves between three modes as it is resized:
//! - `Strip`: one or two lines of key numbers, with no title bar.
//! - `Rows`: a single column of metric rows, for tall, thin windows.
//! - `Tiles`: a grid that adds tiles, and enlarges them, as space grows.

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
/// The grid must show at least this many tiles, otherwise the strip is better.
const MIN_TILES: usize = 4;

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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub metric: MetricId,
    pub rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
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
}

impl Layout {
    #[cfg(test)]
    pub fn shows(&self, metric: MetricId) -> bool {
        self.cells.iter().any(|c| c.metric == metric)
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

pub fn compute(width: f32, height: f32) -> Layout {
    let mut layout = Layout {
        width,
        height,
        mode: Mode::Strip,
        cells: Vec::new(),
        threads: None,
        buttons: Vec::new(),
    };
    let body = (height - TITLE_H - PAD).max(0.0);
    if width < ROWS_BELOW_WIDTH {
        let count = (((body + ROW_GAP) / (MIN_ROW_H + ROW_GAP)).floor() as usize).min(ROWS.len());
        if count >= MIN_TILES {
            layout.mode = Mode::Rows;
            grid(&mut layout, &ROWS[..count], 1, body, ROW_GAP);
        }
    } else {
        let large = width >= LARGE_W && height >= LARGE_H;
        let (order, cols): (&[MetricId], usize) = if width >= LARGE_W {
            (if large { &TILES_4 } else { &TILES_4[..12] }, 4)
        } else if width >= 500.0 {
            (&TILES_3, 3)
        } else {
            (&TILES_2, 2)
        };
        let grid_h = if large { body - THREADS_H - GAP } else { body };
        let rows = ((grid_h + GAP) / (MIN_TILE_H + GAP)).floor().max(0.0) as usize;
        let count = order.len().min(rows * cols);
        if count >= MIN_TILES {
            layout.mode = Mode::Tiles;
            grid(&mut layout, &order[..count], cols, grid_h, GAP);
            if large {
                let y = TITLE_H + grid_h + GAP;
                layout.threads = Some(Rect::new(PAD, y, width - PAD * 2.0, THREADS_H));
            }
        }
    }
    layout.buttons = buttons(width, layout.mode);
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
    // Pin and close are kept at every width. The others step aside when narrow.
    let order: &[Button] = if mode == Mode::Strip || width < 200.0 {
        &[Button::Close, Button::Pin]
    } else {
        &[
            Button::Close,
            Button::Maximize,
            Button::Minimize,
            Button::Pin,
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

    fn sizes() -> impl Iterator<Item = (f32, f32)> {
        (140..=1600)
            .step_by(23)
            .flat_map(|w| (36..=1000).step_by(17).map(move |h| (w as f32, h as f32)))
    }

    #[test]
    fn every_cell_stays_inside_the_window() {
        for (w, h) in sizes() {
            for c in compute(w, h).cells {
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
            let l = compute(w, h);
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
            let l = compute(w, h);
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
            let l = compute(w, h);
            if l.mode != Mode::Strip {
                assert!(l.cells.len() >= MIN_TILES, "{w}x{h}");
                assert!(l.shows(CpuTemp) && l.shows(GpuTemp), "{w}x{h}");
            }
        }
    }

    #[test]
    fn four_tiles_fit_from_about_146_tall() {
        assert_eq!(compute(340.0, 146.0).mode, Mode::Tiles);
        assert_eq!(compute(340.0, 146.0).cells.len(), 4);
        assert_eq!(compute(340.0, 120.0).mode, Mode::Strip);
    }

    #[test]
    fn two_column_grid_keeps_cpu_left_and_gpu_right() {
        let l = compute(340.0, 470.0);
        let x = |m: MetricId| l.cells.iter().find(|c| c.metric == m).unwrap().rect.x;
        assert!(x(CpuTemp) < x(GpuTemp));
        assert!(x(CpuLoad) < x(GpuLoad));
        assert!(x(CpuPower) < x(GpuPower));
    }

    #[test]
    fn default_window_shows_hotspot_and_memory() {
        let l = compute(340.0, 470.0);
        assert!(l.shows(GpuHotspot) && l.shows(Memory) && l.shows(VideoMemory));
    }

    #[test]
    fn small_window_shows_hotspot_and_memory_in_its_first_four() {
        let l = compute(250.0, 160.0);
        assert_eq!(l.cells.len(), 4);
        assert!(l.shows(GpuHotspot) && l.shows(Memory));
    }

    #[test]
    fn tall_thin_window_uses_rows_and_shows_eleven_metrics() {
        let l = compute(190.0, 640.0);
        assert_eq!(l.mode, Mode::Rows);
        assert_eq!(l.cells.len(), 11);
    }

    #[test]
    fn large_window_adds_extra_tiles_and_thread_bars() {
        let l = compute(1240.0, 760.0);
        assert_eq!(l.cells.len(), 16);
        assert!(l.shows(Upload) && l.shows(DiskRead));
        assert!(l.threads.is_some());
        assert!(compute(1240.0, 400.0).threads.is_none());
    }

    #[test]
    fn pin_and_close_exist_at_every_size() {
        for (w, h) in sizes() {
            let l = compute(w, h);
            assert!(l.buttons.iter().any(|(b, _)| *b == Button::Pin), "{w}x{h}");
            assert!(
                l.buttons.iter().any(|(b, _)| *b == Button::Close),
                "{w}x{h}"
            );
        }
    }

    #[test]
    fn hit_testing_finds_cells_and_buttons() {
        let l = compute(340.0, 470.0);
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
}
