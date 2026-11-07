//! Finds out what every app is using, which apps are frozen, and ends an app
//! when the user asks.
//!
//! One `NtQuerySystemInformation` call returns CPU time and memory for every
//! process at once, without opening any of them. That is why this can run every
//! second on a PC with hundreds of processes. GPU use per app is far costlier,
//! so it is only read while the GPU is actually busy or the Apps page is open.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;
use std::sync::Arc;
use std::time::Instant;

use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SystemProcessInformation};
use windows::Win32::Foundation::{
    CloseHandle, E_ACCESSDENIED, FILETIME, HANDLE, HWND, LPARAM, STATUS_INFO_LENGTH_MISMATCH,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, EnumWindows, GW_OWNER, GetClassNameW, GetIconInfo, GetWindow,
    GetWindowThreadProcessId, HICON, ICONINFO, IsHungAppWindow, IsWindowVisible,
};
use windows::core::{BOOL, HSTRING, PWSTR};

use super::pdh::GpuApps;
use crate::apps::{AppRow, AppSample, BusyApp, Capacity, Icon, ProcessId, Tracker, Usage};

const BYTES_PER_GB: f32 = 1024.0 * 1024.0 * 1024.0;
const BYTES_PER_MB: f32 = 1024.0 * 1024.0;
/// CPU time is reported in units of 100 nanoseconds.
const TICKS_PER_SECOND: f32 = 10_000_000.0;
const INITIAL_BUFFER: usize = 1024 * 1024;

/// Processes that aren't apps, so they are never listed.
const NEVER_LISTED: [&str; 5] = [
    "idle",
    "system",
    "registry",
    "memory compression",
    "secure system",
];

/// Windows' own processes, in the user's session. They can be listed if busy,
/// but ending one could take down the desktop, so End is never offered.
const NEVER_ENDED: [&str; 17] = [
    "audiodg",
    "conhost",
    "csrss",
    "ctfmon",
    "dwm",
    "explorer",
    "fontdrvhost",
    "lsass",
    "runtimebroker",
    "searchhost",
    "services",
    "shellexperiencehost",
    "sihost",
    "smss",
    "startmenuexperiencehost",
    "svchost",
    "winlogon",
];

/// The fixed start of the `SYSTEM_PROCESS_INFORMATION` record, which the kernel
/// has kept stable since Windows Vista. The `windows` crate marks most of these
/// fields reserved, so they are named here.
#[repr(C)]
#[derive(Clone, Copy)]
struct Entry {
    next: u32,
    threads: u32,
    private_bytes: i64,
    hard_faults: u32,
    peak_threads: u32,
    cycles: u64,
    created: i64,
    user_time: i64,
    kernel_time: i64,
    name_bytes: u16,
    name_capacity: u16,
    name: *const u16,
    base_priority: i32,
    pid: usize,
    parent: usize,
    handles: u32,
    session: u32,
    process_key: usize,
    peak_virtual_size: usize,
    virtual_size: usize,
    page_faults: u32,
    peak_working_set: usize,
    working_set: usize,
    quota_peak_paged_pool: usize,
    quota_paged_pool: usize,
    quota_peak_non_paged_pool: usize,
    quota_non_paged_pool: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_pages: usize,
    read_operations: i64,
    write_operations: i64,
    other_operations: i64,
    bytes_read: i64,
    bytes_written: i64,
}

#[derive(Clone, Debug, PartialEq)]
struct Process {
    pid: u32,
    session: u32,
    created: i64,
    cpu_time: i64,
    /// Everything read and written since the process started.
    io_bytes: i64,
    private_bytes: i64,
    /// The exe's name, without the `.exe`.
    name: String,
}

