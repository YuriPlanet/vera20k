# Modding VERA20k

VERA20k is a new game engine that reads the same files as Red Alert 2: Yuri's Revenge.
It isn't a patch on `gamemd.exe`. A mod that only changes the game's data files works
here, and a mod that changes the program doesn't.

## How it differs from the original

| | Original game | VERA20k |
| --- | --- | --- |
| Program | `gamemd.exe`, for Windows | A new engine for Windows, macOS and Linux |
| Game files | MIX, INI, SHP, VXL, CSF and maps | The same files, searched in the same order |
| DLL mods and exe patches | Run inside the game | Can't run |
| Map size | Width plus height up to 512 | Up to 1024 |
| Players | 8 | Up to 30, on maps made for it |
| Player colours | 8 | 30, and a mod can [list its own](skirmish-colors.md) |
| Playing online | With the original game | Only with other VERA20k players |

## What you can and can't do

**You can:**

- Change costs, health, damage, speed and other stats.
- Add new units, buildings and weapons.
- Replace pictures, models, sounds and music.
- Change the AI, the game modes and the game's text.
- Make your own maps, including bigger ones and maps for up to 30 players.
- Play your mod online, if everyone has the same files.

**Only partly:**

- Menus. You can replace their pictures, but you can't move things around.
- The match screen. You can replace its pictures, and with an
  [interface layout](interface-layout.md) put the sidebar on any edge, put the command
  bar on top or leave it out, move the sidebar's parts, leave parts out, change its
  size and its columns of build pictures, place the messages, timers, banner and
  tooltips, and add buttons, text and more grids of build pictures. Players can drag
  the parts in a match and keep their own layout. You can't yet add other kinds of
  parts.
