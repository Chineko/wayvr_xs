# WayVR XS 0.1.1 — media controls and upstream update

Compact desktop and application overlays for Arch Linux, KDE Plasma Wayland and
WiVRn. This release retains the upstream WayVR core and adds the XS workflow.

## New in 0.1.1

- Compact wrist media strip with album artwork, track/artist information,
  Previous, Play/Pause and Next controls. The music-note button shows/hides it;
  tapping artwork cycles MPRIS players.
- Progress bar with elapsed/total time, playback-rate handling and pause/seek/track
  resynchronization. Spotify unsigned durations and standard signed timing are
  supported. Missing timing stays unavailable rather than showing invented data.
- Background media queries and bounded artwork loading; hidden media stops polling.
  The existing wrist theme and stationary shelf/base geometry are retained.
- Upstream adf85297: speech-to-text GPU selector. Speech-to-text remains disabled
  in the lightweight Arch package; the upstream feature-enabled builds include it.
- User confirmed upstream integration, media controls and progress work with
  Spotify and YouTube. No settings migration is required.

## Features of this fork

### Desktop and application panels

- Split an ultrawide monitor into two halves, three columns or a wide center
  layout; restore the full desktop when needed. Splits share the source capture.
- Move screen, app and keyboard panels independently with the independent movement
  option, while retaining the existing shared workspace movement mode.
- Regular mono screens, split panels and interactive app panels have compact quick
  controls for size, distance, opacity, curvature, pin/unpin, recenter and hide.
- Add view-only app windows through the portal, or foreground interactive KDE app
  panels with controller mouse input and existing keyboard routing.
- Choose apps across monitors, switch the app in an existing panel using its
  dropdown, and close an interactive panel without closing the desktop app.
- Persist app-panel placement and identity; reconnect uniquely matched apps and
  offer a chooser for missing or ambiguous matches.

### Compact wrist interface

- Floating action shelf above a stationary clock/session base, using the compact
  charcoal theme throughout.
- Embedded saved-layout manager with inline selection, in-VR naming, save/replace,
  rename, delete, startup selection and explicit Load & restart.
- Hide / Restore overlays remembers the workspace; the wrist remains available.
- Integrated PC performance drawer: WayVR frame rate/interval, CPU/RAM and available
  GPU/VRAM/temperature readings, with GPU selection where available.
- Compact controller/tracker battery strip when the runtime supplies battery data,
  including charging and low-battery states.

### Updates, performance and recovery

- Arch x86_64 packages, exact-source CI and normal pacman repository updates.
- Native capture resolution, shared split captures and pausing when all sibling
  screen panels are hidden; sensor requests stop when the stats drawer is closed.
- Reduced redundant texture binding and opt-in capture delivery/CPU import metrics.
- `wayvr-xs-support doctor`, `backup`, `restore` and `diagnose` for local checks,
  private settings/layout backups, recoverable restoration and log collection.

## Install or update

Existing users with the wayvr-xs repository configured:

```sh
sudo pacman -Syu wayvr-xs-git
```

Close and reopen WayVR after updating. Start WiVRn and connect the headset first.
Use `/usr/bin/wayvr-xs --openxr --show` to bypass an older custom PATH launcher.

[First-time setup and recovery](https://github.com/Chineko/wayvr_xs/blob/a24ca4257ca202578d28ece66ae84eaa0268be48/docs/ARCH-INSTALL.md)

The attached package is the verified release snapshot. The existing `arch-repo`
release continues to provide rolling updates; this snapshot does not change its
URL or replace its repository database. Packages are unsigned; use the signature
policy scoped to this personal repository in the setup guide.

## Validation

Source: `a24ca4257ca202578d28ece66ae84eaa0268be48`
Package: `26.8.0.r1724.ga24ca425-1` (x86_64)

All six CI workflows pass: Default, Wayland/OpenXR, Wayland/OpenVR, X11/OpenXR,
X11/OpenVR and Arch package build/test/install/publication. Local validation:
50 Rust tests and four recovery tests pass; two hardware-dependent native tests
are ignored. Public pacman download and package integrity checks pass.

On the user's Arch/KDE/WiVRn setup, text and scrolling look correct, Hide / Restore
returns all four panels with correct positions/interaction, and configuration
backup/restore works. The workspace was a main monitor split in two, an interactive
app and a secondary monitor. Whole-device GPU utilization dropped from 25% to 15%
when hidden; reported total VRAM was about 3 GB and CPU about 2%. These are
single-setup observations, not WayVR-only resource measurements. End-to-end
latency is unmeasured. Other hardware/runtime combinations are not certified by
compilation checks. The upstream integration and new media controls/progress were also headset-confirmed.

## Known limitations

- Interactive app capture/input is KDE Wayland specific and requires foreground
  access. Minimized apps may need manual restoration; protected content may not
  be capturable. Ambiguous reconnection asks the user to choose an app.
- Named layout loading currently restarts WayVR. Backup/restore requires WayVR
  and standard WayVR to be closed; they share configuration.
- GPU sensors, temperatures and device battery information depend on driver and
  runtime support. Missing telemetry is shown as unavailable.
- Windows support, skins, snapping/alignment and placement undo are future work.

The upstream portal edit-wrapper fix and Arch system-shaderc builds are retained.
