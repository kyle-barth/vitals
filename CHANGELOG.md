# Changelog

Notable changes to Vitals are listed here. Version numbers follow
[Semantic Versioning](https://semver.org).

## [Unreleased]

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
