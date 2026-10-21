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

## Interact with an existing KDE app window (experimental)

On KDE Wayland, open the watch's **mouse +** button or choose **Add interactive
app (KDE, foreground)** in the keyboard menu. Choose Browser, Discord or another app from the list
inside VR. The list includes all monitors and shows each window’s monitor name;
use Previous/Next to browse additional pages, or Refresh to include newly opened apps.
This is a separate option from the tested view-only portal mirrors.

The selected KWin window ID is used for both capture and input. Click the new VR
panel to bring the original app to the foreground and interact. Left/right/middle
click modes, scrolling, and the existing virtual keyboard are reused. Click the
app panel successfully before typing with the virtual keyboard. Existing quick
panel controls and Independent panel movement apply to placement in VR.

This first version uses the physical desktop pointer. It requires the original
window to be visible and foreground; it cannot click a covered app in the
background. Unrelated windows covering the point block input. Minimized windows
must be restored on the desktop. Input pauses while the original window is being
moved/resized or while the capture dimensions no longer match its current bounds.
Menus/popups belonging to the app are included in coordinate mapping. A separate
modal dialog may need its own interactive overlay.

Only one controller owns a held mouse button at a time. Releasing the trigger,
leaving the panel, hiding/removing it, losing focus or disconnecting the bridge
releases held input. Drags are supported while the pointer remains on the panel;
leaving it ends the drag. Keyboard input follows the desktop's actual focused app.
Multiple app pickers can be opened before selecting their windows.

### Reuse an app panel

Click the **app-name ▾** bar underneath the interactive window to open its app
chooser. The current capture stays visible while choosing. Select an app from
either monitor; use Refresh if it was opened after the list appeared. Click the
bar again to cancel and keep the current app.

Switching preserves placement, positioning mode, physical width, opacity and
curvature. Height follows the selected app's aspect ratio. The chooser expands
below the content without moving the app's center. Existing edit controls and
Independent panel movement remain available. While the chooser is open, app
pointer input is suspended; moving a held pointer into the controls releases its
mouse button. Switching clears pending input before promoting the new capture.
Click the new app once before typing with the virtual keyboard, as usual.

### Restore app panels next session

Interactive panels now save their app identity alongside the normal saved
layout. On a normal exit/restart, each panel returns to its saved set, placement,
width, opacity and curvature; hidden panels remain hidden. Closing/removing a
panel removes its saved app definition. New panel names are stable unique IDs.

Only a unique match for the saved app is reconnected automatically. An exact
window title can distinguish multiple windows of the same app; if matching is
ambiguous, the panel offers the chooser instead of guessing. Window UUIDs and
process IDs are never persisted. If the app is not running, open it and use
Refresh/choose a window in the existing panel. This feature does not launch apps.
Panels created with older builds must be created once using this version before
they can be saved. The layout lives in the existing WayVR saved-state file.

The bridge is a temporary KWin script plus KDE's private screencast protocol,
not a permanent installed plugin. These implementation-detail APIs can change;
other desktops should use the view-only portal option. Normal cleanup unloads
the script and removes its runtime file. Closing the panel also cancels selection.

Local verification covers the Rust input state, JavaScript geometry/focus guards,
a regression test opening two pickers before selection and replacing one while
the other remains active, and native KWin capture/validated coordinate replies
before and after moving and resizing a disposable window. App identity matching, hidden-layout serialization,
footer input bounds and fixed-width/center geometry have dedicated checks.
Actual controller clicks, typing, popup alignment,
focus transitions and WiVRn reconnect still need one headset session. Check those
along with hiding or deleting a panel during a held trigger. The existing portal
edit-wrapper fix and system-shaderc build configuration are preserved.

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

### Compact interactive-panel controls

Use **Controls ▾** beside the app selector to open the quick drawer without
entering edit mode. It reuses size ±5%, distance ±10 cm and recenter behavior;
opacity changes by 10% (minimum 10%), curvature by 0.05 (0–0.5).
**Pin / unpin** locks controller grabbing while retaining app/button interaction.
**Hide** keeps the saved panel; show it again from the existing panel list.
The drawer may stay open alongside the app chooser. Opening either drawer keeps
the app center and width fixed. Existing edit controls remain available for
all panels; the compact drawer is currently for interactive app panels.

