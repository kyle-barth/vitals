//! Puts text on the clipboard.

use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

/// The standard clipboard format for UTF-16 text.
const CF_UNICODETEXT: u32 = 13;
/// Another app can hold the clipboard open for a moment, so opening it is
/// tried a few times before giving up.
const ATTEMPTS: u32 = 5;
const RETRY_DELAY: Duration = Duration::from_millis(15);

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
        let mut opened = false;
        for attempt in 0..ATTEMPTS {
            if OpenClipboard(Some(owner)).is_ok() {
                opened = true;
                break;
            }
            if attempt + 1 < ATTEMPTS {
                thread::sleep(RETRY_DELAY);
            }
        }
        if !opened {
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
