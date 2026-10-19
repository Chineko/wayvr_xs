#!/usr/bin/env bash
set -euo pipefail
: "${GH_REPO:?Set GH_REPO to the destination repository}"
: "${SOURCE_COMMIT:?Set SOURCE_COMMIT to the verified source commit}"
shopt -s nullglob
packages=(wayvr-xs-git-*.pkg.tar.zst)
(( ${#packages[@]} == 1 ))
# A fixed prerelease tag provides stable pacman URLs without changing main or PRs.
if ! gh release view arch-repo >/dev/null 2>&1; then
  gh release create arch-repo --target "$SOURCE_COMMIT" --prerelease \
    --title 'WayVR XS Arch repository' \
    --notes 'Rolling Arch Linux x86_64 packages built and tested from feat/xs-ultrawide-panels. Setup: see docs/ARCH-INSTALL.md on that branch. Packages are unsigned and served over HTTPS.'
fi
# Upload the package before the database advertises it. Keep previous package
# versions available for clients whose cached database still references them.
gh release upload arch-repo "${packages[0]}" --clobber
gh release upload arch-repo wayvr-xs.conf SOURCE-COMMIT.txt SHA256SUMS --clobber
gh release upload arch-repo wayvr-xs.files --clobber
gh release upload arch-repo wayvr-xs.db --clobber
