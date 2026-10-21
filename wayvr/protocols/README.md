# KDE screencast protocol

`zkde-screencast-unstable-v1.xml` is vendored from KDE plasma-wayland-protocols:
https://github.com/KDE/plasma-wayland-protocols/blob/master/src/protocols/zkde-screencast-unstable-v1.xml

Copyright 2020–2021 Aleix Pol Gonzalez. LGPL-2.1-or-later; the original SPDX notice
is preserved in the file. The Arch package installs this notice/protocol alongside
its license. Rust client bindings are generated with wayland-scanner at build time.

This is a KDE implementation detail, used only by the explicitly selected KDE
foreground-interaction mode. Bind versions 1–5, consume the created node event,
and retain the stream's lifetime so a closed window disables capture/input.
Other app-window mirrors continue to use the standard desktop portal.