/// Walks the kernel's records. Every offset is checked against the buffer, and
/// a name is only read if it lies inside the buffer, so a bad record ends the
/// walk rather than reading stray memory.
fn parse_processes(buffer: &[u8]) -> Vec<Process> {
    let range = buffer.as_ptr_range();
    let mut processes = Vec::new();
    let mut offset = 0usize;
    while let Some(end) = offset.checked_add(size_of::<Entry>()) {
        if end > buffer.len() {
            break;
        }
        // SAFETY: the bytes are in bounds, and `read_unaligned` needs no alignment.
        let entry = unsafe { ptr::read_unaligned(buffer.as_ptr().add(offset).cast::<Entry>()) };
        let start = entry.name as usize;
        let bytes = entry.name_bytes as usize;
        let inside = start >= range.start as usize
            && start
                .checked_add(bytes)
                .is_some_and(|e| e <= range.end as usize);
        let name = if inside && bytes > 0 {
            let at = start - range.start as usize;
            let units: Vec<u16> = buffer[at..at + bytes]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        } else {
            String::new()
        };
        processes.push(Process {
            pid: entry.pid as u32,
            session: entry.session,
            created: entry.created,
            cpu_time: entry.user_time + entry.kernel_time,
            io_bytes: entry.bytes_read + entry.bytes_written,
            private_bytes: entry.private_bytes,
            name: strip_exe(&name).to_string(),
        });
        if entry.next == 0 {
            break;
        }
        offset += entry.next as usize;
    }
    processes
}

fn strip_exe(name: &str) -> &str {
    match name.len().checked_sub(4) {
        Some(at) if name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".exe") => {
            &name[..at]
        }
        _ => name,
    }
}

/// The processes owning a visible, top-level window that has stopped
/// responding. This is the same check Windows and Task Manager rely on.
fn frozen_processes() -> HashSet<u32> {
    unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the set passed to EnumWindows below, which outlives
        // the enumeration, and the window handle comes from Windows.
        unsafe {
            if IsHungAppWindow(hwnd).as_bool() && is_app_window(hwnd) {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                (*(found.0 as *mut HashSet<u32>)).insert(pid);
            }
        }
        true.into()
    }
    let mut found = HashSet::new();
    // SAFETY: the callback only uses `found`, which lives until this returns.
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&raw mut found as isize));
    }
    found
}

fn is_app_window(hwnd: HWND) -> bool {
    let mut class = [0u16; 16];
    // SAFETY: the handle is one Windows just gave us, and a stale handle simply
    // makes these fail. The class buffer is valid for its length.
    unsafe {
        let length = GetClassNameW(hwnd, &mut class).max(0) as usize;
        // Windows covers a frozen window with a "Ghost" of its own. It belongs
        // to dwm, so it must not be mistaken for the app.
        let ghost = String::from_utf16_lossy(&class[..length]) == "Ghost";
        !ghost && IsWindowVisible(hwnd).as_bool() && GetWindow(hwnd, GW_OWNER).is_err()
    }
}

/// An app's display name and icon, read once from its exe.
#[derive(Clone, Debug, Default)]
struct Identity {
    name: String,
    icon: Option<Arc<Icon>>,
}

fn image_path(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed before returning, and the buffer and its
    // length are valid for the call.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path = [0u16; 1024];
        let mut length = path.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        Some(String::from_utf16_lossy(&path[..length as usize]))
    }
}

/// Reads a string from the exe's version resource, such as "FileDescription".
fn version_string(path: &str, field: &str) -> Option<String> {
    let path = HSTRING::from(path);
    // SAFETY: the buffer is sized as Windows asks, and the pointers VerQueryValueW
    // returns point into that buffer, which outlives every use of them.
    unsafe {
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()).ok()?;

        let (mut pointer, mut length) = (ptr::null_mut::<c_void>(), 0u32);
        let block = data.as_ptr().cast();
        if !VerQueryValueW(
            block,
            &HSTRING::from(r"\VarFileInfo\Translation"),
            &mut pointer,
            &mut length,
        )
        .as_bool()
            || (length as usize) < 4
        {
            return None;
        }
        let languages = pointer as *const u16;
        let (language, codepage) = (
            languages.read_unaligned(),
            languages.add(1).read_unaligned(),
        );
        let key = format!(r"\StringFileInfo\{language:04x}{codepage:04x}\{field}");
        if !VerQueryValueW(block, &HSTRING::from(key), &mut pointer, &mut length).as_bool()
            || length == 0
        {
            return None;
        }
        let text = std::slice::from_raw_parts(pointer as *const u16, length as usize);
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        let value = String::from_utf16_lossy(&text[..end]).trim().to_string();
        (!value.is_empty()).then_some(value)
    }
}

