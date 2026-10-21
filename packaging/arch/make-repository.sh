#!/usr/bin/env bash
set -euo pipefail
# Run inside the artifact directory after the package passed its smoke test.
shopt -s nullglob
packages=(wayvr-xs-git-*.pkg.tar.zst)
if (( ${#packages[@]} != 1 )); then
  echo 'Expected exactly one verified WayVR XS package' >&2
  exit 1
fi
repo-add --nocolor wayvr-xs.db.tar.gz "${packages[0]}"
# GitHub release assets must be regular files, not repo-add's symlinks.
cp -L wayvr-xs.db.tar.gz wayvr-xs.db.download
mv -f wayvr-xs.db.download wayvr-xs.db
cp -L wayvr-xs.files.tar.gz wayvr-xs.files.download
mv -f wayvr-xs.files.download wayvr-xs.files
sha256sum "${packages[0]}" wayvr-xs.db wayvr-xs.files > SHA256SUMS
