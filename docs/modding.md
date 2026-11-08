# Modding VERA20k

VERA20k is a new game engine that reads the same files as Red Alert 2: Yuri's Revenge.
It isn't a patch on `gamemd.exe`. That decides what works: a mod that only changes the
game's data files works the same way here, and a mod that changes the program doesn't.

## How it differs from the original

| | Original game | VERA20k |
| --- | --- | --- |
| Program | `gamemd.exe`, for Windows | A new engine for Windows, macOS and Linux |
| Game files | MIX, INI, SHP, VXL, CSF and maps | The same files, searched in the same order |
| File tools | FinalAlert 2, XCC Mixer and others | The same tools, because the files are the same |
| DLL mods such as Ares and Phobos, and exe patches | Run inside the game | Can't run. Parts of Ares and Phobos are rewritten in VERA20k, but only in [builds you make yourself](#4-build-vera20k-yourself) |
| Map size | Width plus height up to 512 | Up to 1024 |
| Players | 8 | Up to 30, on maps made for it |
| Changing rules | Edit the game's files | `VERARULES.INI` does it without touching them |
| Playing online | With the original game | Only with other VERA20k players |

VERA20k aims to play exactly like the original. Some things happen in a different order
inside a frame so it can handle 20,000 units, but you shouldn't be able to see that. If
you can, it's a bug.

## What you can and can't do

