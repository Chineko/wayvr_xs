# WayVR XS: 49-inch ultrawide desktop panels

This fork adds desktop splitting to WayVR for Arch Linux, Wayland and WiVRn
(OpenXR). The first target is a 5120 x 1440 monitor.

## Use the split controls

1. Start WiVRn and connect your headset. Launch this build of WayVR.
2. Share the physical ultrawide desktop when the desktop portal asks.
3. Enter WayVR's edit mode and point at the desktop overlay.
4. Open **Split desktop** (the screen icon).
5. Choose **Two halves**, **Three columns**, or **Wide center**.

| Preset | Desktop regions at 5120 x 1440 |
| --- | --- |
| Two halves | 2560 x 1440 + 2560 x 1440 |
| Three columns | 1706 x 1440 + 1707 x 1440 + 1707 x 1440 |
| Wide center | 1280 x 1440 + 2560 x 1440 + 1280 x 1440 |

The panels initially form a row in front of you. Grab and position each one,
or use the existing pinning, curvature, opacity and input-lock controls.
Leave edit mode to save their positions. Open **Split desktop** on any panel
to choose a different preset or **Full desktop** to restore the original.

All panels share one capture session. Splitting does not create extra monitor
capture requests or change the desktop resolution. Mouse input maps back into
each panel's region of the physical desktop, including its logical desktop
offset and scale. These are cropped views of your existing desktop: applications
still need to be placed in the desired desktop regions by your window manager.

The layout choice is saved in `conf.d/zz-saved-state.json5`, alongside WayVR's
existing saved overlay sets. Startup recreates the panels after the original
capture source becomes available. A missing monitor is left in the saved
configuration so its panels can be restored when that source is available again.
Stereo-packed desktop sources must be switched to **None** before splitting.
Window mirrors retain their existing behavior.

## Quick panel controls

Edit mode opens on **Quick panel controls** (resize icon). For the pointed panel:

- **Width −5% / +5%** changes its physical size while retaining aspect ratio,
  capture region, position and orientation. Width is limited to 0.1–20 metres.
- **Nearer / Farther** moves it 10 cm along the line from your headset, keeping
  its size and orientation. Distance is limited to 0.25–20 metres.
- **Recenter panel** brings just this panel directly in front of your current
  headset pose, retaining its distance and size and matching your head orientation.
- **Opacity** and **Curvature** use the existing sliders, now together here.
- The existing **Disable grab** toolbar button locks placement against grabbing.
  **Positioning** still offers Static, Anchored, Floating and head/hand following.
  Quick adjustments preserve that mode and update its saved relative transform.
- The fade icon retains additive blending and angle fade options.

Switch back to the resize icon after using other tabs. Changes use the existing
saved-state path; leave edit mode to save as usual. Adjustments affect only the
selected panel, so siblings keep their positions.

For one headset session, check the portal picker while editing, then all split
presets and pointer seams. On one panel, change width, distance, opacity and
curvature; recenter while looking sideways; verify placement locking and each
positioning mode. Confirm siblings remain unchanged, then restart once and check
restoration and WiVRn reconnect. These controls still need hardware validation.

## Move screens and keyboard independently

Open **Settings → Controls** and enable **Independent panel movement**.
Normal grabs now move only the selected screen, split panel or keyboard, including
items using Anchored positioning. The setting applies to all anchored overlays.
It takes effect on your next grab and is saved through the normal settings path.
No restart or conversion of existing positioning modes is required.

Disable it to restore grouped workspace grabbing. Neither toggling direction
repositions panels. Existing grab locks, head/hand following, edit-mode grabs,
and the middle-click individual-grab modifier retain their behavior. Workspace
sets and explicit workspace recenter actions still operate on the workspace.

In the same headset session, enable the toggle and separately move two screens
and the keyboard; verify their siblings stay still. Restart once to check the
setting and positions persist. Disable it and verify anchored overlays move
together again. Also test an item with Disable grab enabled.

## Show an individual app window

On KDE Wayland, open Browser, Discord or the app you want to see. In WayVR,
press the **mirror +** button on the watch, or open the keyboard menu and choose
**Add app window (view-only)**. KDE's desktop portal opens a window-only picker;
select the desired app window and confirm sharing. Repeat for additional windows.
The existing **Add a new mirror overlay** entry remains available as before.

Each selected window gets its own movable mirror overlay. Use the same edit-mode
width, distance, recenter, opacity and curvature controls, and enable Independent
panel movement to grab it separately from other anchored overlays. Hide/show
mirrors with their watch buttons; remove them using the existing edit-mode delete
control. This captures one window, rather than cropping its position on the desktop.

These mirrors are **view-only**: controller clicking, scrolling and typing into
an arbitrary captured window are not implemented. Use the desktop overlay or
physical mouse/keyboard to interact with the original app. Window mirrors do not
expose desktop splitting or desktop pointer mapping. Window selection is requested
afresh after restarting; automatic app-window restoration is not implemented.
If a portal cannot share windows, this request fails instead of sharing a monitor.
The watch button is hidden outside supported Wayland builds/sessions.

In one headset session, select Browser and Discord separately, move both overlays,
resize the original windows, and verify only the selected content appears. Check
minimizing, closing and reopening an app, cancellation of the picker, and choosing
while edit mode is active. Capture behavior when minimized or closed needs target
KDE validation; a closed stream may retain its last image. Keep desktop split
panels available if you need controller input.

## Build for Arch Linux and WiVRn

For a pacman-managed install, KDE launcher, and prebuilt CI packages, use the
[Arch installation guide](ARCH-INSTALL.md).

Install the native dependencies from the upstream
[building guide](https://wayvr.org/docs/basics/building-from-source/), plus a Rust toolchain.
From this repository's root:

```sh
SHADERC_LIB_DIR=/usr/lib cargo build --release --locked -p wayvr --no-default-features --features wayland,openxr
./target/release/wayvr --openxr --show
```

WiVRn must be the selected OpenXR runtime. If WayVR is already running, stop
that instance before starting this build. Back up `~/.config/wayvr` before
testing an experimental fork with an existing setup.

## Validation

The integer boundary and pointer-coordinate tests can also run without the
VR runtime:

```sh
rustc --test wlx-common/src/screen_region.rs -o /tmp/wayvr-xs-regions
/tmp/wayvr-xs-regions
```

On the target headset, check all three layouts; click, scroll and drag in each
panel; check the seam between panels; hide one panel while using another; hide
and restore the whole set; move and pin panels; restart and verify saved
positions; restore the full desktop. Also test fractional desktop scaling and
a monitor whose logical desktop origin is not zero.

The geometry tests are automated. Headset rendering, capture performance and
layout restoration still require validation on Arch Linux with WiVRn.
