//! Vitals: a small, fully resizable sensor monitor for Windows.

// Release builds have no console window. Debug builds keep one for diagnostics.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps;
mod clipboard;
mod gfx;
mod helper;
mod history;
mod insights;
mod layout;
mod menu;
mod metrics;
mod sensors;
mod settings;
mod severity;
mod specs;
mod startup;
mod ui;
mod window;

use windows::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories,
};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::{HSTRING, w};

/// Stops DLLs loaded at runtime from being found in the exe's own folder, which
/// is often Downloads. `build.rs` applies the same rule to the exe's imports.
fn restrict_dll_search() {
    // SAFETY: this takes no pointers, and only changes this process's search
    // order. AMD's libraries are loaded separately, by full path.
    unsafe {
        let _ = SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32);
    }
}

fn main() {
    restrict_dll_search();
    let mut arguments = std::env::args().skip(1);
    let first = arguments.next();
    if first.as_deref() == Some(helper::ARG) {
        let pipe = arguments.next().unwrap_or_default();
        std::process::exit(helper::run(&pipe));
    }
    let at_sign_in = first.as_deref() == Some(startup::ARG);
    if let Err(error) = window::run(at_sign_in) {
        let text = HSTRING::from(format!("Vitals could not start.\n\n{error}"));
        // SAFETY: both strings are valid for the call.
        unsafe { MessageBoxW(None, &text, w!("Vitals"), MB_OK | MB_ICONERROR) };
    }
}
