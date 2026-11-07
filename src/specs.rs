//! What is in this PC, for the Specs page.
//!
//! [`crate::sensors::hardware`] reads the raw facts from Windows. This module
//! turns them into parts with a headline, a line of key facts and a list of
//! details, and into the plain text that Copy puts on the clipboard. It has no
//! Win32 in it, so every rule here can be tested.

use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    Cpu,
    Gpu,
    Board,
    /// A laptop or prebuilt PC's model, shown where its board name means nothing.
    Model,
    Memory,
    Storage,
    Display,
    Windows,
}

impl PartKind {
    pub fn label(self) -> &'static str {
        match self {
            PartKind::Cpu => "CPU",
            PartKind::Gpu => "GPU",
            PartKind::Board => "Motherboard",
            PartKind::Model => "Model",
            PartKind::Memory => "Memory",
            PartKind::Storage => "Storage",
            PartKind::Display => "Display",
            PartKind::Windows => "Windows",
        }
    }

    /// The label at the start of a copied line, where "OS" is what people expect.
    fn copy_label(self) -> &'static str {
        match self {
            PartKind::Windows => "OS",
            other => other.label(),
        }
    }
}

/// One device within a part, such as one of several drives.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub name: String,
    /// Its key facts, joined with " · ".
    pub line: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub kind: PartKind,
    pub items: Vec<Item>,
    /// Label and value pairs, for when there is room for detail.
    pub facts: Vec<(String, String)>,
    /// What each item says in the copied text, without the label.
    copies: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Specs {
    pub parts: Vec<Part>,
}

impl Specs {
    /// Plain lines that paste cleanly into a chat or a forum post.
    pub fn copy_text(&self) -> String {
        let mut text = String::new();
        for part in &self.parts {
            let label = part.kind.copy_label();
            // A long list reads better a line at a time than as one long line.
            if part.copies.len() > 2 {
                for copy in &part.copies {
                    let _ = writeln!(text, "{label}: {copy}");
                }
            } else {
                let _ = writeln!(text, "{label}: {}", part.copies.join(", "));
            }
        }
        text.trim_end().to_string()
    }

    #[cfg(test)]
    pub fn part(&self, kind: PartKind) -> Option<&Part> {
        self.parts.iter().find(|p| p.kind == kind)
    }
}

