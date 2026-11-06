# Vitals

A small, fully resizable sensor monitor for Windows, written in Rust with
Direct2D. It tracks CPU and GPU temperatures, load, power, memory, disk and
network in a single 360 KB exe with no runtime dependencies.

The window adapts to whatever size you give it:

| Window | Layout |
| --- | --- |
| Slim strip, up to about 145 px tall | One or two lines of key numbers, with no title bar. The controls appear on hover, and you can drag it from anywhere. |
| Narrow and tall, under 240 px wide | A single column of up to 11 rows |
| Small to medium | A grid of 2 to 4 columns, ordered by importance |
| Large, at least 820 × 600 | 16 tiles, plus a bar for every CPU thread |

## Using it

- **Temperatures** show a session peak. Click a tile to reset its peak.
- **Status** is shown in stages: Warm adds a label, Hot tints the tile, and
  Critical turns it solid red. A metric that is Critical, or Hot but not on
  screen, also takes over the title bar and the taskbar text, so you can see it
  while a game covers the window.
- **The pin button** keeps the window on top.
- **Position, size and pin state** are remembered in `%APPDATA%\Vitals\settings.ini`.

### Thresholds

| Sensor | Warm | Hot | Critical |
| --- | --- | --- | --- |
| CPU core | 80° | 90° | 95° (9800X3D limit) |
| CPU I/O die | 80° | 90° | 100° |
| GPU edge | 80° | 90° | 100° |
| GPU hotspot | 95° | 105° | 110° (RX 7900 XTX limit) |
| VRAM | 90° | 100° | 106° |

They are set in `MetricId::spec` in `src/metrics.rs`. Severity drops back only
once a value is 3° below the threshold, so it does not flicker.

## CPU temperature

Windows has no standard API for CPU temperature, and this PC exposes no ACPI
thermal zones, so Vitals reads what AMD provides:

- **Without admin rights**, the CPU tile shows the **I/O die** temperature, from
  the integrated GPU's sensors. It is labelled "CPU I/O die" and is not a core
  temperature: under a 16-second all-thread load it did not move at all.
- **Clicking that tile** elevates the helper, then shows the true core temperature,
  labelled "CPU temp". Windows normally shows a UAC prompt for this. If UAC
  prompts are turned off it happens silently, so the click is the only consent.
  Vitals repeats this at each launch until a prompt is declined. To stop it, set
  `core_temp=0` in `settings.ini`.

The core reading comes from `GetRmCpuParameters` in AMD's `Platform.dll`, which
is installed with AMD Software. AMD does not document that export. Its layout
was worked out on a 9800X3D, so an AMD Software update could change it.
`sensors::ryzen::parse` range-checks every field, so a changed layout means the
tile stays on the I/O die rather than showing a wrong number.

### Why there is a helper process

The UI never runs elevated. Clicking the tile starts this same exe a second time
with `--cpu-helper`, elevated, and that process does nothing except send one
reading per second down a one-way pipe:

- It reads nothing from the pipe, so the UI cannot ask it to do anything else.
- It exits as soon as the pipe closes, so it cannot outlive the UI.
- A fault in AMD's library ends the helper, not the UI.
- It loads AMD's DLL by a full path from Program Files, which only
  administrators can write to, and connects to the pipe anonymously so that the
  unelevated pipe owner cannot impersonate it.

## Hardware support

Built for and tested on a Ryzen 7 9800X3D with a Radeon RX 7900 XTX. Load,
memory, disk and network work on any PC. GPU sensors currently need an AMD GPU,
and on other hardware those tiles show a dash.

## Efficiency

Measured at the default size while a game was running:

| | |
| --- | --- |
| Exe size | 358 KB |
| Smallest window | 190 × 36 |
| CPU | 0.3% of one thread, or 0.02% of a 16-thread CPU |
| Memory | 33 MB working set, 15 MB private |
| Repaint during live resize | 8.8 ms |

Memory rises after resizing to very large windows, then settles at about 45 MB
working set. Over 1,500 resizes the handle count stayed flat at 332, and private
memory peaked at 52 MB before falling back to 27 MB, so it is cache, not a leak.

Rendering uses Direct2D's software backend. On this app that used the same CPU
as the hardware backend, with less than half the memory and no load on the GPU
being monitored. Vitals also requests Windows efficiency mode.

## Building

Needs Rust 1.85 or later and the MSVC toolchain with the Windows SDK.

```bash
cargo build --release
```

```bash
cargo test
```

The exe is written to `target/release/vitals.exe`. If the linker reports
`LNK1104` for a build script, the project path is too long for Windows. Build
from a shorter path, or set `CARGO_TARGET_DIR` to one.

## Layout of the code

| Path | Purpose |
| --- | --- |
| `src/window.rs` | The Win32 window, input and Snap support |
| `src/ui.rs` | Painting |
| `src/layout.rs` | Pure window layout, tested across thousands of sizes |
| `src/gfx.rs` | A thin Direct2D and DirectWrite layer |
| `src/metrics.rs` | What is tracked, and its state |
| `src/severity.rs`, `src/history.rs` | Status rules and fixed-size history |
| `src/sensors/` | The sampler thread, and ADL, PDH and Ryzen Master access |
| `src/helper.rs` | The elevated helper and the UI's link to it |
| `src/settings.rs` | Saved preferences |
| `designs/` | The original HTML design directions |
