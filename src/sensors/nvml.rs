//! NVIDIA Management Library bindings, for NVIDIA GPU sensors that need no
//! elevation.
//!
//! `nvml.dll` is part of every current NVIDIA driver and lives in System32, so
//! nothing extra has to be installed. It is loaded at runtime, so Vitals still
//! runs on a PC without NVIDIA graphics.
//!
//! GeForce cards do not report hotspot or memory temperature through NVML, and
//! recent ones only report fan speed as a share of its maximum, not in rpm.

use std::ffi::{CStr, c_char, c_void};
use std::mem::transmute;
use std::ptr::null_mut;

use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{PCSTR, s, w};

use super::GpuReading;

const SUCCESS: i32 = 0;
const TEMPERATURE_GPU: i32 = 0;
const CLOCK_GRAPHICS: i32 = 0;
const CLOCK_MEMORY: i32 = 2;
const BYTES_PER_MB: f32 = 1024.0 * 1024.0;
const TEXT_LEN: usize = 96;

type Device = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default)]
struct Memory {
    total: u64,
    free: u64,
    used: u64,
}

type UintQuery = unsafe extern "C" fn(Device, *mut u32) -> i32;
/// A query that takes a kind, such as which clock or which temperature sensor.
type ClockQuery = unsafe extern "C" fn(Device, i32, *mut u32) -> i32;
type TextQuery = unsafe extern "C" fn(Device, *mut c_char, u32) -> i32;
type UtilizationQuery = unsafe extern "C" fn(Device, *mut Utilization) -> i32;
type MemoryQuery = unsafe extern "C" fn(Device, *mut Memory) -> i32;

/// Facts about the card that do not change while Vitals runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NvidiaInfo {
    pub name: Option<String>,
    pub driver: Option<String>,
    pub vbios: Option<String>,
    pub memory_bus_bits: Option<u32>,
    pub max_core_mhz: Option<u32>,
    pub max_memory_mhz: Option<u32>,
    pub cuda_cores: Option<u32>,
    pub pcie_generation: Option<u32>,
    pub pcie_lanes: Option<u32>,
    /// Lanes in use now, which is fewer than `pcie_lanes` in some slots.
    pub pcie_lanes_now: Option<u32>,
    pub power_limit_w: Option<f32>,
}

pub struct Nvml {
    module: HMODULE,
    device: Device,
    shutdown: unsafe extern "C" fn() -> i32,
    temperature: ClockQuery,
    utilization: UtilizationQuery,
    memory: MemoryQuery,
    power: Option<UintQuery>,
    fan: Option<UintQuery>,
    clock: Option<ClockQuery>,
}

// SAFETY: NVML is documented as thread-safe, and the device handle is only a
// token that NVML resolves on each call.
unsafe impl Send for Nvml {}

