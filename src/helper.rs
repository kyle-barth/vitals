//! The elevated sensor helper, and the UI's link to it.
//!
//! Core temperature requires administrator rights, but a sensor window should
//! not run with them. So the UI stays unelevated and starts this same exe with
//! [`ARG`] through a UAC prompt, only when the user asks. The split also means
//! that a fault in AMD's library can only end the helper, never the UI.
//!
//! The helper's job is deliberately narrow: it sends one fixed-size record per
//! second down a one-way pipe, and reads nothing from it, so the UI has no way
//! to ask the elevated process to do anything else. It exits when the pipe
//! closes, so it cannot outlive the UI.

use std::mem::size_of;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_CANCELLED, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GENERIC_WRITE,
    GetLastError, HANDLE, HWND, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE,
    FILE_TYPE_PIPE, GetFileType, OPEN_EXISTING, PIPE_ACCESS_INBOUND, ReadFile, SECURITY_ANONYMOUS,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcessId, GetExitCodeProcess, GetProcessId, INFINITE,
    WaitForMultipleObjects, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{HSTRING, PCWSTR, w};

use crate::metrics::CoreStatus;
use crate::sensors::ryzen::{CoreReading, RyzenMaster};

/// The command-line switch that runs this exe as the helper.
pub const ARG: &str = "--cpu-helper";

const PIPE_PREFIX: &str = r"\\.\pipe\vitals-";
const RECORD_LEN: usize = size_of::<f32>() * 3;
const EXIT_OK: i32 = 0;
const EXIT_BAD_ARGUMENT: i32 = 2;
const EXIT_UNSUPPORTED: i32 = 3;
const CONNECT_TIMEOUT_MS: u32 = 20_000;
/// A reading older than this is treated as lost.
const STALE_AFTER: Duration = Duration::from_secs(5);
/// How long AMD's library may fail before the helper gives up.
const MAX_FAILURES: u32 = 10;

