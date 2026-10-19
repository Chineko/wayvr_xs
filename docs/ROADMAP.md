# WayVR XS roadmap

## Current checkpoint

Independent movement, interactive app selection/switching, dropdown alignment,
and layout persistence across closing/updating/reopening are headset-confirmed.
Compact interactive-panel controls are published in
`26.8.0.r1706.gdea408c8-1`; their headset acceptance is next.

## Planned batches

1. Finish quick-controls headset acceptance and address findings. Consider the
   compact drawer for regular screen panels, reusing existing edit controls.
2. Named layout presets: implemented with in-VR naming, save/replace/rename/delete,
   startup selection and explicit Load & restart. Await combined headset acceptance.
3. App reconnection polish: implemented disconnected placeholders, replacement
   lists, explicit saved-app retry and ambiguous-window labels. Await headset test.
4. Placement tools: alignment, matching sizes, edge snapping, curved rows and
   undo for accidental moves.
5. Wrist dropdown and PC performance monitor: initial PC dropdown implemented;
   request-driven CPU/RAM/GPU/VRAM/temperature readings await headset testing.
   Further work: a compact menu attached to the
   wrist for quick panel actions and layout switching, with an expandable PC
   performance view. Investigate CPU/GPU utilization, RAM/VRAM use, temperatures
   and VR frame timing where available. Show unavailable metrics clearly;
   refresh conservatively and suspend monitoring while hidden. Reuse existing
   wrist/watch infrastructure and controls where practical. Final metrics and
   layout will be decided during implementation.
6. Configurable controller shortcuts for panels, controls and presets, integrated
   with the wrist menu.
7. Readability and performance: measure capture latency, GPU cost and text
   clarity; improve idle and hidden-panel behavior based on measurements.
8. Installation and recovery: first-run guidance, configuration backups, update
   information and troubleshooting logs.
9. Stable release: headset regression checklist, documented limitations, draft
   PR review and a versioned release. Do not merge the draft automatically.

Separate research: compositor support for background/obscured/minimized app
interaction, followed by feasibility of other desktop environments. Keep the
current KDE Wayland path dependable first.

Ship small batches with one combined headset test per batch. Preserve the portal
edit-wrapper fix and use system shaderc for Arch builds.
