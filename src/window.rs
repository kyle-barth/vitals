//! The Win32 window: a borderless, fully resizable frame that keeps the native
//! behaviour people expect, including Snap, the Win11 snap layout flyout on the
//! maximize button, rounded corners, shadows and per-monitor DPI scaling.

use std::collections::HashMap;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT,
    POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTONEAREST,
    MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromRect, MonitorFromWindow, PAINTSTRUCT,
};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    ProcessPowerThrottling, SetProcessInformation,
};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PCWSTR, Result, w};

use crate::apps::{self, ProcessId, Sort, SortKey};
use crate::clipboard;
use crate::gfx::Gfx;
use crate::helper::CoreLink;
use crate::layout::{self, Button, Layout, Mode, Options, Page, PartShape, Preset, TITLE_H};
use crate::menu::{self, Action, MenuState, WM_APP_MENU};
use crate::metrics::{ALL, CoreStatus, MetricId, Monitor};
use crate::sensors::processes::{EndOutcome, end_app};
use crate::sensors::system::{CpuVendor, cpu_vendor};
use crate::sensors::{Latest, Sampler};
use crate::settings::Settings;
use crate::severity::Severity;
use crate::specs::{PartKind, Specs};
use crate::startup;
use crate::ui::{self, DARK, Hover, LIGHT, View};

const CLASS_NAME: PCWSTR = w!("Vitals.Window");
const INSTANCE_MUTEX: PCWSTR = w!("Local\\Vitals.SingleInstance");
const APP_TITLE: &str = "Vitals";
const WM_APP_SAMPLE: u32 = WM_APP + 1;
const WM_APP_ENDED: u32 = WM_APP + 2;
/// The Specs page's facts have been read.
const WM_APP_SPECS: u32 = WM_APP + 4;
/// A copy has been tried. `wparam` is 1 if it worked.
const WM_APP_COPIED: u32 = WM_APP + 5;
const COPIED_TIMER: usize = 1;
/// How long the copy button says what happened.
const COPIED_SHOWS_MS: u32 = 2000;
const KEY_C: usize = 0x43;
/// Samples before an unanswered End question goes away by itself.
const CONFIRM_SAMPLES: u32 = 10;
/// Samples that a message stays on an app's row.
const NOTICE_SAMPLES: u32 = 6;
const NEEDS_ADMIN: &str = "Needs admin to end";
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_NCMOUSELEAVE: u32 = 0x02A2;
const BASE_DPI: f32 = 96.0;
/// Lines of the Apps table moved by one notch of the wheel.
const WHEEL_LINES: f32 = 3.0;
const WHEEL_NOTCH: f32 = 120.0;
/// Clicking the menu's button while it is open closes the menu by taking focus.
/// That click must not then open it again.
const MENU_REOPEN_DELAY: Duration = Duration::from_millis(300);
/// The same alert sounds again no sooner than this.
const ALERT_AGAIN_AFTER: Duration = Duration::from_secs(600);
/// Narrower than this and the strip could not show all three temperatures.
const MIN_SIZE: (i32, i32) = (190, 36);
const EDGE: f32 = 5.0;
const TOP_EDGE: f32 = 3.0;

/// Win32 calls that send messages straight back to this window. Making one while
/// holding `&mut App` would re-enter `window_proc` and create a second `&mut App`,
/// which is undefined behaviour, so they wait in `App::pending` until the borrow
/// has ended.
enum Deferred {
    SetTitle(HSTRING),
    Show(SHOW_WINDOW_CMD),
    Pin {
        on_top: bool,
    },
    Place(RECT),
    /// Resizes to a size in device-independent pixels, keeping to the screen.
    Resize((i32, i32)),
    TitleBarTheme {
        dark: bool,
    },
    OpenMenu {
        anchor: (i32, i32),
        above: i32,
        state: MenuState,
        dark: bool,
        topmost: bool,
    },
    /// Emptying the clipboard sends a message to its last owner, which may be
    /// this window.
    Copy(String),
    /// One alert sound, and the taskbar button flashing until it's looked at.
    Alert,
}

impl Deferred {
    fn run(self, hwnd: HWND) {
        // SAFETY: the handle is this window's own, and every pointer passed
        // outlives its call. A failure here only leaves the window as it was.
        unsafe {
            match self {
                Deferred::SetTitle(title) => {
                    let _ = SetWindowTextW(hwnd, &title);
                }
                Deferred::Show(command) => {
                    let _ = ShowWindow(hwnd, command);
                }
                Deferred::Pin { on_top } => {
                    let order = if on_top { HWND_TOPMOST } else { HWND_NOTOPMOST };
                    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                    let _ = SetWindowPos(hwnd, Some(order), 0, 0, 0, 0, flags);
                }
                Deferred::Place(r) => {
                    let (width, height) = (r.right - r.left, r.bottom - r.top);
                    let flags = SWP_NOZORDER | SWP_NOACTIVATE;
                    let _ = SetWindowPos(hwnd, None, r.left, r.top, width, height, flags);
                }
                Deferred::Resize((width, height)) => {
                    let scale = GetDpiForWindow(hwnd) as f32 / BASE_DPI;
                    let mut now = RECT::default();
                    let mut info = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                    if GetWindowRect(hwnd, &mut now).is_ok()
                        && GetMonitorInfoW(monitor, &mut info).as_bool()
                    {
                        let work = info.rcWork;
                        let (left, top, right, bottom) = layout::fit_window(
                            (now.left, now.top, now.right, now.bottom),
                            (work.left, work.top, work.right, work.bottom),
                            (
                                (width as f32 * scale).round() as i32,
                                (height as f32 * scale).round() as i32,
                            ),
                        );
                        let flags = SWP_NOZORDER | SWP_NOACTIVATE;
                        let (w, h) = (right - left, bottom - top);
                        let _ = SetWindowPos(hwnd, None, left, top, w, h, flags);
                    }
                }
                Deferred::OpenMenu {
                    anchor,
                    above,
                    state,
                    dark,
                    topmost,
                } => {
                    if menu::open(hwnd, anchor, above, state, dark, topmost).is_err() {
                        menu::tell_owner(hwnd, Action::Closed);
                    }
                }
                Deferred::Copy(text) => {
                    let copied = clipboard::set_text(hwnd, &text);
                    let _ = PostMessageW(
                        Some(hwnd),
                        WM_APP_COPIED,
                        WPARAM(usize::from(copied)),
                        LPARAM(0),
                    );
                }
                Deferred::Alert => {
                    let _ = MessageBeep(MB_ICONEXCLAMATION);
                    let flash = FLASHWINFO {
                        cbSize: size_of::<FLASHWINFO>() as u32,
                        hwnd,
                        dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
                        uCount: 0,
                        dwTimeout: 0,
                    };
                    let _ = FlashWindowEx(&flash);
                }
                Deferred::TitleBarTheme { dark } => {
                    let value = i32::from(dark);
                    let _ = DwmSetWindowAttribute(
                        hwnd,
                        DWMWA_USE_IMMERSIVE_DARK_MODE,
                        (&raw const value).cast(),
                        size_of::<i32>() as u32,
                    );
                }
            }
        }
    }
}