/// This value comes from an elevated process's command line, so it is checked
/// strictly. A prefix check alone would pass `\\.\pipe\vitals-\..\..\C:\file`,
/// which Windows resolves to an ordinary file.
fn is_valid_pipe_name(name: &str) -> bool {
    name.strip_prefix(PIPE_PREFIX)
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

/// Runs the elevated helper until the UI closes the pipe. Returns the exit code.
pub fn run(pipe_name: &str) -> i32 {
    if !is_valid_pipe_name(pipe_name) {
        return EXIT_BAD_ARGUMENT;
    }
    let Some(ryzen) = RyzenMaster::load() else {
        return EXIT_UNSUPPORTED;
    };

    // SAFETY: the name is a valid string, and the handle is checked below.
    let pipe = unsafe {
        CreateFileW(
            &HSTRING::from(pipe_name),
            GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            // The pipe belongs to an unelevated process, which must never be
            // able to impersonate this elevated one.
            SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS,
            None,
        )
    };
    let Ok(pipe) = pipe else { return EXIT_OK };
    // SAFETY: the handle was opened just above.
    if unsafe { GetFileType(pipe) } != FILE_TYPE_PIPE {
        return EXIT_BAD_ARGUMENT;
    }

    let mut failures = 0;
    loop {
        let Some(reading) = ryzen.read() else {
            failures += 1;
            if failures >= MAX_FAILURES {
                return EXIT_UNSUPPORTED;
            }
            thread::sleep(Duration::from_secs(1));
            continue;
        };
        failures = 0;
        let record = encode(reading);
        // SAFETY: the record outlives the call. A failed write means the UI
        // has gone away, which is the signal to exit.
        if unsafe { WriteFile(pipe, Some(&record), None, None) }.is_err() {
            return EXIT_OK;
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn encode(r: CoreReading) -> [u8; RECORD_LEN] {
    let mut bytes = [0u8; RECORD_LEN];
    bytes[..4].copy_from_slice(&r.temp_c.to_le_bytes());
    bytes[4..8].copy_from_slice(&r.package_w.to_le_bytes());
    bytes[8..].copy_from_slice(&r.limit_w.to_le_bytes());
    bytes
}

fn decode(bytes: &[u8; RECORD_LEN]) -> CoreReading {
    CoreReading {
        temp_c: f32::from_le_bytes(bytes[..4].try_into().unwrap()),
        package_w: f32::from_le_bytes(bytes[4..8].try_into().unwrap()),
        limit_w: f32::from_le_bytes(bytes[8..].try_into().unwrap()),
    }
}

#[derive(Default)]
struct Shared {
    status: CoreStatus,
    reading: Option<(Instant, CoreReading)>,
}

/// The UI's side of the connection. Cheap to clone and safe to share.
#[derive(Clone, Default)]
pub struct CoreLink {
    shared: Arc<Mutex<Shared>>,
}

impl CoreLink {
    /// Asks for elevation and starts the helper, unless it is already running.
    /// The UAC prompt blocks, so this all happens on its own thread.
    pub fn start(&self, owner: isize) {
        {
            let Ok(mut shared) = self.shared.lock() else {
                return;
            };
            if matches!(shared.status, CoreStatus::Requesting | CoreStatus::Running) {
                return;
            }
            shared.status = CoreStatus::Requesting;
        }
        let link = self.clone();
        thread::spawn(move || {
            let status = link.run_session(owner);
            link.set_status(status);
        });
    }

    /// The current status and, if fresh, the latest reading.
    pub fn latest(&self) -> (CoreStatus, Option<CoreReading>) {
        let Ok(shared) = self.shared.lock() else {
            return (CoreStatus::Off, None);
        };
        let fresh = shared.reading.filter(|(at, _)| at.elapsed() < STALE_AFTER);
        (shared.status, fresh.map(|(_, reading)| reading))
    }

    fn set_status(&self, status: CoreStatus) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.status = status;
        }
    }

    fn publish(&self, reading: CoreReading) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.status = CoreStatus::Running;
            shared.reading = Some((Instant::now(), reading));
        }
    }

    /// Runs one helper from launch to exit, returning the final status.
    fn run_session(&self, owner: isize) -> CoreStatus {
        // SAFETY: all handles are created here and closed before returning,
        // and the event and OVERLAPPED outlive every operation that uses them
        // because each one is waited on before they are dropped.
        unsafe {
            let name = HSTRING::from(format!("{PIPE_PREFIX}{}", GetCurrentProcessId()));
            let pipe = CreateNamedPipeW(
                &name,
                PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                0,
                RECORD_LEN as u32 * 8,
                0,
                None,
            );
            if pipe.is_invalid() {
                return CoreStatus::Unavailable;
            }
            let Ok(event) = CreateEventW(None, true, false, None) else {
                let _ = CloseHandle(pipe);
                return CoreStatus::Unavailable;
            };
            let status = match launch(owner, &name) {
                Ok(process) => {
                    let status = self.serve(pipe, event, process);
                    let _ = CloseHandle(process);
                    status
                }
                Err(status) => status,
            };
            let _ = CloseHandle(event);
            let _ = CloseHandle(pipe);
            status
        }
    }

    /// Waits for the helper to connect, then reads records until it exits.
    unsafe fn serve(&self, pipe: HANDLE, event: HANDLE, process: HANDLE) -> CoreStatus {
        let mut overlapped = OVERLAPPED {
            hEvent: event,
            ..Default::default()
        };
        // SAFETY: see `run_session`.
        unsafe {
            let connect = ConnectNamedPipe(pipe, Some(&mut overlapped));
            let error = GetLastError();
            if connect.is_err() && error == ERROR_IO_PENDING {
                let wait = WaitForMultipleObjects(&[event, process], false, CONNECT_TIMEOUT_MS);
                if wait != WAIT_OBJECT_0 {
                    let _ = CancelIoEx(pipe, Some(&overlapped));
                    // Windows keeps using `overlapped` until the cancel completes.
                    let mut ignored = 0u32;
                    let _ = GetOverlappedResult(pipe, &overlapped, &mut ignored, true);
                    return exit_status(process);
                }
            } else if connect.is_err() && error != ERROR_PIPE_CONNECTED {
                return CoreStatus::Unavailable;
            }

            // Only accept the process this link started, so another program
            // cannot connect first and feed the UI false temperatures.
            let mut client = 0u32;
            let connected = GetNamedPipeClientProcessId(pipe, &mut client).is_ok();
            if !connected || client == 0 || client != GetProcessId(process) {
                return CoreStatus::Unavailable;
            }

            let mut bytes = [0u8; RECORD_LEN];
            loop {
                let mut read = 0u32;
                let mut overlapped = OVERLAPPED {
                    hEvent: event,
                    ..Default::default()
                };
                let started = ReadFile(pipe, Some(&mut bytes), None, Some(&mut overlapped));
                if started.is_err() && GetLastError() != ERROR_IO_PENDING {
                    break;
                }
                if GetOverlappedResult(pipe, &overlapped, &mut read, true).is_err() {
                    break;
                }
                if read as usize == RECORD_LEN {
                    self.publish(decode(&bytes));
                }
            }
            WaitForSingleObject(process, INFINITE);
            exit_status(process)
        }
    }
}

