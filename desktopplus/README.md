# Desktop+ XS 0.3.1

Windows now uses Desktop+ 3.6 as its foundation. The user confirmed that both
unmodified capture backends work on their 5120x1440 display without issues.
The Rust Windows prototype is retained as a reference; current development
uses Desktop+'s native capture, rendering, input and controller systems.
Linux WayVR development remains separate.

## Changes in 0.3.1

- Screen grip capture now survives pointer focus changes. It binds to the
  initiating controller and original panel until release, handles a squeeze held
  before entering the panel, and releases if the action/device disappears.
  Trigger still clicks outside a grab; explicit transform locks still apply.
  The diagnostic log distinguishes started, released, transform-locked and
  tracking-invalid grab attempts. The panel strip now has an explicit Lock/Unlock
  button so locks left from older layouts are visible and easy to clear. Settings
  uses its existing working grab path.
- SteamVR autostart defaults **off**. The first run of this update also clears the
  XS autostart flag left by earlier builds; a marker in `xs-ui.ini` prevents
  repeatedly overriding a later explicit opt-in. Settings > Miscellaneous >
  Startup retains the native autostart checkbox. This affects only XS's app key.
- During the user's encoder check, Windows' per-process GPU counters attributed
  79% Video Encode usage to `VirtualDesktop.Streamer`, and the user confirmed it
  remained high with XS closed. XS contains no video encoding/streaming pipeline.
  No Virtual Desktop quality, codec, bitrate or refresh setting is changed.

## Changes in 0.3.0

- Dual, Thirds and Wide centre use a selected monitor's **current pixel bounds**.
  The selector is available in both Settings and the wrist. Graphics Capture uses
  monitor-local crops; Desktop Duplication includes the monitor's offset within
  its adapter's combined capture texture. Neither XS button uses a hard-coded
  5120x1440 crop. Disconnected monitors produce an error and keep the workspace.
- Point at any input-enabled panel to reveal the charcoal strip immediately
  below it: **Size -5%, Size +5%, Properties, Hide, Close**. Hide can be restored
  from the wrist list. Close removes the VR panel and keeps the underlying app
  open; it is also available as **X** in each wrist panel row. Removal is deferred
  until widget rendering has finished, then floating controls and properties IDs
  are reset safely.
- Settings opens in front of the current view, at 1.15m and a smaller size, instead
  of using a stale dashboard-relative position. Grip drags of Settings/Properties
  finish with grip release, independently of trigger release.
- Wrist default width is **20cm** (previously 34cm). A 12-32cm slider is in Settings
  and Quick Actions. Size, hand and visibility persist in `xs-ui.ini`. Hovering
  icons no longer changes their drawing position; device icons and charge bars
  replace the plain battery list. Performance now has CPU/VR-GPU history graphs
  and a RAM bar.
- Active panel drags use a 3ms maximum capture/event wait. UI window drags skip
  idle throttling and UI frame skipping. Capture backends remain upstream;
  headset testing is needed to assess the reported intermittent lag.

Quit using **Quick Actions > Exit XS**, **Settings > Exit Desktop+ XS**, or the
Windows notification-area icon's **Quit** command. All three use the native
shutdown path, save the workspace and wrist preferences, and leave SteamVR and
Virtual Desktop running. The exit action also prevents the UI's desktop-mode
automatic relaunch. Start DesktopPlus.exe again when you want the panels back.

## Run

1. Exit the baseline Desktop+ copy and the Rust WayVR Windows prototype.
   Keep SteamVR and Virtual Desktop running, with the headset connected.
2. Extract `DesktopPlus-XS-0.3.1-windows-x64.zip` into a fresh folder and run
   `DesktopPlus.exe`. Complete the upstream first-run setup if shown.
3. Open Desktop+ Settings > Profiles > Overlays. The new **XS Ultrawide** section
   appears above the normal profile list in VR and in desktop mode.
4. Select the monitor and **Graphics Capture** or **Desktop Duplication**, then click **Dual**,
   **Thirds**, or **Wide center**. No SteamVR restart is needed between layouts.