/// Runs the deferred calls.
///
/// # Safety
/// `app` must be valid, and no reference to the `App` may be live.
unsafe fn run_deferred(app: *mut App, hwnd: HWND) {
    // SAFETY: the caller guarantees this, and the borrow ends with the statement.
    let pending = unsafe { std::mem::take(&mut (*app).pending) };
    for action in pending {
        action.run(hwnd);
    }
}

struct App {
    hwnd: HWND,
    gfx: Gfx,
    layout: Layout,
    monitor: Monitor,
    latest: Latest,
    core: CoreLink,
    sampler: Option<Sampler>,
    settings: Settings,
    dark: bool,
    hover: Hover,
    title: String,
    pending: Vec<Deferred>,
    /// The app being asked about before it is ended, and for how many samples.
    confirm: Option<(String, u32)>,
    /// A message on one app's row: its key, the text, and samples left to show.
    notice: Option<(String, &'static str, u32)>,
    /// The last End request's result, set by its background thread.
    ended: Arc<Mutex<Option<(String, EndOutcome)>>>,
    /// Asks the sampler for every app, which is only worth paying for while the
    /// Apps page is on screen.
    wants_table: Arc<AtomicBool>,
    /// How many lines of the Apps table are scrolled off the top.
    scroll: usize,
    /// Where the pointer last was, to refresh what it is over after a scroll.
    pointer: (f32, f32),
    menu_open: bool,
    menu_closed: Option<Instant>,
    /// What is in this PC, once read. It is read the first time Specs opens.
    specs: Option<Specs>,
    specs_reading: bool,
    /// Where the reading thread leaves its result.
    specs_read: Arc<Mutex<Option<Specs>>>,
    /// Whether the last copy worked, while the button is saying so.
    copied: Option<bool>,
    pc_name: Option<String>,
    /// When each alert last sounded, so one reading can't keep repeating.
    alerted: HashMap<String, Instant>,
    /// Where the summary's link to everything that needs attention was drawn.
    more_link: Option<layout::Rect>,
}

/// Where each part sits when the Specs page has two columns: the GPU side of
/// the PC on the right, everything else on the left.
fn shapes(specs: &Specs) -> Vec<PartShape> {
    specs
        .parts
        .iter()
        .map(|part| PartShape {
            items: part.items.len(),
            facts: part.facts.len(),
            right: matches!(
                part.kind,
                PartKind::Gpu | PartKind::Display | PartKind::Windows
            ),
        })
        .collect()
}

impl App {
    fn scale(&self) -> f32 {
        // SAFETY: the handle is valid for as long as `App` exists.
        unsafe { GetDpiForWindow(self.hwnd) as f32 / BASE_DPI }
    }

    fn options(&self) -> Options {
        let (apps, frozen) = self.app_counts();
        Options {
            apps,
            frozen,
            page: self.settings.page,
            simple: self.settings.simple,
            sort: self.settings.sort.key,
            absent: self.monitor.absent,
            parts: self.specs.as_ref().map(shapes).unwrap_or_default(),
        }
    }

    /// Plays the alert sound for `key`, at most once every 10 minutes each.
    fn alert(&mut self, key: String) {
        if !self.settings.sound_on_critical {
            return;
        }
        let now = Instant::now();
        if self
            .alerted
            .get(&key)
            .is_some_and(|at| now.duration_since(*at) < ALERT_AGAIN_AFTER)
        {
            return;
        }
        self.alerted.insert(key, now);
        self.pending.push(Deferred::Alert);
    }

    fn relayout(&mut self, width_px: u32, height_px: u32) {
        let scale = self.scale();
        self.layout = layout::compute(
            width_px as f32 / scale,
            height_px as f32 / scale,
            self.options(),
        );
        self.sync_table();
        if self.layout.specs.is_some() || self.layout.large.is_some() {
            self.read_specs();
        }
        self.clamp_scroll();
    }

    /// Reads what is in this PC on a thread of its own, once. It takes a
    /// moment, and most people never open the page, so it waits until then.
    fn read_specs(&mut self) {
        if self.specs.is_some() || self.specs_reading {
            return;
        }
        self.specs_reading = true;
        let (slot, target) = (self.specs_read.clone(), self.hwnd.0 as isize);
        std::thread::spawn(move || {
            let specs = crate::specs::build(&crate::sensors::hardware::read());
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(specs);
            }
            // SAFETY: posting to a window that has gone simply fails.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(target as *mut _)),
                    WM_APP_SPECS,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        });
    }

