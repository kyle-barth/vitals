//! Puts text on the clipboard, and reads it back for pasting.

use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};

/// The standard clipboard format for UTF-16 text.
const CF_UNICODETEXT: u32 = 13;
/// Another app can hold the clipboard open for a moment, so opening it is
/// tried a few times before giving up.
const ATTEMPTS: u32 = 5;
const RETRY_DELAY: Duration = Duration::from_millis(15);

/// Opens the clipboard, trying a few times. Returns whether it opened.
///
/// # Safety
/// Every successful open must be paired with `CloseClipboard`.
unsafe fn open(owner: HWND) -> bool {
    for attempt in 0..ATTEMPTS {
        // SAFETY: the caller closes what this opens.
        if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
            return true;
        }
        if attempt + 1 < ATTEMPTS {
            thread::sleep(RETRY_DELAY);
        }
    }
    false
}

/// The text on the clipboard, if there is any.
///
/// Reading can ask another app to produce its text, so this too must not run
/// while the window's state is borrowed.
pub fn get_text(owner: HWND) -> Option<String> {
    // SAFETY: the clipboard is opened and closed in pairs. The data belongs to
    // the clipboard and is only read, while locked, within its reported size.
    unsafe {
        IsClipboardFormatAvailable(CF_UNICODETEXT).ok()?;
        if !open(owner) {
            return None;
        }
        let text = (|| -> Option<String> {
            let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
            let memory = HGLOBAL(handle.0);
            let source = GlobalLock(memory).cast::<u16>();
            if source.is_null() {
                return None;
            }
            let units = GlobalSize(memory) / size_of::<u16>();
            let wide = std::slice::from_raw_parts(source, units);
            let end = wide.iter().position(|&u| u == 0).unwrap_or(units);
            let text = String::from_utf16_lossy(&wide[..end]);
            let _ = GlobalUnlock(memory);
            Some(text)
        })();
        let _ = CloseClipboard();
        text
    }
}

/// Replaces the clipboard's contents with `text`. Returns whether it worked.
///
/// Emptying the clipboard sends a message to its previous owner, which can be
/// `owner` itself, so this must not run while the window's state is borrowed.
pub fn set_text(owner: HWND, text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: the clipboard is opened and closed in pairs. The memory is
    // allocated movable as the clipboard requires, filled while locked, and
    // owned by the clipboard once SetClipboardData succeeds. Otherwise it is
    // freed here.
    unsafe {
        if !open(owner) {
            return false;
        }
        let placed = (|| -> Option<()> {
            EmptyClipboard().ok()?;
            let memory = GlobalAlloc(GMEM_MOVEABLE, wide.len() * size_of::<u16>()).ok()?;
            let target = GlobalLock(memory).cast::<u16>();
            if target.is_null() {
                let _ = GlobalFree(Some(memory));
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len());
            let _ = GlobalUnlock(memory);
            if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(memory.0))).is_err() {
                let _ = GlobalFree(Some(memory));
                return None;
            }
            Some(())
        })()
        .is_some();
        let _ = CloseClipboard();
        placed
    }
}
