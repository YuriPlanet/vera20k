#!/usr/bin/env bash
# Put a packaged macOS release folder on a disk image as VERA20k.app, for the
# Release workflow (.github/workflows/release.yml):
#   app.sh <package folder> <output .dmg>
# VERSION (x.y.z) is the app's version.
#
# The app keeps its files in ~/Library/Application Support/VERA20k and asks
# for the Yuri's Revenge folder on its first launch (src/app/app_bundle.rs).
# It is signed ad hoc, not notarized, so macOS asks the player to allow it once
# (README.txt). The signature seals the whole bundle; without that, Gatekeeper
# calls a downloaded app damaged instead of offering to open it.
set -euo pipefail
package="$1"
dmg="$2"
here="$(cd "$(dirname "$0")" && pwd)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
app="$stage/VERA20k.app"

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
sed "s/@VERSION@/$VERSION/g" "$here/Info.plist.in" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
cp "$package/vera20k" "$app/Contents/MacOS/"
cp "$package/LICENSE-GPL" "$package/THIRD_PARTY_LICENSES.html" "$app/Contents/Resources/"
codesign --force --sign - "$app"
codesign --verify --strict --verbose=2 "$app"

cp "$package/README.txt" "$stage/"
ln -s /Applications "$stage/Applications"
# hdiutil on hosted runners now and then fails with "Resource busy".
for attempt in 1 2 3 4 5; do
  if hdiutil create -volname VERA20k -srcfolder "$stage" -format UDZO -ov "$dmg"; then
    exit 0
  fi
  sleep $((attempt * 5))
done
echo "hdiutil could not create $dmg" >&2
exit 1