    fn on_specs(&mut self) {
        if let Some(specs) = self.specs_read.lock().ok().and_then(|mut slot| slot.take()) {
            self.specs = Some(specs);
            self.specs_reading = false;
            self.refresh_layout();
            self.invalidate();
        }
    }

    fn copy_specs(&mut self) {
        if let Some(specs) = &self.specs {
            self.pending.push(Deferred::Copy(specs.copy_text()));
        }
    }

    fn on_copied(&mut self, worked: bool) {
        self.copied = Some(worked);
        // SAFETY: the handle is valid, and a timer only posts WM_TIMER later.
        unsafe { SetTimer(Some(self.hwnd), COPIED_TIMER, COPIED_SHOWS_MS, None) };
        self.invalidate();
    }

    fn on_copied_timer(&mut self) {
        // SAFETY: the handle is valid, and the timer was set by `on_copied`.
        let _ = unsafe { KillTimer(Some(self.hwnd), COPIED_TIMER) };
        self.copied = None;
        self.invalidate();
    }

    /// Starts or stops the extra work of the Apps page to match what is showing.
    fn sync_table(&mut self) {
        let wanted = self.layout.table.is_some();
        let before = self.wants_table.swap(wanted, Ordering::Relaxed);
        if wanted && !before {
            if let Some(sampler) = &self.sampler {
                sampler.wake();
            }
        } else if before && !wanted {
            self.monitor.table = Vec::new();
            self.gfx.drop_icons();
        }
    }

    fn clamp_scroll(&mut self) {
        let last = match &self.layout.specs {
            Some(page) => page.max_scroll(),
            None => {
                let visible = self.layout.table.as_ref().map_or(0, |t| t.visible);
                self.monitor.table.len().saturating_sub(visible)
            }
        };
        self.scroll = self.scroll.min(last);
    }

    fn set_page(&mut self, page: Page) {
        if page != self.settings.page {
            self.settings.page = page;
            self.scroll = 0;
            self.confirm = None;
            self.refresh_layout();
            self.invalidate();
        }
    }

    fn set_sort(&mut self, sort: Sort) {
        self.settings.sort = sort;
        self.scroll = 0;
        let rows = std::mem::take(&mut self.monitor.table);
        self.monitor.table = apps::arrange(rows, sort, None);
        self.refresh_layout();
        self.invalidate();
    }

    fn click_heading(&mut self, key: SortKey) {
        let Some(table) = &self.layout.table else {
            return;
        };
        let shown: Vec<SortKey> = table.columns.iter().map(|(key, _)| *key).collect();
        let allowed = layout::allowed_columns(self.settings.simple);
        self.set_sort(self.settings.sort.after_click(key, &shown, allowed));
    }

    /// The order to keep for now, if rows must not move under the pointer.
    fn held_order(&self) -> Option<Vec<String>> {
        (self.hover.row.is_some() || self.confirm.is_some())
            .then(|| self.monitor.table.iter().map(|r| r.key.clone()).collect())
    }

    /// Scrolls the Apps table by lines, or the Specs page by whole parts.
    fn scroll_by(&mut self, delta: i16) {
        let per_notch = if self.layout.specs.is_some() {
            1.0
        } else {
            WHEEL_LINES
        };
        let lines = (delta as f32 / WHEEL_NOTCH * per_notch).round() as isize;
        self.scroll = self.scroll.saturating_add_signed(-lines);
        self.clamp_scroll();
        let (x, y) = self.pointer;
        self.set_hover(self.hover_at(x, y));
        self.invalidate();
    }

    fn app_counts(&self) -> (usize, bool) {
        let apps = &self.monitor.apps;
        (apps.len(), apps.iter().any(|a| a.frozen))
    }

    /// Fits the layout to the current window, after the list of apps changes.
    fn refresh_layout(&mut self) {
        let scale = self.scale();
        let (width, height) = (self.layout.width, self.layout.height);
        self.relayout((width * scale) as u32, (height * scale) as u32);
    }

