#!/usr/bin/env bash
#
# Per-architecture APK sizes — what a device actually downloads.
#
# The universal APK carries four copies of the Rust static library and is
# roughly four times what anyone installs, so judging the size budget by it is
# how a build passes review and then surprises everyone on the store listing.
#
# Ticket 38. See docs/android-release.md.

set -euo pipefail

cd "$(dirname "$0")/.."

# The JDK is not a free choice: Gradle 8.14.3 rejects Java 26 outright with
# "Unsupported class file major version 70". Ticket 08 installed a 21 LTS at
# the user level for exactly this.
if [[ -z "${JAVA_HOME:-}" ]]; then
  for candidate in "$HOME"/Library/Java/JavaVirtualMachines/*/Contents/Home; do
    if [[ -x "$candidate/bin/java" ]]; then
      export JAVA_HOME="$candidate"
      break
    fi
  done
fi
echo "JAVA_HOME=${JAVA_HOME:-<unset>}"

pushd src-tauri/gen/android >/dev/null

# splitApks turns on the abi split block in app/build.gradle.kts. Without it
# Gradle produces the universal APK, which is the thing this script exists to
# avoid measuring.
./gradlew --quiet assembleRelease -PsplitApks

popd >/dev/null

outputs="src-tauri/gen/android/app/build/outputs/apk/release"
if [[ ! -d "$outputs" ]]; then
  echo "no release APKs at $outputs" >&2
  exit 1
fi

echo
echo "Per-device download size:"
echo

# Sorted by size so the arm64 figure — the one the budget is judged against —
# is easy to find rather than buried in build order.
find "$outputs" -name '*.apk' -print0 |
  xargs -0 ls -l |
  awk '{ printf "  %8.2f MB  %s\n", $5 / 1048576, $NF }' |
  sort -rn

echo
echo "Judge the budget against arm64-v8a: every device shipped in years installs that one."
echo "An unsigned APK here means keystore.properties is missing — see docs/android-release.md."
