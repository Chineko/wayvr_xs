# WayVR XS for Windows — first native prototype

This is a separate Windows/SteamVR application on `feat/windows-steamvr`.
It does not replace the Linux application. This milestone implements native
D3D11 desktop capture and world-locked SteamVR panels. **Headset validation is
still required.** It is view-only: controller clicking, controller grabbing,
saved profiles, wrist/media UI, window capture and hardware-cursor composition
are not implemented yet.

## Run

Use Windows 10/11 x64 with a D3D11-capable GPU and current SteamVR. Connect the
selected monitor and headset to the same GPU. Launch SteamVR and connect the
headset, then open PowerShell in the extracted build folder:

```powershell
.\wayvr-xs-windows.exe --list-monitors
.\wayvr-xs-windows.exe --monitor 0 --layout dual
```

Use the index printed for your 5120×1440 monitor. Layouts:

| Layout | 5120×1440 regions |
| --- | --- |
| `full` | 5120×1440 |
| `dual` (default) | 2560×1440 + 2560×1440 |
| `thirds` | 1706×1440 + 1707×1440 + 1707×1440 |
| `center` | 1280×1440 + 2560×1440 + 1280×1440 |

Defaults: panel height 0.7 meters, distance 1.5 meters, maximum 60 capture
updates/sec. Panels keep their captured aspect ratio and are initially arranged
side by side in front of a snapshot of headset yaw. They stay in the world when
you turn your head. The app waits for valid headset tracking before showing them.

```powershell
.\wayvr-xs-windows.exe --monitor 0 --layout center --height 0.6 --distance 1.4 --fps 60
```

Type commands in the running console and press Enter; no restart is required:

```text
layout thirds
layout center
recenter
height 0.8
distance 1.6
move 1 -0.1 0.2 0
hide 1
show 1
hide
show
quit
```

`move <panel> <right> <up> <away>` sets an individual panel's offset in meters
from its initial slot, relative to the captured headset yaw. Panel numbers start
at 1. Use `hide 1`/`show 1` for one panel; `hide`/`show` affect all panels.
Layout changes reset offsets and show all panels. Settings are session-only for now.
Ctrl+C also exits. Exit this app before starting a second instance.

SteamVR's loader is found through `%LOCALAPPDATA%\openvr\openvrpaths.vrpath`.
For a custom SteamVR installation, pass an absolute path:

```powershell
.\wayvr-xs-windows.exe --openvr-dll 'D:\SteamLibrary\steamapps\common\SteamVR\bin\win64\openvr_api.dll'
```

No Valve DLL is bundled. If Windows reports a missing `VCRUNTIME`/`MSVCP` DLL,
install Microsoft's current x64 Visual C++ Redistributable.

## Capture diagnostic

This performs real GPU capture without opening a SteamVR session. It prints
dimensions and the count of successfully copied desktop updates; it does not
save screenshots or send pixels anywhere. Static desktops may produce few updates.

```powershell
.\wayvr-xs-windows.exe --monitor 0 --capture-test 5
```

`Access denied (0x80070005)` can occur in a restricted session, on a locked/secure
desktop, or when capture access is unavailable. Run in a normal unlocked desktop
session. Administrator privileges are not a default requirement. Rotated
monitors and cross-GPU capture are explicitly rejected in this prototype.
HDR color fidelity has not been validated; the initial path uses DXGI's BGRA8
desktop image. The hardware cursor may be absent when Windows supplies it as a
separate pointer shape.

On desktop duplication interruption, the app hides and clears the overlays,
releases capture, and retries the same display name once per second. It does not
silently switch to another monitor. Capture uses one app-owned shared GPU texture
for every split. GPU copies complete before releasing DXGI's borrowed frame.

## Build from source

Install Rust stable with the `x86_64-pc-windows-msvc` toolchain and Visual Studio
Build Tools with **Desktop development with C++** and a Windows SDK. From the
repository root on the Windows branch:

```powershell
cargo build --release --locked --manifest-path windows/Cargo.toml
.\windows\target\release\wayvr-xs-windows.exe --list-monitors
.\windows\target\release\wayvr-xs-windows.exe --monitor 0 --layout dual
```

`windows/` has its own workspace and lockfile. Linux libraries and a Linux
toolchain are not needed. CI runs on Windows only, builds this executable, and
uploads a ZIP with these instructions and license notices.

## First headset acceptance session

Check that a moving desktop is visible alongside a SteamVR game, the four layouts
have the expected aspect ratios/seams, and hide/show/recenter work without a
restart. Test individual `move` offsets, quit and restart, then lock/unlock or
change display mode and check recovery. Confirm capture frame rate and GPU use
at 5120×1440. Automated build/tests cannot establish visual quality, compositor
synchronization, hardware recovery or acceptable frame timing.

Local validation so far: native x64 release build, formatting, Clippy, two
layout/placement tests, eight shared-core tests, and real monitor enumeration.
Live desktop capture returned `0x80070005` in the restricted development session;
capture and headset rendering have not yet been accepted on hardware.
