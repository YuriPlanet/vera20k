#!/usr/bin/env bash
# Copy every language but English into a packaged release folder, for the
# Release workflows (.github/workflows/release.yml, windows-signing-check.yml):
#   languages.sh <package folder>
# English is compiled into the program, which those workflows build with
# VERA20K_LANGUAGE_FOLDER (build.rs). Each installer puts the folder this makes
# where the game looks for it (packaged_language_dir in src/i18n/mod.rs). The
# letters of vera20k.<tag>.letters are compiled into the font code instead.
set -euo pipefail
shopt -s nullglob
languages="$1/languages"
source="$(dirname "$0")/../../src/i18n"
mkdir "$languages"
for file in "$source"/vera20k.*.ini "$source"/vera20k.*.subtitles.txt "$source"/vera20k.*.cameos; do
  if [ "$(basename "$file")" != vera20k.en.ini ]; then
    cp "$file" "$languages/"
  fi
done
