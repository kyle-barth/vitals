//! Reads what is in this PC, for the Specs page.
//!
//! Everything here comes from Windows itself or the GPU driver, needs no
//! elevation, and installs nothing: SMBIOS for the board, BIOS and memory
//! sticks, the processor topology for cores and cache, DXGI and NVML for the
//! GPUs, storage queries for the drives, the display configuration for the
//! monitors, and the registry for the CPU's name and the Windows version.
//!
//! It runs once, on its own thread, the first time the Specs page opens.

use std::mem::size_of;

use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
    QueryDisplayConfig,
};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND, IDXGIDevice,
    IDXGIFactory1,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery,
    STORAGE_PROPERTY_QUERY, StorageDeviceProperty, StorageDeviceSeekPenaltyProperty,
};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::Win32::System::SystemInformation::{
    FIRMWARE_TABLE_PROVIDER, GetLogicalProcessorInformationEx, GetPhysicallyInstalledSystemMemory,
    GetSystemFirmwareTable, RelationAll, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows::core::{HSTRING, Interface, PCWSTR, w};

use super::nvml::Nvml;
use super::system::{CpuVendor, cpu_vendor};
use crate::specs::{
    CpuFacts, DisplayFacts, DriveFacts, Facts, GpuFacts, WindowsFacts, nvidia_driver, read_smbios,
    read_topology,
};

const CPU_KEY: PCWSTR = w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");
const BIOS_KEY: PCWSTR = w!(r"HARDWARE\DESCRIPTION\System\BIOS");
const WINDOWS_KEY: PCWSTR = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
const NVIDIA: u32 = 0x10DE;
const GIB: u64 = 1024 * 1024 * 1024;
const MAX_DRIVES: u32 = 32;

pub fn read() -> Facts {
    let mut facts = Facts {
        cpu: cpu(),
        ..Default::default()
    };
    if let Some(table) = smbios_table() {
        read_smbios(&table, &mut facts.cpu, &mut facts.board, &mut facts.memory);
    }
    if facts.board.product.is_none() {
        facts.board.maker = reg_string(BIOS_KEY, w!("BaseBoardManufacturer"));
        facts.board.product = reg_string(BIOS_KEY, w!("BaseBoardProduct"));
        facts.board.system_maker = reg_string(BIOS_KEY, w!("SystemManufacturer"));
        facts.board.system_product = reg_string(BIOS_KEY, w!("SystemProductName"));
        facts.board.bios_version = reg_string(BIOS_KEY, w!("BIOSVersion"));
        facts.board.bios_date = reg_string(BIOS_KEY, w!("BIOSReleaseDate"));
    }
    facts.memory.installed_mb = installed_memory_mb();
    facts.gpus = gpus();
    facts.drives = drives();
    facts.displays = displays();
    facts.windows = windows();
    facts
}

/// Unused strings in firmware often hold spaces, so they are trimmed and
/// dropped when empty.
fn reg_string(key: PCWSTR, value: PCWSTR) -> Option<String> {
    let mut size = 0u32;
    // SAFETY: the first call only reports the size. The buffer is then sized
    // in u16s to hold it, and the strings are valid for both calls.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            value,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
        .ok()
        .ok()?;
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            value,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .ok()?;
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        let text = String::from_utf16_lossy(&buffer[..end]).trim().to_string();
        (!text.is_empty()).then_some(text)
    }
}

fn reg_dword(key: PCWSTR, value: PCWSTR) -> Option<u32> {
    reg_value(key, value, RRF_RT_REG_DWORD)
}

fn reg_value(key: PCWSTR, value: PCWSTR, kind: REG_ROUTINE_FLAGS) -> Option<u32> {
    let (mut data, mut size) = (0u32, size_of::<u32>() as u32);
    // SAFETY: the out pointers are valid and sized for a REG_DWORD.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            value,
            kind,
            None,
            Some((&raw mut data).cast()),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(data)
}

fn cpu() -> CpuFacts {
    let topology = topology();
    CpuFacts {
        brand: reg_string(CPU_KEY, w!("ProcessorNameString")),
        cores: topology.map(|t| t.cores).filter(|c| *c > 0),
        threads: topology.map(|t| t.threads).filter(|t| *t > 0),
        base_mhz: reg_dword(CPU_KEY, w!("~MHz")).filter(|m| *m > 0),
        l2_kb: topology.map(|t| t.l2_kb).filter(|k| *k > 0),
        l3_kb: topology.map(|t| t.l3_kb).filter(|k| *k > 0),
        socket: None,
        intel: cpu_vendor() == CpuVendor::Intel,
    }
}

