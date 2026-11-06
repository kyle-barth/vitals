# Changelog

Notable changes to Vitals are listed here. Version numbers follow
[Semantic Versioning](https://semver.org).

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
