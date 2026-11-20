#!/bin/sh
# Fetch a licensed Yuri's Revenge copy, keep the files the engine reads, and
# point this checkout at them.
#
#     sh tools/game_files_setup.sh
#     sh tools/game_files_setup.sh "<a Yuri's Revenge folder you already have>"
#
# The game files are EA's. The only source this script knows is a Steam
# account that owns "Command & Conquer Red Alert 2 and Yuri's Revenge"
# (appid 2229850), so steamcmd asks for that account and its password here,
# in your terminal. Nothing is downloaded from any other place, and nothing
# leaves your machine: the retail copy stays in the ignored game-files/
# folder of this checkout.
#
# A retail folder you already have needs none of this:
#
#     python3 -m tools.game_files slim --source "<Yuri's Revenge folder>" --target game-files/yr
#     python3 -m tools.game_files config --dir game-files/yr

set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
install_dir=${1:-"$root/game-files/steam"}
target=${2:-"$root/game-files/yr"}
appid=2229850

# A folder the player already has needs no download: use it as the source.
if [ -f "$install_dir/ra2md.mix" ]; then
    echo "using the retail folder $install_dir"
    cd "$root"
    case "$target" in
        "$root/game-files"/*)
            rm -rf "${target:?}"
            ;;
    esac
    python3 -m tools.game_files slim --source "$install_dir" --target "$target"
    python3 -m tools.game_files config --dir "$target"
    echo
    echo "done. the game folder is $target"
    echo "run the game with the executable this checkout built:"
    echo "  cd $root"
    echo "  $(cd "$root" && python3 -m tools.cargo_run --resolve vera20k --profile fast)"
    exit 0
fi

if ! command -v steamcmd >/dev/null 2>&1; then
    if command -v brew >/dev/null 2>&1; then
        echo "installing steamcmd"
        brew install steamcmd
    else
        echo "steamcmd is not installed; see https://developer.valvesoftware.com/wiki/SteamCMD" >&2
        exit 1
    fi
fi

mkdir -p "$install_dir"
echo "logging in to Steam; the application is paid, so an anonymous session cannot install it"
steamcmd "+force_install_dir $install_dir" "+login" "+app_update $appid" "+quit"

echo "keeping only the files the engine reads"
cd "$root"
# A stale file in the target shadows the copy that replaces it, so the
# checkout's own folder starts empty.
case "$target" in
    "$root/game-files"/*)
        rm -rf "${target:?}"
        ;;
esac
python3 -m tools.game_files slim --source "$install_dir" --target "$target"
python3 -m tools.game_files config --dir "$target"

echo
echo "done. the game folder is $target"
echo "run the game with the executable this checkout built:"
echo "  cd $root"
echo "  $(cd "$root" && python3 -m tools.cargo_run --resolve vera20k --profile fast)"