/// Copies an icon out into plain pixels, so nothing here holds a GDI object.
fn icon_pixels(path: &str) -> Option<Icon> {
    let mut large = HICON::default();
    // SAFETY: `large` receives one icon, which is destroyed below.
    let count = unsafe { ExtractIconExW(&HSTRING::from(path), 0, Some(&mut large), None, 1) };
    if count == 0 || large.is_invalid() {
        return None;
    }
    let mut info = ICONINFO::default();
    // SAFETY: the icon is valid. The two bitmaps GetIconInfo creates are deleted
    // below, and the icon is destroyed once its pixels have been read.
    unsafe {
        let got_info = GetIconInfo(large, &mut info).is_ok();
        let icon = if got_info {
            read_bitmap(info.hbmColor, info.hbmMask)
        } else {
            None
        };
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        }
        let _ = DestroyIcon(large);
        icon
    }
}

/// # Safety
/// Both bitmaps must be valid, or null where an icon has no such bitmap.
unsafe fn read_bitmap(color: HBITMAP, mask: HBITMAP) -> Option<Icon> {
    // SAFETY: the DC is deleted before returning, and each buffer holds exactly
    // the rows requested of it.
    unsafe {
        let mut bitmap = BITMAP::default();
        let bytes = GetObjectW(
            HGDIOBJ(color.0),
            size_of::<BITMAP>() as i32,
            Some((&raw mut bitmap).cast()),
        );
        if bytes == 0 || bitmap.bmWidth != bitmap.bmHeight || !(8..=256).contains(&bitmap.bmWidth) {
            return None;
        }
        let size = bitmap.bmWidth;
        let dc = CreateCompatibleDC(None);
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (size * size * 4) as usize];
        let rows = GetDIBits(
            dc,
            color,
            0,
            size as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        );

        // An icon with no alpha channel keeps its transparency in the mask.
        let mut coverage = None;
        if rows != 0 && pixels.chunks_exact(4).all(|p| p[3] == 0) {
            let mut mask_pixels = vec![0u8; pixels.len()];
            let read = GetDIBits(
                dc,
                mask,
                0,
                size as u32,
                Some(mask_pixels.as_mut_ptr().cast()),
                &mut header,
                DIB_RGB_COLORS,
            );
            if read != 0 {
                coverage = Some(mask_pixels);
            }
        }
        let _ = DeleteDC(dc);
        if rows == 0 {
            return None;
        }
        premultiply(&mut pixels, coverage.as_deref());
        Some(Icon {
            size: size as u32,
            pixels,
        })
    }
}

/// Direct2D expects each color scaled by its alpha.
fn premultiply(pixels: &mut [u8], mask: Option<&[u8]>) {
    for (i, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        let alpha = match mask {
            Some(mask) if mask[i * 4] == 0 => 255,
            Some(_) => 0,
            None => pixel[3],
        };
        pixel[3] = alpha;
        for channel in &mut pixel[..3] {
            *channel = (*channel as u32 * alpha as u32 / 255) as u8;
        }
    }
}

/// Reads an app's name and icon from its exe. System processes can't be opened
/// from an unelevated app, and they simply keep the name of their exe.
fn read_identity(pid: u32, fallback: &str) -> Identity {
    let Some(path) = image_path(pid) else {
        return Identity {
            name: fallback.to_string(),
            icon: None,
        };
    };
    let name = version_string(&path, "FileDescription")
        .or_else(|| version_string(&path, "ProductName"))
        .unwrap_or_else(|| fallback.to_string());
    Identity {
        name,
        icon: icon_pixels(&path).map(Arc::new),
    }
}

fn query_processes(buffer: &mut Vec<u8>) -> Option<usize> {
    for _ in 0..4 {
        let mut needed = 0u32;
        // SAFETY: the buffer is valid for the length given, and `needed` for writing.
        let status = unsafe {
            NtQuerySystemInformation(
                SystemProcessInformation,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut needed,
            )
        };
        if status.is_ok() {
            return Some(needed as usize);
        }
        if status != STATUS_INFO_LENGTH_MISMATCH {
            return None;
        }
        // The process list can grow between calls, so leave a margin.
        buffer.resize((needed as usize).max(buffer.len()) + 64 * 1024, 0);
    }
    None
}

#[derive(Default)]
struct Group {
    name: String,
    usage: Usage,
    can_end: bool,
    frozen: bool,
    processes: Vec<ProcessId>,
}