    fn invalidate(&self) {
        // SAFETY: the handle is valid for as long as `App` exists.
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn paint(&mut self) {
        let theme = if self.dark { &DARK } else { &LIGHT };
        let view = View {
            layout: &self.layout,
            monitor: &self.monitor,
            theme,
            hover: self.hover,
            pinned: self.settings.always_on_top,
            confirm: self.confirm.as_ref().map(|(key, _)| key.as_str()),
            notice: self
                .notice
                .as_ref()
                .map(|(key, text, _)| (key.as_str(), *text)),
            simple: self.settings.simple,
            sort: self.settings.sort,
            scroll: self.scroll,
            menu_open: self.menu_open,
            specs: self.specs.as_ref(),
            copied: self.copied,
            pc_name: self.pc_name.as_deref(),
        };
        let mut link = None;
        let _ = self.gfx.draw(|canvas| link = ui::paint(canvas, &view));
        self.more_link = link;
    }

    /// Takes the newest snapshot from the sampler and updates the display.
    fn on_sample(&mut self) {
        let snapshot = self.latest.lock().ok().and_then(|mut slot| slot.take());
        let Some(snapshot) = snapshot else { return };
        let before = (self.app_counts(), self.monitor.absent);
        let held = self.held_order();
        let severities: Vec<Severity> = ALL
            .iter()
            .map(|&id| self.monitor.get(id).severity)
            .collect();
        let frozen_before: Vec<String> = self
            .monitor
            .apps
            .iter()
            .filter(|a| a.frozen)
            .map(|a| a.key.clone())
            .collect();
        self.monitor.update(snapshot);

        // A reading that has just turned Critical, or an app that has just
        // frozen, is worth a sound when that's switched on.
        for (i, &id) in ALL.iter().enumerate() {
            if self.monitor.get(id).severity == Severity::Critical
                && severities[i] != Severity::Critical
            {
                self.alert(format!("{id:?}"));
            }
        }
        let newly_frozen: Vec<String> = self
            .monitor
            .apps
            .iter()
            .filter(|a| a.frozen && !frozen_before.contains(&a.key))
            .map(|a| a.key.clone())
            .collect();
        for key in newly_frozen {
            self.alert(format!("frozen {key}"));
        }

        if !self.monitor.table.is_empty() {
            let rows = std::mem::take(&mut self.monitor.table);
            self.monitor.table = apps::arrange(rows, self.settings.sort, held.as_deref());
            self.clamp_scroll();
        }
        if (self.app_counts(), self.monitor.absent) != before {
            self.refresh_layout();
        }
        self.age_app_messages();

        // Declining the prompt is a clear answer, so don't ask at every launch.
        if self.monitor.core == CoreStatus::Declined {
            self.settings.core_temp = false;
        }

        // The title reaches the taskbar, so an alert shows even when covered.
        let frozen = self.monitor.apps.iter().find(|a| a.frozen);
        let alert = frozen
            .map(|app| format!("{} not responding", app.name))
            .or_else(|| ui::alert_text(&self.layout, &self.monitor));
        let title = alert.map_or_else(
            || APP_TITLE.to_string(),
            |text| format!("{text} - {APP_TITLE}"),
        );
        if title != self.title {
            self.pending
                .push(Deferred::SetTitle(HSTRING::from(title.as_str())));
            self.title = title;
        }

        // SAFETY: the handle is valid for as long as `App` exists.
        if unsafe { IsIconic(self.hwnd) }.as_bool() {
            // Nobody can see the table, so stop paying for it until restored.
            self.wants_table.store(false, Ordering::Relaxed);
        } else {
            self.invalidate();
        }
    }

    /// Drops the End question if its app has gone or it has been left too long,
    /// so an old question can't sit waiting for a stray click.
    fn age_app_messages(&mut self) {
        let (apps, table) = (&self.monitor.apps, &self.monitor.table);
        if let Some((key, age)) = &mut self.confirm {
            *age += 1;
            let running = apps.iter().any(|a| a.key == *key) || table.iter().any(|r| r.key == *key);
            if *age >= CONFIRM_SAMPLES || !running {
                self.confirm = None;
            }
        }
        if let Some((_, _, left)) = &mut self.notice {
            *left = left.saturating_sub(1);
            if *left == 0 {
                self.notice = None;
            }
        }
    }

    /// Ends every process of an app, away from the UI thread.
    fn end_app(&mut self, key: String, processes: Vec<ProcessId>) {
        let (result, target) = (self.ended.clone(), self.hwnd.0 as isize);
        std::thread::spawn(move || {
            let outcome = end_app(&processes);
            if let Ok(mut slot) = result.lock() {
                *slot = Some((key, outcome));
            }
            // SAFETY: posting to a window that has gone simply fails.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(target as *mut _)),
                    WM_APP_ENDED,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        });
    }

    fn on_ended(&mut self) {
        let result = self.ended.lock().ok().and_then(|mut slot| slot.take());
        if let Some((key, EndOutcome::NeedsAdmin)) = result {
            self.notice = Some((key, NEEDS_ADMIN, NOTICE_SAMPLES));
            self.invalidate();
        }
    }

    /// Handles a click on or away from the busy list. Returns true if it used it.
    fn click_app(&mut self, x: f32, y: f32) -> bool {
        let Some(index) = self.layout.app_at(x, y) else {
            // Clicking anywhere else is a clear "no".
            let had_question = self.confirm.take().is_some();
            if had_question {
                self.invalidate();
            }
            return had_question;
        };
        let row = self.layout.app_rows[index];
        let Some(app) = self.monitor.apps.get(index) else {
            return true;
        };
        let asking = self
            .confirm
            .as_ref()
            .is_some_and(|(key, _)| *key == app.key);
        if asking {
            let (end, keep) = layout::confirm_buttons(row);
            if end.contains(x, y) {
                let (key, processes) = (app.key.clone(), app.processes.clone());
                self.confirm = None;
                self.end_app(key, processes);
            } else if keep.contains(x, y) {
                self.confirm = None;
            }
        } else if app.can_end && layout::end_button(row).contains(x, y) {
            self.confirm = Some((app.key.clone(), 0));
        } else if self.layout.mode != Mode::Strip {
            // The rest of the row leads to the full list, in the order that puts
            // this app at the top. A frozen app is first in any order.
            let key = if app.frozen {
                SortKey::Cpu
            } else {
                app.biggest
            };
            self.settings.sort = Sort {
                key,
                descending: true,
            };
            self.set_page(Page::Apps);
        }
        self.invalidate();
        true
    }

    /// Handles a click on the Apps page.
    fn click_table(&mut self, x: f32, y: f32) {
        let Some(table) = &self.layout.table else {
            return;
        };
        if let Some(key) = table.header_at(x, y) {
            self.click_heading(key);
            return;
        }
        let slot = table.row_at(x, y);
        let target = slot.and_then(|slot| {
            let row = self.monitor.table.get(slot + self.scroll)?;
            Some((table.row(slot), row))
        });
        let Some((rect, row)) = target else {
            // Clicking anywhere else is a clear "no".
            if self.confirm.take().is_some() {
                self.invalidate();
            }
            return;
        };
        let asking = self
            .confirm
            .as_ref()
            .is_some_and(|(key, _)| *key == row.key);
        if asking {
            let (end, keep) = layout::confirm_buttons(rect);
            if end.contains(x, y) {
                let (key, processes) = (row.key.clone(), row.processes.clone());
                self.confirm = None;
                self.end_app(key, processes);
            } else if keep.contains(x, y) {
                self.confirm = None;
            }
        } else if row.can_end && layout::end_button(rect).contains(x, y) {
            self.confirm = Some((row.key.clone(), 0));
        } else {
            self.confirm = None;
        }
        self.invalidate();
    }

    /// Whether a point is on a button of the busy list, which must get clicks
    /// even in the strip, where everything else drags the window.
    fn on_app_control(&self, x: f32, y: f32) -> bool {
        let Some(index) = self.layout.app_at(x, y) else {
            return false;
        };
        let row = self.layout.app_rows[index];
        let Some(app) = self.monitor.apps.get(index) else {
            return false;
        };
        if self
            .confirm
            .as_ref()
            .is_some_and(|(key, _)| *key == app.key)
        {
            let (end, keep) = layout::confirm_buttons(row);
            end.contains(x, y) || keep.contains(x, y)
        } else {
            app.can_end && layout::end_button(row).contains(x, y)
        }
    }

    /// Pointer position in device-independent pixels, relative to the window.
    fn window_point(&self, screen_x: i32, screen_y: i32) -> (f32, f32) {
        let mut rect = RECT::default();
        // SAFETY: the handle and out pointer are valid.
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut rect);
        }
        let scale = self.scale();
        (
            (screen_x - rect.left) as f32 / scale,
            (screen_y - rect.top) as f32 / scale,
        )
    }

    fn hit_test(&self, lparam: LPARAM) -> u32 {
        let (x, y) = self.window_point(signed_low(lparam), signed_high(lparam));
        let (w, h) = (self.layout.width, self.layout.height);
        // SAFETY: the handle is valid for as long as `App` exists.
        let maximized = unsafe { IsZoomed(self.hwnd) }.as_bool();
        if !maximized {
            let (left, right) = (x < EDGE, x >= w - EDGE);
            let (top, bottom) = (y < TOP_EDGE, y >= h - EDGE);
            match (left, right, top, bottom) {
                (true, _, true, _) => return HTTOPLEFT,
                (_, true, true, _) => return HTTOPRIGHT,
                (true, _, _, true) => return HTBOTTOMLEFT,
                (_, true, _, true) => return HTBOTTOMRIGHT,
                (true, ..) => return HTLEFT,
                (_, true, ..) => return HTRIGHT,
                (_, _, true, _) => return HTTOP,
                (_, _, _, true) => return HTBOTTOM,
                _ => {}
            }
        }
        if let Some(button) = self.layout.button_at(x, y) {
            if self.layout.mode != Mode::Strip || self.hover.inside {
                // Reporting the real maximize button is what makes Windows
                // show the snap layout flyout when the pointer rests on it.
                return if button == Button::Maximize {
                    HTMAXBUTTON
                } else {
                    HTCLIENT
                };
            }
        }
        if self.on_app_control(x, y) || self.layout.tab_at(x, y).is_some() {
            return HTCLIENT;
        }
        if self.layout.mode == Mode::Strip || y < TITLE_H {
            HTCAPTION
        } else {
            HTCLIENT
        }
    }

    fn set_hover(&mut self, hover: Hover) {
        if hover != self.hover {
            self.hover = hover;
            self.invalidate();
        }
    }

    fn pointer_moved(&mut self, x: f32, y: f32, non_client: bool) {
        let mut flags = TME_LEAVE;
        if non_client {
            flags |= TME_NONCLIENT;
        }
        let mut track = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: flags,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        // SAFETY: the structure is valid and sized for the call.
        unsafe {
            let _ = TrackMouseEvent(&mut track);
        }
        self.pointer = (x, y);
        self.set_hover(self.hover_at(x, y));
    }

    fn hover_at(&self, x: f32, y: f32) -> Hover {
        let table = self.layout.table.as_ref();
        Hover {
            inside: true,
            button: self.layout.button_at(x, y),
            app: self.layout.app_at(x, y),
            tab: self.layout.tab_at(x, y),
            row: table
                .and_then(|t| t.row_at(x, y))
                .map(|slot| slot + self.scroll),
            heading: table.and_then(|t| t.header_at(x, y)),
            copy: self
                .layout
                .specs
                .as_ref()
                .is_some_and(|page| page.copy.contains(x, y)),
            more: self.more_link.is_some_and(|r| r.contains(x, y)),
        }
    }

    /// Whether the pointer is really gone, rather than crossing between the
    /// client and non-client parts of this same window.
    fn pointer_left(&mut self) {
        let mut point = POINT::default();
        let mut rect = RECT::default();
        // SAFETY: the out pointers are valid.
        let inside = unsafe {
            GetCursorPos(&mut point).is_ok()
                && GetWindowRect(self.hwnd, &mut rect).is_ok()
                && point.x >= rect.left
                && point.x < rect.right
                && point.y >= rect.top
                && point.y < rect.bottom
        };
        if !inside {
            self.set_hover(Hover::default());
        }
    }

    fn press(&mut self, button: Button) {
        match button {
            Button::Close => {
                // SAFETY: the handle is valid for as long as `App` exists.
                let _ = unsafe { PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
            Button::Minimize => self.pending.push(Deferred::Show(SW_MINIMIZE)),
            Button::Maximize => {
                // SAFETY: the handle is valid for as long as `App` exists.
                let maximized = unsafe { IsZoomed(self.hwnd) }.as_bool();
                let command = if maximized { SW_RESTORE } else { SW_MAXIMIZE };
                self.pending.push(Deferred::Show(command));
            }
            Button::Pin => {
                self.settings.always_on_top = !self.settings.always_on_top;
                self.queue_pin();
                self.invalidate();
            }
            Button::Menu => self.open_menu(),
        }
    }

    fn is_maximized(&self) -> bool {
        // SAFETY: the handle is valid for as long as `App` exists.
        unsafe { IsZoomed(self.hwnd) }.as_bool()
    }

    /// The window's size in device-independent pixels.
    fn current_size(&self) -> (i32, i32) {
        (
            self.layout.width.round() as i32,
            self.layout.height.round() as i32,
        )
    }

    fn menu_state(&self) -> MenuState {
        let here = self.current_size();
        let maximized = self.is_maximized();
        let preset = Preset::matching(here).filter(|_| !maximized);
        let is_yours = preset.is_none() && !maximized;
        MenuState {
            preset,
            yours: if is_yours {
                Some(here)
            } else {
                self.settings.your_size
            },
            is_yours,
            simple: self.settings.simple,
            strip: self.layout.mode == Mode::Strip,
            start_with_windows: startup::is_enabled(),
            sound_on_critical: self.settings.sound_on_critical,
        }
    }

    fn open_menu(&mut self) {
        let just_closed = self
            .menu_closed
            .is_some_and(|closed| closed.elapsed() < MENU_REOPEN_DELAY);
        let button = self
            .layout
            .buttons
            .iter()
            .find(|(button, _)| *button == Button::Menu)
            .map(|(_, rect)| *rect);
        let Some(button) = button.filter(|_| !self.menu_open && !just_closed) else {
            return;
        };
        let mut window = RECT::default();
        // SAFETY: the handle and out pointer are valid.
        if unsafe { GetWindowRect(self.hwnd, &mut window) }.is_err() {
            return;
        }
        let scale = self.scale();
        let at = |offset: f32| (offset * scale).round() as i32;
        self.menu_open = true;
        self.pending.push(Deferred::OpenMenu {
            anchor: (
                window.left + at(button.right()),
                window.top + at(button.bottom()),
            ),
            above: window.top + at(button.y),
            state: self.menu_state(),
            dark: self.dark,
            topmost: self.settings.always_on_top,
        });
        self.invalidate();
    }

    fn on_menu(&mut self, action: Action) {
        match action {
            Action::Size(preset) => self.resize_to(preset.size()),
            Action::YourSize => {
                if let Some(size) = self
                    .settings
                    .your_size
                    .filter(|_| !self.menu_state().is_yours)
                {
                    self.resize_to(size);
                }
            }
            Action::OpenApps => {
                self.set_page(Page::Apps);
                self.resize_to(Preset::Medium.size());
            }
            Action::Simple | Action::Advanced => {
                self.settings.simple = action == Action::Simple;
                self.refresh_layout();
                self.invalidate();
            }
            Action::StartWithWindows => {
                startup::set(!startup::is_enabled());
            }
            Action::SoundOnCritical => {
                self.settings.sound_on_critical = !self.settings.sound_on_critical;
            }
            Action::Closed => {
                self.menu_open = false;
                self.menu_closed = Some(Instant::now());
                self.invalidate();
            }
        }
    }

    /// Moves to a size, first remembering one chosen by hand so that the menu can
    /// offer it again.
    fn resize_to(&mut self, size: (i32, i32)) {
        let here = self.current_size();
        if self.is_maximized() {
            self.pending.push(Deferred::Show(SW_RESTORE));
        } else if Preset::matching(here).is_none() {
            self.settings.your_size = Some(here);
        }
        self.pending.push(Deferred::Resize(size));
    }

    fn queue_pin(&mut self) {
        self.pending.push(Deferred::Pin {
            on_top: self.settings.always_on_top,
        });
    }

    fn click(&mut self, x: f32, y: f32) {
        if let Some(button) = self.layout.button_at(x, y) {
            self.press(button);
            return;
        }
        if let Some(page) = self.layout.tab_at(x, y) {
            self.set_page(page);
            return;
        }
        if let Some(page) = &self.layout.specs {
            if page.copy.contains(x, y) {
                self.copy_specs();
            }
            return;
        }
        if self.layout.table.is_some() {
            self.click_table(x, y);
            return;
        }
        if self.click_app(x, y) {
            return;
        }
        if self.more_link.is_some_and(|r| r.contains(x, y)) {
            self.resize_to(Preset::Large.size());
            return;
        }
        let Some(metric) = self.layout.cell_at(x, y) else {
            return;
        };
        if metric == MetricId::CpuTemp && self.monitor.cpu_temp_metric() == MetricId::IoDieTemp {
            if matches!(self.monitor.core, CoreStatus::Off | CoreStatus::Declined) {
                self.settings.core_temp = true;
                self.core.start(self.hwnd.0 as isize);
            }
            return;
        }
        if self.monitor.get(metric).peak.is_some() {
            self.monitor.reset_peak(metric);
            self.invalidate();
        }
    }

    fn save_settings(&mut self) {
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        // SAFETY: the handle and structure are valid, and the length is set.
        if unsafe { GetWindowPlacement(self.hwnd, &mut placement) }.is_ok() {
            let r = placement.rcNormalPosition;
            self.settings.placement = Some((r.left, r.top, r.right, r.bottom));
            self.settings.maximized = placement.showCmd == SW_SHOWMAXIMIZED.0 as u32;
        }
        let _ = self.settings.save();
    }

    fn apply_theme(&mut self) {
        self.dark = system_uses_dark_mode();
        self.pending
            .push(Deferred::TitleBarTheme { dark: self.dark });
        self.invalidate();
    }
}

fn control_held() -> bool {
    // SAFETY: this only reads the calling thread's keyboard state.
    unsafe { GetKeyState(i32::from(VK_CONTROL.0)) < 0 }
}

fn signed_low(value: LPARAM) -> i32 {
    (value.0 & 0xffff) as i16 as i32
}

fn signed_high(value: LPARAM) -> i32 {
    ((value.0 >> 16) & 0xffff) as i16 as i32
}

fn system_uses_dark_mode() -> bool {
    let (mut light, mut size) = (0u32, size_of::<u32>() as u32);
    // SAFETY: the out pointers are valid and sized for a REG_DWORD.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut light).cast()),
            Some(&mut size),
        )
    };
    status.is_err() || light == 0
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: reads a value this window stores for itself.
    let app = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut App;
    if app.is_null() {
        if message == WM_NCCALCSIZE && wparam.0 != 0 {
            return LRESULT(0);
        }
    } else if message == WM_PAINT {
        let mut paint = PAINTSTRUCT::default();
        // SAFETY: Begin and End are paired on this window. BeginPaint can send
        // WM_ERASEBKGND, so the `App` is only touched once it has returned.
        unsafe {
            BeginPaint(hwnd, &mut paint);
            (*app).paint();
            let _ = EndPaint(hwnd, &paint);
        }
        return LRESULT(0);
    } else {
        // SAFETY: the pointer is stored once, straight after creation, and is
        // cleared before the `App` is freed. Everything runs on the UI thread,
        // and this borrow ends before anything can re-enter this function.
        let result = handle(unsafe { &mut *app }, message, wparam, lparam);
        // SAFETY: the borrow above has ended.
        unsafe { run_deferred(app, hwnd) };
        if let Some(result) = result {
            return result;
        }
    }
    // SAFETY: forwards this window's own message unchanged.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn handle(app: &mut App, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let hwnd = app.hwnd;
    match message {
        WM_APP_SAMPLE => app.on_sample(),
        WM_APP_ENDED => app.on_ended(),
        WM_APP_SPECS => app.on_specs(),
        WM_APP_COPIED => app.on_copied(wparam.0 != 0),
        WM_TIMER if wparam.0 == COPIED_TIMER => app.on_copied_timer(),
        WM_KEYDOWN if wparam.0 == KEY_C && app.layout.specs.is_some() && control_held() => {
            app.copy_specs();
        }
        WM_APP_MENU => {
            if let Some(action) = Action::from_code(wparam.0) {
                app.on_menu(action);
            }
        }
        WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 && app.confirm.is_some() => {
            app.confirm = None;
            app.invalidate();
        }
        WM_NCCALCSIZE if wparam.0 != 0 => {
            // Claiming the whole window as client area removes the standard
            // frame while Windows keeps its resize, Snap and shadow behaviour.
            // A maximized window hangs past the screen by its frame thickness,
            // so that overhang is taken back here.
            // SAFETY: for this message, `lparam` points to NCCALCSIZE_PARAMS.
            unsafe {
                if IsZoomed(hwnd).as_bool() {
                    let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
                    let dpi = GetDpiForWindow(hwnd);
                    let frame = GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi)
                        + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    let rect = &mut params.rgrc[0];
                    rect.left += frame;
                    rect.top += frame;
                    rect.right -= frame;
                    rect.bottom -= frame;
                }
            }
            return Some(LRESULT(0));
        }
        WM_NCHITTEST => return Some(LRESULT(app.hit_test(lparam) as isize)),
        WM_ERASEBKGND => return Some(LRESULT(1)),
        WM_SIZE => {
            let (width, height) = (
                (lparam.0 & 0xffff) as u32,
                ((lparam.0 >> 16) & 0xffff) as u32,
            );
            if width > 0 && height > 0 {
                app.gfx.resize(width, height);
                app.relayout(width, height);
                // Painting now, rather than waiting for WM_PAINT, keeps the
                // content in step with the frame during a live resize.
                app.paint();
            }
            return Some(LRESULT(0));
        }
        WM_GETMINMAXINFO => {
            // SAFETY: for this message, `lparam` points to MINMAXINFO.
            unsafe {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                let dpi = GetDpiForWindow(hwnd) as f32 / BASE_DPI;
                info.ptMinTrackSize.x = (MIN_SIZE.0 as f32 * dpi) as i32;
                info.ptMinTrackSize.y = (MIN_SIZE.1 as f32 * dpi) as i32;
            }
            return Some(LRESULT(0));
        }
        WM_DPICHANGED => {
            let dpi = (wparam.0 & 0xffff) as f32;
            app.gfx.set_dpi(dpi);
            // SAFETY: for this message, `lparam` points to the suggested rectangle.
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            app.pending.push(Deferred::Place(suggested));
            return Some(LRESULT(0));
        }
        WM_MOUSEMOVE => {
            let scale = app.scale();
            let (x, y) = (
                signed_low(lparam) as f32 / scale,
                signed_high(lparam) as f32 / scale,
            );
            app.pointer_moved(x, y, false);
        }
        WM_NCMOUSEMOVE => {
            let (x, y) = app.window_point(signed_low(lparam), signed_high(lparam));
            app.pointer_moved(x, y, true);
        }
        WM_MOUSELEAVE | WM_NCMOUSELEAVE => app.pointer_left(),
        WM_MOUSEWHEEL => {
            app.scroll_by(((wparam.0 >> 16) & 0xffff) as i16);
            return Some(LRESULT(0));
        }
        WM_LBUTTONUP => {
            let scale = app.scale();
            app.click(
                signed_low(lparam) as f32 / scale,
                signed_high(lparam) as f32 / scale,
            );
        }
        WM_NCLBUTTONDOWN if wparam.0 as u32 == HTMAXBUTTON => return Some(LRESULT(0)),
        WM_NCLBUTTONUP if wparam.0 as u32 == HTMAXBUTTON => {
            app.press(Button::Maximize);
            return Some(LRESULT(0));
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED => app.apply_theme(),
        WM_ENDSESSION if wparam.0 != 0 => app.save_settings(),
        WM_DESTROY => {
            app.save_settings();
            // SAFETY: called on the thread that owns the message queue.
            unsafe { PostQuitMessage(0) };
            return Some(LRESULT(0));
        }
        _ => {}
    }
    None
}

