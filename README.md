# 🩺 Vitals

A tiny, fully resizable sensor monitor for Windows, written in Rust. One exe of about 450 KB, with nothing to install.

<p align="center">
  <img src="docs/images/default.png" width="340" alt="Vitals at its default size: twelve tiles showing temperatures, memory, load and power, each with a live history line">
  <img src="docs/images/busy.png" width="340" alt="Vitals with the Busy list showing: a red row for a frozen app with an End button, and a row for an app using 37% of the CPU">
</p>

## ⬇️ Download

**[Get the latest release](https://github.com/kyle-barth/vitals/releases/latest)**, download `vitals.exe` and run it.

- 🪟 Needs Windows 10 or 11
- ⚠️ The exe isn't code-signed, so SmartScreen may warn you the first time. Choose **More info**, then **Run anyway**.
- 🔏 Each release is built by GitHub Actions from the tagged source, and lists a SHA-256 hash so you can check your download

## ✨ Features

- 🌡️ **Temperatures:** CPU, GPU, hotspot and VRAM, each with a session peak. Click a tile to reset its peak.
- 📈 **Live stats:** load, power, memory, disk, network and fan speed, with history lines
- 🚦 **Warnings:** Warm adds a label, Hot tints the tile and Critical turns it red. Serious ones also reach the title bar and taskbar, so you can see them while a game covers the window.
- 🔥 **Busy apps:** only shown when an app is using a lot, with End and a "not responding" warning
- 📌 **Pin:** keeps the window on top. Position, size and pin state are remembered.
- 🌗 **Themes:** light and dark, following Windows
- 🪶 **Light:** about 0.7% of one CPU thread and 35 MB of memory at the default size

<details>
<summary>🌡️ Temperature limits</summary>

Warm / Hot / Critical:

- **CPU core:** 80° / 90° / 95°
- **CPU I/O die:** 80° / 90° / 100°
- **GPU edge:** 80° / 90° / 100°
- **GPU hotspot:** 95° / 105° / 110°
- **VRAM:** 90° / 100° / 106°

A warning clears once the value is 3° below its limit, so it doesn't flicker. Limits are set in `MetricId::spec` in `src/metrics.rs`.

</details>

## 📐 Any size

- ➖ **Slim strip:** up to about 145 px tall, with one or two lines of key numbers. Controls appear on hover, and you can drag it from anywhere.
- 📏 **Narrow:** under 240 px wide, with a single column of up to 11 rows
- 🔲 **Grid:** 2 to 4 columns, ordered by importance
- 🖥️ **Dashboard:** at least 820 × 600, with 16 tiles and a bar for every CPU thread

![Vitals at a large size: sixteen tiles with history lines, and a load bar for each of the sixteen CPU threads](docs/images/dashboard.png)

<p align="center">
  <img src="docs/images/strip.png" width="320" alt="Vitals as a slim strip, showing CPU, GPU and hotspot temperatures with RAM and VRAM use on one line">
</p>

## 🔥 Busy apps

The list appears along the bottom of the window only when it's needed. An app is listed once it uses:

- 🧮 **CPU:** 25% of the whole CPU
- 🎮 **GPU:** 25% load
- 🧠 **Memory:** 20% of your RAM
- 🎞️ **Video memory:** 25% of your VRAM
- 🥶 **Not responding:** always, however little it's using

To keep the window calm:

- ⏱️ An app must hold a limit for 5 seconds to appear, and leaves 10 seconds after dropping back
- 🧩 Processes are grouped by exe, so a browser counts as one app
- 📋 Rows keep their order, so End can't shift under your pointer. At most 3 show, or 6 in a wide window.

### 🛑 Ending an app

Hover a row to reveal **End**, then confirm. Keep, Esc or clicking elsewhere cancels.

- 💥 **End is immediate:** every process of that app stops at once, and unsaved work is lost
- 🥶 **Frozen apps** show End straight away, with their CPU use. Windows also says "not responding" when an app is only busy, so one still using CPU may recover if you wait, while 0% suggests it's stuck.
- 🔐 **Apps running as administrator** can't be ended, and the row says "Needs admin to end"
- 🛡️ **Windows' own processes** never offer End, because ending one could take down the desktop

## 🌡️ CPU temperature

Windows has no standard API for this, so Vitals reads what AMD provides:

- 🟡 **Without admin rights:** the tile shows the I/O die, labelled "CPU I/O die". This is not a core temperature.
- 🟢 **Click that tile** for the true core temperature, labelled "CPU temp". This starts a small helper with admin rights. Windows normally shows a UAC prompt, but if prompts are turned off it happens silently, so the click is the only consent.
- 🔁 **It repeats at each launch** until a prompt is declined. To stop it, set `core_temp=0` in `%APPDATA%\Vitals\settings.ini`.
- ⚠️ **It relies on an undocumented AMD export** in `Platform.dll`, worked out on a 9800X3D, so an AMD Software update could change it. Every field is range-checked, so the tile would stay on the I/O die rather than show a wrong number.

## 🔒 Security

- 👤 The main app never runs elevated
- 📡 The helper only sends one reading per second down a one-way pipe, and exits as soon as the pipe closes
- 📁 It loads AMD's DLL by full path from Program Files, which only administrators can write to
- 🧱 The exe resolves its DLLs from System32 only, so a malicious DLL sitting in Downloads can't run inside it

## 🖥️ Hardware

- ✅ Built and tested on a Ryzen 7 9800X3D with a Radeon RX 7900 XTX
- 🌐 Load, memory, disk and network work on any PC
- 🔴 GPU sensors need an AMD GPU. On other hardware those tiles show a dash.

## 🛠️ Building

Needs Rust 1.85 or later and the MSVC toolchain with the Windows SDK.

```bash
cargo build --release
```

- 📦 The exe is written to `target/release/vitals.exe`
- 🧪 Run the tests with `cargo test`
- 🩹 If the linker reports `LNK1104`, the project path is too long. Build from a shorter path, or set `CARGO_TARGET_DIR` to one.
- 🏷️ Pushing a version tag runs [the release workflow](.github/workflows/release.yml). Changes are listed in [CHANGELOG.md](CHANGELOG.md).

## 📄 License

Released under the [MIT License](LICENSE).
