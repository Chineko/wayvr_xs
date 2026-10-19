# Install WayVR XS on Arch Linux / KDE Plasma

Use Plasma's Wayland session. Start WiVRn and connect the headset before
launching WayVR XS. This package builds the fork's `feat/xs-ultrawide-panels`
branch with Wayland and OpenXR enabled.

## Enable normal Arch updates

This fork publishes successful Arch builds to its own **wayvr-xs** pacman
repository on GitHub. It follows `feat/xs-ultrawide-panels` and uses the same
package name as the manually installed package, so settings are preserved.

Add this block once to the end of `/etc/pacman.conf` using `sudoedit /etc/pacman.conf`:

```ini
[wayvr-xs]
SigLevel = Optional
Server = https://github.com/Chineko/wayvr_xs/releases/download/arch-repo
```

`SigLevel = Optional` applies only to this personal repository: its packages are
unsigned and downloaded over HTTPS. Other repositories keep their existing
signature policy. This is a third-party repository, not an official Arch repo
or an AUR package.

Then run:

```sh
sudo pacman -Syu wayvr-xs-git
```

Future updates are included in your normal `sudo pacman -Syu` (and graphical
package managers that use pacman repositories). Only successful builds are
published; failed builds leave the last published version available. You do
not need to download Actions ZIPs or compile the application for routine updates.
Close WayVR before relaunching an updated version.

## Install a package manually

As a fallback, open the [Arch package workflow](https://github.com/Chineko/wayvr_xs/actions/workflows/build-arch.yml),
sign in to GitHub, and download **wayvr-xs-arch-x86_64** from a successful run's
Artifacts section. Extract the ZIP, then run:

```sh
sha256sum -c SHA256SUMS
sudo pacman -U ./wayvr-xs-git-*.pkg.tar.zst
```

The archive includes the source commit ID. Artifacts are retained for 30 days;
the [Arch repository release](https://github.com/Chineko/wayvr_xs/releases/tag/arch-repo)
provides persistent package downloads without requiring a GitHub login.
Packages use current Arch libraries; keep the system up to date.

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
