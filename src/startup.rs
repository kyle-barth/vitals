//! Starting Vitals when you sign in to Windows. Nothing is installed: it's one
//! value in your own Run key, which unticking removes again.
//!
//! Task Manager's Startup apps page can switch an entry off without removing
//! it, by marking it in a sibling key. Vitals reads that mark, so it never shows
//! itself as starting when Windows won't start it, and never quietly undoes the
//! choice. Only ticking the box in Vitals again turns it back on.

use std::ffi::c_void;

use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_BINARY, REG_ROUTINE_FLAGS, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{PCWSTR, w};

/// The command-line switch Windows starts Vitals with at sign-in.
pub const ARG: &str = "--startup";

const RUN: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const APPROVED: PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");
const NAME: PCWSTR = w!("Vitals");
/// What Task Manager writes to mark an entry as enabled.
const ENABLED_MARK: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// The command the Run key holds for this exe.
pub fn command(exe: &str) -> String {
    format!("\"{exe}\" {ARG}")
}

/// Task Manager marks a disabled entry with an odd first byte.
fn marked_disabled(mark: &[u8]) -> bool {
    mark.first().is_some_and(|b| b % 2 == 1)
}

fn current_exe() -> Option<String> {
    Some(std::env::current_exe().ok()?.to_str()?.to_string())
}

fn read_bytes(key: PCWSTR, kind: REG_ROUTINE_FLAGS) -> Option<Vec<u8>> {
    let mut size = 0u32;
    // SAFETY: the first call only asks for the size, and the second is given a
    // buffer of exactly that size.
    unsafe {
        if RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            NAME,
            kind,
            None,
            None,
            Some(&mut size),
        )
        .is_err()
        {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        let pointer = Some(data.as_mut_ptr().cast::<c_void>());
        if RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            NAME,
            kind,
            None,
            pointer,
            Some(&mut size),
        )
        .is_err()
        {
            return None;
        }
        data.truncate(size as usize);
        Some(data)
    }
}

fn run_command() -> Option<String> {
    let bytes = read_bytes(RUN, RRF_RT_REG_SZ)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

fn switched_off_in_task_manager() -> bool {
    read_bytes(APPROVED, RRF_RT_REG_BINARY).is_some_and(|mark| marked_disabled(&mark))
}

/// Whether Windows will start Vitals at sign-in.
pub fn is_enabled() -> bool {
    run_command().is_some() && !switched_off_in_task_manager()
}

fn write_command(command: &str) -> bool {
    let wide: Vec<u16> = command.encode_utf16().chain([0]).collect();
    // SAFETY: the data is valid for its length, in bytes, for the call.
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN,
            NAME,
            REG_SZ.0,
            Some(wide.as_ptr().cast()),
            (wide.len() * 2) as u32,
        )
        .is_ok()
    }
}

/// Turns starting at sign-in on or off. Returns whether it worked.
pub fn set(enabled: bool) -> bool {
    if !enabled {
        // SAFETY: plain calls on the current user's own keys. A value that
        // isn't there is already the wanted result.
        unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, APPROVED, NAME);
            return RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, NAME).is_ok() || !is_enabled();
        }
    }
    let Some(exe) = current_exe() else {
        return false;
    };
    if !write_command(&command(&exe)) {
        return false;
    }
    if switched_off_in_task_manager() {
        // SAFETY: the mark is valid for its length for the call.
        unsafe {
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                APPROVED,
                NAME,
                REG_BINARY.0,
                Some(ENABLED_MARK.as_ptr().cast()),
                ENABLED_MARK.len() as u32,
            );
        }
    }
    is_enabled()
}

/// Points the Run key at this exe again if it has moved since it was set, so
/// sign-in doesn't try to start a file that's gone. A switched-off entry is
/// left alone.
pub fn follow_moved_exe() {
    let (Some(saved), Some(exe)) = (run_command(), current_exe()) else {
        return;
    };
    let wanted = command(&exe);
    if !saved.eq_ignore_ascii_case(&wanted) && !switched_off_in_task_manager() {
        write_command(&wanted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_quotes_paths_with_spaces() {
        assert_eq!(
            command(r"C:\Users\Kyle\My Apps\vitals.exe"),
            r#""C:\Users\Kyle\My Apps\vitals.exe" --startup"#
        );
    }

    #[test]
    fn an_odd_first_byte_means_switched_off() {
        assert!(marked_disabled(&[3, 0, 0, 0]));
        assert!(marked_disabled(&[7, 0]));
        assert!(!marked_disabled(&ENABLED_MARK));
        assert!(!marked_disabled(&[6, 0]));
        assert!(!marked_disabled(&[]));
    }
}
