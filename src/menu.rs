//! The small menu beside the pin: window sizes, and how much to show.
//!
//! It is a window of its own because the main window can be far smaller than the
//! menu, and has no title bar at all as a strip. It only exists while it is open,
//! and goes away like any menu when it loses focus or Esc is pressed. Choices are
//! posted back to the main window, which does the work.

use std::mem::size_of;

use windows::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, GetLastError};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUNDSMALL, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromWindow, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, Result, w};

use crate::gfx::Gfx;
use crate::layout::{Preset, Rect};
use crate::ui::{self, DARK, LIGHT};

const CLASS_NAME: PCWSTR = w!("Vitals.Menu");
/// Sent to the main window with an [`Action`]'s code in `wparam`.
pub const WM_APP_MENU: u32 = WM_APP + 3;
const WM_MOUSELEAVE: u32 = 0x02A3;
const BASE_DPI: f32 = 96.0;

const WIDTH: f32 = 216.0;
const INSET: f32 = 6.0;
const HEADING_H: f32 = 18.0;
const ROW_H: f32 = 30.0;
const RULE_H: f32 = 13.0;
const SEGMENT_H: f32 = 34.0;

/// What choosing a menu item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Size(Preset),
    YourSize,
    OpenApps,
    Simple,
    Advanced,
    StartWithWindows,
    SoundOnCritical,
    /// The menu has gone.
    Closed,
}

impl Action {
    const ALL: [Action; 10] = [
        Action::Size(Preset::Small),
        Action::Size(Preset::Medium),
        Action::Size(Preset::Large),
        Action::YourSize,
        Action::OpenApps,
        Action::Simple,
        Action::Advanced,
        Action::Closed,
        Action::StartWithWindows,
        Action::SoundOnCritical,
    ];

    /// Ticks that switch something on or off, rather than choosing between options.
    pub fn is_tick(self) -> bool {
        matches!(self, Action::StartWithWindows | Action::SoundOnCritical)
    }

    fn code(self) -> usize {
        Self::ALL.iter().position(|a| *a == self).unwrap_or(0)
    }

    pub fn from_code(code: usize) -> Option<Self> {
        Self::ALL.get(code).copied()
    }
}

/// What the menu needs to know about the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuState {
    /// The preset the window is at, if any.
    pub preset: Option<Preset>,
    /// The size to offer as "Your size", which is ticked if `is_yours`.
    pub yours: Option<(i32, i32)>,
    pub is_yours: bool,
    pub simple: bool,
    /// A strip has no tabs, so the menu offers the way to Apps.
    pub strip: bool,
    pub start_with_windows: bool,
    pub sound_on_critical: bool,
}

/// One row of the menu, or one half of the Simple and Advanced switch.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub action: Action,
    pub rect: Rect,
    pub label: &'static str,
    pub detail: String,
    /// The shape of the little window drawn beside a size, if it is one.
    pub glyph: Option<(f32, f32)>,
    pub ticked: bool,
}

/// Where everything in the menu goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub width: f32,
    pub height: f32,
    pub size_heading_y: f32,
    pub show_heading_y: f32,
    pub rule_y: f32,
    pub switch: Rect,
    /// The line above the ticks.
    pub ticks_rule_y: f32,
    pub items: Vec<Item>,
}