fn topology() -> Option<crate::specs::Topology> {
    let mut length = 0u32;
    // SAFETY: the first call only reports the length. The buffer is u64s, so
    // it is aligned for the records, and is at least as long as reported.
    unsafe {
        let _ = GetLogicalProcessorInformationEx(RelationAll, None, &mut length);
        if length == 0 {
            return None;
        }
        let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
        GetLogicalProcessorInformationEx(
            RelationAll,
            Some(
                buffer
                    .as_mut_ptr()
                    .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>(),
            ),
            &mut length,
        )
        .ok()?;
        let bytes = std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), length as usize);
        Some(read_topology(bytes))
    }
}

/// The raw SMBIOS table, without the 8-byte header Windows puts before it.
fn smbios_table() -> Option<Vec<u8>> {
    let provider = FIRMWARE_TABLE_PROVIDER(u32::from_be_bytes(*b"RSMB"));
    // SAFETY: the first call reports the size, and the second fills a buffer
    // of exactly that size.
    unsafe {
        let size = GetSystemFirmwareTable(provider, 0, None);
        if size <= 8 {
            return None;
        }
        let mut buffer = vec![0u8; size as usize];
        if GetSystemFirmwareTable(provider, 0, Some(&mut buffer)) != size {
            return None;
        }
        let length = u32::from_le_bytes(buffer[4..8].try_into().ok()?) as usize;
        Some(buffer.get(8..8 + length)?.to_vec())
    }
}

fn installed_memory_mb() -> Option<u64> {
    let mut kb = 0u64;
    // SAFETY: the out pointer is valid for the call.
    unsafe { GetPhysicallyInstalledSystemMemory(&mut kb) }.ok()?;
    (kb > 0).then_some(kb / 1024)
}

fn gpus() -> Vec<GpuFacts> {
    let mut gpus = Vec::new();
    // SAFETY: DXGI's factory and adapters are reference-counted COM objects,
    // and the description is returned by value.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return gpus;
        };
        for index in 0.. {
            let adapter = match factory.EnumAdapters1(index) {
                Ok(adapter) => adapter,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(_) => break,
            };
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let end = desc.Description.iter().position(|c| *c == 0).unwrap_or(128);
            let name = String::from_utf16_lossy(&desc.Description[..end])
                .trim()
                .to_string();
            let driver = adapter
                .CheckInterfaceSupport(&IDXGIDevice::IID)
                .ok()
                .map(|v| {
                    let v = v as u64;
                    format!(
                        "{}.{}.{}.{}",
                        v >> 48,
                        (v >> 32) & 0xFFFF,
                        (v >> 16) & 0xFFFF,
                        v & 0xFFFF
                    )
                });
            let vram = desc.DedicatedVideoMemory as u64;
            gpus.push(GpuFacts {
                name,
                vram_bytes: vram,
                // Integrated GPUs set aside a little memory; cards have a gigabyte or more.
                integrated: vram < GIB,
                driver: if desc.VendorId == NVIDIA {
                    driver.as_deref().and_then(nvidia_driver).or(driver)
                } else {
                    driver
                },
                nvidia: None,
            });
            if desc.VendorId == NVIDIA && !gpus.iter().any(|g| g.nvidia.is_some()) {
                if let (Some(nvml), Some(last)) = (Nvml::load(), gpus.last_mut()) {
                    last.nvidia = Some(nvml.info());
                }
            }
        }
    }
    // The card people mean when they say "my GPU" goes first.
    gpus.sort_by_key(|g| (g.integrated, std::cmp::Reverse(g.vram_bytes)));
    gpus.dedup_by(|a, b| a.name == b.name && a.vram_bytes == b.vram_bytes);
    gpus
}

/// Bus types from the STORAGE_BUS_TYPE enumeration that are worth naming.
fn bus_name(bus: u32) -> Option<&'static str> {
    Some(match bus {
        0x01 => "SCSI",
        0x03 => "ATA",
        0x08 => "RAID",
        0x0A => "SAS",
        0x0B => "SATA",
        0x11 => "NVMe",
        0x12 => "SCM",
        0x13 => "UFS",
        _ => return None,
    })
}

