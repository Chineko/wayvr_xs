# Windows / SteamVR port

**Current Windows foundation: Desktop+ 3.6.** The user confirmed both upstream
capture backends work without issues on the 5120x1440 monitor. Active Windows
development now uses the native Desktop+ build in [desktopplus/](../desktopplus/README.md),
with XS layout controls and presets. Windows CI builds that target only.
The Rust prototype and the notes below are retained as historical reference.
The Linux WayVR branches remain separate.

Branch: `feat/windows-steamvr`. Base: published tag `xs-v0.1.1`, commit
`a24ca4257ca202578d28ece66ae84eaa0268be48`.

**Status: first native Windows executable implemented; capture and headset
acceptance are still pending.** This is a view-only prototype, not feature parity
with the Linux app. See [Windows build/run instructions](../windows/README.md).
The existing Arch packages remain Linux builds.

The target is a native Windows SteamVR overlay with the same 5120×1440 ultrawide
splits, quick controls, independent movement, saved placements, window mirrors
and wrist media experience. The Linux feature and release branches are separate
from this work. This branch has only a Windows build workflow; Linux and Arch
workflows remain on their Linux branches. It is not intended to merge into the
Linux branch. User reports the Linux version runs well.

## Repository review

`main` still contains upstream WayVR. The newer XS functionality is on
`feat/xs-ultrawide-panels` and the `release/xs-v0.1.x` branches. Do not start a
Windows port from `main` or the older October 18 handover checkpoint.

The reviewed XS snapshot includes quick panel controls, independent anchored
movement, view-only app-window mirrors, recovery tools, wrist media controls,
Spotify/YouTube progress, and versioned Arch releases. Window mirrors currently
do not support controller input or automatic window restoration. Keep those
limitations explicit rather than treating them as existing portable features.

The reusable parts include region math, layout/placement logic, UI assets,
controller interactions and much of the OpenVR overlay code. The desktop and OS
services need native backends; changing Cargo flags alone cannot port the app.

| Area | Current coupling | Windows direction |
| --- | --- | --- |
| VR presentation | Existing OpenVR backend in `wayvr/src/backend/openvr/` | SteamVR/OpenVR overlay application; preserve alongside-game overlays |
| Desktop capture | `wlx-capture`, PipeWire/portal, Wayland/X11 | DXGI Desktop Duplication for full monitors; evaluate Windows.Graphics.Capture for windows |
| Graphics transfer | Vulkan renderer; DMA-BUF and Unix file descriptors in `wgui/src/gfx/{raw,dmabuf}.rs` | Separate Linux imports; implement a measured D3D11/Vulkan interop path or a Windows capture presentation path; do not assume shared handles work automatically |
| Input | libinput, evdev, uinput and Wayland devices | DPI-aware physical coordinates plus SendInput; virtual desktop bounds and negative origins matter |
| Embedded apps | Smithay compositor, Xwayland, Linux process management | Platform-gate the embedded Linux compositor; Windows app-window capture is a separate native feature |
| Media | MPRIS/D-Bus in `wayvr/src/overlays/media.rs` | Windows GlobalSystemMediaTransportControlsSessionManager with session-targeted actions |
| IPC | UnixStream/OwnedFd in `wayvr-ipc/src/client.rs` | Named pipes or the Windows local-socket support of the IPC library; remove FD transfer assumptions |
| Configuration | XDG paths and `/tmp/wayvr` fallback in `wlx-common/src/config_io.rs` | Windows known folders; separate Windows settings location and compatibility-conscious schema |
| Process lifecycle | Unix signals and `/proc` in `wayvr/src/main.rs` and `backend/wayvr/` | Native shutdown/single-instance/process APIs |
| Native dependencies | Unconditional Linux dependencies in manifests | Target-specific dependencies/modules, not pretend-success stubs |

`openxr` also pulls in libmonado. SteamVR is the initial Windows runtime; Windows
OpenXR support and Meta-only runtime support are not part of the first milestone.

## Foundation implemented on this branch

- `xs-core` is a dependency-free Rust crate in its own workspace. Windows tests
  do not resolve or compile the Linux application dependency graph.
- The original `ScreenRegion` implementation and its tests moved unchanged into
  `xs-core/src/screen_region.rs`; `wlx-common::screen_region` re-exports it so Linux
  callers retain the same public path and behavior.
- `DesktopRect` maps panel UV into physical monitor pixels, respecting negative
  origins and keeping edge clicks inside the selected region. It also normalizes
  physical pixels for virtual-desktop absolute input.
- Invalid geometry, non-finite/out-of-range UV and empty monitors are rejected.
- These coordinate helpers are not yet connected to a Windows capture/input API.
  Callers must establish per-monitor DPI awareness and supply physical bounds.
- `windows/` now contains an independent native x64 application: DXGI/D3D11
  monitor capture, a dynamically loaded SteamVR overlay backend, shared-texture
  split presets, headset-yaw placement, console adjustment commands and recovery.
  Controller input, saved profiles, wrist UI, window capture and cursor composition
  remain future work. The Linux UI/rendering stack is not built on Windows.
