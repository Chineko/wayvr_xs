# WayVR XS roadmap

## Windows / Desktop+ XS next session (2026-10-22)

Work is paused at the user's request. Screen-panel grip grabbing in Desktop+ XS
0.3.1 is now headset-confirmed working.

Requested for the next session:

1. Add a VRAM usage graph to the performance monitor.
2. Disable the default screen and menu bar spawning automatically under SteamVR.
   Inspect the startup/dashboard auto-show behavior when resuming; this is
   separate from the SteamVR application autostart setting, which defaults off.

The remaining sections track the Linux build.

## Current checkpoint

Independent movement, interactive app selection/switching, dropdown alignment,
and layout persistence across closing/updating/reopening are headset-confirmed.
Compact interactive-panel controls and the wrist PC performance dropdown are
headset-confirmed. The latest combined build is reported working, including
layouts and restarting. The Floating shelf wrist base is headset-confirmed. The integrated PC performance shelf and charcoal theme are headset-confirmed.
The user confirmed Hide / Restore, text/scrolling and backup/recovery, followed
by the upstream integration build working correctly. All six CI checks pass for
the upstream test PR #2. The compact wrist media controls, progress bar and elapsed/total time are
headset-confirmed with Spotify and YouTube and ready for the 0.1.1 update; see `WRIST-MEDIA.md`.

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
   `PERFORMANCE.md`. Real text clarity, end-to-end latency and GPU savings still
   require headset measurements; these are not claimed verified.
3. **Installation and recovery:** implemented `wayvr-xs-support` doctor, private
   backups, staged restore with previous-settings preservation, and local
   diagnostic/log collection. Arch packaging installs the helper and guidance;
   recovery round-trip and failure rollback have automated tests.

Implementation for this batch is ready for combined headset acceptance. Continue
checking existing features and fixing reported bugs; use measured results before
more aggressive capture changes.

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
