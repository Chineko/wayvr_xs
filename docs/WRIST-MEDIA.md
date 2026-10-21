# Compact wrist media controls

The music-note button on the wrist shows or hides a compact media strip beneath
the clock/base. It is independent of the action/layout/performance shelves:
opening it keeps the base and active drawer stationary. The battery strip moves
down when needed. The fixed transparent canvas reserves enough space for media
and the existing 18-device battery layout; hidden media has no pointer region.

The strip uses the existing charcoal style and shows album art (or a music icon),
track title, artist/player name, Previous, Play/Pause and Next. A slim progress
bar and elapsed/total time fit in the metadata column without increasing the
strip height. Progress follows the player's MPRIS position and playback rate,
freezes while paused, and resynchronizes on each poll after seeking or changing
tracks. It updates in whole seconds and is read-only. Unknown duration/position
uses a dash and an empty bar rather than invented progress. Long text is
shortened and clipped within the metadata column. Tap the artwork to cycle
between available players. The initial selection prefers a playing player;
manual selection stays selected while that player is available. Missing players
fall back to another available player. No compatible player shows a clear empty
state; unsupported transport actions are dimmed and ignored.

Media uses the Linux MPRIS session-bus interface, including Spotify and compatible
browser/native players. It does not launch Spotify, take focus, synthesize global
media keys or require playerctl. Only applications exposing MPRIS are listed.
Sandbox/browser integration and the advertised controls depend on the player.
Commands target the selected player's unique bus owner and recheck capabilities,
so a departed player's queued command cannot control a newly launched process.

Bus queries run on a worker, at most once per second while media and the wrist
are visible. The worker sleeps between requests; hiding can leave one request in
flight. Bus calls have short timeouts, player discovery is bounded, stale data
expires, and unchanged metadata does not redraw the strip every timer tick.
Artwork is fetched only when its URL changes, from local file URLs or HTTPS.
Downloads are bounded to 4 MiB with a two-second timeout. Decoding is limited to
2048×2048/32 MiB and creates a maximum 128-pixel thumbnail on the worker before
upload. Missing, unsupported or failed artwork uses the music icon. No playback
history is stored and no telemetry is uploaded.

## One headset test before publishing

1. Close the installed WayVR and back up with `wayvr-xs-support backup`.
2. Run the local `wayvr-xs-media-progress-test --openxr --show` build.
3. Start Spotify, play a track, and tap the wrist music-note button. Check cover,
   title/artist, elapsed/total time, Previous/Next, and both directions of Play/Pause.
   Verify progress stops while paused and follows seeking in Spotify. Commands should
   affect Spotify without changing desktop focus.
4. Change tracks outside VR and check the strip updates. If another MPRIS player
   is available, tap the artwork to select it and verify commands reach only it.
5. Toggle the media strip while performance/layout shelves are open. Check wrist
   buttons, text, battery placement and pointer alignment remain correct. Hide
   overlays should keep the wrist/media available; reopening media should refresh.
6. Close the player: the strip should fall back or show No media player. Restart
   it and check discovery. Close the test build and launch the installed app to
   return to the published version.

Automated tests use a private D-Bus fixture and never control the user's real
player. They cover metadata variant decoding, signed/unsigned Spotify timing, capability-checked transport,
unique-owner targeting after disconnect, player selection, artwork bounds,
rendered button hits, stationary drawers/base and battery capacity. The initial media strip is headset-confirmed; the progress addition awaits
headset acceptance. Visibility is session-only and initially hidden.
