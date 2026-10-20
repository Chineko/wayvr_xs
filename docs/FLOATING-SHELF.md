# Floating shelf wrist UI

The clock, session timer, status strip and four controls stay on a compact wrist base. The horizontal icon row opens panels/sets, saved layouts, PC performance and
quick actions/settings. Selecting the same shelf button closes it; selecting
another switches shelves. Layouts opens the preset manager inside the same wrist shelf. Its selector opens an anchored dropdown directly beneath the field, retaining
the surrounding controls and listing three presets per page, the name field opens a compact in-VR keyboard, and Save
creates a new preset or confirms replacement of an existing name. Rename, Delete
and startup selection reuse the existing storage. Clicking an active startup
choice clears it back to last-session startup. Loading still explicitly restarts
WayVR. Delete and replacement require a second click. PC expands
above the clock on this same wrist canvas, with a collapse button in its header.
There is no separate performance overlay behind the hand.

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
areas allow pointing through to other overlays. The charcoal surfaces, pale blue-grey bars, rounded cards and light active icons
follow the selected mockup. Skins remain a later update. User-supplied watch XML still takes precedence over the built-in layout; the
embedded monitor requires the new shelf elements.

## Combined headset check

- Open each shelf, switch between them and close it. Check pointer alignment and
  that the wrist base stays in place.
- Try keyboard, edit mode, recenter, floor correction and dashboard/settings.
- Add a capture/app panel and an interactive panel; toggle existing panels/sets.
- Open saved layouts and PC monitoring, check GPU switching and monitor collapse.
  Confirm PC monitoring is above the clock and does not appear behind the hand.
  Sensor requests stop when the performance shelf or watch is hidden; missing
  readings show a dash, and WayVR frame timing is not compositor GPU timing.
- Check controller/tracker percentages, charging marks and device disconnection
  where the runtime supports them. Confirm existing layout persistence.

Regular screen-panel quick controls, capture optimization and installation/recovery
remain the next three roadmap targets. This change reorganizes the wrist UI.

The panels/sets shelf includes an eye button using `OverlayTask::ShowHide`. It
shows **Hide overlays** normally and **Restore overlays** with a highlighted
state while the workspace is hidden. The wrist remains accessible. Restoration
uses `restore_set`, retains the stored transforms and does not recenter. Global
overlays retain the existing ShowHide behavior.
