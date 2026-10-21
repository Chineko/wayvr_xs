# Windows / SteamVR port

Branch: `feat/windows-steamvr`. Base: published tag `xs-v0.1.1`, commit
`a24ca4257ca202578d28ece66ae84eaa0268be48`.

**Status: port foundation only. There is no runnable Windows VR application or
Windows installer yet.** The existing Arch packages remain Linux builds.

The target is a native Windows SteamVR overlay with the same 5120×1440 ultrawide
splits, quick controls, independent movement, saved placements, window mirrors
and wrist media experience. The Linux feature and release branches are separate
from this work. User reports the Linux version runs well.

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
- `.github/workflows/check-windows-core.yml` checks formatting, tests and Clippy
  on native Windows and Ubuntu. It does **not** build the full VR application.

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

1. **Make the application build for Windows.** Split target-specific manifests
   and modules. Port shared configuration/IPC boundaries and the renderer's
   platform-independent path. Keep unavailable features visibly unavailable;
   do not silently disable arbitrary behavior behind success-returning stubs.
2. **One monitor in SteamVR.** Enumerate physical monitors, capture one using a
   D3D11 device on the correct GPU, present it through a real SteamVR overlay,
   and handle lock/unlock, mode changes, device loss and clean shutdown.
   Gate: moving desktop is visible while a VR scene application is running.
3. **Shared-capture ultrawide splits.** Reuse the region math and preset boundary
   rules. Create independent overlay views from one capture and preserve physical
   aspect ratio. Gate: 5120×1440 halves, thirds and wide-center work; hiding one
   sibling does not stop the others; texture ownership stays valid until SteamVR
   finishes consuming it. Profile frame time and bandwidth, not just compilation.
4. **Input and positioning.** Connect coordinate helpers to SendInput, preserve
   virtual-desktop origins, handle mixed DPI and release held buttons on loss of
   focus/disconnect. Port controller grabbing and existing quick controls. Gate:
   clicks/scroll/drags hit the intended panel, including seams and edge pixels.
   Input to elevated applications is restricted by Windows integrity rules;
   do not promise universal input or require running the app elevated by default.
5. **Persistence, wrist UI and native app services.** Save/restore Windows monitor
   identities and panel placement. Port media sessions and artwork/progress,
   clipboard/keyboard and app-window selection with honest capabilities.
6. **Distributable Windows build.** Add full-application Windows CI, package only
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
