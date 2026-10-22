# Desktop+ XS 0.2.0

Windows now uses Desktop+ 3.6 as its foundation. The user confirmed that both
unmodified capture backends work on their 5120x1440 display without issues.
The Rust Windows prototype is retained as a reference; current development
uses Desktop+'s native capture, rendering, input and controller systems.
Linux WayVR development remains separate.

## Run

1. Exit the baseline Desktop+ copy and the Rust WayVR Windows prototype.
   Keep SteamVR and Virtual Desktop running, with the headset connected.
2. Extract `DesktopPlus-XS-0.2.0-windows-x64.zip` into a fresh folder and run
   `DesktopPlus.exe`. Complete the upstream first-run setup if shown.
3. Open Desktop+ Settings > Profiles > Overlays. The new **XS Ultrawide** section
   appears above the normal profile list in VR and in desktop mode.
4. Choose **Graphics Capture** or **Desktop Duplication**, then click **Dual**,
   **Thirds**, or **Wide center**. No SteamVR restart is needed between layouts.

The version in Settings/logs is **Desktop+ XS 0.2.0 (Desktop+ 3.6)**. If the XS
buttons are absent, check that the running copy is this build rather than the
unmodified baseline or a Steam installation. Run one Desktop+ copy at a time.
For desktop configuration use `DesktopPlusUI.exe --DesktopMode` while the main
DesktopPlus.exe process is running.

## Layouts

All presets currently target **Desktop 1 (monitor ID 0), 5120x1440**:

| Button | Horizontal regions |
| --- | --- |
| Dual | 2560 + 2560 |
| Thirds | 1706 + 1707 + 1707 |
| Wide center | 1280 + 2560 + 1280 |

Profiles have native pointer input enabled, flat opaque panels, 3cm gaps and
room anchoring. They start at standing-space height 1.4m and distance 1.5m.
Point at an unlocked panel and hold **grip** to move it; release grip to place it.
**Trigger** clicks the desktop without locking the panel or enabling Move mode.
Trigger presses during a grip drag are ignored, and releasing trigger does not
end the grip drag. Explicit transform locks still prevent movement.
Properties > Position remains available for width, curvature and opacity. Save your adjusted arrangement
as a named profile using the existing profile controls.

**Each layout button replaces the overlay set with a preset.** Before doing so,
it saves the current set as **XS Previous Layout**. If backup fails, replacement
does not proceed. Load that profile to undo the most recent layout switch.
The backup is overwritten on the next switch; save named profiles for layouts
you want to keep permanently. Missing preset files also prevent replacement.

Monitor selection is explicit in this first version: the source is Desktop 1,
matching the successful baseline. Restore 5120x1440 while Virtual Desktop is
connected before testing. Other monitors/resolutions and English-only labels
in the new XS controls are current limitations.

## Add panels and controller bindings

Settings > Profiles > Overlays now includes **Full screen / Left half / Right
half** add buttons. They append one panel using the chosen capture backend and
leave the existing set intact. A new panel starts in the centre at 1.5m; grip it
to put it where you want. All source presets still target Desktop 1 at 5120x1440.

Quest/Touch, Vive wand, WMR and HP motion-controller grip buttons use the native
Desktop+ **Drag** action. Trigger remains **Left Click**. XS uses application key
`chineko.DesktopPlusXS` so the official Steam Desktop+ app cannot override these
defaults. If using custom SteamVR bindings, select the default XS binding or map
**Drag** to grip and **Left Click** to trigger. Other controller types retain
upstream mappings. Turn legacy Move mode off once if it was enabled previously;
applying an XS layout or adding an XS panel also turns it off automatically.
Only run one Desktop+ copy and disable the baseline copy's automatic startup
if SteamVR starts it alongside XS.

## Wrist shelf

The shelf starts on the **left controller**. Point with the opposite controller
and use trigger. Quick actions can move it to the right hand. Settings >
Profiles > Overlays > **Show XS wrist shelf** hides or restores it. Wrist hand
and visibility currently last for this session only.

