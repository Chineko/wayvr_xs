# Readability and performance

Screen and app captures retain their native content resolution; the quick-control
footer is extra space, not a downscaled replacement for the capture. Split panels
share the original capture instead of starting one stream per split. Moving,
resizing and curving a panel changes its presentation; it does not create more
source pixels. For clearer text, enlarge the panel or bring it nearer with the
existing quick controls. Increase browser/editor text size if necessary. Test
flat and curved presentation before changing desktop scaling.

Unchanged captures reuse their submitted overlay frame. Hidden regular screen
captures pause when the last visible sibling is hidden; hiding one split does not
pause visible siblings. The performance shelf stops requesting sensor samples
while closed. Screen texture bindings are now reused when a redraw uses the same
image view (for example, a footer change); new views still update the binding.
These changes do not reduce source resolution or introduce a frame-rate cap.

## One combined headset test

Use the same headset refresh rate, WiVRn encoding settings, source resolution,
panel distance/size and application content for each run. Keep other GPU workloads
consistent. Compare the previous package and this one if measuring improvement.

1. Read small browser/editor text on the full ultrawide, a split and an app panel.
   Open controls/app chooser menus: content should stay sharp and pointer aligned.
2. Compare a stationary desktop and continuous scrolling/video for 30 seconds each.
   Record WayVR frame FPS/ms from the wrist monitor and GPU utilization/VRAM from
   the same GPU. These are utilization/frame-loop indicators, not GPU render time.
3. Hide one split, then all overlays. Verify the other split keeps updating, the
   wrist remains accessible, and Restore returns the workspace and live captures.
   Check GPU/CPU cost after a few seconds of settling; WiVRn and the compositor
   continue running, so total GPU use is not expected to fall to zero.
4. Close/reopen the performance drawer. Confirm sensor readings resume; repeat
   app switching, minimize/restore, layout loading and both monitor interactions.

Optional capture diagnostics, starting with WayVR closed:

```sh
WAYVR_CAPTURE_STATS=1 /usr/bin/wayvr-xs --openxr --show --log-to "$HOME/wayvr-xs-performance.log"
```

Each active capture logs delivered frames/sec, average/maximum CPU import time,
and import failures in windows of at least five seconds. No per-frame logging is
added. The normal build collects none of these counters unless the variable is
`1`. Idle sources can deliver fewer frames; after hiding there can be a final
in-flight frame, and the first resumed window can include the hidden interval.
Use subsequent steady-state windows when comparing rates. CPU import time covers
WayVR's capture callback, including any waits in it; it excludes portal queueing,
GPU execution, encoding and headset display latency. It is **not end-to-end capture
latency**. True source-to-headset latency needs external measurement or correlated
compositor/runtime timestamps; no such result is claimed here.

For CPU cost use your system monitor on the WayVR process, and record the same
active/idle/hidden cases. The GPU monitor reports whole-device utilization, not
WayVR-only cost. No headset clarity, latency or GPU saving is considered verified
until this comparison is performed on the real setup.