impl Nvml {
    pub fn load() -> Option<Self> {
        // SAFETY: the library is loaded from System32 only, the function
        // signatures match NVIDIA's published nvml.h, and every pointer passed
        // to it is valid for the duration of the call.
        unsafe {
            let module = LoadLibraryExW(w!("nvml.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
            let symbol = |name: PCSTR| GetProcAddress(module, name);

            type Init = unsafe extern "C" fn() -> i32;
            type Count = unsafe extern "C" fn(*mut u32) -> i32;
            type Handle = unsafe extern "C" fn(u32, *mut Device) -> i32;

            let loaded = (|| {
                let init: Init = transmute(symbol(s!("nvmlInit_v2"))?);
                let shutdown: Init = transmute(symbol(s!("nvmlShutdown"))?);
                let count: Count = transmute(symbol(s!("nvmlDeviceGetCount_v2"))?);
                let handle: Handle = transmute(symbol(s!("nvmlDeviceGetHandleByIndex_v2"))?);
                Some((init, shutdown, count, handle))
            })();
            let Some((init, shutdown, count, handle)) = loaded else {
                let _ = FreeLibrary(module);
                return None;
            };
            if init() != SUCCESS {
                let _ = FreeLibrary(module);
                return None;
            }

            let required = (|| {
                let temperature: ClockQuery = transmute(symbol(s!("nvmlDeviceGetTemperature"))?);
                let utilization: UtilizationQuery =
                    transmute(symbol(s!("nvmlDeviceGetUtilizationRates"))?);
                let memory: MemoryQuery = transmute(symbol(s!("nvmlDeviceGetMemoryInfo"))?);
                Some((temperature, utilization, memory))
            })();
            let mut total = 0u32;
            let mut device: Device = null_mut();
            let found = count(&mut total) == SUCCESS
                && total > 0
                && handle(0, &mut device) == SUCCESS
                && !device.is_null();
            let (Some((temperature, utilization, memory)), true) = (required, found) else {
                shutdown();
                let _ = FreeLibrary(module);
                return None;
            };

            Some(Self {
                module,
                device,
                shutdown,
                temperature,
                utilization,
                memory,
                power: symbol(s!("nvmlDeviceGetPowerUsage")).map(|f| transmute(f)),
                fan: symbol(s!("nvmlDeviceGetFanSpeed")).map(|f| transmute(f)),
                clock: symbol(s!("nvmlDeviceGetClockInfo")).map(|f| transmute(f)),
            })
        }
    }

    fn uint(&self, query: Option<UintQuery>) -> Option<u32> {
        let query = query?;
        let mut value = 0u32;
        // SAFETY: `value` is valid for the call, and the device came from NVML.
        (unsafe { query(self.device, &mut value) } == SUCCESS).then_some(value)
    }

    pub fn gpu(&self) -> GpuReading {
        let mut temp = 0u32;
        let mut busy = Utilization::default();
        let mut memory = Memory::default();
        let mut clock = 0u32;
        // SAFETY: each out pointer is valid for its call, and the device came
        // from NVML.
        let (temp_ok, busy_ok, memory_ok, clock_ok) = unsafe {
            (
                (self.temperature)(self.device, TEMPERATURE_GPU, &mut temp) == SUCCESS,
                (self.utilization)(self.device, &mut busy) == SUCCESS,
                (self.memory)(self.device, &mut memory) == SUCCESS,
                self.clock
                    .is_some_and(|f| f(self.device, CLOCK_GRAPHICS, &mut clock) == SUCCESS),
            )
        };
        GpuReading {
            temp_edge: temp_ok.then_some(temp as f32),
            load: busy_ok.then_some(busy.gpu.min(100) as f32),
            power_w: self.uint(self.power).map(|mw| mw as f32 / 1000.0),
            fan_percent: self.uint(self.fan).map(|p| p.min(100) as f32),
            vram_used_mb: memory_ok.then_some(memory.used as f32 / BYTES_PER_MB),
            clock_mhz: clock_ok.then_some(clock as f32),
            ..GpuReading::default()
        }
    }

    /// Reads the card's fixed details, for the Specs page.
    pub fn info(&self) -> NvidiaInfo {
        // SAFETY: each symbol is looked up by its published name and cast to the
        // signature in nvml.h. Every buffer is valid and sized for its call.
        unsafe {
            let symbol = |name: PCSTR| GetProcAddress(self.module, name);
            let text = |name: PCSTR| -> Option<String> {
                let query: TextQuery = transmute(symbol(name)?);
                let mut buffer = [0 as c_char; TEXT_LEN];
                if query(self.device, buffer.as_mut_ptr(), TEXT_LEN as u32) != SUCCESS {
                    return None;
                }
                let text = CStr::from_ptr(buffer.as_ptr()).to_str().ok()?.trim();
                (!text.is_empty()).then(|| text.to_string())
            };
            let uint = |name: PCSTR| -> Option<u32> {
                let query: UintQuery = transmute(symbol(name)?);
                self.uint(Some(query))
            };
            let max_clock = |kind: i32| -> Option<u32> {
                let query: ClockQuery = transmute(symbol(s!("nvmlDeviceGetMaxClockInfo"))?);
                let mut mhz = 0u32;
                (query(self.device, kind, &mut mhz) == SUCCESS && mhz > 0).then_some(mhz)
            };
            let driver = (|| {
                type Version = unsafe extern "C" fn(*mut c_char, u32) -> i32;
                let query: Version = transmute(symbol(s!("nvmlSystemGetDriverVersion"))?);
                let mut buffer = [0 as c_char; TEXT_LEN];
                if query(buffer.as_mut_ptr(), TEXT_LEN as u32) != SUCCESS {
                    return None;
                }
                Some(CStr::from_ptr(buffer.as_ptr()).to_str().ok()?.to_string())
            })();
            NvidiaInfo {
                name: text(s!("nvmlDeviceGetName")),
                driver,
                vbios: text(s!("nvmlDeviceGetVbiosVersion")),
                memory_bus_bits: uint(s!("nvmlDeviceGetMemoryBusWidth")).filter(|b| *b > 0),
                max_core_mhz: max_clock(CLOCK_GRAPHICS),
                max_memory_mhz: max_clock(CLOCK_MEMORY),
                cuda_cores: uint(s!("nvmlDeviceGetNumGpuCores")).filter(|c| *c > 0),
                pcie_generation: uint(s!("nvmlDeviceGetMaxPcieLinkGeneration")).filter(|g| *g > 0),
                pcie_lanes: uint(s!("nvmlDeviceGetMaxPcieLinkWidth")).filter(|w| *w > 0),
                pcie_lanes_now: uint(s!("nvmlDeviceGetCurrPcieLinkWidth")).filter(|w| *w > 0),
                power_limit_w: uint(s!("nvmlDeviceGetEnforcedPowerLimit"))
                    .filter(|mw| *mw > 0)
                    .map(|mw| mw as f32 / 1000.0),
            }
        }
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: NVML was initialised in `load`, and is not used after this.
        unsafe {
            (self.shutdown)();
            let _ = FreeLibrary(self.module);
        }
    }
}
