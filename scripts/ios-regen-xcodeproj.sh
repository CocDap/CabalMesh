#!/usr/bin/env bash
# Regenerate the iOS Xcode project from project.yml, then downgrade the project
# file format so Xcode 15.x can open it.
#
# XcodeGen >= 2.44 always emits objectVersion 77 (Xcode 16 format) and ignores
# the objectVersion / xcodeVersion / compatibilityVersion spec options. The
# generated project uses no Xcode 16-only object types, so rewriting the format
# header is sufficient.
#
# Run this after any edit to gen/apple/project.yml, and after `tauri ios init`.
# Not needed once the toolchain is on Xcode 16+.
set -euo pipefail

APPLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../src-tauri/gen/apple" && pwd)"
PBXPROJ="$APPLE_DIR/cabalmesh.xcodeproj/project.pbxproj"

cd "$APPLE_DIR"
xcodegen generate

python3 - "$PBXPROJ" <<'PY'
import sys

path = sys.argv[1]
src = open(path).read()

src = src.replace('objectVersion = 77;', 'objectVersion = 56;')
src = '\n'.join(l for l in src.split('\n') if 'preferredProjectObjectVersion' not in l)

if 'compatibilityVersion' not in src:
    anchor = src.index('isa = PBXProject;')
    eol = src.index('\n', anchor)
    src = src[:eol + 1] + '\t\t\tcompatibilityVersion = "Xcode 14.0";\n' + src[eol + 1:]

open(path, 'w').write(src)
PY

echo "Patched $(basename "$PBXPROJ") to Xcode 15 project format."