/// USB sticks, card readers and virtual disks are not part of the PC.
fn is_internal(bus: u32, removable: bool) -> bool {
    const USB: u32 = 0x07;
    const SD: u32 = 0x0C;
    const MMC: u32 = 0x0D;
    const VIRTUAL: u32 = 0x0E;
    const FILE_BACKED: u32 = 0x0F;
    !removable && ![USB, SD, MMC, VIRTUAL, FILE_BACKED].contains(&bus)
}

/// The model name, bus type and whether the media is removable, from a
/// STORAGE_DEVICE_DESCRIPTOR. Its strings are found by offsets into itself.
pub fn parse_device_descriptor(bytes: &[u8]) -> Option<(String, u32, bool)> {
    let u32_at = |o: usize| {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let text_at = |offset: u32| -> Option<String> {
        let start = offset as usize;
        if start == 0 || start >= bytes.len() {
            return None;
        }
        let end = bytes[start..]
            .iter()
            .position(|b| *b == 0)
            .map_or(bytes.len(), |e| start + e);
        let text = String::from_utf8_lossy(&bytes[start..end])
            .trim()
            .to_string();
        (!text.is_empty()).then_some(text)
    };
    let removable = *bytes.get(10)? != 0;
    let vendor = text_at(u32_at(12)?);
    let product = text_at(u32_at(16)?)?;
    let bus = u32_at(28)?;
    let model = match vendor {
        Some(v)
            if !["ATA", "NVME"].contains(&v.to_uppercase().as_str())
                && !product.to_lowercase().starts_with(&v.to_lowercase()) =>
        {
            format!("{v} {product}")
        }
        _ => product,
    };
    Some((model, bus, removable))
}

fn drives() -> Vec<DriveFacts> {
    let mut drives = Vec::new();
    let mut misses = 0;
    for n in 0..MAX_DRIVES {
        let path = HSTRING::from(format!(r"\\.\PhysicalDrive{n}"));
        // SAFETY: the path is a valid string. No access rights are asked for,
        // which is enough for these queries and needs no elevation.
        let handle = unsafe {
            CreateFileW(
                &path,
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        };
        let Ok(handle) = handle else {
            // Numbers can have gaps after a drive is removed, but not many.
            misses += 1;
            if misses >= 4 {
                break;
            }
            continue;
        };
        if let Some(drive) = drive(handle) {
            drives.push(drive);
        }
        // SAFETY: the handle was opened above and is closed once.
        let _ = unsafe { CloseHandle(handle) };
    }
    drives
}

fn query_property(handle: HANDLE, property: i32, out: &mut [u8]) -> Option<usize> {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: windows::Win32::System::Ioctl::STORAGE_PROPERTY_ID(property),
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    let mut returned = 0u32;
    // SAFETY: the query and output buffer are valid for their stated sizes.
    unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&raw const query).cast()),
            size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(out.as_mut_ptr().cast()),
            out.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    Some(returned as usize)
}

fn drive(handle: HANDLE) -> Option<DriveFacts> {
    let mut descriptor = vec![0u8; 1024];
    let length = query_property(handle, StorageDeviceProperty.0, &mut descriptor)?;
    let (model, bus, removable) = parse_device_descriptor(&descriptor[..length])?;
    if !is_internal(bus, removable) {
        return None;
    }
    let mut penalty = [0u8; 12];
    let solid_state = query_property(handle, StorageDeviceSeekPenaltyProperty.0, &mut penalty)
        .filter(|n| *n >= 9)
        .map(|_| penalty[8] == 0);

    let mut geometry = [0u8; 256];
    let mut returned = 0u32;
    // SAFETY: the buffer is larger than DISK_GEOMETRY_EX's fixed part, and the
    // handle is open.
    let sized = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_DISK_GET_DRIVE_GEOMETRY_EX,
            None,
            0,
            Some(geometry.as_mut_ptr().cast()),
            geometry.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .is_ok();
    let bytes = (sized && returned >= 32)
        .then(|| i64::from_le_bytes(geometry[24..32].try_into().unwrap()))
        .filter(|b| *b > 0)
        .map(|b| b as u64);
    Some(DriveFacts {
        model,
        bytes,
        solid_state,
        bus: bus_name(bus),
    })
}

