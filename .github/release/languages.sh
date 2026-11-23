#!/usr/bin/env bash
# Copy every language but English and the fonts behind them into a packaged
# release folder, for the Release workflows (.github/workflows/release.yml,
# windows-signing-check.yml):
#   languages.sh <package folder>
# English and Noto Sans are compiled into the program, which those workflows
# build with VERA20K_LANGUAGE_FOLDER (build.rs), as are the letters the game
# draws before it reads any of this: vera20k.<tag>.letters and Unifont's table.
# Each installer puts the folder this makes where the game looks for it
# (packaged_language_dir in src/i18n/mod.rs).
set -euo pipefail
shopt -s nullglob
languages="$1/languages"
source="$(dirname "$0")/../../src"
mkdir "$languages"
for file in "$source"/i18n/vera20k.*.ini "$source"/i18n/vera20k.*.subtitles.txt \
  "$source"/i18n/vera20k.*.cameos "$source"/render/fonts/*.ttf "$source"/render/fonts/*.otf; do
  case "$(basename "$file")" in
    vera20k.en.ini | NotoSans-VF.ttf) ;;
    *) cp "$file" "$languages/" ;;
  esac
done