/// Lets Windows run this on efficient cores at low clocks where it can. A
/// monitor should cost as little as possible, and one update per second has no
/// need for peak speed.
fn request_efficiency_mode() {
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    };
    // SAFETY: the structure is valid for the size given.
    unsafe {
        let _ = SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            (&raw const state).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

/// Brings an existing window forward. Returns true if there was one.
fn activate_existing() -> bool {
    // SAFETY: the class name is a valid string.
    let Ok(existing) = (unsafe { FindWindowW(CLASS_NAME, None) }) else {
        return false;
    };
    // SAFETY: a stale handle simply makes these calls fail harmlessly.
    unsafe {
        let _ = ShowWindow(existing, SW_RESTORE);
        let _ = SetForegroundWindow(existing);
    }
    true
}

/// The saved window rectangle, if it is still on a connected monitor.
fn saved_rect(settings: &Settings) -> Option<RECT> {
    let (left, top, right, bottom) = settings.placement?;
    let rect = RECT {
        left,
        top,
        right,
        bottom,
    };
    let sized = right - left >= MIN_SIZE.0 && bottom - top >= MIN_SIZE.1;
    // SAFETY: the rectangle is valid for the call.
    let on_screen = !unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) }.is_invalid();
    (sized && on_screen).then_some(rect)
}

