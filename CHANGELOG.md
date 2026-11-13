# Changelog

Notable changes to Vitals are listed here. Version numbers follow
[Semantic Versioning](https://semver.org).

## [Unreleased]

### Added

- Search on the Apps tab. Typing anywhere on the tab filters the list to the
  apps whose name or exe name contains the text, and marks the match in each
  name. The field says how many apps match, and has a button that clears it.
  Ctrl+F or a click puts you in the field, which edits like any text box:
  Ctrl+A, Shift with the arrows, a drag or a double-click selects, and Ctrl+C,
  X and V copy, cut and paste. Esc clears it.

## [0.6.0] - 2026-11-11

### Changed

- A new look. The CPU, GPU edge and hotspot temperatures lead as big numbers
  in one framed card, over a chart of the last 2 minutes on a fixed scale with
  the 110° limit marked. A card of CPU, GPU and memory use, details, network
  and disk follows. Each part keeps one colour everywhere, text is set in
  Segoe UI Variable on Windows 11, and the tabs sit in a segmented control.
- Warnings follow one ladder everywhere: Warm adds an amber chip, Hot turns the
  number orange and frames the card, and Critical puts a red tile behind the
  reading.
- A GPU held at its temperature limit, such as a hotspot at 110°, shows an
  outlined At limit chip instead of turning red, because cards are built to run
  there under full load. A CPU at its limit stays Critical.
- A large window has a chart with an axis, a label at each line's end and a
  mark from when the hotspot reached its limit, six readings with history
  beside it, and cards for what needs attention, the GPU and the CPU. Needs
  attention lists everything that's warm, hot or at a limit in plain words, or
  the session's peaks when nothing is. It replaces the Limits row.
- A smaller window names the worst thing under its chart, and +N more opens the
  large window.
- The strip has fixed slots that never move: CPU, GPU edge, hotspot and GPU
  load, with more as it widens. A slot is underlined for Warm, dashed at a
  limit, outlined for Hot and filled red for Critical. A temperature without a
  slot, such as VRAM, gets the last one while it needs attention.
- Simple mode gives up the chart and history lines. In a large window its
  numbers grow to fill the room.

### Removed

- The 2-minute averages, and the shading between the hotspot and edge lines.
  The gap itself is still shown.

## [0.5.0] - 2026-11-10

### Added

- Hotspot gap: on AMD GPUs at 80% load or more, the hotspot tile shows how far
  it runs above the edge temperature. Once the gap holds at 30° or more for a
  minute at 90% load, the tile says Wide gap, and a large window shades the
  band between the two lines.
- At limit: a tile says when the GPU or CPU has been held at its temperature
  or power limit for 5 seconds, and for how much of the last 2 minutes. A
  large window adds a Limits row. The CPU's limits need the core temperature
  helper, and NVIDIA cards report their own reasons.
- Start with Windows, in the menu. It respects the switch in Task Manager's
  Startup apps, and doesn't start the core temperature helper at sign-in, so
  signing in never opens on a UAC prompt.
- Sound on Critical, in the menu and off by default: the Windows warning sound
  and a flash of the taskbar button when a reading turns Critical or an app
  stops responding, at most once every 10 minutes for each.

### Changed

- A warning no longer takes over the top of the window, so the tabs always
  show, even at the Medium size. It still reaches the taskbar.

## [0.4.0] - 2026-11-07

### Added

- A Specs tab that lists the CPU, GPU, motherboard, memory, storage, displays
  and Windows version. Copy specs, or Ctrl+C, puts them on the clipboard as
  plain lines ready to paste into a chat or a forum post. Laptops and prebuilt
  PCs show their model instead of a board code.
- NVIDIA GPU support: temperature, load, power, clock, fan speed and video
  memory, read through the library that comes with the NVIDIA driver.

### Changed

- A reading this PC can't report at all no longer leaves a tile that only ever
  shows a dash. Another reading takes its place: GPU clock where an NVIDIA card
  has no hotspot sensor, for example, or CPU clock where an Intel CPU's
  temperature can't be read. The strip and the Apps summary leave the hotspot
  out on such GPUs, and show CPU load in place of CPU temperature on Intel.
- The title bar uses slightly smaller buttons below 280 wide, so that all
  three tabs fit.

### Fixed

- CPU load, CPU clock, disk and network no longer all go blank on PCs where
  Windows is missing some of its performance counters. Disk and network are
  then read straight from Windows instead.
- The core temperature prompt is no longer shown on Intel CPUs, where the
  helper it starts has nothing to read.

## [0.3.0] - 2026-11-07

### Added

- An Apps tab with every app in a table, showing CPU, memory, GPU, video memory
  and disk. Click a heading to sort, and hover a row to end that app. Clicking a
  row in the Busy list opens the table.
- Simple and Advanced modes. Simple shows just the numbers. Advanced adds a peak
  on every tile, the 2-minute average and history.
- A menu beside the pin with Small, Medium and Large window sizes, your own size,
  and the switch between Simple and Advanced.

### Changed

- New windows open at the Medium size, 340 × 520.
- Clicking any tile now resets its peak, since every tile has one.

## [0.2.0] - 2026-11-06

### Added

- A Busy list along the bottom of the window, showing apps that are using a lot
  of CPU, GPU, memory or video memory. It only appears when needed.
- Apps that have stopped responding are always listed, and reach the taskbar text.
  Their CPU use is shown too, as a sign of whether they are still working.
- End an app from its row, with a confirmation first.

### Fixed

- On short tiles, the memory bar no longer runs through the number.

## [0.1.0] - 2026-11-06

First public release.

### Added

- CPU and GPU temperatures, load, power, memory, disk and network, updated once
  a second with two minutes of history.
- A window that adapts from a slim strip to a full dashboard, with a minimum size
  of 190 × 36.
- Warm, Hot and Critical states. A temperature that needs attention also appears
  in the title bar and the taskbar.
- A session peak for each temperature. Click a tile to reset its peak.
- True CPU core temperature on supported AMD systems, through an optional
  elevated helper.
- Light and dark themes that follow Windows, and an always-on-top pin.