| You want to | Can you? | How |
| --- | --- | --- |
| Change costs, health, damage, speed and other stats | Yes | [`VERARULES.INI`](#1-your-own-rules-verarulesini) or `rulesmd.ini` |
| Change the rules of one map | Yes | [In the map file](#2-a-map) |
| Change a game mode, or add one | Yes | `MPModesMD.ini` and the INI file each mode names |
| Add new units, buildings and weapons | Yes | `rulesmd.ini`, `artmd.ini` and the art they need, as in the original |
| Replace pictures, models, sounds and music | Yes | [The game's own files](#3-the-games-own-files) |
| Change how the AI plays | Yes | `aimd.ini`, or in the map file |
| Change unit names and other game text | Yes | `ra2md.csf`, or a [language file](../src/i18n/README.md#other-text) |
| Make bigger maps, or maps for 9 to 30 players | Yes | [In the map file](#2-a-map) |
| Play your mod online | Yes, if everyone matches | [Playing together](#playing-together) |
| Change how menus and the sidebar look | Partly | You can replace their pictures. VERA20k draws them itself, so you can't move things around |
| Use Ares or Phobos INI keys, or `[#include]` and `[$Include]` | Partly | Only [in your own build](#4-build-vera20k-yourself), and not every key. See also [INI directives](ini-directives.md) |
| Load DLLs, Syringe or a patched `gamemd.exe` | No | VERA20k never runs that code |
| Add new behaviour with a script or plugin | No | There's no scripting or plugin system. New behaviour means changing VERA20k's code |
| Play a big conversion such as Mental Omega | Not as a whole | These depend on DLL features that VERA20k has only in part |
| Play without the original game | No | Everyone needs their own copy |

## Before you start

You'll use two folders:

- **The game folder** is your copy of Red Alert 2: Yuri's Revenge, with the `.mix` files
  in it. The `ra2_dir` line in `config.toml` says where it is.
- **The settings folder** is VERA20k's own. It holds `config.toml`, `VERA.INI`, saves and
  logs.

| System | Settings folder |
| --- | --- |
| Windows | The folder you installed VERA20k in, next to `vera20k.exe` |
| macOS | `~/Library/Application Support/VERA20k/` |
| Linux | `~/.local/share/VERA20k/` |

To see what you can change, read the game's own INI files:

- **Get the files.** They're packed inside the `.mix` files. `rulesmd.ini` is usually in
  `expandmd01.mix`, and most others, such as `artmd.ini`, are in `localmd.mix`, inside
  `ra2md.mix`. On Windows, XCC Mixer can open these and copy files out. On macOS or
  Linux, [build VERA20k](../README.md#build-and-run), then run this in its source folder.
  The files land in a new `ini` folder:

  ```sh
  cargo run --release --bin extract-ini -- "<game folder>"
  ```

- **Look up what the keys do.** [ModEnc](https://modenc.renegadeprojects.com/) is the
  wiki for Red Alert 2 modding.

## Four ways to mod, easiest first

### 1. Your own rules: `VERARULES.INI`

The game creates this file in the settings folder, next to `VERA.INI`. Use the same
section names and keys as `rulesmd.ini`:

```ini
[E1]
Cost=100
Strength=200
```

This file changes rules only. It can't replace art or sounds, and a map's own rules win
over it unless you turn that off. [More about this file](personal-rules.md).

### 2. A map

As in the original, a map can change any rule for that match. To add a map, give it a
`.yrm` ending and put it in the game folder, or in a `Maps` folder inside the settings
folder.

VERA20k adds two things. Open the map in a text editor to set them:

- **Bigger maps:** under `[Map]`, the width and height in `Size=` can add up to 1024.
- **More players:** under `[Basic]`, add `MaxPlayers=` with a number from 9 to 30. The
  map needs a start waypoint for every player, numbered from 0. For 30 players that's
  waypoints 0 to 29.

### 3. The game's own files

For art, sounds, the AI or the main rules, work on a copy so your real game stays clean:

1. Copy your whole game folder.
2. Open `config.toml` in the settings folder and change the `ra2_dir` line to the copy.
   Use forward slashes, even on Windows. With backslashes the game can't read the file:

   ```toml
   ra2_dir = "C:/Games/My Mod"
   ```

3. Put your files in the copy, in either of these two ways:

| Way | How it works |
| --- | --- |
| Loose files | Put the file next to the `.mix` files. It wins over every copy inside an archive. |
| A MIX archive | Name it `expandmd50.mix`. The number can be 00 to 99, and higher numbers win. All of them win over `ra2md.mix` and `ra2.mix`. Pick a number no other file uses. |

Yuri's Revenge reads the files ending in `md`, such as `rulesmd.ini` and `artmd.ini`.
Changing `rules.ini` or `art.ini` does nothing. To use your own rules, art and AI files
under other names, see [INI profiles](ini-roots.md).

There's no mod menu. VERA20k plays whatever is in the folder `ra2_dir` points to, so keep
one copy for each mod and change `ra2_dir` to switch. Point it back at your original
folder to play the normal game. `VERARULES.INI` stays in the settings folder, so it
applies whichever folder you use.

### 4. Build VERA20k yourself

The official downloads don't include the Ares, Phobos and other extensions, and no INI
file can add them. To use them, follow [Build and run](../README.md#build-and-run) and
name the ones you want:

```sh
cargo run --release --bin vera20k --features ares,phobos
```

Each one only covers part of the original. [Ares](ares.md), [Phobos](phobos.md),
[NPatch](npatch.md) and [VeraX](verax.md) list what's there. Your build can only play
with builds that have the same extensions, and only loads saves made with the same ones.

Building it yourself is also how you add behaviour the engine doesn't have.

## When a change doesn't work

- Restart the game and start a new match. A loaded save keeps the rules it was saved with.
- Check your spelling. A wrong section or key name is ignored, with no error. Capitals
  count: `Cost=` works, `cost=` doesn't.
- `;` starts a comment. `#` doesn't.
- `Key=` with nothing after it is ignored, so it can't clear a value set earlier.
- A list replaces the whole old list, so write out all of it.
- Only the first 511 characters of a line are read.
- Look in `logs/ra2.log`, in the settings folder. It shows which archives were found and
  any problems loading them.

## Playing together

A match starts only if every player has the same:

- VERA20k version and build options
- `rulesmd.ini`, `artmd.ini`, `aimd.ini`, the game mode's INI file, and `LANGRULE.INI` if
  there is one
- `VERARULES.INI`
- map

Quick Match pairs you with anyone on the same VERA20k version, so use it with an
unchanged game. One changed line in `VERARULES.INI` is enough to stop the match from
starting. Before you use it, put `;` in front of your changes and point `ra2_dir` back
at your original folder.

In an online room, players who don't have your map are asked whether to download it.
It's saved in their `Maps` folder. Some rooms take only official maps.

The game doesn't compare pictures, models or sounds, so give everyone the same mod folder.
A replay only plays back correctly with the rules it was recorded with.

Please don't use mods to cheat on the official servers. See the
[online rules](../LEGAL.md#online-play).

## Sharing your mod

- Give people the files you made or changed, not the whole game folder. They put them in
  a copy of their own game folder, as in [step 3](#3-the-games-own-files). A
  `VERARULES.INI` goes in their settings folder.
- Everyone needs their own copy of the game. Don't share VERA20k together with the game's
  files.
- You can say your mod is made for VERA20k or works with it.
- If you share a changed VERA20k program, you must share its full source code too, and
  give it your own name. See the [legal notice](../LEGAL.md).

## What isn't finished

VERA20k is still being built. Campaign missions launch, but playing them to the end still
needs work, so skirmish and online play are the best place for a mod today.

If a mod that only changes data files looks or plays differently here than in the
original, that's a bug. Tell us on [Discord](https://discord.gg/kmjRUn5m5F) or in the QQ
group (1097193563), or [open an issue](https://github.com/YuriPlanet/vera20k/issues).
