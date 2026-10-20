# Floating shelf wrist UI

The clock and four controls stay on a compact wrist base. The top-left button
opens quick actions; the top-right button opens the panel/set list. Selecting the
same button closes its shelf, and selecting the other switches shelves. Layouts
and PC open the existing preset manager and performance monitor.

Quick actions reuse the keyboard, layout editing, recenter, floor correction,
dashboard/settings, capture-window and KDE interactive-window commands. Panel/set
selection follows the existing `sets_on_watch` setting. Additional timezones stay
available in quick actions. App switching and quick controls beneath interactive
panels continue to work as before.

The battery strip sits below the base. It uses the existing runtime reports for
headset, left/right controllers and trackers, with percentages, charging marks and
low-charge highlighting. Devices without reported charge data are omitted, and an
empty strip is hidden. Long lists wrap into rows. It does not estimate remaining
hours or provide battery data that the VR runtime does not expose.

The canvas reserves transparent space so opening a shelf or connecting a device
does not resize the wrist base or change its pointer mapping. Transparent reserved
areas allow pointing through to other overlays. Colors follow the current theme;
skins remain a later update. Existing user-supplied watch XML without a shelf
continues to work and takes precedence over the built-in layout.

## Combined headset check

- Open each shelf, switch between them and close it. Check pointer alignment and
  that the wrist base stays in place.
- Try keyboard, edit mode, recenter, floor correction and dashboard/settings.
- Add a capture/app panel and an interactive panel; toggle existing panels/sets.
- Open saved layouts and PC monitoring, and check their existing controls.
- Check controller/tracker percentages, charging marks and device disconnection
  where the runtime supports them. Confirm existing layout persistence.

Regular screen-panel quick controls, capture optimization and installation/recovery
remain the next three roadmap targets. This change reorganizes the wrist UI.
