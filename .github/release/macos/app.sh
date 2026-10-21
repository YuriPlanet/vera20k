#!/usr/bin/env bash
# Put a packaged macOS release folder on a disk image as VERA20k.app, for the
# Release workflow (.github/workflows/release.yml):
#   app.sh <package folder> <output .dmg>
# Environment: VERSION (x.y.z), STAMP (the build time, YYYYMMDD-HHMMSS) and
# MACOSX_DEPLOYMENT_TARGET (the oldest macOS the program was built for).
#
# The app keeps its files in ~/Library/Application Support/VERA20k and finds or
# asks for the Yuri's Revenge folder on its first launch (src/app/app_bundle.rs).
# It is signed ad hoc, not notarized, so macOS asks the player to allow it once
# (README.txt). The signature seals the whole bundle; without that, Gatekeeper
# calls a downloaded app damaged instead of offering to open it. The image
# window shows only the app, an arrow and Applications (dmg_settings.py);
# README.txt and the licenses are in the app's Resources.
set -euo pipefail
package="$1"
dmg="$2"
here="$(cd "$(dirname "$0")" && pwd)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
app="$stage/VERA20k.app"

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
# The build number, the build time as YYYYMMDD.HHMMSS, tells two builds of one
# version apart in About VERA20k.
sed -e "s/@VERSION@/$VERSION/g" -e "s/@BUILD@/${STAMP/-/.}/g" \
  -e "s/@MINIMUM_MACOS@/$MACOSX_DEPLOYMENT_TARGET/g" \
  "$here/Info.plist.in" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" "$here/zh-Hans.lproj/InfoPlist.strings"
# Named like the app, so its menu reads "Quit VERA20k".
cp "$package/vera20k" "$app/Contents/MacOS/VERA20k"
cp "$package/README.txt" "$package/LICENSE-GPL" "$package/THIRD_PARTY_LICENSES.html" \
  "$app/Contents/Resources/"
cp -R "$here/zh-Hans.lproj" "$app/Contents/Resources/"
# The icon, drawn from its SVG at every size macOS uses.
iconset="$stage/VERA20k.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
  sips -s format png -z "$size" "$size" "$here/../icon.svg" \
    --out "$iconset/icon_${size}x${size}.png" > /dev/null
  sips -s format png -z $((size * 2)) $((size * 2)) "$here/../icon.svg" \
    --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns -o "$app/Contents/Resources/VERA20k.icns" "$iconset"
codesign --force --sign - "$app"
codesign --verify --strict --verbose=2 "$app"

python3 -m venv "$stage/venv"
"$stage/venv/bin/pip" install --quiet --disable-pip-version-check --require-hashes \
  -r "$here/requirements.txt"
# hdiutil, which dmgbuild runs, now and then fails on hosted runners with
# "Resource busy".
for attempt in 1 2 3; do
  rm -f "$dmg"
  if "$stage/venv/bin/dmgbuild" -s "$here/dmg_settings.py" -D app="$app" VERA20k "$dmg"; then
    exit 0
  fi
  sleep $((attempt * 5))
done
echo "dmgbuild could not create $dmg" >&2
exit 1