impl Plan {
    pub fn new(state: &MenuState) -> Self {
        let inner = WIDTH - INSET * 2.0;
        let mut items = Vec::new();
        let size_heading_y = INSET;
        let mut y = size_heading_y + HEADING_H;
        let mut row = |action, label, detail, glyph, ticked, items: &mut Vec<Item>| {
            items.push(Item {
                action,
                rect: Rect::new(INSET, y, inner, ROW_H),
                label,
                detail,
                glyph,
                ticked,
            });
            y += ROW_H;
        };
        for (preset, glyph) in
            Preset::ALL
                .into_iter()
                .zip([(20.0, 6.0), (11.0, 15.0), (22.0, 14.0)])
        {
            let (width, height) = preset.size();
            row(
                Action::Size(preset),
                preset.label(),
                format!("{width} × {height}"),
                Some(glyph),
                state.preset == Some(preset),
                &mut items,
            );
        }
        if let Some((width, height)) = state.yours {
            row(
                Action::YourSize,
                "Your size",
                format!("{width} × {height}"),
                Some((9.0, 16.0)),
                state.is_yours,
                &mut items,
            );
        }
        if state.strip {
            row(
                Action::OpenApps,
                "Open Apps",
                String::new(),
                None,
                false,
                &mut items,
            );
        }
        let rule_y = y + RULE_H / 2.0;
        let show_heading_y = y + RULE_H;
        let switch = Rect::new(INSET, show_heading_y + HEADING_H, inner, SEGMENT_H);
        let half = (switch.w - 6.0) / 2.0;
        let segment = |x: f32| Rect::new(x, switch.y + 3.0, half, switch.h - 6.0);
        for (action, label, x, ticked) in [
            (Action::Simple, "Simple", switch.x + 2.0, state.simple),
            (
                Action::Advanced,
                "Advanced",
                switch.x + 4.0 + half,
                !state.simple,
            ),
        ] {
            items.push(Item {
                action,
                rect: segment(x),
                label,
                detail: String::new(),
                glyph: None,
                ticked,
            });
        }
        let ticks_rule_y = switch.bottom() + RULE_H / 2.0;
        let mut y = switch.bottom() + RULE_H;
        for (action, label, ticked) in [
            (
                Action::StartWithWindows,
                "Start with Windows",
                state.start_with_windows,
            ),
            (
                Action::SoundOnCritical,
                "Sound on Critical",
                state.sound_on_critical,
            ),
        ] {
            items.push(Item {
                action,
                rect: Rect::new(INSET, y, inner, ROW_H),
                label,
                detail: String::new(),
                glyph: None,
                ticked,
            });
            y += ROW_H;
        }
        Self {
            width: WIDTH,
            height: y + INSET,
            size_heading_y,
            show_heading_y,
            rule_y,
            switch,
            ticks_rule_y,
            items,
        }
    }

    fn item_at(&self, x: f32, y: f32) -> Option<usize> {
        self.items.iter().position(|i| i.rect.contains(x, y))
    }
}

struct Menu {
    owner: HWND,
    gfx: Gfx,
    plan: Plan,
    state: MenuState,
    dark: bool,
    hover: Option<usize>,
    closing: bool,
}

fn scale_of(hwnd: HWND) -> f32 {
    // SAFETY: a stale handle simply returns zero, which is handled below.
    let dpi = unsafe { GetDpiForWindow(hwnd) } as f32;
    if dpi > 0.0 { dpi / BASE_DPI } else { 1.0 }
}

fn low_word(value: isize) -> i16 {
    (value & 0xffff) as i16
}

fn high_word(value: isize) -> i16 {
    ((value >> 16) & 0xffff) as i16
}

pub fn tell_owner(owner: HWND, action: Action) {
    // SAFETY: posting to a window that has gone simply fails.
    let _ = unsafe { PostMessageW(Some(owner), WM_APP_MENU, WPARAM(action.code()), LPARAM(0)) };
}

impl Menu {
    fn paint(&mut self) {
        let theme = if self.dark { &DARK } else { &LIGHT };
        let (plan, hover) = (&self.plan, self.hover);
        let _ = self
            .gfx
            .draw(|canvas| ui::paint_menu(canvas, theme, plan, hover));
    }

    fn point(&self, hwnd: HWND, lparam: LPARAM) -> (f32, f32) {
        let scale = scale_of(hwnd);
        (
            low_word(lparam.0) as f32 / scale,
            high_word(lparam.0) as f32 / scale,
        )
    }