/// Measures every app, once per sample.
pub struct ProcessSource {
    buffer: Vec<u8>,
    /// Each process's CPU time and bytes moved, as of the last sample.
    previous: HashMap<ProcessKey, (i64, i64)>,
    last_sample: Option<Instant>,
    threads: f32,
    own_name: String,
    gpu: Option<GpuApps>,
    identities: HashMap<String, Identity>,
    tracker: Tracker,
}

type ProcessKey = (u32, i64);

impl ProcessSource {
    pub fn new() -> Self {
        let own_name = std::env::current_exe()
            .ok()
            .and_then(|path| path.file_stem().map(|n| n.to_string_lossy().to_lowercase()))
            .unwrap_or_default();
        Self {
            buffer: vec![0; INITIAL_BUFFER],
            previous: HashMap::new(),
            last_sample: None,
            threads: std::thread::available_parallelism().map_or(1.0, |n| n.get() as f32),
            own_name,
            gpu: None,
            identities: HashMap::new(),
            tracker: Tracker::default(),
        }
    }

    /// Returns the busy apps to show right now, and every app if `wants_table`.
    /// `gpu_busy` says whether it's worth paying for per-app GPU data otherwise.
    pub fn apps(
        &mut self,
        gpu_busy: bool,
        capacity: Capacity,
        wants_table: bool,
    ) -> (Vec<BusyApp>, Vec<AppRow>) {
        let Some(length) = query_processes(&mut self.buffer) else {
            return (Vec::new(), Vec::new());
        };
        let processes = parse_processes(&self.buffer[..length.min(self.buffer.len())]);
        let now = Instant::now();
        let seconds = self
            .last_sample
            .replace(now)
            .map_or(0.0, |last| (now - last).as_secs_f32());

        let gpu_use = if gpu_busy || wants_table {
            if self.gpu.is_none() {
                self.gpu = GpuApps::open();
            }
            self.gpu.as_ref().map(GpuApps::sample).unwrap_or_default()
        } else {
            HashMap::new()
        };
        let frozen = frozen_processes();

        let mut current = HashMap::with_capacity(processes.len());
        let mut groups: HashMap<String, Group> = HashMap::new();
        for process in processes {
            let key = process.name.to_lowercase();
            let identity = (process.pid, process.created);
            current.insert(identity, (process.cpu_time, process.io_bytes));
            if process.pid == 0 || key == self.own_name || NEVER_LISTED.contains(&key.as_str()) {
                continue;
            }
            let group = groups.entry(key.clone()).or_insert_with(|| Group {
                name: process.name.clone(),
                can_end: !NEVER_ENDED.contains(&key.as_str()),
                ..Default::default()
            });
            if let Some((cpu_before, io_before)) = self.previous.get(&identity) {
                if seconds > 0.0 {
                    let busy = (process.cpu_time - cpu_before).max(0) as f32 / TICKS_PER_SECOND;
                    group.usage.cpu += busy / seconds / self.threads * 100.0;
                    let moved = (process.io_bytes - io_before).max(0) as f32 / BYTES_PER_MB;
                    group.usage.disk_mb += moved / seconds;
                }
            }
            group.usage.memory_gb += process.private_bytes.max(0) as f32 / BYTES_PER_GB;
            if let Some(gpu) = gpu_use.get(&process.pid) {
                group.usage.gpu = group.usage.gpu.max(gpu.engine_percent);
                group.usage.video_memory_gb += gpu.dedicated_gb;
            }
            // Session 0 is where Windows services run.
            group.can_end &= process.session != 0;
            group.frozen |= frozen.contains(&process.pid);
            group.processes.push(ProcessId {
                pid: process.pid,
                created: process.created,
            });
        }
        self.previous = current;

        let running: Vec<&str> = groups.keys().map(String::as_str).collect();
        self.tracker.forget_all_but(&running);
        let samples = groups
            .iter()
            .map(|(key, g)| AppSample {
                key: key.clone(),
                usage: g.usage,
                frozen: g.frozen,
                can_end: g.can_end,
                processes: g.processes.clone(),
            })
            .collect();

        let shown = self.tracker.update(samples, capacity);

        // Names and icons are read once, then kept only for as long as they are
        // needed, so closing the Apps page gives their memory back.
        let wanted = |key: &String| {
            if wants_table {
                groups.contains_key(key)
            } else {
                shown.iter().any(|app| app.key == *key)
            }
        };
        self.identities.retain(|key, _| wanted(key));

        let table = if wants_table {
            let mut rows = Vec::with_capacity(groups.len());
            for (key, group) in &groups {
                let identity = self.identity(key, group);
                rows.push(AppRow {
                    key: key.clone(),
                    name: identity.name,
                    icon: identity.icon,
                    usage: group.usage,
                    frozen: self.tracker.is_frozen(key),
                    can_end: group.can_end,
                    processes: group.processes.clone(),
                });
            }
            rows
        } else {
            Vec::new()
        };

        let mut busy = Vec::with_capacity(shown.len());
        for app in shown {
            let identity = match groups.get(&app.key) {
                Some(group) => self.identity(&app.key, group),
                None => Identity::default(),
            };
            busy.push(BusyApp {
                labels: app.usage.labels(capacity),
                cpu_percent: app.usage.cpu,
                biggest: app.usage.biggest(capacity),
                key: app.key,
                name: identity.name,
                icon: identity.icon,
                frozen: app.frozen,
                can_end: app.can_end,
                processes: app.processes,
            });
        }
        (busy, table)
    }

