#!/usr/bin/env bash
# Put a packaged Linux release folder in a single-file AppImage, for the Release
# workflow (.github/workflows/release.yml):
#   appimage.sh <package folder> <output .AppImage>
# Needs desktop-file-validate (desktop-file-utils).
#
# The AppImage keeps its files in ~/.local/share/VERA20k and finds or asks for
# the Yuri's Revenge folder on its first launch (src/app/app_bundle.rs). It holds
# only the program, which uses the system's ALSA, Vulkan and window libraries.
# README.txt and the licenses are in usr/share/doc/vera20k.
set -euo pipefail
package="$1"
output="$2"
here="$(cd "$(dirname "$0")" && pwd)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

# appimagetool, and the AppImage runtime it puts in front of the files, pinned
# by checksum; without --runtime-file, appimagetool downloads the newest.
fetch() {
  curl -sSfL --retry 3 -o "$2" "$1"
  echo "$3  $2" | sha256sum --check --strict --quiet
}
fetch https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage \
  "$stage/appimagetool" ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
fetch https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64 \
  "$stage/runtime" 2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
chmod +x "$stage/appimagetool"

app="$stage/VERA20K.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/doc/vera20k"
cp "$package/vera20k" "$app/usr/bin/"
cp "$package/README.txt" "$package/LICENSE-GPL" "$package/THIRD_PARTY_LICENSES.html" \
  "$app/usr/share/doc/vera20k/"
ln -s usr/bin/vera20k "$app/AppRun"
cp "$here/vera20k.desktop" "$app/"
# Red Alert 2's red star, as the icon desktops show and as the .DirIcon file
# managers read; appimagetool keeps a .DirIcon that is a file.
cp "$here/../icon.png" "$app/vera20k.png"
cp "$here/../icon.png" "$app/.DirIcon"

# appimagetool is an AppImage itself; extracted, it runs without FUSE.
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$stage/appimagetool" --no-appstream \
  --runtime-file "$stage/runtime" "$app" "$output"