The version in Settings/logs is **Desktop+ XS 0.3.1 (Desktop+ 3.6)**. If the XS
buttons are absent, check that the running copy is this build rather than the
unmodified baseline or a Steam installation. Run one Desktop+ copy at a time.
For desktop configuration use `DesktopPlusUI.exe --DesktopMode` while the main
DesktopPlus.exe process is running.

## Layouts

XS buttons generate profiles from the chosen monitor at click time. Odd pixel
widths are distributed without gaps or overlap. For example, at **5120x1440**:

| Button | Horizontal regions |
| --- | --- |
| Dual | 2560 + 2560 |
| Thirds | 1706 + 1707 + 1707 |
| Wide center | 1280 + 2560 + 1280 |

Generated layouts have native pointer input enabled, flat opaque panels, 3cm gaps
and room anchoring. They appear 1.5m in front of the headset at comfortable physical
size. Reapply a layout after changing a monitor mode to regenerate its regions.
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

The explicitly named `XS 5120 ...` profiles retained in the normal profile list
are legacy files for Desktop 1 at that resolution. Use the XS buttons for adaptive
layouts. New labels are currently English only. Native single-desktop mirroring
still shares one desktop source across Desktop Duplication panels; use Graphics
Capture for independent sources across multiple GPUs/monitors.

## Add panels and controller bindings

Settings > Profiles > Overlays now includes **Full screen / Left half / Right
half** add buttons. They append one panel using the chosen capture backend and
leave the existing set intact. A new panel starts in front of your current view
at 1.5m; grip it to put it where you want.

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
and visibility are saved to `xs-ui.ini`.

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
- **PC:** native Windows CPU/RAM readings, CPU/VR-GPU history graphs, a RAM bar,
  and SteamVR compositor GPU timing. Polling
  stops when this drawer or wrist is hidden. These are not system-wide GPU usage.
- **Media:** an independent strip with Windows Previous, Play/Pause and Next
  media keys; Windows chooses the receiving player. Album art, player selection
  and track progress from the Linux MPRIS implementation are not ported yet.
- **Actions:** keyboard, settings, wrist hand and normal trigger interaction.

The base shows local time, date, session duration and panel count. A battery
strip shows only values reported by SteamVR, including charging and low charge;
unsupported battery data is omitted. Device icons and charge bars use the same
colours as the base. Long device lists scroll inside the strip.

## Validation

The crop generator checks every written preset for exact, non-overlapping
5120-pixel coverage, 1440-pixel height, and the intended capture backend.
The controls use the same native profile load and IPC path as the working
baseline. Capture/rendering engines are unchanged. The input patch separates
native grip dragging from trigger button-up/down handling; the wrist participates
in the existing atlas, sharing, event, intersection-mask and idle mechanisms.

The 0.1.0 layout UI was headset-confirmed. The 0.2.0 headset test identified the
bugs addressed above. Production C++ crop functions were checked across seven
widths (1080,1280,1920,1921,2560,3440,5120), odd-width remainder coverage, all layout
boundaries and monitor origins including negative coordinates. Local release
build and fresh patch application are checked.

The 0.3.0 headset test confirmed Settings grabbing but reported failed screen
panel grabbing. Production grip-state regression checks cover squeeze-before-hit,
focus/target changes, release without hover, action/device loss and independent
hands. The focus-leave cancellation path now excludes active grip captures.

Headset acceptance of 0.3.1 remains necessary: switch all three
layouts with both backends, test seam clicks, drag a panel, and load XS Previous
Layout to check recovery of the preceding setup. Also check the normal profile
list still fits and scrolls in both the VR and desktop interfaces.
Check all layouts at a smaller resolution with the secondary monitor attached,
then select the other monitor and repeat with both capture backends. Verify the
panel strip's size/hide/close controls, wrist X removal, and Settings placement
after turning around. Resize the wrist, restart only Desktop+ and check persistence.
Check grip move/release and trigger clicks without locks, including pressing and
releasing trigger during a grip drag. Add a panel and verify existing panels stay
put. Open/close each wrist drawer and media strip, check the base stays stationary,
then test left/right attachment, tracking loss/reconnection and empty-space
pointer pass-through. Check CPU/RAM readings, keyboard/settings and media keys.
Finally exit using the wrist, confirm SteamVR/Virtual Desktop stay up, then reopen
DesktopPlus.exe and check the workspace returns. Repeat exit from Settings or tray.
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