/* ---------- raw facts, as read from Windows ---------- */

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuFacts {
    /// The brand string, such as "Intel(R) Core(TM) i7-4790K CPU @ 4.00GHz".
    pub brand: Option<String>,
    pub cores: Option<u32>,
    pub threads: Option<u32>,
    pub base_mhz: Option<u32>,
    pub l2_kb: Option<u32>,
    pub l3_kb: Option<u32>,
    /// SMBIOS socket designation, such as "SOCKET 1150" or "AM5".
    pub socket: Option<String>,
    pub intel: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuFacts {
    pub name: String,
    pub vram_bytes: u64,
    pub integrated: bool,
    /// The driver's version as Windows reports it, such as "31.0.15.5186".
    pub driver: Option<String>,
    pub nvidia: Option<crate::sensors::nvml::NvidiaInfo>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoardFacts {
    pub maker: Option<String>,
    pub product: Option<String>,
    pub version: Option<String>,
    pub system_maker: Option<String>,
    pub system_product: Option<String>,
    pub bios_version: Option<String>,
    /// As SMBIOS gives it, "MM/DD/YYYY".
    pub bios_date: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stick {
    pub mb: u64,
    pub kind: Option<&'static str>,
    pub mts: Option<u32>,
    pub part: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoryFacts {
    pub sticks: Vec<Stick>,
    /// Every slot, used or not.
    pub slots: Option<u32>,
    /// What Windows sees, for when SMBIOS lists no sticks.
    pub installed_mb: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DriveFacts {
    pub model: String,
    pub bytes: Option<u64>,
    pub solid_state: Option<bool>,
    pub bus: Option<&'static str>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayFacts {
    pub name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub hertz: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowsFacts {
    pub product: Option<String>,
    pub version: Option<String>,
    pub build: Option<u32>,
    pub revision: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub cpu: CpuFacts,
    pub gpus: Vec<GpuFacts>,
    pub board: BoardFacts,
    pub memory: MemoryFacts,
    pub drives: Vec<DriveFacts>,
    pub displays: Vec<DisplayFacts>,
    pub windows: WindowsFacts,
}

/* ---------- building the parts ---------- */

const DOT: &str = " · ";

fn join(parts: impl IntoIterator<Item = String>) -> String {
    parts
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(DOT)
}

pub fn build(facts: &Facts) -> Specs {
    let parts = [
        cpu_part(&facts.cpu),
        gpu_part(&facts.gpus),
        board_part(&facts.board),
        memory_part(&facts.memory),
        storage_part(&facts.drives),
        display_part(&facts.displays),
        windows_part(&facts.windows),
    ];
    Specs {
        parts: parts.into_iter().flatten().collect(),
    }
}

fn ghz(mhz: u32) -> String {
    format!("{:.1} GHz", mhz as f32 / 1000.0)
}

fn cache(kb: u32) -> String {
    if kb >= 1024 && kb % 1024 == 0 {
        format!("{} MB", kb / 1024)
    } else if kb >= 1024 {
        format!("{:.1} MB", kb as f32 / 1024.0)
    } else {
        format!("{kb} KB")
    }
}

fn cpu_part(cpu: &CpuFacts) -> Option<Part> {
    let name = cpu.brand.as_deref().map(clean_cpu_name)?;
    let cores = match (cpu.cores, cpu.threads) {
        (Some(c), Some(t)) if t != c => Some(format!("{c} cores, {t} threads")),
        (Some(c), _) => Some(format!("{c} cores")),
        (None, Some(t)) => Some(format!("{t} threads")),
        (None, None) => None,
    };
    let short_cores = match (cpu.cores, cpu.threads) {
        (Some(c), Some(t)) if t != c => vec![format!("{c} cores"), format!("{t} threads")],
        (Some(c), _) => vec![format!("{c} cores")],
        _ => Vec::new(),
    };
    let clock = cpu.base_mhz.filter(|m| *m > 0).map(ghz);
    let caches = [
        cpu.l2_kb.map(|k| format!("{} L2", cache(k))),
        cpu.l3_kb.map(|k| format!("{} L3", cache(k))),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let socket = cpu
        .socket
        .as_deref()
        .and_then(|s| clean_socket(s, cpu.intel));

    let mut facts = Vec::new();
    facts.extend(cores.clone().map(|c| ("Cores".to_string(), c)));
    facts.extend(clock.clone().map(|c| ("Base clock".to_string(), c)));
    if !caches.is_empty() {
        facts.push(("Cache".to_string(), caches.join(", ")));
    }
    facts.extend(socket.map(|s| ("Socket".to_string(), s)));

    let line = join(short_cores.into_iter().chain(clock.clone()));
    let detail = [cores, clock]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
    let copy = if detail.is_empty() {
        name.clone()
    } else {
        format!("{name} ({detail})")
    };
    Some(Part {
        kind: PartKind::Cpu,
        items: vec![Item { name, line }],
        facts,
        copies: vec![copy],
    })
}

/// Video memory as people quote it: 8 GB, 12 GB, 512 MB.
fn vram(bytes: u64) -> String {
    let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    if gb >= 1.0 {
        format!("{:.0} GB", gb.round())
    } else {
        format!("{} MB", bytes / (1024 * 1024))
    }
}

/// NVIDIA's own driver numbering, such as 551.86, from the version Windows
/// reports, such as 31.0.15.5186. Other makers' versions are left as they are.
pub fn nvidia_driver(windows_version: &str) -> Option<String> {
    let parts: Vec<u32> = windows_version
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let [_, _, c, d] = parts[..] else { return None };
    let number = (c % 10) * 10_000 + d;
    Some(format!("{}.{:02}", number / 100, number % 100))
}

fn gpu_part(gpus: &[GpuFacts]) -> Option<Part> {
    if gpus.is_empty() {
        return None;
    }
    let mut items = Vec::new();
    let mut copies = Vec::new();
    let mut facts = Vec::new();
    for gpu in gpus {
        let nv = gpu.nvidia.as_ref();
        let memory = (!gpu.integrated && gpu.vram_bytes > 0).then(|| vram(gpu.vram_bytes));
        let cores = nv
            .and_then(|n| n.cuda_cores)
            .map(|c| format!("{c} CUDA cores"));
        let line = if gpu.integrated {
            "Integrated".to_string()
        } else {
            join(memory.clone().into_iter().chain(cores.clone()))
        };
        copies.push(match &memory {
            Some(m) => format!("{} ({m})", gpu.name),
            None => gpu.name.clone(),
        });
        items.push(Item {
            name: gpu.name.clone(),
            line,
        });

        // Details are given for the main GPU only, which is listed first.
        if facts.is_empty() {
            let bus = nv
                .and_then(|n| n.memory_bus_bits)
                .map(|b| format!("{b}-bit"));
            if let Some(m) = &memory {
                facts.push((
                    "Video memory".to_string(),
                    [Some(m.clone()), bus]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(", "),
                ));
            }
            if let Some(c) = nv.and_then(|n| n.cuda_cores) {
                facts.push(("CUDA cores".to_string(), c.to_string()));
            }
            if let Some(m) = nv.and_then(|n| n.max_core_mhz) {
                facts.push(("Max clock".to_string(), format!("{m} MHz")));
            }
            if let Some(link) = nv.and_then(pcie_link) {
                facts.push(("Link".to_string(), link));
            }
            if let Some(w) = nv.and_then(|n| n.power_limit_w) {
                facts.push(("Power limit".to_string(), format!("{w:.0} W")));
            }
            let driver = nv
                .and_then(|n| n.driver.clone())
                .or_else(|| gpu.driver.clone());
            facts.extend(driver.map(|d| ("Driver".to_string(), d)));
        }
    }
    Some(Part {
        kind: PartKind::Gpu,
        items,
        facts,
        copies,
    })
}

fn pcie_link(nv: &crate::sensors::nvml::NvidiaInfo) -> Option<String> {
    let generation = nv.pcie_generation?;
    let lanes = nv.pcie_lanes?;
    let mut link = format!("PCIe {generation}.0 x{lanes}");
    if let Some(now) = nv.pcie_lanes_now.filter(|n| *n < lanes) {
        let _ = write!(link, ", running x{now}");
    }
    Some(link)
}

fn board_part(board: &BoardFacts) -> Option<Part> {
    let named =
        |maker: &Option<String>, product: &Option<String>| -> Option<(Option<String>, String)> {
            let product = product.as_deref().map(str::trim).filter(|p| is_real(p))?;
            let maker = maker.as_deref().map(str::trim).filter(|m| is_real(m));
            Some((maker.map(String::from), product.to_string()))
        };
    // A laptop's board name means nothing to its owner, but its model does.
    let (kind, (maker, product)) = match named(&board.maker, &board.product) {
        Some(board) => (PartKind::Board, board),
        None => (
            PartKind::Model,
            named(&board.system_maker, &board.system_product)?,
        ),
    };
    let short = maker.as_deref().map(short_maker);
    let name = match &short {
        Some(m) if !product.to_lowercase().starts_with(&m.to_lowercase()) => {
            format!("{m} {product}")
        }
        _ => product.clone(),
    };
    let date = board.bios_date.as_deref().and_then(bios_date);
    let bios = board
        .bios_version
        .as_deref()
        .map(str::trim)
        .filter(|v| is_real(v));
    let mut facts = Vec::new();
    facts.extend(maker.clone().map(|m| ("Maker".to_string(), m)));
    if let Some(v) = board
        .version
        .as_deref()
        .map(str::trim)
        .filter(|v| is_real(v))
    {
        facts.push(("Revision".to_string(), v.to_string()));
    }
    if let Some(b) = bios {
        let text = match &date {
            Some(d) => format!("{b}, {d}"),
            None => b.to_string(),
        };
        facts.push(("BIOS".to_string(), text));
    }
    let line = join(bios.map(|b| format!("BIOS {b}")).into_iter().chain(date));
    Some(Part {
        kind,
        items: vec![Item {
            name: name.clone(),
            line,
        }],
        facts,
        copies: vec![name],
    })
}

fn memory_part(memory: &MemoryFacts) -> Option<Part> {
    let total_mb: u64 = memory.sticks.iter().map(|s| s.mb).sum();
    let total_mb = if total_mb > 0 {
        total_mb
    } else {
        memory.installed_mb?
    };
    let total = format!("{} GB", (total_mb as f64 / 1024.0).round());
    let kind = memory.sticks.iter().find_map(|s| s.kind);
    let mts = memory.sticks.iter().filter_map(|s| s.mts).max();
    let kind_speed = match (kind, mts) {
        (Some(k), Some(s)) => Some(format!("{k}-{s}")),
        (Some(k), None) => Some(k.to_string()),
        (None, Some(s)) => Some(format!("{s} MT/s")),
        (None, None) => None,
    };
    let name = match &kind_speed {
        Some(ks) => format!("{total} {ks}"),
        None => total.clone(),
    };
    let sticks = sticks_text(&memory.sticks);
    let slots = memory
        .slots
        .filter(|s| *s as usize >= memory.sticks.len() && !memory.sticks.is_empty())
        .map(|s| format!("{} of {s} slots", memory.sticks.len()));
    let mut facts = Vec::new();
    facts.extend(sticks.clone().map(|s| ("Installed".to_string(), s)));
    facts.extend(
        memory
            .slots
            .filter(|s| *s as usize >= memory.sticks.len() && !memory.sticks.is_empty())
            .map(|s| {
                (
                    "Slots".to_string(),
                    format!("{} of {s} used", memory.sticks.len()),
                )
            }),
    );
    facts.extend(mts.map(|s| ("Speed".to_string(), format!("{s} MT/s"))));
    let parts: Vec<&str> = memory
        .sticks
        .iter()
        .filter_map(|s| s.part.as_deref())
        .collect();
    if let Some(first) = parts.first().filter(|p| parts.iter().all(|q| q == *p)) {
        facts.push(("Part".to_string(), first.to_string()));
    }
    let line = join(sticks.clone().into_iter().chain(slots));
    let copy = match sticks {
        Some(s) => format!("{name} ({s})"),
        None => name.clone(),
    };
    Some(Part {
        kind: PartKind::Memory,
        items: vec![Item { name, line }],
        facts,
        copies: vec![copy],
    })
}

/// "2 × 8 GB" for matching sticks, "8 GB + 4 GB" otherwise.
fn sticks_text(sticks: &[Stick]) -> Option<String> {
    let first = sticks.first()?;
    let gb = |mb: u64| {
        let gb = mb as f64 / 1024.0;
        if gb.fract() == 0.0 {
            format!("{gb:.0} GB")
        } else {
            format!("{gb:.1} GB")
        }
    };
    if sticks.iter().all(|s| s.mb == first.mb) {
        Some(format!("{} × {}", sticks.len(), gb(first.mb)))
    } else {
        Some(
            sticks
                .iter()
                .map(|s| gb(s.mb))
                .collect::<Vec<_>>()
                .join(" + "),
        )
    }
}

/// Drive sizes as they are sold: 240 GB, 1 TB, 1.9 TB.
pub fn drive_size(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 1000.0 {
        let tb = format!("{:.1}", gb / 1000.0);
        format!("{} TB", tb.trim_end_matches(".0"))
    } else {
        format!("{gb:.0} GB")
    }
}

fn storage_part(drives: &[DriveFacts]) -> Option<Part> {
    if drives.is_empty() {
        return None;
    }
    let kind = |d: &DriveFacts| match d.solid_state {
        Some(true) => Some("SSD"),
        Some(false) => Some("hard drive"),
        None => None,
    };
    let items = drives
        .iter()
        .map(|d| {
            let size_kind = match (d.bytes.map(drive_size), kind(d)) {
                (Some(s), Some(k)) => Some(format!("{s} {k}")),
                (s, k) => s.or(k.map(String::from)),
            };
            Item {
                name: d.model.clone(),
                line: join(size_kind.into_iter().chain(d.bus.map(String::from))),
            }
        })
        .collect();
    let facts = drives
        .iter()
        .map(|d| {
            let label = match kind(d) {
                Some("SSD") => "SSD",
                Some(_) => "Hard drive",
                None => "Drive",
            };
            let value = match d.bytes {
                Some(b) => format!("{}, {}", d.model, drive_size(b)),
                None => d.model.clone(),
            };
            (label.to_string(), value)
        })
        .collect();
    let copies = drives
        .iter()
        .map(|d| {
            let detail = match (d.bytes.map(drive_size), d.solid_state) {
                (Some(s), Some(true)) => Some(format!("{s} SSD")),
                (Some(s), Some(false)) => Some(format!("{s} HDD")),
                (s, _) => s,
            };
            match detail {
                Some(x) => format!("{} ({x})", d.model),
                None => d.model.clone(),
            }
        })
        .collect();
    Some(Part {
        kind: PartKind::Storage,
        items,
        facts,
        copies,
    })
}

fn display_part(displays: &[DisplayFacts]) -> Option<Part> {
    if displays.is_empty() {
        return None;
    }
    let mode = |d: &DisplayFacts| {
        let size = format!("{} × {}", d.width, d.height);
        match d.hertz {
            Some(hz) => format!("{size}, {hz} Hz"),
            None => size,
        }
    };
    let items = displays
        .iter()
        .map(|d| Item {
            name: d.name.clone().unwrap_or_else(|| "Display".to_string()),
            line: join(
                [format!("{} × {}", d.width, d.height)]
                    .into_iter()
                    .chain(d.hertz.map(|h| format!("{h} Hz"))),
            ),
        })
        .collect();
    let facts = if let [only] = displays {
        let mut facts = vec![(
            "Resolution".to_string(),
            format!("{} × {}", only.width, only.height),
        )];
        facts.extend(
            only.hertz
                .map(|h| ("Refresh rate".to_string(), format!("{h} Hz"))),
        );
        facts
    } else {
        displays
            .iter()
            .map(|d| {
                (
                    d.name.clone().unwrap_or_else(|| "Display".to_string()),
                    mode(d),
                )
            })
            .collect()
    };
    let copies = displays
        .iter()
        .map(|d| match &d.name {
            Some(n) => format!("{n} ({})", mode(d)),
            None => mode(d),
        })
        .collect();
    Some(Part {
        kind: PartKind::Display,
        items,
        facts,
        copies,
    })
}

fn windows_part(windows: &WindowsFacts) -> Option<Part> {
    let product = windows.product.as_deref()?;
    // Windows 11 still calls itself Windows 10 in the registry.
    let product = match windows.build {
        Some(b) if b >= 22_000 => product.replacen("Windows 10", "Windows 11", 1),
        _ => product.to_string(),
    };
    let name = match &windows.version {
        Some(v) => format!("{product} {v}"),
        None => product.clone(),
    };
    let build = windows.build.map(|b| match windows.revision {
        Some(r) => format!("{b}.{r}"),
        None => b.to_string(),
    });
    let mut facts = Vec::new();
    if let Some(edition) = product
        .strip_prefix("Windows 10 ")
        .or(product.strip_prefix("Windows 11 "))
    {
        facts.push(("Edition".to_string(), edition.to_string()));
    }
    facts.extend(windows.version.clone().map(|v| ("Version".to_string(), v)));
    facts.extend(build.clone().map(|b| ("Build".to_string(), b)));
    let copy = match &build {
        Some(b) => format!("{name} (build {b})"),
        None => name.clone(),
    };
    Some(Part {
        kind: PartKind::Windows,
        items: vec![Item {
            name,
            line: build.map(|b| format!("Build {b}")).unwrap_or_default(),
        }],
        facts,
        copies: vec![copy],
    })
}

/* ---------- cleaning up what firmware and drivers report ---------- */

/// "Intel(R) Core(TM) i7-4790K CPU @ 4.00GHz" becomes "Intel Core i7-4790K",
/// and "AMD Ryzen 7 9800X3D 8-Core Processor" becomes "AMD Ryzen 7 9800X3D".
pub fn clean_cpu_name(brand: &str) -> String {
    let mut name = brand
        .replace("(R)", "")
        .replace("(r)", "")
        .replace("(TM)", "")
        .replace("(tm)", "");
    if let Some(at) = name.find(" @ ") {
        name.truncate(at);
    }
    let core_count = |word: &str| {
        word.strip_suffix("-Core")
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    };
    let mut kept: Vec<&str> = name.split_whitespace().filter(|w| !core_count(w)).collect();
    while kept
        .last()
        .is_some_and(|w| *w == "Processor" || *w == "CPU")
    {
        kept.pop();
    }
    kept.join(" ")
}

/// "SOCKET 1150" becomes "LGA 1150" on Intel. Names like "AM5" stay. Board
/// positions such as "CPU 1" or "U3E1" say nothing useful and are dropped.
pub fn clean_socket(raw: &str, intel: bool) -> Option<String> {
    let raw = raw.trim();
    let upper = raw.to_uppercase();
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if let Some(number) = upper.strip_prefix("SOCKET").map(str::trim) {
        if digits(number) {
            return Some(if intel {
                format!("LGA {number}")
            } else {
                format!("Socket {number}")
            });
        }
        return (!number.is_empty() && is_real(number)).then(|| number.to_string());
    }
    if let Some(number) = upper.strip_prefix("LGA").map(str::trim) {
        return digits(number).then(|| format!("LGA {number}"));
    }
    let known = [
        "AM4", "AM5", "AM3+", "AM3", "FM2+", "TR4", "STRX4", "STR5", "SP3", "SP5", "SWRX8",
    ];
    known.iter().find(|k| upper == **k).map(|k| k.to_string())
}

/// Firmware fills unused strings with placeholders.
pub fn is_real(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    const PLACEHOLDERS: [&str; 12] = [
        "",
        "to be filled by o.e.m.",
        "default string",
        "system product name",
        "system manufacturer",
        "system version",
        "all series",
        "not applicable",
        "none",
        "o.e.m.",
        "x.x",
        "not specified",
    ];
    !PLACEHOLDERS.contains(&t.as_str()) && !t.chars().all(|c| c == '0' || c == ' ')
}

/// The name a maker is known by, rather than its legal name.
pub fn short_maker(maker: &str) -> String {
    let lower = maker.to_lowercase();
    let known = [
        ("asustek", "ASUS"),
        ("asus", "ASUS"),
        ("micro-star", "MSI"),
        ("msi", "MSI"),
        ("gigabyte", "Gigabyte"),
        ("asrock", "ASRock"),
        ("dell", "Dell"),
        ("lenovo", "Lenovo"),
        ("hewlett", "HP"),
        ("hp", "HP"),
        ("acer", "Acer"),
        ("biostar", "Biostar"),
        ("evga", "EVGA"),
        ("nzxt", "NZXT"),
        ("microsoft", "Microsoft"),
        ("intel", "Intel"),
        ("supermicro", "Supermicro"),
        ("framework", "Framework"),
        ("razer", "Razer"),
        ("samsung", "Samsung"),
        ("apple", "Apple"),
    ];
    known
        .iter()
        .find(|(prefix, _)| lower.starts_with(prefix))
        .map_or_else(|| maker.trim().to_string(), |(_, short)| short.to_string())
}

/// "10/28/2015" becomes "28 Oct 2015", which no reader can misread.
pub fn bios_date(raw: &str) -> Option<String> {
    let mut parts = raw.trim().split('/');
    let month: usize = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    let mut year: u32 = parts.next()?.parse().ok()?;
    if year < 100 {
        year += if year < 70 { 2000 } else { 1900 };
    }
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let name = MONTHS.get(month.checked_sub(1)?)?;
    (1..=31)
        .contains(&day)
        .then(|| format!("{day} {name} {year}"))
}

/* ---------- SMBIOS ---------- */

/// One SMBIOS structure: its type, formatted bytes and strings.
pub struct Structure<'a> {
    pub kind: u8,
    data: &'a [u8],
    strings: Vec<&'a [u8]>,
}

impl Structure<'_> {
    pub fn byte(&self, offset: usize) -> Option<u8> {
        self.data.get(offset).copied()
    }

    pub fn word(&self, offset: usize) -> Option<u16> {
        Some(u16::from_le_bytes(
            self.data.get(offset..offset + 2)?.try_into().ok()?,
        ))
    }

    pub fn dword(&self, offset: usize) -> Option<u32> {
        Some(u32::from_le_bytes(
            self.data.get(offset..offset + 4)?.try_into().ok()?,
        ))
    }

    /// The string a byte at `offset` refers to. Zero means none.
    pub fn string(&self, offset: usize) -> Option<String> {
        let index = self.byte(offset)? as usize;
        let raw = self.strings.get(index.checked_sub(1)?)?;
        let text = String::from_utf8_lossy(raw).trim().to_string();
        (!text.is_empty()).then_some(text)
    }
}

/// Splits an SMBIOS table into its structures, stopping at anything malformed.
pub fn smbios_structures(table: &[u8]) -> Vec<Structure<'_>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 4 <= table.len() {
        let kind = table[at];
        let length = table[at + 1] as usize;
        if length < 4 || at + length > table.len() {
            break;
        }
        let data = &table[at..at + length];
        // Strings follow, each ending in a zero, with a second zero at the end.
        let mut cursor = at + length;
        let mut strings = Vec::new();
        loop {
            let Some(end) = table[cursor..].iter().position(|b| *b == 0) else {
                return out;
            };
            if end == 0 {
                cursor += 1;
                break;
            }
            strings.push(&table[cursor..cursor + end]);
            cursor += end + 1;
        }
        if strings.is_empty() {
            // An empty string set is two zeros.
            if table.get(cursor) == Some(&0) {
                cursor += 1;
            }
        }
        out.push(Structure {
            kind,
            data,
            strings,
        });
        if kind == 127 {
            break;
        }
        at = cursor;
    }
    out
}

/// SMBIOS memory types, by the codes in the specification.
pub fn memory_kind(code: u8) -> Option<&'static str> {
    Some(match code {
        0x12 => "DDR",
        0x13 => "DDR2",
        0x18 => "DDR3",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        _ => return None,
    })
}

/// Reads the board, BIOS, CPU socket and memory sticks from an SMBIOS table.
pub fn read_smbios(
    table: &[u8],
    cpu: &mut CpuFacts,
    board: &mut BoardFacts,
    memory: &mut MemoryFacts,
) {
    let mut slots = 0u32;
    for s in smbios_structures(table) {
        match s.kind {
            0 => {
                board.bios_version = s.string(0x05);
                board.bios_date = s.string(0x08);
            }
            1 => {
                board.system_maker = s.string(0x04);
                board.system_product = s.string(0x05);
            }
            2 if board.product.is_none() => {
                board.maker = s.string(0x04);
                board.product = s.string(0x05);
                board.version = s.string(0x06);
            }
            4 if cpu.socket.is_none() => cpu.socket = s.string(0x04),
            // Only arrays used as system memory, not cache or video memory.
            16 if s.byte(0x05) == Some(0x03) => slots += u32::from(s.word(0x0D).unwrap_or(0)),
            17 => {
                if let Some(stick) = stick(&s) {
                    memory.sticks.push(stick);
                }
            }
            _ => {}
        }
    }
    if slots > 0 {
        memory.slots = Some(slots);
    }
}

fn stick(s: &Structure) -> Option<Stick> {
    let size = s.word(0x0C)?;
    let mb = match size {
        0 | 0xFFFF => return None,
        0x7FFF => u64::from(s.dword(0x1C)? & 0x7FFF_FFFF),
        kb if kb & 0x8000 != 0 => u64::from(kb & 0x7FFF) / 1024,
        mb => u64::from(mb),
    };
    if mb == 0 {
        return None;
    }
    let speed = |offset: usize, extended: usize| -> Option<u32> {
        match s.word(offset)? {
            0 => None,
            0xFFFF => s.dword(extended).filter(|v| *v > 0),
            v => Some(u32::from(v)),
        }
    };
    // The speed it runs at, which can be lower than what the stick is rated for.
    let mts = speed(0x20, 0x58).or_else(|| speed(0x15, 0x54));
    let part = s
        .string(0x1A)
        .filter(|p| is_real(p) && p.to_lowercase() != "unknown");
    Some(Stick {
        mb,
        kind: s.byte(0x12).and_then(memory_kind),
        mts,
        part,
    })
}

/* ---------- processor topology ---------- */

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Topology {
    pub cores: u32,
    pub threads: u32,
    pub l2_kb: u32,
    pub l3_kb: u32,
}

/// Reads the records from `GetLogicalProcessorInformationEx(RelationAll)`.
/// Each starts with its relationship and size. Cores carry a mask of their
/// threads; caches carry their level, size and type.
pub fn read_topology(buffer: &[u8]) -> Topology {
    const CORE: u32 = 0;
    const CACHE: u32 = 2;
    const DATA_OR_UNIFIED: [u32; 2] = [0, 2];
    let u32_at = |o: usize| {
        buffer
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let u16_at = |o: usize| {
        buffer
            .get(o..o + 2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
    };
    let u64_at = |o: usize| {
        buffer
            .get(o..o + 8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    };
    let mut t = Topology::default();
    let mut at = 0;
    while let (Some(relation), Some(size)) = (u32_at(at), u32_at(at + 4)) {
        let size = size as usize;
        if size < 8 || at + size > buffer.len() {
            break;
        }
        match relation {
            CORE => {
                t.cores += 1;
                let groups = u16_at(at + 30).unwrap_or(1).max(1) as usize;
                for g in 0..groups {
                    t.threads += u64_at(at + 32 + g * 16).map_or(0, |m| m.count_ones());
                }
            }
            CACHE => {
                let level = buffer.get(at + 8).copied();
                let kb = u32_at(at + 12).unwrap_or(0) / 1024;
                let kind = u32_at(at + 16).unwrap_or(u32::MAX);
                if DATA_OR_UNIFIED.contains(&kind) {
                    match level {
                        Some(2) => t.l2_kb += kb,
                        Some(3) => t.l3_kb += kb,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        at += size;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_names_lose_their_trademarks_and_clock() {
        assert_eq!(
            clean_cpu_name("Intel(R) Core(TM) i7-4790K CPU @ 4.00GHz"),
            "Intel Core i7-4790K"
        );
        assert_eq!(
            clean_cpu_name("AMD Ryzen 7 9800X3D 8-Core Processor           "),
            "AMD Ryzen 7 9800X3D"
        );
        assert_eq!(
            clean_cpu_name("13th Gen Intel(R) Core(TM) i7-13700K"),
            "13th Gen Intel Core i7-13700K"
        );
        assert_eq!(
            clean_cpu_name("Intel(R) Core(TM) Ultra 7 155H"),
            "Intel Core Ultra 7 155H"
        );
        assert_eq!(
            clean_cpu_name("AMD Ryzen 9 7950X 16-Core Processor"),
            "AMD Ryzen 9 7950X"
        );
    }

    #[test]
    fn sockets_read_the_way_people_say_them() {
        assert_eq!(
            clean_socket("SOCKET 1150", true).as_deref(),
            Some("LGA 1150")
        );
        assert_eq!(clean_socket("LGA1700", true).as_deref(), Some("LGA 1700"));
        assert_eq!(clean_socket("AM5", false).as_deref(), Some("AM5"));
        assert_eq!(clean_socket("CPU 1", true), None);
        assert_eq!(clean_socket("U3E1", true), None);
    }

    #[test]
    fn placeholders_are_not_names() {
        assert!(!is_real("To Be Filled By O.E.M."));
        assert!(!is_real("All Series"));
        assert!(!is_real("  "));
        assert!(!is_real("0000"));
        assert!(is_real("MAXIMUS VII IMPACT"));
    }

    #[test]
    fn makers_go_by_their_short_names() {
        assert_eq!(short_maker("ASUSTeK COMPUTER INC."), "ASUS");
        assert_eq!(short_maker("Micro-Star International Co., Ltd."), "MSI");
        assert_eq!(short_maker("Gigabyte Technology Co., Ltd."), "Gigabyte");
        assert_eq!(short_maker("Framework"), "Framework");
        assert_eq!(short_maker("Some Board Co"), "Some Board Co");
    }

    #[test]
    fn bios_dates_are_unambiguous() {
        assert_eq!(bios_date("10/28/2015").as_deref(), Some("28 Oct 2015"));
        assert_eq!(bios_date("1/2/24").as_deref(), Some("2 Jan 2024"));
        assert_eq!(bios_date("13/01/2020"), None);
        assert_eq!(bios_date("nonsense"), None);
    }

    #[test]
    fn drive_sizes_match_the_box() {
        assert_eq!(drive_size(240_057_409_536), "240 GB");
        assert_eq!(drive_size(1_000_204_886_016), "1 TB");
        assert_eq!(drive_size(2_000_398_934_016), "2 TB");
        assert_eq!(drive_size(1_920_383_410_176), "1.9 TB");
    }

    #[test]
    fn nvidia_driver_versions_use_nvidia_numbering() {
        assert_eq!(nvidia_driver("31.0.15.5186").as_deref(), Some("551.86"));
        assert_eq!(nvidia_driver("32.0.15.6094").as_deref(), Some("560.94"));
        assert_eq!(nvidia_driver("not a version"), None);
    }

    /// Builds an SMBIOS structure from its formatted bytes and strings.
    fn structure(kind: u8, body: &[u8], strings: &[&str]) -> Vec<u8> {
        let mut s = vec![kind, (4 + body.len()) as u8, 0, 0];
        s.extend_from_slice(body);
        for text in strings {
            s.extend_from_slice(text.as_bytes());
            s.push(0);
        }
        if strings.is_empty() {
            s.push(0);
        }
        s.push(0);
        s
    }

    fn memory_device(size_mb: u16, kind: u8, speed: u16, part: u8) -> Vec<u8> {
        // Offsets are from the start of the structure, so the body starts at 4.
        let mut body = vec![0u8; 0x28 - 4];
        body[0x0C - 4..0x0E - 4].copy_from_slice(&size_mb.to_le_bytes());
        body[0x12 - 4] = kind;
        body[0x15 - 4..0x17 - 4].copy_from_slice(&speed.to_le_bytes());
        body[0x1A - 4] = part;
        body[0x20 - 4..0x22 - 4].copy_from_slice(&speed.to_le_bytes());
        body
    }

    fn sample_table() -> Vec<u8> {
        let mut t = Vec::new();
        let mut bios = vec![0u8; 0x12 - 4];
        // Vendor, version and date, as offsets from the start of the body.
        bios[0] = 1;
        bios[1] = 2;
        bios[4] = 3;
        t.extend(structure(
            0,
            &bios,
            &["American Megatrends Inc.", "3003", "10/28/2015"],
        ));
        let mut board = vec![0u8; 0x0F - 4];
        board[0] = 1;
        board[1] = 2;
        board[2] = 3;
        t.extend(structure(
            2,
            &board,
            &["ASUSTeK COMPUTER INC.", "MAXIMUS VII IMPACT", "Rev 1.xx"],
        ));
        let mut array = vec![0u8; 0x0F - 4];
        array[0x05 - 4] = 3;
        array[0x0D - 4..0x0F - 4].copy_from_slice(&4u16.to_le_bytes());
        t.extend(structure(16, &array, &[]));
        t.extend(structure(
            17,
            &memory_device(8192, 0x18, 2400, 1),
            &["DDR3-2400 CL11 8GB"],
        ));
        t.extend(structure(17, &memory_device(0, 0x02, 0, 0), &[]));
        t.extend(structure(
            17,
            &memory_device(8192, 0x18, 2400, 1),
            &["DDR3-2400 CL11 8GB"],
        ));
        t.extend(structure(127, &[], &[]));
        t
    }

    #[test]
    fn smbios_gives_the_board_bios_and_sticks() {
        let (mut cpu, mut board, mut memory) = Default::default();
        read_smbios(&sample_table(), &mut cpu, &mut board, &mut memory);
        assert_eq!(board.product.as_deref(), Some("MAXIMUS VII IMPACT"));
        assert_eq!(board.bios_version.as_deref(), Some("3003"));
        assert_eq!(memory.sticks.len(), 2, "the empty slot is not a stick");
        assert_eq!(memory.slots, Some(4));
        assert_eq!(memory.sticks[0].kind, Some("DDR3"));
        assert_eq!(memory.sticks[0].mts, Some(2400));
    }

    #[test]
    fn a_truncated_smbios_table_is_read_up_to_the_damage() {
        let table = sample_table();
        let cut = &table[..table.len() / 2];
        let structures = smbios_structures(cut);
        assert!(!structures.is_empty() && structures.len() < 7);
        let (mut cpu, mut board, mut memory) = Default::default();
        read_smbios(&[0xFF, 0x02], &mut cpu, &mut board, &mut memory);
        assert_eq!(board, BoardFacts::default());
    }

    fn core_record(mask: u64) -> Vec<u8> {
        let mut r = vec![0u8; 48];
        r[4..8].copy_from_slice(&48u32.to_le_bytes());
        r[30..32].copy_from_slice(&1u16.to_le_bytes());
        r[32..40].copy_from_slice(&mask.to_le_bytes());
        r
    }

    fn cache_record(level: u8, bytes: u32, kind: u32) -> Vec<u8> {
        let mut r = vec![0u8; 48];
        r[0..4].copy_from_slice(&2u32.to_le_bytes());
        r[4..8].copy_from_slice(&48u32.to_le_bytes());
        r[8] = level;
        r[12..16].copy_from_slice(&bytes.to_le_bytes());
        r[16..20].copy_from_slice(&kind.to_le_bytes());
        r
    }

    #[test]
    fn topology_counts_cores_threads_and_cache() {
        let mut buffer = Vec::new();
        for core in 0..4u64 {
            buffer.extend(core_record(0b11 << (core * 2)));
            buffer.extend(cache_record(1, 32 * 1024, 1));
            buffer.extend(cache_record(1, 32 * 1024, 2));
            buffer.extend(cache_record(2, 256 * 1024, 0));
        }
        buffer.extend(cache_record(3, 8 * 1024 * 1024, 0));
        let t = read_topology(&buffer);
        assert_eq!((t.cores, t.threads), (4, 8));
        assert_eq!((t.l2_kb, t.l3_kb), (1024, 8192));
    }

    fn this_pc() -> Facts {
        Facts {
            cpu: CpuFacts {
                brand: Some("Intel(R) Core(TM) i7-4790K CPU @ 4.00GHz".into()),
                cores: Some(4),
                threads: Some(8),
                base_mhz: Some(4001),
                l2_kb: Some(1024),
                l3_kb: Some(8192),
                socket: Some("SOCKET 1150".into()),
                intel: true,
            },
            gpus: vec![GpuFacts {
                name: "NVIDIA GeForce RTX 2070 SUPER".into(),
                vram_bytes: 8 * 1024 * 1024 * 1024,
                integrated: false,
                driver: Some("31.0.15.5186".into()),
                nvidia: Some(crate::sensors::nvml::NvidiaInfo {
                    cuda_cores: Some(2560),
                    memory_bus_bits: Some(256),
                    driver: Some("551.86".into()),
                    pcie_generation: Some(3),
                    pcie_lanes: Some(16),
                    pcie_lanes_now: Some(8),
                    ..Default::default()
                }),
            }],
            board: BoardFacts {
                maker: Some("ASUSTeK COMPUTER INC.".into()),
                product: Some("MAXIMUS VII IMPACT".into()),
                bios_version: Some("3003".into()),
                bios_date: Some("10/28/2015".into()),
                system_product: Some("All Series".into()),
                ..Default::default()
            },
            memory: MemoryFacts {
                sticks: vec![
                    Stick {
                        mb: 8192,
                        kind: Some("DDR3"),
                        mts: Some(2400),
                        part: None,
                    },
                    Stick {
                        mb: 8192,
                        kind: Some("DDR3"),
                        mts: Some(2400),
                        part: None,
                    },
                ],
                slots: Some(4),
                installed_mb: None,
            },
            drives: vec![
                DriveFacts {
                    model: "KINGSTON SV300S37A240G".into(),
                    bytes: Some(240_057_409_536),
                    solid_state: Some(true),
                    bus: Some("SATA"),
                },
                DriveFacts {
                    model: "WDC WD10EZEX-22BN5A0".into(),
                    bytes: Some(1_000_204_886_016),
                    solid_state: Some(false),
                    bus: Some("SATA"),
                },
            ],
            displays: vec![DisplayFacts {
                name: Some("LG TV SSCR2".into()),
                width: 3840,
                height: 2160,
                hertz: Some(120),
            }],
            windows: WindowsFacts {
                product: Some("Windows 10 Pro".into()),
                version: Some("22H2".into()),
                build: Some(19045),
                revision: Some(6466),
            },
        }
    }

    #[test]
    fn this_pc_copies_as_plain_lines() {
        assert_eq!(
            build(&this_pc()).copy_text(),
            "CPU: Intel Core i7-4790K (4 cores, 8 threads, 4.0 GHz)\n\
             GPU: NVIDIA GeForce RTX 2070 SUPER (8 GB)\n\
             Motherboard: ASUS MAXIMUS VII IMPACT\n\
             Memory: 16 GB DDR3-2400 (2 × 8 GB)\n\
             Storage: KINGSTON SV300S37A240G (240 GB SSD), WDC WD10EZEX-22BN5A0 (1 TB HDD)\n\
             Display: LG TV SSCR2 (3840 × 2160, 120 Hz)\n\
             OS: Windows 10 Pro 22H2 (build 19045.6466)"
        );
    }

    #[test]
    fn each_part_has_a_headline_and_a_line_of_facts() {
        let specs = build(&this_pc());
        let line = |kind| specs.part(kind).unwrap().items[0].line.clone();
        assert_eq!(line(PartKind::Cpu), "4 cores · 8 threads · 4.0 GHz");
        assert_eq!(line(PartKind::Gpu), "8 GB · 2560 CUDA cores");
        assert_eq!(line(PartKind::Memory), "2 × 8 GB · 2 of 4 slots");
        assert_eq!(line(PartKind::Board), "BIOS 3003 · 28 Oct 2015");
        let gpu = specs.part(PartKind::Gpu).unwrap();
        assert!(
            gpu.facts
                .contains(&("Link".into(), "PCIe 3.0 x16, running x8".into()))
        );
    }

    #[test]
    fn a_laptop_shows_its_model_rather_than_its_board() {
        let mut facts = this_pc();
        facts.board = BoardFacts {
            maker: Some("Dell Inc.".into()),
            product: Some("0X1234".into()),
            system_maker: Some("Dell Inc.".into()),
            system_product: Some("XPS 15 9520".into()),
            ..Default::default()
        };
        // A board code is still a real name, so it is shown, with the maker.
        assert_eq!(
            build(&facts).part(PartKind::Board).unwrap().items[0].name,
            "Dell 0X1234"
        );
        facts.board.product = Some("Default string".into());
        let specs = build(&facts);
        assert!(specs.part(PartKind::Board).is_none());
        assert_eq!(
            specs.part(PartKind::Model).unwrap().items[0].name,
            "Dell XPS 15 9520"
        );
        assert!(specs.copy_text().contains("\nModel: Dell XPS 15 9520\n"));
    }

    #[test]
    fn many_drives_copy_a_line_each() {
        let mut facts = this_pc();
        facts.drives = (1..=3)
            .map(|n| DriveFacts {
                model: format!("Drive {n}"),
                ..Default::default()
            })
            .collect();
        let text = build(&facts).copy_text();
        assert!(text.contains("Storage: Drive 1\nStorage: Drive 2\nStorage: Drive 3"));
    }

    #[test]
    fn windows_11_is_called_windows_11() {
        let mut facts = this_pc();
        facts.windows.build = Some(26100);
        let specs = build(&facts);
        assert_eq!(
            specs.part(PartKind::Windows).unwrap().items[0].name,
            "Windows 11 Pro 22H2"
        );
    }

    #[test]
    fn missing_facts_drop_out_rather_than_showing_blanks() {
        let facts = Facts {
            cpu: CpuFacts {
                brand: Some("Some Chip".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let specs = build(&facts);
        assert_eq!(specs.parts.len(), 1);
        assert_eq!(specs.copy_text(), "CPU: Some Chip");
        assert_eq!(specs.parts[0].items[0].line, "");
    }

    #[test]
    fn mixed_sticks_are_listed_one_by_one() {
        let sticks = [
            Stick {
                mb: 8192,
                ..Default::default()
            },
            Stick {
                mb: 4096,
                ..Default::default()
            },
        ];
        assert_eq!(sticks_text(&sticks).as_deref(), Some("8 GB + 4 GB"));
    }
}