- `.github/workflows/check-windows-core.yml` builds and packages the native Windows
  executable, checks formatting/Clippy and runs both Windows and shared-core tests.
  It runs on Windows only. See `windows/README.md` for hardware test limitations.

Run from the repository root:

```powershell
cargo fmt --manifest-path xs-core/Cargo.toml -- --check
cargo test --locked --manifest-path xs-core/Cargo.toml
cargo clippy --locked --manifest-path xs-core/Cargo.toml --all-targets -- -D warnings
```

There are eight tests: four existing split/UV geometry tests and four physical
monitor/absolute-input tests. Tests need no capture permission, headset, GPU,
SteamVR installation or Linux native libraries.

## Implementation sequence and acceptance gates

The first implementation uses a separate native application rather than
compiling the embedded Linux compositor or its UI stack. The native release build
and unit tests pass locally; real monitor enumeration found the 5120×1440 display.
DXGI capture returned access denied in the restricted development session. The
following hardware gates remain unverified.

1. **Platform separation and parity.** Split target-specific manifests
   and modules. Port shared configuration/IPC boundaries and the renderer's
   platform-independent path. Keep unavailable features visibly unavailable;
   do not silently disable arbitrary behavior behind success-returning stubs.
2. **One monitor in SteamVR (implemented, needs hardware acceptance).** Enumerate physical monitors, capture one using a
   D3D11 device on the correct GPU, present it through a real SteamVR overlay,
   and handle lock/unlock, mode changes, device loss and clean shutdown.
   Gate: moving desktop is visible while a VR scene application is running.
3. **Shared-capture ultrawide splits (implemented, needs hardware acceptance).** Reuse the region math and preset boundary
   rules. Create independent overlay views from one capture and preserve physical
   aspect ratio. Gate: 5120×1440 halves, thirds and wide-center work; hiding one
   sibling does not stop the others; texture ownership stays valid until SteamVR
   finishes consuming it. Profile frame time and bandwidth, not just compilation.
4. **Input and positioning.** Console positioning is implemented; controller input
   and grabbing are pending. Connect coordinate helpers to SendInput, preserve
   virtual-desktop origins, handle mixed DPI and release held buttons on loss of
   focus/disconnect. Port controller grabbing and existing quick controls. Gate:
   clicks/scroll/drags hit the intended panel, including seams and edge pixels.
   Input to elevated applications is restricted by Windows integrity rules;
   do not promise universal input or require running the app elevated by default.
5. **Persistence, wrist UI and native app services.** Save/restore Windows monitor
   identities and panel placement. Port media sessions and artwork/progress,
   clipboard/keyboard and app-window selection with honest capabilities.
6. **Distributable Windows build.** Native executable CI and a ZIP are implemented;
   clean-machine and hardware acceptance remain. Package only
   required redistributable runtime files, and verify startup on a clean machine.
   Do not label core unit-test artifacts as a Windows app release.

First hardware session should test the monitor/capture/SteamVR loop before
attempting feature parity. Batch coherent changes; the user prefers fewer
headset restarts. The Linux release is not to be replaced or republished as part
of starting the Windows branch.

## API references

- [Valve IVROverlay overview](https://github.com/ValveSoftware/openvr/wiki/IVROverlay_Overview)
- [Microsoft Desktop Duplication API](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api)
- [Windows screen capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)
- [SendInput and integrity restrictions](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- [Windows media sessions](https://learn.microsoft.com/en-us/uwp/api/windows.media.control.globalsystemmediatransportcontrolssessionmanager)

## Windows 0.1.1 capture fixes

The first hardware report returned `DXGI_FORMAT(10)` and repeatedly reconnected.
The capture backend now accepts RGBA16F/scRGB and keeps that floating-point
format through OpenVR submission with linear color space. BGRA/RGBA8 retain
gamma submission. HDR tone mapping/fidelity remains unvalidated on the headset.
Permanent format errors now exit rather than entering device recovery; actual
access/device/desktop loss and size/format changes still reconnect.

Monitor listing now includes the current Windows pixel mode. Optional
`--expect-size 5120x1440` guards both initial active mode and captured surface
size. The app does not force display modes or assume that streaming software has
kept the physical monitor's native resolution. See `windows/README.md`.

## Windows 0.1.2 live surface transitions

User confirmed full-screen 5120x1440 capture, then reported repeated overlay
recovery on frame size/format changes (BGRA8 and RGBA16F in the log). Those changes
now replace the application-owned texture inside the existing duplication
session. The renderer clears all old OpenVR texture references before releasing
or reusing the previous texture. A bounded two-texture cache handles alternating
SDR/HDR frames without repeated allocation. Actual device/access/desktop loss
still uses recovery.

Tests cover 100 SDR/HDR transitions, resize/lifetime ordering, and preservation
of the published texture if allocation fails. There are nine Windows unit tests
plus the existing eight shared-core tests. Native build and Clippy pass; this
latest overlay change still requires headset confirmation.
