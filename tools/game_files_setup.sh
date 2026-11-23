#!/bin/sh
# Fetch a licensed Yuri's Revenge copy, keep the files the engine reads, and
# point this checkout at them.
#
#     sh tools/game_files_setup.sh
#     sh tools/game_files_setup.sh "<a Yuri's Revenge folder you already have>"
#     sh tools/game_files_setup.sh "<the archive that folder came in>"
#
# The game files are EA's. The only source this script downloads from is a
# Steam account with EA Play: EA publishes "Command & Conquer Red Alert 2 and
# Yuri's Revenge" (appid 2229850) through the EA Play subscription on Steam,
# so steamcmd asks for that account and its password here, in your terminal.
# Nothing is downloaded from any other place, and nothing leaves your machine:
# the retail copy stays in the ignored game-files/ folder of this checkout.
#
# A copy you already have needs none of this: pass its folder, or the archive
# it came in, as the first argument.
#
# The copy is staged and checked before it replaces the target, so a source
# that turns out to be incomplete leaves the working folder alone.

set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
source=${1:-"$root/game-files/steam"}
target=${2:-"$root/game-files/yr"}
appid=2229850

# A copy the player already has needs no download: use it as the source.
if [ -f "$source/ra2md.mix" ]; then
    echo "using the retail folder $source"
elif [ -f "$source" ]; then
    echo "using the archive $source"
else
    if ! command -v steamcmd >/dev/null 2>&1; then
        if command -v brew >/dev/null 2>&1; then
            echo "installing steamcmd"
            brew install steamcmd
        else
            echo "steamcmd is not installed; see https://developer.valvesoftware.com/wiki/SteamCMD" >&2
            exit 1
        fi
    fi

    mkdir -p "$source"
    echo "logging in to Steam; EA Play carries this application, so the account needs it"
    echo "if steamcmd answers 'No subscription', subscribe to EA Play on Steam and run this again"
    steamcmd "+force_install_dir $source" "+login" "+app_update $appid" "+quit"
fi

cd "$root"
python3 -m tools.game_files prepare --source "$source" --target "$target"

echo
echo "running a headless match on the prepared copy"
# The install check above only proves the archives are there. A match loads the
# rules, the art and the AI and plays them, so it is what says the content
# works.
if ! python3 -m tools.game_files smoke --dir "$target"; then
    echo "the match did not run; the game folder is $target" >&2
    exit 1
fi

echo
echo "done. the game folder is $target"
echo "run the game with the executable this checkout built:"
echo "  cd $root"
echo "  $(cd "$root" && python3 -m tools.cargo_run --resolve vera20k --profile fast)"