    fn set_hover(&mut self, hwnd: HWND, hover: Option<usize>) {
        if hover != self.hover {
            self.hover = hover;
            // SAFETY: the handle is this window's own.
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
    }

    /// Acts on a click. Returns true if the menu is finished with.
    fn click(&mut self, hwnd: HWND, x: f32, y: f32) -> bool {
        let Some(index) = self.plan.item_at(x, y) else {
            return false;
        };
        let action = self.plan.items[index].action;
        tell_owner(self.owner, action);
        match action {
            // Staying open means the difference can be seen before moving on.
            Action::Simple
            | Action::Advanced
            | Action::StartWithWindows
            | Action::SoundOnCritical => {
                match action {
                    Action::Simple | Action::Advanced => {
                        self.state.simple = action == Action::Simple;
                    }
                    Action::StartWithWindows => {
                        self.state.start_with_windows = !self.state.start_with_windows;
                    }
                    _ => self.state.sound_on_critical = !self.state.sound_on_critical,
                }
                self.plan = Plan::new(&self.state);
                // SAFETY: the handle is this window's own.
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                false
            }
            _ => true,
        }
    }
}

/// Closes the menu, once. Destroying the active window deactivates it, which
/// would otherwise ask for the same thing again part way through.
///
/// # Safety
/// `menu` must be valid, and no reference to it may be live.
unsafe fn close(menu: *mut Menu, hwnd: HWND) {
    // SAFETY: the caller guarantees this, and the borrow ends with the block.
    let already = unsafe {
        let already = (*menu).closing;
        (*menu).closing = true;
        already
    };
    if !already {
        // SAFETY: the handle is this window's own.
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

unsafe extern "system" fn menu_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: reads a value this window stores for itself.
    let menu = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Menu;
    if menu.is_null() {
        // SAFETY: forwards this window's own message unchanged.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    // SAFETY: the pointer is set once after creation and cleared in
    // WM_NCDESTROY, everything runs on the UI thread, and each borrow below ends
    // before anything that could re-enter this function.
    unsafe {
        match message {
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut paint);
                (*menu).paint();
                let _ = EndPaint(hwnd, &paint);
                return LRESULT(0);
            }
            WM_ERASEBKGND => return LRESULT(1),
            WM_MOUSEMOVE => {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
                let (x, y) = (*menu).point(hwnd, lparam);
                let hover = (*menu).plan.item_at(x, y);
                (*menu).set_hover(hwnd, hover);
                return LRESULT(0);
            }
            WM_MOUSELEAVE => {
                (*menu).set_hover(hwnd, None);
                return LRESULT(0);
            }
            WM_LBUTTONUP => {
                let (x, y) = (*menu).point(hwnd, lparam);
                if (*menu).click(hwnd, x, y) {
                    close(menu, hwnd);
                }
                return LRESULT(0);
            }
            WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
                close(menu, hwnd);
                return LRESULT(0);
            }
            WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => close(menu, hwnd),
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let menu = Box::from_raw(menu);
                tell_owner(menu.owner, Action::Closed);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

/// Opens the menu with its top right corner under `anchor`, a point on the screen
/// in pixels. `above` is the y to sit above instead if it doesn't fit below.
pub fn open(
    owner: HWND,
    anchor: (i32, i32),
    above: i32,
    state: MenuState,
    dark: bool,
    topmost: bool,
) -> Result<()> {
    // SAFETY: the class and creation parameters are valid, the out pointers are
    // valid for each call, and the `Menu` is freed in WM_NCDESTROY.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(menu_proc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_thread());
        }

        let plan = Plan::new(&state);
        let scale = scale_of(owner);
        let (width, height) = (
            (plan.width * scale).ceil() as i32,
            (plan.height * scale).ceil() as i32,
        );
        let monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT {
                left: i32::MIN / 2,
                top: i32::MIN / 2,
                right: i32::MAX / 2,
                bottom: i32::MAX / 2,
            }
        };
        let left = (anchor.0 - width).clamp(work.left, (work.right - width).max(work.left));
        let top = if anchor.1 + height <= work.bottom {
            anchor.1
        } else {
            (above - height).max(work.top)
        };

        let mut style = WS_EX_TOOLWINDOW;
        if topmost {
            style |= WS_EX_TOPMOST;
        }
        let hwnd = CreateWindowExW(
            style,
            CLASS_NAME,
            w!("Vitals menu"),
            WS_POPUP,
            left,
            top,
            width,
            height,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )?;
        let corners = DWMWCP_ROUNDSMALL;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corners).cast(),
            size_of::<i32>() as u32,
        );
        let gfx = match Gfx::new(hwnd, GetDpiForWindow(hwnd) as f32) {
            Ok(gfx) => gfx,
            Err(error) => {
                let _ = DestroyWindow(hwnd);
                return Err(error);
            }
        };
        let menu = Box::into_raw(Box::new(Menu {
            owner,
            gfx,
            plan,
            state,
            dark,
            hover: None,
            closing: false,
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, menu as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> MenuState {
        MenuState {
            preset: Some(Preset::Medium),
            yours: None,
            is_yours: false,
            simple: false,
            strip: false,
            start_with_windows: false,
            sound_on_critical: false,
        }
    }

    fn actions(plan: &Plan) -> Vec<Action> {
        plan.items.iter().map(|i| i.action).collect()
    }

    #[test]
    fn every_action_survives_the_trip_through_a_message() {
        for action in Action::ALL {
            assert_eq!(Action::from_code(action.code()), Some(action));
        }
        assert_eq!(Action::from_code(99), None);
    }

    #[test]
    fn it_offers_the_three_sizes_and_the_switch() {
        assert_eq!(
            actions(&Plan::new(&state())),
            [
                Action::Size(Preset::Small),
                Action::Size(Preset::Medium),
                Action::Size(Preset::Large),
                Action::Simple,
                Action::Advanced,
                Action::StartWithWindows,
                Action::SoundOnCritical,
            ]
        );
    }

    #[test]
    fn each_tick_shows_its_own_setting() {
        let plan = Plan::new(&MenuState {
            start_with_windows: true,
            sound_on_critical: false,
            ..state()
        });
        let ticked = |action| plan.items.iter().any(|i| i.action == action && i.ticked);
        assert!(ticked(Action::StartWithWindows));
        assert!(!ticked(Action::SoundOnCritical));
    }

    #[test]
    fn your_size_only_appears_once_there_is_one() {
        let with_yours = MenuState {
            preset: None,
            yours: Some((254, 661)),
            is_yours: true,
            ..state()
        };
        let plan = Plan::new(&with_yours);
        let item = plan
            .items
            .iter()
            .find(|i| i.action == Action::YourSize)
            .unwrap();
        assert_eq!(item.detail, "254 × 661");
        assert!(item.ticked);
    }

    #[test]
    fn only_a_strip_offers_the_way_to_apps() {
        let strip = MenuState {
            strip: true,
            ..state()
        };
        assert!(actions(&Plan::new(&strip)).contains(&Action::OpenApps));
        assert!(!actions(&Plan::new(&state())).contains(&Action::OpenApps));
    }

    #[test]
    fn exactly_one_of_simple_and_advanced_is_ticked() {
        for simple in [false, true] {
            let plan = Plan::new(&MenuState { simple, ..state() });
            let ticked = |action| plan.items.iter().any(|i| i.action == action && i.ticked);
            assert_eq!(ticked(Action::Simple), simple);
            assert_eq!(ticked(Action::Advanced), !simple);
        }
    }

    #[test]
    fn nothing_overlaps_or_leaves_the_menu() {
        let full = MenuState {
            yours: Some((254, 661)),
            strip: true,
            ..state()
        };
        let plan = Plan::new(&full);
        for (i, a) in plan.items.iter().enumerate() {
            assert!(a.rect.x >= 0.0 && a.rect.right() <= plan.width, "{a:?}");
            assert!(a.rect.y >= 0.0 && a.rect.bottom() <= plan.height, "{a:?}");
            for b in &plan.items[i + 1..] {
                let apart = a.rect.right() <= b.rect.x
                    || b.rect.right() <= a.rect.x
                    || a.rect.bottom() <= b.rect.y
                    || b.rect.bottom() <= a.rect.y;
                assert!(apart, "{a:?} and {b:?}");
            }
        }
    }
}