/// Starts this exe elevated. The handle returned must be closed by the caller.
fn launch(owner: isize, pipe: &HSTRING) -> Result<HANDLE, CoreStatus> {
    let exe = std::env::current_exe().map_err(|_| CoreStatus::Unavailable)?;
    let file = HSTRING::from(exe.as_os_str());
    let arguments = HSTRING::from(format!("{ARG} {pipe}"));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: HWND(owner as *mut _),
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(arguments.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: `info` and the strings it points to outlive the call.
    match unsafe { ShellExecuteExW(&mut info) } {
        Ok(()) => Ok(info.hProcess),
        Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => Err(CoreStatus::Declined),
        Err(_) => Err(CoreStatus::Unavailable),
    }
}

/// Maps the helper's exit code to what the UI should show.
fn exit_status(process: HANDLE) -> CoreStatus {
    let mut code = 0u32;
    // SAFETY: the process handle is valid, and `code` is valid for the call.
    let known = unsafe { GetExitCodeProcess(process, &mut code) }.is_ok();
    if known && code as i32 == EXIT_UNSUPPORTED {
        CoreStatus::Unavailable
    } else {
        CoreStatus::Off
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let reading = CoreReading {
            temp_c: 88.5,
            package_w: 101.25,
            limit_w: 162.0,
        };
        assert_eq!(decode(&encode(reading)), reading);
    }

    #[test]
    fn helper_refuses_a_pipe_outside_its_namespace() {
        assert_eq!(run(r"\\.\pipe\something-else"), EXIT_BAD_ARGUMENT);
        assert_eq!(run(r"C:\file.txt"), EXIT_BAD_ARGUMENT);
        assert_eq!(run(""), EXIT_BAD_ARGUMENT);
    }

    #[test]
    fn helper_refuses_a_path_that_escapes_the_pipe_namespace() {
        assert_eq!(
            run(r"\\.\pipe\vitals-\..\..\C:\Windows\win.ini"),
            EXIT_BAD_ARGUMENT
        );
        assert!(!is_valid_pipe_name(r"\\.\pipe\vitals-123\..\..\C:\file"));
    }

    #[test]
    fn pipe_names_must_end_in_a_process_id() {
        assert!(is_valid_pipe_name(r"\\.\pipe\vitals-20616"));
        assert!(!is_valid_pipe_name(r"\\.\pipe\vitals-"));
        assert!(!is_valid_pipe_name(r"\\.\pipe\vitals-12a"));
        assert!(!is_valid_pipe_name(r"\\.\pipe\vitals--1"));
    }
}
