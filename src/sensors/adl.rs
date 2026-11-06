//! AMD Display Library bindings, for GPU sensors that need no elevation.
//!
//! The library is loaded at runtime, so Vitals still runs on a PC without AMD
//! graphics. On a Ryzen desktop CPU the integrated GPU is useful too: its
//! firmware reports the I/O die temperature and package power. The I/O die
//! does not follow core load, so it must never be presented as core temperature.

use std::ffi::c_void;
use std::mem::{size_of, transmute, zeroed};
use std::ptr::null_mut;

use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::System::Memory::{GetProcessHeap, HEAP_FLAGS, HeapAlloc};
use windows::core::{PCSTR, s, w};

const MAX_SENSORS: usize = 256;
const AMD_VENDOR_ID: i32 = 1002;

/// Sensor indices from the `ADLSensorType` enum in AMD's ADL SDK.
mod sensor {
    pub const TEMP_EDGE: usize = 8;
    pub const TEMP_MEMORY: usize = 9;
    pub const FAN_RPM: usize = 14;
    pub const ACTIVITY_GFX: usize = 19;
    pub const ASIC_POWER: usize = 23;
    pub const TEMP_HOTSPOT: usize = 27;
    pub const TEMP_SOC: usize = 29;
    pub const BOARD_POWER: usize = 73;
}