The design follows the Linux `wayvr/src/assets/gui/watch.xml`: a fixed 584x880
transparent canvas, 520x176 charcoal clock base, 520x320 shelf above it, rounded
cards, blue-grey borders and pale active icons. Drawers never move the clock or
pointer coordinates. Empty reserved space allows pointing through it. The wrist
is hidden when its controller is disconnected or has no valid tracked pose.

- **Panels:** add full/left/right captures or toggle individual panel visibility.
- **Layouts:** Dual, Thirds, Wide centre, a quick **XS Wrist Saved** slot, and undo
  the last layout switch. Replacing the quick-save slot requires confirmation.
  Use the normal profile manager for named layouts. Layout changes back up the
  current set before replacing it and never restart the VR runtime.
- **PC:** native Windows CPU/RAM readings and SteamVR frame/GPU timing. Polling
  stops when this drawer or wrist is hidden. These are not system-wide GPU usage.
- **Media:** an independent strip with Windows Previous, Play/Pause and Next
  media keys; Windows chooses the receiving player. Album art, player selection
  and track progress from the Linux MPRIS implementation are not ported yet.
- **Actions:** keyboard, settings, wrist hand and normal trigger interaction.

The base shows local time, date, session duration and panel count. A battery
strip shows only values reported by SteamVR, including charging and low charge;
unsupported battery data is omitted. Long device lists scroll inside the strip.

## Validation

The crop generator checks every written preset for exact, non-overlapping
5120-pixel coverage, 1440-pixel height, and the intended capture backend.
The controls use the same native profile load and IPC path as the working
baseline. Capture/rendering engines are unchanged. The input patch separates
native grip dragging from trigger button-up/down handling; the wrist participates
in the existing atlas, sharing, event, intersection-mask and idle mechanisms.

The 0.1.0 layout UI was headset-confirmed. Headset acceptance of 0.2.0 remains necessary: switch all three
layouts with both backends, test seam clicks, drag a panel, and load XS Previous
Layout to check recovery of the preceding setup. Also check the normal profile
list still fits and scrolls in both the VR and desktop interfaces.
Check grip move/release and trigger clicks without locks, including pressing and
releasing trigger during a grip drag. Add a panel and verify existing panels stay
put. Open/close each wrist drawer and media strip, check the base stays stationary,
then test left/right attachment, tracking loss/reconnection and empty-space
pointer pass-through. Check CPU/RAM readings, keyboard/settings and media keys.
Only Desktop+ needs to restart for this update; keep SteamVR/Virtual Desktop up.

## Build and source

This folder holds a reproducible downstream build of pinned Desktop+ source,
using a small patch and native profile files. It stays on the Windows branch of
the existing repository, without importing an unrelated complete source history.

```powershell
./desktopplus/build.ps1
```

Requirements: Python 3, Git, Visual Studio 2022 C++ tools and a Windows SDK.
The script obtains NuGet 6.13.2 and restores the versions pinned by upstream.
It clones Desktop+ v3.6 at commit
`031d74d69f08411a9a9b1030d3d428ac02b93549`, applies the patch, and builds x64.
Release builds omit PDB generation; the development sandbox could not use the
PDB server. Builds run one MSBuild node to avoid sandbox IPC restrictions.

Output in `desktopplus/dist/` contains the binary ZIP and a complete matching
source ZIP, including patched upstream source, presets and this build recipe.
When building directly from the source ZIP, its source is already patched:
restore NuGet packages in `src/packages`, then build `src/DesktopPlus.sln` with
`PlatformToolset=v143`, `Configuration=Release` and `Platform=x64`. Use
`ForceImportBeforeCppTargets` pointing at `XS-build-recipe/release.targets` to
apply the same release settings. No application source is patched during runtime.

Upstream: https://github.com/elvissteinjr/DesktopPlus/releases/tag/v3.6

License: GPL-3.0; retain Desktop+ attribution and the upstream third-party
notices. This is an unofficial modified Desktop+ build. It is not an upstream
release. Windows CI builds only this target; it does not compile the old Rust
prototype or run Linux/Arch checks.