fn displays() -> Vec<DisplayFacts> {
    let mut out = Vec::new();
    // SAFETY: the arrays are sized from GetDisplayConfigBufferSizes, and
    // QueryDisplayConfig writes back how many it filled. The name request's
    // header gives its own size.
    unsafe {
        let (mut path_count, mut mode_count) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
            .is_err()
        {
            return out;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return out;
        }
        paths.truncate(path_count as usize);
        modes.truncate(mode_count as usize);
        for path in &paths {
            let source = &path.sourceInfo;
            let Some(mode) = modes.iter().find(|m| {
                m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE
                    && m.id == source.id
                    && m.adapterId.LowPart == source.adapterId.LowPart
                    && m.adapterId.HighPart == source.adapterId.HighPart
            }) else {
                continue;
            };
            let size = mode.Anonymous.sourceMode;
            let rate = path.targetInfo.refreshRate;
            let hertz = (rate.Denominator > 0)
                .then(|| (rate.Numerator as f64 / rate.Denominator as f64).round() as u32)
                .filter(|h| *h > 0);
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target.header.adapterId = path.targetInfo.adapterId;
            target.header.id = path.targetInfo.id;
            let name = (DisplayConfigGetDeviceInfo(&mut target.header) == 0)
                .then(|| {
                    let raw = &target.monitorFriendlyDeviceName;
                    let end = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
                    String::from_utf16_lossy(&raw[..end]).trim().to_string()
                })
                .filter(|n| !n.is_empty());
            out.push(DisplayFacts {
                name,
                width: size.width,
                height: size.height,
                hertz,
            });
        }
    }
    out
}

fn windows() -> WindowsFacts {
    WindowsFacts {
        product: reg_string(WINDOWS_KEY, w!("ProductName")),
        version: reg_string(WINDOWS_KEY, w!("DisplayVersion"))
            .or_else(|| reg_string(WINDOWS_KEY, w!("ReleaseId"))),
        build: reg_string(WINDOWS_KEY, w!("CurrentBuild")).and_then(|b| b.parse().ok()),
        revision: reg_dword(WINDOWS_KEY, w!("UBR")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(vendor: &str, product: &str, bus: u32, removable: bool) -> Vec<u8> {
        let mut d = vec![0u8; 40];
        d[10] = removable as u8;
        d[28..32].copy_from_slice(&bus.to_le_bytes());
        let mut put = |text: &str, field: usize| {
            if text.is_empty() {
                return;
            }
            let offset = d.len() as u32;
            d[field..field + 4].copy_from_slice(&offset.to_le_bytes());
            d.extend_from_slice(text.as_bytes());
            d.push(0);
        };
        put(vendor, 12);
        put(product, 16);
        d
    }

    #[test]
    fn a_drive_is_named_by_its_model() {
        let d = descriptor("", "KINGSTON SV300S37A240G", 0x0B, false);
        assert_eq!(
            parse_device_descriptor(&d),
            Some(("KINGSTON SV300S37A240G".into(), 0x0B, false))
        );
    }

    #[test]
    fn a_vendor_is_added_only_when_the_model_lacks_it() {
        let d = descriptor("Samsung", "SSD 980 PRO 1TB", 0x11, false);
        assert_eq!(
            parse_device_descriptor(&d).unwrap().0,
            "Samsung SSD 980 PRO 1TB"
        );
        let d = descriptor("NVMe", "Samsung SSD 980 PRO 1TB", 0x11, false);
        assert_eq!(
            parse_device_descriptor(&d).unwrap().0,
            "Samsung SSD 980 PRO 1TB"
        );
        let d = descriptor("WDC", "WDC WD10EZEX", 0x0B, false);
        assert_eq!(parse_device_descriptor(&d).unwrap().0, "WDC WD10EZEX");
    }

    #[test]
    fn usb_sticks_and_virtual_disks_are_not_listed() {
        assert!(is_internal(0x11, false));
        assert!(!is_internal(0x07, false));
        assert!(!is_internal(0x0E, false));
        assert!(!is_internal(0x0B, true));
    }

    #[test]
    fn a_damaged_descriptor_is_rejected() {
        assert_eq!(parse_device_descriptor(&[0u8; 8]), None);
        let mut d = descriptor("", "Model", 0x0B, false);
        d[16..20].copy_from_slice(&9999u32.to_le_bytes());
        assert_eq!(parse_device_descriptor(&d), None);
    }
}