type Context = *mut c_void;
type AllocCallback = unsafe extern "system" fn(i32) -> *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct AdapterInfo {
    size: i32,
    index: i32,
    udid: [u8; 256],
    bus: i32,
    device: i32,
    function: i32,
    vendor: i32,
    name: [u8; 256],
    display: [u8; 256],
    present: i32,
    exist: i32,
    driver_path: [u8; 256],
    driver_path_ext: [u8; 256],
    pnp: [u8; 256],
    os_display_index: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Sensor {
    supported: i32,
    value: i32,
}

#[repr(C)]
struct PmLog {
    size: i32,
    sensors: [Sensor; MAX_SENSORS],
}

impl PmLog {
    fn get(&self, index: usize) -> Option<f32> {
        let s = self.sensors[index];
        (s.supported != 0).then_some(s.value as f32)
    }
}

/// ADL allocates through this callback. It is only used by calls that return
/// allocated memory, and Vitals passes its own buffers everywhere.
unsafe extern "system" fn adl_alloc(size: i32) -> *mut c_void {
    // SAFETY: the process heap is always valid, and a failed allocation
    // returns null, which ADL handles.
    unsafe {
        GetProcessHeap().map_or(null_mut(), |heap| {
            HeapAlloc(heap, HEAP_FLAGS(0), size.max(0) as usize)
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GpuReading {
    pub temp_edge: Option<f32>,
    pub temp_hotspot: Option<f32>,
    pub temp_memory: Option<f32>,
    pub load: Option<f32>,
    pub power_w: Option<f32>,
    pub fan_rpm: Option<f32>,
    pub vram_used_mb: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PackageReading {
    pub io_die_temp: Option<f32>,
    pub package_power_w: Option<f32>,
}

pub struct Adl {
    module: HMODULE,
    context: Context,
    destroy: unsafe extern "system" fn(Context) -> i32,
    query: unsafe extern "system" fn(Context, i32, *mut PmLog) -> i32,
    vram_used: Option<unsafe extern "system" fn(Context, i32, *mut i32) -> i32>,
    discrete: Option<i32>,
    integrated: Option<i32>,
}

// SAFETY: the context is created for, and only ever used by, the sampler
// thread that owns this value. ADL2 contexts are not tied to a thread.
unsafe impl Send for Adl {}

impl Adl {
    pub fn load() -> Option<Self> {
        // SAFETY: the library is loaded from System32 only, the function
        // signatures match AMD's published ADL headers, and every pointer
        // passed to it is valid for the duration of the call.
        unsafe {
            let module =
                LoadLibraryExW(w!("atiadlxx.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
            let symbol = |name: PCSTR| GetProcAddress(module, name);

            type Create = unsafe extern "system" fn(AllocCallback, i32, *mut Context) -> i32;
            type Count = unsafe extern "system" fn(Context, *mut i32) -> i32;
            type Info = unsafe extern "system" fn(Context, *mut AdapterInfo, i32) -> i32;
            type Destroy = unsafe extern "system" fn(Context) -> i32;
            type Query = unsafe extern "system" fn(Context, i32, *mut PmLog) -> i32;

            let create: Create = transmute(symbol(s!("ADL2_Main_Control_Create"))?);
            let count: Count = transmute(symbol(s!("ADL2_Adapter_NumberOfAdapters_Get"))?);
            let info: Info = transmute(symbol(s!("ADL2_Adapter_AdapterInfo_Get"))?);
            let destroy: Destroy = transmute(symbol(s!("ADL2_Main_Control_Destroy"))?);
            let query: Query = transmute(symbol(s!("ADL2_New_QueryPMLogData_Get"))?);
            let vram_used = symbol(s!("ADL2_Adapter_DedicatedVRAMUsage_Get")).map(|f| transmute(f));

            let mut context: Context = null_mut();
            if create(adl_alloc, 1, &mut context) != 0 {
                let _ = FreeLibrary(module);
                return None;
            }
            let mut adl = Self {
                module,
                context,
                destroy,
                query,
                vram_used,
                discrete: None,
                integrated: None,
            };

            let mut total = 0;
            if count(context, &mut total) != 0 || total <= 0 {
                return None;
            }
            let mut adapters = vec![zeroed::<AdapterInfo>(); total as usize];
            let bytes = (adapters.len() * size_of::<AdapterInfo>()) as i32;
            if info(context, adapters.as_mut_ptr(), bytes) != 0 {
                return None;
            }

            // One physical GPU appears as many logical adapters, so visit each
            // PCI location once and tell the GPUs apart by what they report.
            let mut seen = Vec::new();
            for adapter in adapters.iter().filter(|a| a.vendor == AMD_VENDOR_ID) {
                let location = (adapter.bus, adapter.device, adapter.function);
                if seen.contains(&location) {
                    continue;
                }
                seen.push(location);
                let Some(log) = adl.pm_log(adapter.index) else {
                    continue;
                };
                if log.get(sensor::FAN_RPM).is_some() {
                    adl.discrete.get_or_insert(adapter.index);
                } else if log.get(sensor::TEMP_SOC).is_some() {
                    adl.integrated.get_or_insert(adapter.index);
                }
            }
            Some(adl)
        }
    }

    fn pm_log(&self, adapter: i32) -> Option<Box<PmLog>> {
        // SAFETY: `PmLog` is plain integers, so all-zero is a valid value, and
        // the buffer is valid for the call.
        unsafe {
            let mut log: Box<PmLog> = Box::new(zeroed());
            log.size = size_of::<PmLog>() as i32;
            ((self.query)(self.context, adapter, &mut *log) == 0).then_some(log)
        }
    }

    pub fn gpu(&self) -> GpuReading {
        let Some(adapter) = self.discrete else {
            return GpuReading::default();
        };
        let Some(log) = self.pm_log(adapter) else {
            return GpuReading::default();
        };
        let vram_used_mb = self.vram_used.and_then(|f| {
            let mut mb = 0;
            // SAFETY: `mb` is valid for the call, and the signature matches
            // the published ADL header.
            let ok = unsafe { f(self.context, adapter, &mut mb) } == 0;
            (ok && mb >= 0).then_some(mb as f32)
        });
        GpuReading {
            temp_edge: log.get(sensor::TEMP_EDGE),
            temp_hotspot: log.get(sensor::TEMP_HOTSPOT),
            temp_memory: log.get(sensor::TEMP_MEMORY),
            load: log.get(sensor::ACTIVITY_GFX),
            power_w: log.get(sensor::BOARD_POWER).or(log.get(sensor::ASIC_POWER)),
            fan_rpm: log.get(sensor::FAN_RPM),
            vram_used_mb,
        }
    }

    pub fn package(&self) -> PackageReading {
        let Some(log) = self.integrated.and_then(|a| self.pm_log(a)) else {
            return PackageReading::default();
        };
        PackageReading {
            io_die_temp: log.get(sensor::TEMP_SOC),
            package_power_w: log.get(sensor::ASIC_POWER),
        }
    }
}

impl Drop for Adl {
    fn drop(&mut self) {
        // SAFETY: the context and module were created in `load` and are not
        // used again after this.
        unsafe {
            (self.destroy)(self.context);
            let _ = FreeLibrary(self.module);
        }
    }
}
