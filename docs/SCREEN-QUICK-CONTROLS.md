# Regular screen quick controls

Regular desktop screens and split regions now have a charcoal Controls drawer
below the capture, with size ±5%, distance ±10 cm, opacity, curvature, pin/unpin,
recenter and Hide. These call the same actions as interactive panels. Hide uses
the existing per-panel toggle; the wrist panel list can show the panel again.

The drawer wraps existing capture backends. Region clones continue sharing the
source stream; native input scaling, rotation and monitor origins are retained.
Rendering and pointer mapping reuse the interactive-panel geometry, with native
portrait width/height preserved. Opening controls does not resize or move the
desktop image. Footer hits never map to desktop coordinates. Desktop drags can
cross panels, and releases retain the original button despite orientation changes.
Hiding/pausing a panel and using its controls releases its held desktop input.

The edit wrapper still forwards capture identity, split creation and interaction
pose. A native-content extent accessor prevents footer height from changing split
aspect calculations. System shaderc remains the Arch build path. Stereo sources
retain their existing presentation/input; their footer is disabled while stereo
is active. View-only app mirrors are outside this regular-screen control change.

## Combined headset test

- Expand/collapse controls on the 5120×1440 source, split halves and portrait screen.
- Check pointer alignment on desktop edges and every control, including curvature.
- Check desktop clicks, scrolling and dragging between regions; controls must not
  click the desktop or leave a mouse button held.
- Try size, distance, opacity, curve, pin/unpin, recenter and Hide independently.
- Hide one region and confirm sibling regions still capture; restore via the wrist.
- Check existing edit mode and splitting, plus layout persistence after reopening.
- Check the saved-layout selector opens a dropdown directly underneath its field,
  keeps other controls in place, and selects/closes correctly across pages.