    fn identity(&mut self, key: &str, group: &Group) -> Identity {
        if let Some(identity) = self.identities.get(key) {
            return identity.clone();
        }
        let pid = group.processes.first().map_or(0, |p| p.pid);
        let identity = read_identity(pid, &group.name);
        self.identities.insert(key.to_string(), identity.clone());
        identity
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndOutcome {
    Ended,
    /// Part of the app runs as administrator, which this unelevated app can't end.
    NeedsAdmin,
}

fn created_at(process: HANDLE) -> Option<i64> {
    let mut times = [FILETIME::default(); 4];
    let [created, exited, kernel, user] = &mut times;
    // SAFETY: the handle is open, and all four out pointers are valid.
    unsafe { GetProcessTimes(process, created, exited, kernel, user) }.ok()?;
    Some(((created.dwHighDateTime as i64) << 32) | created.dwLowDateTime as i64)
}

/// Ends every process of an app. Windows reuses process ids, so a process is
/// only touched if its creation time still matches what was measured.
pub fn end_app(processes: &[ProcessId]) -> EndOutcome {
    let mut denied = false;
    for target in processes {
        let access = PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION;
        // SAFETY: the handle is closed below, and is only used while open.
        let process = match unsafe { OpenProcess(access, false, target.pid) } {
            Ok(process) => process,
            Err(e) if e.code() == E_ACCESSDENIED => {
                denied = true;
                continue;
            }
            // It has already gone.
            Err(_) => continue,
        };
        if created_at(process) == Some(target.created) {
            // SAFETY: the handle was opened with terminate rights.
            denied |= unsafe { TerminateProcess(process, 1) }.is_err();
        }
        // SAFETY: the handle is open, and isn't used again.
        unsafe {
            let _ = CloseHandle(process);
        }
    }
    if denied {
        EndOutcome::NeedsAdmin
    } else {
        EndOutcome::Ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds records shaped like the kernel's, with names in the same buffer.
    fn buffer(items: &[(&str, u32, u32, i64)]) -> Vec<u8> {
        const STEP: usize = 512;
        let mut data = vec![0u8; STEP * items.len()];
        let base = data.as_ptr() as usize;
        for (i, (name, pid, session, private)) in items.iter().enumerate() {
            let at = i * STEP;
            let name_at = at + size_of::<Entry>() + 8;
            let units: Vec<u16> = name.encode_utf16().collect();
            for (j, unit) in units.iter().enumerate() {
                data[name_at + j * 2..name_at + j * 2 + 2].copy_from_slice(&unit.to_le_bytes());
            }
            let entry = Entry {
                next: if i + 1 == items.len() { 0 } else { STEP as u32 },
                threads: 1,
                private_bytes: *private,
                hard_faults: 0,
                peak_threads: 1,
                cycles: 0,
                created: 100 + i as i64,
                user_time: 30,
                kernel_time: 12,
                name_bytes: (units.len() * 2) as u16,
                name_capacity: (units.len() * 2) as u16,
                name: (base + name_at) as *const u16,
                base_priority: 8,
                pid: *pid as usize,
                parent: 0,
                handles: 1,
                session: *session,
                process_key: 0,
                peak_virtual_size: 0,
                virtual_size: 0,
                page_faults: 0,
                peak_working_set: 0,
                working_set: 0,
                quota_peak_paged_pool: 0,
                quota_paged_pool: 0,
                quota_peak_non_paged_pool: 0,
                quota_non_paged_pool: 0,
                pagefile_usage: 0,
                peak_pagefile_usage: 0,
                private_pages: 0,
                read_operations: 0,
                write_operations: 0,
                other_operations: 0,
                bytes_read: 700,
                bytes_written: 300,
            };
            // SAFETY: the record is in bounds of `data`.
            unsafe { ptr::write_unaligned(data.as_mut_ptr().add(at).cast::<Entry>(), entry) };
        }
        data
    }

    #[test]
    fn entry_matches_the_kernel_layout() {
        assert_eq!(std::mem::offset_of!(Entry, private_bytes), 0x08);
        assert_eq!(std::mem::offset_of!(Entry, created), 0x20);
        assert_eq!(std::mem::offset_of!(Entry, user_time), 0x28);
        assert_eq!(std::mem::offset_of!(Entry, kernel_time), 0x30);
        assert_eq!(std::mem::offset_of!(Entry, name_bytes), 0x38);
        assert_eq!(std::mem::offset_of!(Entry, name), 0x40);
        assert_eq!(std::mem::offset_of!(Entry, pid), 0x50);
        assert_eq!(std::mem::offset_of!(Entry, session), 0x64);
        assert_eq!(std::mem::offset_of!(Entry, bytes_read), 0xE8);
        assert_eq!(std::mem::offset_of!(Entry, bytes_written), 0xF0);
    }

    #[test]
    fn walks_records_and_strips_exe() {
        let data = buffer(&[("Fusion360.exe", 40, 1, 9), ("node", 41, 1, 3)]);
        let list = parse_processes(&data);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Fusion360");
        assert_eq!(list[0].pid, 40);
        assert_eq!(list[0].cpu_time, 42);
        assert_eq!(list[0].io_bytes, 1000);
        assert_eq!(list[0].created, 100);
        assert_eq!(list[1].name, "node");
        assert_eq!(list[1].private_bytes, 3);
    }

    #[test]
    fn a_record_past_the_end_stops_the_walk() {
        let mut data = buffer(&[("a.exe", 4, 0, 0), ("b.exe", 5, 0, 0)]);
        data.truncate(512 + 16);
        assert_eq!(parse_processes(&data).len(), 1);
    }

    #[test]
    fn a_name_outside_the_buffer_is_not_read() {
        let mut data = buffer(&[("a.exe", 4, 0, 0)]);
        data[0x40..0x48].copy_from_slice(&8usize.to_le_bytes());
        assert_eq!(parse_processes(&data)[0].name, "");
    }

    #[test]
    fn an_empty_buffer_has_no_processes() {
        assert!(parse_processes(&[]).is_empty());
    }

    #[test]
    fn strip_exe_handles_short_and_mixed_case_names() {
        assert_eq!(strip_exe("App.EXE"), "App");
        assert_eq!(strip_exe("exe"), "exe");
        assert_eq!(strip_exe(""), "");
    }

    #[test]
    fn premultiply_scales_color_by_alpha() {
        let mut pixels = [200, 100, 50, 128];
        premultiply(&mut pixels, None);
        assert_eq!(pixels, [100, 50, 25, 128]);
    }

    #[test]
    fn a_mask_decides_transparency_for_icons_without_alpha() {
        let mut pixels = [200, 100, 50, 0, 200, 100, 50, 0];
        let mask = [0, 0, 0, 0, 255, 255, 255, 255];
        premultiply(&mut pixels, Some(&mask));
        assert_eq!(pixels, [200, 100, 50, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn the_desktop_is_never_offered_end() {
        for name in ["explorer", "dwm", "csrss", "winlogon", "svchost"] {
            assert!(NEVER_ENDED.contains(&name), "{name}");
        }
    }
}