/// `at_sign_in` is set when Windows starts Vitals as you sign in.
pub fn run(at_sign_in: bool) -> Result<()> {
    // SAFETY: the mutex is held for the life of the process, so only one
    // window is ever running.
    let _instance: HANDLE = unsafe { CreateMutexW(None, true, INSTANCE_MUTEX)? };
    // SAFETY: reads this thread's last error, set by the call above.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        activate_existing();
        return Ok(());
    }
    request_efficiency_mode();
    startup::follow_moved_exe();

    let settings = Settings::load();
    // SAFETY: the window class and creation parameters are valid, and the
    // `App` box outlives the message loop.
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hIcon: LoadIconW(Some(instance), PCWSTR(std::ptr::without_provenance(1)))?,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let system_scale = GetDpiForSystem() as f32 / BASE_DPI;
        let (x, y, width, height) = match saved_rect(&settings) {
            Some(r) => (r.left, r.top, r.right - r.left, r.bottom - r.top),
            None => (
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                (Preset::Medium.size().0 as f32 * system_scale) as i32,
                (Preset::Medium.size().1 as f32 * system_scale) as i32,
            ),
        };
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            &HSTRING::from(APP_TITLE),
            WS_OVERLAPPEDWINDOW,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(instance),
            None,
        )?;

        let corners = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corners).cast(),
            size_of::<i32>() as u32,
        );

        let latest: Latest = Arc::new(Mutex::new(None));
        let core = CoreLink::default();
        let dpi = GetDpiForWindow(hwnd) as f32;
        let mut client = RECT::default();
        GetClientRect(hwnd, &mut client)?;
        let scale = dpi / BASE_DPI;
        let layout = layout::compute(
            client.right as f32 / scale,
            client.bottom as f32 / scale,
            Options {
                page: settings.page,
                simple: settings.simple,
                sort: settings.sort.key,
                ..Default::default()
            },
        );
        let wants_table = Arc::new(AtomicBool::new(layout.table.is_some()));

        let app = Box::into_raw(Box::new(App {
            hwnd,
            gfx: Gfx::new(hwnd, dpi)?,
            layout,
            monitor: Monitor::default(),
            latest: latest.clone(),
            core: core.clone(),
            sampler: None,
            settings,
            dark: system_uses_dark_mode(),
            hover: Hover::default(),
            title: APP_TITLE.to_string(),
            pending: Vec::new(),
            confirm: None,
            notice: None,
            ended: Arc::default(),
            wants_table: wants_table.clone(),
            scroll: 0,
            pointer: (0.0, 0.0),
            menu_open: false,
            menu_closed: None,
            specs: None,
            specs_reading: false,
            specs_read: Arc::default(),
            copied: None,
            pc_name: std::env::var("COMPUTERNAME").ok().filter(|n| !n.is_empty()),
            alerted: HashMap::new(),
            more_link: None,
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as isize);
        (*app).apply_theme();
        (*app).queue_pin();
        run_deferred(app, hwnd);

        // Recalculate the frame now that WM_NCCALCSIZE reaches `App`.
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        )?;

        let target = hwnd.0 as isize;
        (*app).sampler = Some(Sampler::start(
            latest,
            core.clone(),
            wants_table,
            move || {
                // SAFETY: posting to a window that has gone simply fails.
                let _ = PostMessageW(
                    Some(HWND(target as *mut _)),
                    WM_APP_SAMPLE,
                    WPARAM(0),
                    LPARAM(0),
                );
            },
        ));

        let maximized = (*app).settings.maximized;
        let _ = ShowWindow(hwnd, if maximized { SW_SHOWMAXIMIZED } else { SW_SHOW });
        // The helper reads AMD's library, so there is nothing for it to do on Intel.
        // At sign-in it waits for a click, so a login never opens on a UAC prompt.
        if (*app).settings.core_temp && cpu_vendor() != CpuVendor::Intel && !at_sign_in {
            core.start(hwnd.0 as isize);
        }

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        // The window is gone, so no more messages can reach `App`.
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        drop(Box::from_raw(app));
        let _ = CloseHandle(_instance);
    }
    Ok(())
}