- Ares and Phobos. Only some of their keys work, and only in a build you make yourself.
  See [More detail](#more-detail).
- Big conversions such as Mental Omega. They depend on DLL features, so they don't work
  as a whole.

**You can't:**

- Load DLLs, Syringe or a patched `gamemd.exe`.
- Add new behaviour with a script or plugin. VERA20k has neither, so that means changing
  its code.
- Play without the original game.

## Where things go

VERA20k uses two folders: its own folder, and the game folder. Your mods go in the
first, and the game folder is never changed.

### The VERA20k folder

| System | Where it is |
| --- | --- |
| Windows | The folder you installed VERA20k in, next to `vera20k.exe`. **VERA20K Mods** in the Start menu opens its `Mods` folder. |
| macOS | `~/Library/Application Support/VERA20k/` |
| Linux | `~/.local/share/VERA20k/` |

| What you can put here | What it does |
| --- | --- |
| A folder for your mod, inside `Mods` | Holds the mod's files. See [The Mods folder](#the-mods-folder). |
| `VERARULES.INI` | Your own rule changes, with or without a mod. The game creates this file for you. |
| A `Maps` folder of `.yrm` files | Adds your maps to the map list, with or without a mod. Create the folder if it isn't there. Maps you download online go here too. |
| `vera20k.<language>.ini` | Adds a language, or changes the game's text. See [translating](../src/i18n/README.md). |

`config.toml` is here too. It says where the game folder is and which mod to play. The
game writes everything else itself, such as `VERA.INI` (your options and hotkeys),
`saves`, `replays` and `logs`.

Art, sounds and files such as `rulesmd.ini` do nothing directly in this folder. They go
in a mod's folder. Don't share this folder with anyone, because it holds your online
sign-in.

### The Mods folder

`Mods` holds one folder for each mod. The game makes `Mods` the first time it starts,
with a `README.txt` in it.

1. Make a folder inside `Mods` and give it your mod's name, such as `Mods/MyMod`.
2. Put the mod's files in it, under the same names as in the original game.
3. [Pick the mod](#pick-a-mod).

The game then reads the mod's files as if you had copied them on top of the game folder.
Nothing is copied, and the game folder isn't touched. A mod made to be copied into the
game folder works the same from its own folder.

| What you can put in a mod's folder | What it does |
| --- | --- |
| A file named the same as a game file, such as `rulesmd.ini` or a `.shp` | Replaces the game's copy. It must be directly in the mod's folder, not in a subfolder. |
| An archive named `expandmd50.mix` | Replaces the game's copies of the files inside it, except the ones in `langmd.mix` and `language.mix`, which the original game reads first. An Ares build reads the `expandmd` archives before those two as well, as Ares does. The number can be 00 to 99, and higher numbers win. An archive with the same name as one in the game folder takes that archive's place, so pick a number the game doesn't use unless you mean that. |
| `ares.mix` (Ares builds) | Read after the `expandmd` archives and before the game's own, as Ares reads it. The string tables `ares.csf` and `ares_<language>.csf` in it add to the game's text. |
| An archive whose name starts with `ecache` or `elocal`, such as `ecachemd01.mix` | Adds the files inside it, as the original game does. All `ecache` archives load before all `elocal` ones, each group by name. They load after the `expandmd` archives and the game's main, cache and local archives, which keep a file name they share, and before the game's art, cameo and theater archives, whose copies they replace. |
| Maps | Adds them to the map list while the mod is in use. |
| `veraui.ini` | Lays out the match screen: which edge the sidebar and the command bar sit on, where the sidebar's parts sit, which it has, and how big it is. See [interface layout](interface-layout.md). |
| `mod.ini` | Says what the mod is called. Optional. |

#### mod.ini

```ini
[Mod]
Name=My Mod
Version=1.2
Needs=0.2.11
```

Every line is optional. Save the file as UTF-8. As in the game's own files, `;` starts
a comment.

- `Name` and `Version` are what the game calls the mod. Without them it uses the
  folder's name.
- `Needs` is the oldest VERA20k version that can play the mod. An older game says so and
  starts without the mod. Leave it out unless the mod uses something a newer version added.

#### Pick a mod

Open `config.toml` and add a `mod` line under `[paths]` with the folder's name:

```toml
[paths]
ra2_dir = "C:/Games/Red Alert 2"
mod = "MyMod"
```

Remove the line to play the original game. There's no mod menu yet.

A switch after the program's name picks a mod for one start, whatever `config.toml`
says. Put it on a shortcut to start a mod with one click:

| Switch | What it does |
| --- | --- |
| `--mod MyMod` | Plays with the mod in `Mods/MyMod`. |
| `--no-mod` | Plays the original game. |

#### If a mod doesn't start

- If the folder isn't there, or the mod needs a newer VERA20k, the game starts without
  the mod and says why in the main menu.
- If the game can't start with a mod, the next start is without it, so you always get
  back to the menu. Start the game once more to try the mod again.

### The game folder

This is your Red Alert 2: Yuri's Revenge folder, the one with the `.mix` files in it.
VERA20k only reads it. The `ra2_dir` line in `config.toml` says where it is. Use forward
slashes there, even on Windows. With backslashes the game can't read the file:

```toml
ra2_dir = "C:/Games/Red Alert 2"
```

You don't need to put anything in this folder. A file you do put there, as you would for
the original game, is used with every mod and without one.

## The basics

### Find what to change

Mods change the settings in the game's INI files, so start by reading them. Most of what
you'll want is in `rulesmd.ini` (units, buildings and weapons) and `artmd.ini` (how they
look). They're packed in the game's `.mix` files: `rulesmd.ini` is usually in
`expandmd01.mix`, and `artmd.ini` is in `localmd.mix`, inside `ra2md.mix`.

- **On Windows,** XCC Mixer can open these and copy files out.
- **On macOS or Linux,** [build VERA20k](../README.md#build-and-run), then run
  `cargo run --release --bin extract-ini -- "<game folder>"` in its source folder. The
  files land in a new `ini` folder.

Copy the ones you want to change into your mod's folder.

[ModEnc](https://modenc.renegadeprojects.com/) explains what the keys do. Yuri's Revenge
only reads the files ending in `md`, so changing `rules.ini` or `art.ini` does nothing.

### Change a unit

Open `VERARULES.INI` and write the unit's section with only the keys you want to change.
This changes the GI's cost and health:

```ini
[E1]
Cost=100
Strength=200
```

A map's own rules win over this file, unless you
[turn that off](personal-rules.md#mission-and-map-priority).

### Replace art, sounds, the AI or the main rules

Put your version in your mod's folder under the same name, such as `rulesmd.ini` or
`aimd.ini`. To add new units, buildings or weapons, add them to `rulesmd.ini` and
`artmd.ini` as in the original, and put their art in the same folder.

### Add a map

Give the map a `.yrm` ending and put it in the `Maps` folder, or in your mod's folder if
the map needs the mod. As in the original, a map can change any rule for that match.

VERA20k adds two things. Open the map in a text editor to set them:

- **Bigger maps:** under `[Map]`, the width and height in `Size=` can add up to 1024.
- **More players:** under `[Basic]`, add `MaxPlayers=` with a number from 9 to 30. The
  map needs a start waypoint for every player, numbered from 0.

### If a change doesn't work

- Restart the game and start a new match. A loaded save keeps the rules it was saved with.
- Check that the game uses your mod. Near its start, `logs/ra2.log` has a line beginning
  `Mod in use:`.
- Check your spelling. A wrong name is ignored with no error, and capitals count: `Cost=`
  works, `cost=` doesn't.
- `;` starts a comment. `#` doesn't.
- Look in `logs/ra2.log`, in the VERA20k folder.

## Playing with others

A match starts only if everyone has the same VERA20k version and build, the same map, and
the same `rulesmd.ini`, `artmd.ini`, `aimd.ini` and `VERARULES.INI`. Pictures, sounds and
`veraui.ini` aren't compared, so give everyone the same mod files. Players who don't have your map are
asked whether to download it when they join your online room.

Quick Match pairs you with anyone on the same VERA20k version, so use it with an unchanged
game. Remove the `mod` line from `config.toml` or start with `--no-mod`, and put `;` in
front of your changes in `VERARULES.INI`.

## Sharing your mod

- Give people your mod's folder, with only the files you made or changed in it. They put
  it in their own `Mods` folder and pick it. A `VERARULES.INI` goes in their VERA20k folder.
- Everyone needs their own copy of the game. Don't share VERA20k together with the game's
  files.
- If you share a changed VERA20k program, you must share its source code too and give it
  your own name. See the [legal notice](../LEGAL.md).

## More detail

- [Personal rules](personal-rules.md): everything about `VERARULES.INI`.
- [INI profiles](ini-roots.md): use your own rules, art and AI files under other names.
- [Interface layout](interface-layout.md): lay out the match screen with `veraui.ini`.
- [Ares](ares.md), [Phobos](phobos.md), [NPatch](npatch.md) and [VeraX](verax.md): what
  each extension covers and how to build VERA20k with it. The official downloads don't
  include them, and no INI file can add them.

## Something wrong?

Campaigns aren't finished yet, so test your mod in skirmish. If a mod
that only changes data files looks or plays differently here than in the original, that's
a bug. Tell us on [Discord](https://discord.gg/kmjRUn5m5F) or
[open an issue](https://github.com/YuriPlanet/vera20k/issues).
