#!/usr/bin/env bash
#
# Measures a real iOS release archive.
#
# The App Store's number is not the archive's size and not the .ipa's either —
# it is the thinned, re-signed, DRM-wrapped payload for one device family, and
# only App Store Connect can produce it. What this script gives is the closest
# honest approximation available before an upload: the archive's binary, its
# per-architecture slices, and the payload size.
#
# Ticket 37. See docs/ios-release.md.

set -euo pipefail

cd "$(dirname "$0")/.."

# Supplied by the environment, never committed. See docs/ios-release.md.
if [[ -z "${APPLE_DEVELOPMENT_TEAM:-}" ]]; then
  echo "APPLE_DEVELOPMENT_TEAM is not set." >&2
  echo "It is deliberately not in version control — export it and re-run." >&2
  exit 1
fi

echo "Building a release archive for device arm64…"
npm run build:mobile
npx tauri ios build --target aarch64 -- -allowProvisioningUpdates

archive="src-tauri/gen/apple/build/cabalmesh_iOS.xcarchive"
app="$archive/Products/Applications/CabalMesh.app"

if [[ ! -d "$app" ]]; then
  echo "no archive at $archive" >&2
  exit 1
fi

binary="$app/CabalMesh"

echo
echo "Archive"
echo "-------"
du -sh "$archive" | awk '{ printf "  archive          %s\n", $1 }'
du -sh "$app"     | awk '{ printf "  .app payload     %s\n", $1 }'
ls -l "$binary"   | awk '{ printf "  main binary      %.2f MB\n", $5 / 1048576 }'

echo
echo "Architectures in the binary"
echo "---------------------------"
# A release build that still carries a simulator slice is one built for the
# wrong destination, and it inflates every figure above.
lipo -info "$binary" | sed 's/^/  /'

echo
echo "Largest contents"
echo "----------------"
find "$app" -type f -exec ls -l {} + |
  sort -k5 -rn |
  head -10 |
  awk '{ printf "  %8.2f MB  %s\n", $5 / 1048576, $NF }'

echo
echo "These are pre-thinning figures. App Store Connect reports the real"
echo "per-device download after processing, and it will be smaller."