Headset check: exercise every quick action, pin/unpin and hide/show, verify
pointer alignment with both drawers open, then quit normally and restart to
check size, position, opacity, curvature and the grab lock persist.

### Named layout presets

Open **Layouts** on the wrist panel. Use its letter/number buttons, Space,
Backspace and Clear to enter a name (up to 48 characters), then **Save new**.
Previous/Next select a saved preset. **Rename** applies the entered name;
**Replace** saves the current layout over the selected preset. Replace and
Delete require a second click to confirm; another action cancels confirmation.

**Load & restart** saves the selection and restarts WayVR normally, recreating
captures and using existing app reconnection rules. It does not launch desktop
apps. The snapshot contains all saved sets, hidden states, app identities,
screen splits, positions, sizes, opacity, curvature, grab locks and saved backend
attributes. View-only portal mirrors retain their existing persistence limits.
General settings and controller bindings are not changed by presets.

**Use at startup** loads the selected snapshot on future launches. **Startup:
last session** restores the normal last-session behavior. An explicit load takes
priority once over the startup choice. Deleting the startup preset clears that
choice. Presets are stored in `~/.config/wayvr/layout-presets.json`, outside
`conf.d`, using atomic replacement. Invalid or unsupported files are preserved
and reported instead of overwritten. Loading requires one brief WayVR restart;
it is not an in-place capture switch.

Combined headset check: test the quick controls, save two different arrangements
with one hidden/pinned panel, load each, rename and replace one, select a startup
preset and restart, then switch startup back to last session. Delete a spare
preset and verify it disappears. Recheck app selection and pointer alignment.

### App reconnection and wrist PC monitor

Closing a selected app now keeps its panel size, releases held input and shows
an explicit disconnected placeholder with a fresh replacement list. Use
**Reconnect saved app** after reopening the app to retry its saved identity.
A unique match reconnects; missing/ambiguous matches require an explicit choice.
Identical app titles receive short session tags in the picker; those tags and
window UUIDs are never stored. Refresh updates the list without opening apps.

Use **PC ▾** on the wrist menu to expand a performance view below the watch;
**Close ▴** folds it away. It follows the left wrist and shows CPU utilization,
RAM, up to three GPUs with available utilization/VRAM/temperature, CPU temperature
and WayVR's frame interval. Frame interval is the app loop, not compositor GPU
timing. Unsupported sensors say unavailable. AMD/Intel counters use DRM/sysfs
where exposed; NVIDIA can use optional `nvidia-smi`, queried on the worker with
a 500 ms deadline. No additional monitoring dependency is required.

The panel requests samples once per second only while rendered. The worker
sleeps between requests; hiding/folding it or looking away stops new requests.
An already requested sample may finish. Sensor reads and NVIDIA queries stay
off the render thread. The initial CPU sample warms up before showing load.

Headset check: close an app while holding a drag, confirm release and the
placeholder, reopen it and reconnect, then test two windows with the same app
identity/title and choose explicitly. Check Refresh and pointer alignment.
Open/close the wrist PC view, check CPU/RAM and each GPU against a desktop tool,
and verify it follows the wrist without overlapping the watch buttons.

Use **×** beside Controls to close an interactive panel and forget its saved
definition in the current layout. **Hide** retains it for later use. Closing
the overlay leaves the desktop application running. Saved named presets remain
snapshots; loading an older preset can intentionally restore its saved panel.

The wrist monitor now uses compact green cards with orange live bars: a wide
WayVR FPS/frame-time row above GPU/VRAM/temperature and CPU/RAM/temperature cards.
Use **GPU ▸** to cycle cards; the selected GPU is named above its readings.
NVIDIA is initially preferred when present, otherwise the largest reported VRAM
card. Memory bars show used/total capacity; temperature bars span 0–100 °C and
the frame bar spans 0–33.3 ms. Values remain numeric beside each bar. Unavailable
or warming-up readings show **—** with an empty track. Frame data is app-loop
timing, not compositor GPU timing. Wrist footprint and hidden-panel sampling
behavior are retained.

Preset restart now replaces the WayVR process in place, retains launch options
and waits for the VR runtime if temporarily unavailable. It resolves an updated
installed binary instead of using Linux's old `(deleted)` executable path. The
one-time preset choice is acknowledged after the backend constructs its scene,
so a runtime retry does not lose the selected layout. Restart failures are
logged rather than silently ignored.
