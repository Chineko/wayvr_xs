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

## Build for Arch Linux and WiVRn

For a pacman-managed install, KDE launcher, and prebuilt CI packages, use the
[Arch installation guide](ARCH-INSTALL.md).

Install the native dependencies from the upstream
[building guide](https://wayvr.org/docs/basics/building-from-source/), plus a Rust toolchain.
From this repository's root:

```sh
cargo build --release --locked -p wayvr --no-default-features --features wayland,openxr
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
