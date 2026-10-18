# Install WayVR XS on Arch Linux / KDE Plasma

Use Plasma's Wayland session. Start WiVRn and connect the headset before
launching WayVR XS. This package builds the fork's `feat/xs-ultrawide-panels`
branch with Wayland and OpenXR enabled.

## Install a prebuilt package

1. Open this fork's [Arch package workflow](https://github.com/Chineko/wayvr_xs/actions/workflows/build-arch.yml).
2. Choose a successful run for `feat/xs-ultrawide-panels`. Check the commit
   shown on that run, then download its **wayvr-xs-arch-x86_64** artifact.
3. Extract the ZIP. In that directory, install the package:

   ```sh
   sudo pacman -Syu --needed xdg-desktop-portal-kde
   sudo pacman -U ./wayvr-xs-git-*.pkg.tar.zst
   ```

The artifact also contains a SHA-256 checksum and source commit ID. You can
check the downloaded package with `sha256sum -c SHA256SUMS` before installation.
Packages are built against current Arch repositories; keep Arch up to date.
Artifacts are retained for 30 days. If no artifact is available, build locally
using the instructions below.

For later updates, download the next successful build and run the same
`pacman -U` command. This is a local package, not an AUR submission or a pacman
repository: `pacman -Syu` alone does not fetch new WayVR XS builds.

## Build and install locally

Install the build tools and Plasma portal:

```sh
sudo pacman -Syu --needed base-devel git xdg-desktop-portal-kde
```

If you already have the checkout from the manual build:

```sh
cd ~/src/wayvr_xs
git switch feat/xs-ultrawide-panels
git pull --ff-only
cd packaging/arch
makepkg -si
```

For a new checkout:

```sh
mkdir -p ~/src
cd ~/src
git clone --branch feat/xs-ultrawide-panels https://github.com/Chineko/wayvr_xs.git
cd wayvr_xs/packaging/arch
makepkg -si
```

Run `makepkg` as your normal user. It requests sudo when installing dependencies
and the finished package. The recipe downloads a separate source checkout,
fetches Cargo.lock dependencies, builds, and runs the Wayland/OpenXR tests.
It does not package uncommitted edits in your original checkout.

For subsequent source updates, repeat `git pull --ff-only` at the repository
root, then `makepkg -si` inside `packaging/arch`. VCS sources refresh automatically.
You no longer need to copy the executable with `install`.

## Launch and migrate from the manual build

Close the existing WayVR process. Connect the headset using WiVRn, then open
**WayVR XS (WiVRn)** in KDE's application launcher or WiVRn's application list.
From Konsole, use the explicit packaged path:

```sh
/usr/bin/wayvr-xs --openxr --show
```

The desktop launcher uses that same path, so an older manual copy at
`~/.local/bin/wayvr-xs` cannot shadow it. If you type just `wayvr-xs`, your shell
may still select that old copy; use the explicit path or remove the old copy
after verifying the package works.

The package keeps the existing `~/.config/wayvr` settings and saved panels.
Back them up before your first package upgrade. Standard WayVR and WayVR XS
share those settings and the single-instance guard; run one at a time. Disable
the old WayVR autostart entry in WiVRn while using this fork.

To remove the package, use `sudo pacman -R wayvr-xs-git`. User settings remain.

## Logs and desktop access

If the launcher appears to do nothing, run the Konsole command above and check
`/tmp/wayvr.log`. Select the physical 5120 x 1440 monitor in KDE's screen-sharing
dialog. See [the ultrawide guide](XS-ULTRAWIDE.md) for split controls.

If mouse/keyboard emulation reports that `/dev/uinput` is unavailable, check
that the uinput module is loaded and that your user has device permissions.
The package does not change your user groups or device permissions.
