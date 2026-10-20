# WayVR XS roadmap

## Current checkpoint

Independent movement, interactive app selection/switching, dropdown alignment,
and layout persistence across closing/updating/reopening are headset-confirmed.
Compact interactive-panel controls and the wrist PC performance dropdown are
headset-confirmed. The latest combined build is reported working, including
layouts and restarting. The Floating shelf wrist base is headset-confirmed. The integrated PC performance shelf and charcoal theme are headset-confirmed.
On 2026-10-20 the user confirmed Hide / Restore returns all four panels with
correct positions and interaction, text and scrolling look fine, and the backup /
restore round trip works. Their workspace uses two shared-capture main-monitor
splits, one interactive app and the secondary monitor. Reported whole-system
usage was about 3 GB VRAM and 2% CPU; GPU utilization fell from 25% to 15% when
hidden, with CPU unchanged. These are observations on one setup, not per-process
benchmarks or end-to-end latency measurements.

## Floating shelf wrist design

The chosen compact design uses an expandable action shelf above a stationary
clock/control base, with a compact device battery strip below. Keep existing
actions, layout presets, performance monitoring, panel/set toggles and runtime
battery sources. Show percentages, charging marks and low-battery states only
for devices reporting battery data. Use the current theme; reference colors are
not requirements. This UI batch does not change the three priorities below.

## Next three priorities

1. **Quick controls for regular screen panels:** implemented for mono desktop
   screens and split regions using the shared actions. Await combined headset
   acceptance with the anchored layout dropdown correction.
2. **Readability and performance:** native capture resolution and shared/hidden
   capture behavior retained; redundant texture rebinding removed; opt-in capture
   delivery/CPU import statistics and a repeatable headset comparison added in
   `PERFORMANCE.md`. Text and scrolling are user-confirmed. Hiding reduced
   whole-device GPU utilization by 10 percentage points on the tested workspace.
   End-to-end latency and WayVR-only GPU cost remain unmeasured.
3. **Installation and recovery:** implemented `wayvr-xs-support` doctor, private
   backups, staged restore with previous-settings preservation, and local
   diagnostic/log collection. Arch packaging installs the helper and guidance;
   recovery round-trip and failure rollback have automated tests. The user also
   confirmed a real backup/restore round trip.

All six CI workflows pass for source `0aa067629ef87bcc298b5eb8c691bd37c8fc7ee3`.
The verified Arch package is `26.8.0.r1720.g0aa06762-1`. Preserve these observations
when reviewing release readiness; individual regular-screen control buttons and
the inline layout dropdown have not been separately reconfirmed in the latest
session. Continue fixing reported bugs before more aggressive capture changes.

## Later updates

- Skins and appearance customization after the compact layout is established.
- Placement tools: alignment, matching sizes, edge snapping, curved rows and undo
  for accidental moves.
- Expanded wrist actions and layout switching, plus configurable controller
  shortcuts for panels, controls and presets.
- Further layout-preset, app-reconnection and performance-dashboard enhancements
  after acceptance of the current implementations. Sensor availability remains
  driver-dependent.
- Research into background/obscured/minimized app interaction and support for
  other desktop environments. Keep the current KDE Wayland path dependable first.

## Release readiness

After the three priorities, run the combined headset regression checklist,
document limitations and prepare a versioned release and draft PR review.
Later feature additions can ship through updates. Do not merge the draft
automatically.

Ship small batches with one combined headset test per batch. Preserve the portal
edit-wrapper fix and use system shaderc for Arch builds.
