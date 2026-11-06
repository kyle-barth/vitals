//! Vitals: a small, fully resizable sensor monitor for Windows.

// Release builds have no console window. Debug builds keep one for diagnostics.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gfx;
mod helper;
mod history;
mod layout;
mod metrics;
mod sensors;
mod settings;
mod severity;
mod ui;
mod window;

use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::{HSTRING, w};

fn main() {
    let mut arguments = std::env::args().skip(1);
    if arguments.next().as_deref() == Some(helper::ARG) {
        let pipe = arguments.next().unwrap_or_default();
        std::process::exit(helper::run(&pipe));
    }
    if let Err(error) = window::run() {
        let text = HSTRING::from(format!("Vitals could not start.\n\n{error}"));
        // SAFETY: both strings are valid for the call.
        unsafe { MessageBoxW(None, &text, w!("Vitals"), MB_OK | MB_ICONERROR) };
    }
}
