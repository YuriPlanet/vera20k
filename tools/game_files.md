# Local game files

VERA20k never ships retail game files. The engine reads them straight out of a
Yuri's Revenge installation: `ra2md.mix`, `ra2.mix` and their siblings are
opened from a game folder that the player owns. This page is how that folder is
found, checked and kept ready for the next launch.

The tool is `tools/game_files.py`; it decides nothing about what the game
needs. `verify` runs the engine's own installer check
(`vera20k --check-game-data`, the same read-only pass the Windows installer
runs before it copies anything), so the answer comes from the production
readers.

## Where the files come from

The retail archives are EA's, and they cannot be redistributed. Every source
below is one the player already has:

| Source | What to do |
| --- | --- |
| An EA Play subscription on Steam | EA publishes *Command & Conquer Red Alert 2 and Yuri's Revenge* (appid `2229850`) through the EA Play subscription on Steam, so the account that downloads it needs that subscription. `steamcmd` with that account installs the retail files; an anonymous session is refused with `No subscription`. |
| A retail YR folder | Copy it, or point the checkout at it where it already is. |
| The archive that folder came in | A download is often a zip or a split rar; `prepare` unpacks it. |
| An unpacked disc | Extract it with the tool of your choice, then use the folder. |

### What the subscription costs

EA Play on Steam (appid `1289670`) is $0.99 for the first month and $5.99 a
month after that in the US store; other regions differ. The subscription is
what carries the game: an anonymous `steamcmd` session is refused with
`No subscription`, and the depot is about 1.2 GB.

The Steam release of the game is Windows-only, but `steamcmd` downloads
Windows depots from any host, so the Mac needs nothing else. A copy that
came from another machine works just as well: point the tool at the folder.

## Prepare the folder

```sh
# 1. Check a folder the player already has.
python -m tools.game_files verify --dir "<Yuri's Revenge folder>"

# 2. Fill the checkout's folder from it, check the copy, and point the
#    checkout at the result. The source may be a folder or the archive it
#    came in.
python -m tools.game_files prepare --source "<Yuri's Revenge folder or archive>" --target game-files/yr
```

`game-files/` is ignored by Git, so a retail copy never lands in a commit.
`prepare` stages the copy beside the target, checks it there, and only then
replaces the target and writes `config.toml`; a source that turns out to be
incomplete leaves the working folder and the configuration untouched.

The copy keeps the archives and the data files the engine reads (`.mix`,
`.csf`, `.pal`, `.shp`, `.wav`, `.fnt`) plus the loose INIs, movies and
scenario files a folder may keep (`.ini`, `.bik`, `.yro`, `.yrm`, `.pkt`,
`.mmx`, `.mpr`, `.map`), and drops the executables and the installers. The
loose files stay because the engine resolves a name from the folder before the
archives, the way gamemd reads its disk files first: a loose `RULESMD.INI` or
a loose scenario pack is content, not clutter.

A Steam installation puts the game in a subfolder of the install directory;
`prepare` and `verify` find the folder that holds `ra2md.mix` themselves.

The executable the checks use is this checkout's build. Name another one with
`--binary <vera20k>` or with `VERA20K_GAME_BINARY=<vera20k>`.

## Fetch it from Steam

```sh
brew install steamcmd
python -m tools.game_files steam --target game-files/steam
```

The command runs `steamcmd`, which asks for the Steam account and its
password. Then fill the checkout's folder from the installation:

```sh
python -m tools.game_files prepare --source game-files/steam --target game-files/yr
```

`tools/game_files_setup.sh` does the whole chain in one go:

```sh
sh tools/game_files_setup.sh
```

It installs `steamcmd`, runs the Steam login, prepares the folder, writes
`config.toml`, runs a headless match on the copy and prints the executable to
launch.

## What the checks say

`verify` runs the engine's installer check, which reports the retail files
that are missing. It is an installer check, not a launch proof: the files
alone do not say that the menu draws or that a match plays. `smoke` runs a
headless match on the folder, which loads the rules, the art and the AI and
plays them:

```sh
python -m tools.game_files smoke --dir game-files/yr
```

The setup script runs it after preparing the folder, so the one command ends
with the content proven rather than only present.

## Inventory a copy

```sh
python -m tools.game_files manifest --dir game-files/yr --out manifest.json
```

The manifest lists every file with its size and SHA-256. It is a fingerprint of
a copy the player owns, useful for telling two installations apart or for
checking that a copy is intact; the files themselves stay where they belong.

## Running in a small window

`VERA.INI` in the user data folder holds the window size. `AllowModeToggle=yes`
takes the `640x480` branch gamemd uses for its 640x480 shell, so the player
window opens at that size instead of VERA's default `1024x768`:

```ini
[Video]
AllowModeToggle=yes
ScreenWidth=640
ScreenHeight=480

[VERA]
FullScreen=no
```

The window is resizable, and the match takes the window's size.
