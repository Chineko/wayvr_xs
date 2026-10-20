# WayVR XS roadmap

## Current checkpoint

Independent movement, interactive app selection/switching, dropdown alignment,
and layout persistence across closing/updating/reopening are headset-confirmed.
Compact interactive-panel controls and the wrist PC performance dropdown are
headset-confirmed. The compact bar-dashboard redesign awaits headset checking.
The layout manager now responds; the load/restart correction awaits testing;
app reconnection polish still awaits headset acceptance.

## Next three priorities

1. **Quick controls for regular screen panels:** bring the compact controls drawer
   to regular screen panels, reusing existing edit controls and shared actions.
2. **Readability and performance:** measure text clarity, capture latency and GPU
   cost; improve idle and hidden-panel behavior based on those measurements.
3. **Installation and recovery:** first-run guidance, configuration backups and
   restoration, update information and troubleshooting logs.

These are the next implementation targets. Continue checking the features already
implemented and fixing reported bugs alongside these batches.

## Later updates

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
