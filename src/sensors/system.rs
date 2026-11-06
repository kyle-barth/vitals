//! System facts that come straight from Windows: memory use, base clock and
//! installed video memory.

use std::mem::size_of;

use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, IDXGIFactory1};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::core::w;

const BYTES_PER_GB: f32 = 1024.0 * 1024.0 * 1024.0;

/// Returns (used, total) physical memory in GB.
pub fn memory_gb() -> Option<(f32, f32)> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` is valid and its length field is set as required.
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    let total = status.ullTotalPhys as f32 / BYTES_PER_GB;
    let free = status.ullAvailPhys as f32 / BYTES_PER_GB;
    Some((total - free, total))
}

/// The CPU's base clock, which the performance counter is a percentage of.
pub fn base_clock_mhz() -> Option<f32> {
    let (mut mhz, mut size) = (0u32, size_of::<u32>() as u32);
    // SAFETY: the out pointers are valid and sized for a REG_DWORD.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"),
            w!("~MHz"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut mhz).cast()),
            Some(&mut size),
        )
    };
    (status.is_ok() && mhz > 0).then_some(mhz as f32)
}

/// Installed dedicated video memory of the largest GPU, in GB.
pub fn video_memory_gb() -> Option<f32> {
    // SAFETY: DXGI's factory and adapters are reference-counted COM objects,
    // and the description is returned by value.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut largest = 0usize;
        for index in 0.. {
            let adapter = match factory.EnumAdapters1(index) {
                Ok(adapter) => adapter,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(_) => return None,
            };
            if let Ok(description) = adapter.GetDesc1() {
                largest = largest.max(description.DedicatedVideoMemory);
            }
        }
        (largest > 0).then_some(largest as f32 / BYTES_PER_GB)
    }
}
