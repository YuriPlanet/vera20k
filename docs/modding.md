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
- Ares and Phobos. Only some of their keys work. A game with Ares keys turns Ares on by
  itself, and a mod that ships `ares.mix` gets all the Ares support VERA20k has. Phobos
  needs a build you make yourself. See [More detail](#more-detail).
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
| A `Maps` folder of `.yrm` files | Adds your maps to the map list, with or without a mod. Create the folder if it isn't there. Maps you download online go here too. |
| `vera20k.<language>.ini` | Adds a language, or changes the game's text. See [translating](../src/i18n/README.md). |

`config.toml` is here too. It says where the game folder is and which mod to play; the
Mods menu writes the mod for you. The game writes everything else itself, such as
`VERA.INI` (your options and hotkeys), `saves`, `replays` and `logs`.

Art, sounds and files such as `rulesmd.ini` do nothing directly in this folder. They go
in a mod's folder. Don't share this folder with anyone, because it holds your online
sign-in.

### The Mods folder

`Mods` holds one folder for each mod. The game makes `Mods` the first time it starts,
with a `README.txt` in it.

1. Make a folder inside `Mods` and give it your mod's name, such as `Mods/MyMod`.
   Or press **New Mod** in the Mods menu, which makes one with the original game's
   files in it: see [Start a new mod](#start-a-new-mod).
2. Put the mod's files in it, under the same names as in the original game.
3. [Pick the mod](#pick-a-mod).

The game then reads the mod's files as if you had copied them on top of the game folder.
Nothing is copied, and the game folder isn't touched. A mod made to be copied into the
game folder works the same from its own folder.

With **No mod** selected, an `ares.mix` already in the game folder is ignored, unless
the [Ares setting](ares.md#the-ares-setting) is On; set to Off, no `ares.mix` is used.
A selected mod can use that copy or provide its own, which takes precedence.
The optional `langrule.ini` rules override is also skipped with **No mod**,
whether loose or inside an archive. Selected mods keep that layer, from the
installation or the mod, and its normal Digest check. This does not skip the
`.csf` files used for translated text.
Other changed rules and terrain files still need to match between players.

A mod is only data files, and VERA20k never runs a program from one. If a mod you
download comes with an `.exe`, a `.dll` or a launcher, VERA20k doesn't need it, so don't
run it.

| What you can put in a mod's folder | What it does |
| --- | --- |
| A file named the same as a game file, such as `rulesmd.ini` or a `.shp` | Replaces the game's copy. It must be directly in the mod's folder, not in a subfolder. |
| An archive named `expandmd50.mix` | Replaces the game's copies of the files inside it, except the ones in `langmd.mix` and `language.mix`, which the original game reads first. A game with `ares.mix` reads the `expandmd` archives before those two as well, as Ares does. The number can be 00 to 99, and higher numbers win. An archive with the same name as one in the game folder takes that archive's place, so pick a number the game doesn't use unless you mean that. |
| `ares.mix` | Makes the game an [Ares](ares.md) game: its INIs load with Ares' syntax and `[#include]`, and its rules are read the Ares way. Read after the `expandmd` archives and before the game's own, as Ares reads it. The string tables `ares.csf` and `ares_<language>.csf` in it add to the game's text. |
| An archive whose name starts with `ecache` or `elocal`, such as `ecachemd01.mix` | Adds the files inside it, as the original game does. All `ecache` archives load before all `elocal` ones, each group by name. They load after the `expandmd` archives and the game's main, cache and local archives, which keep a file name they share, and before the game's art, cameo and theater archives, whose copies they replace. |
| Maps | Adds them to the map list while the mod is in use. |
| Only INI files the game doesn't read, such as an AI another author made | Adds them to the map of every Skirmish and online match. See [Add to every map](#add-to-every-map). |
| `veraui.ini` | Lays out the match screen: which edge the sidebar and the command bar sit on, where the sidebar's parts sit, which it has, and how big it is. See [interface layout](interface-layout.md). |
| `mod.ini` | Says what the mod is called, who made it and what it is. Optional. |
| `mod-files.txt` | Lists the mod's files, so players can check their copy. The game writes it for you: see [Check a mod's files](#check-a-mods-files). Optional. |

#### mod.ini

```ini
[Mod]
Name=My Mod
Version=1.2
Author=Your name
About=A heavier Rhino tank and a new snow tank.
Terms=Free to share. Ask before you reuse the art.
Needs=0.2.11
```

Every line is optional. Save the file as UTF-8. As in the game's own files, `;` starts
a comment, so don't use one in your text. The game keeps the first 300 characters of a
line.

- `Name` and `Version` are what the game calls the mod. Without them it uses the
  folder's name.
- `Author`, `About` and `Terms` show in the Mods menu when the mod is picked: who made
  it, what it is in a line or two, and what others may do with your work.
- `Needs` is the oldest VERA20k version that can play the mod. An older game says so and
  plays without the mod, and its Mods menu shows what the mod needs. Leave it out unless
  the mod uses something a newer version added.

#### Pick a mod

Start the game, open **Options** and press **Mods**. The list shows the original game
as **No mod**, then every folder in `Mods`. Pick one and press **Use This Mod**. The game
restarts with the mod, and the main menu then shows the mod's name. To go back to the
original game, pick **No mod** the same way. **Open Mods Folder** opens `Mods`.

The menu keeps your choice in `config.toml`, as a `mod` line under `[paths]`, and makes
the file beside the program if there is none yet. You can write the line yourself:

```toml
[paths]
ra2_dir = "C:/Games/Red Alert 2"
mod = "MyMod"
```

A switch after the program's name picks a mod for one start, whatever `config.toml`
says. Put it on a shortcut to start a mod with one click:

| Switch | What it does |
| --- | --- |
| `--mod MyMod` | Plays with the mod in `Mods/MyMod`. |
| `--no-mod` | Plays the original game. |

#### Start a new mod

Press **New Mod** in the Mods menu. The game makes a folder in `Mods` called `New Mod`
(`New Mod 2` and on if that name is taken) and copies these files from your original
game into it: `rulesmd.ini`, `artmd.ini`, `aimd.ini`, `soundmd.ini`, `evamd.ini` and
`thememd.ini`. It also writes a `mod.ini` and a `veraui.ini` with every line commented
out, ready for you to fill in: the mod's name and maker, and its
[match screen layout](interface-layout.md), with an example to try. The copies are what
the game plays with no mod, so until you change them the new mod plays as the original
game.

1. Press **Open Folder** in the box that comes up, or **Open Mods Folder**.
2. Rename the folder if you like, and change the files.
3. [Pick the mod](#pick-a-mod).

To start over, delete the folder and press **New Mod** again. New Mod never writes into
a folder that is already there, and your original game isn't changed.

#### Check a mod's files

Pick a mod in the Mods menu and press **Check Files**. The game reads every file in the
mod's folder and tells you how many there are and their **code**, such as `4F2A-91C3`.
Two copies of a mod with the same code hold the same files, so if a friend can't join
your game, compare codes to see whether your copies differ.

A mod can carry a list of its files, `mod-files.txt`. With one, Check Files also says
whether your copy is the mod as its maker listed it. If it isn't, it says how many files
are changed, missing or not in the list. The log file, `logs/ra2.log` in the VERA20k
folder, names each one. Get the mod again to fix your copy.

If you made the mod, press **Make File List** in that box before you share it, and
**Update File List** after you change the mod. The list has one line for each file: the
SHA-256 of what the file holds, its size in bytes and its path. `mod.ini`, `veraui.ini`
and the list itself are not in it. Don't edit it by hand.

Playing online never trusts the list: the game still compares the files themselves, as
[Playing with others](#playing-with-others) says.

#### If a mod doesn't start

- If the folder isn't there, or the mod needs a newer VERA20k, the game starts without
  the mod and says why in the main menu. To stop the message, open Options, press Mods,
  pick **No mod** and press **Use This Mod**.
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
look). [New Mod](#start-a-new-mod) copies these and four more into a mod folder for you.

They're packed in the game's `.mix` files: `rulesmd.ini` is usually in `expandmd01.mix`,
and `artmd.ini` is in `localmd.mix`, inside `ra2md.mix`. For other files:

- **On Windows,** XCC Mixer can open these and copy files out.
- **On macOS or Linux,** [build VERA20k](../README.md#build-and-run), then run
  `cargo run --release --bin extract-ini -- "<game folder>"` in its source folder. The
  files land in a new `ini` folder.

Copy the ones you want to change into your mod's folder.

[ModEnc](https://modenc.renegadeprojects.com/) explains what the keys do. Yuri's Revenge
only reads the files ending in `md`, so changing `rules.ini` or `art.ini` does nothing.

### Change a unit

Make a mod with [New Mod](#start-a-new-mod), open its `rulesmd.ini` and find the unit's
section, such as `[E1]` for the GI. Change the keys you want and leave the rest as they
are. This makes the GI cheaper and tougher:

```ini
[E1]
Cost=100
Strength=200
```

Then [pick the mod](#pick-a-mod). As in the original game, a map's own rules win over the
mod's.

### Replace art, sounds, the AI or the main rules

Put your version in your mod's folder under the same name, such as `rulesmd.ini` or
`aimd.ini`. To add new units, buildings or weapons, add them to `rulesmd.ini` and
`artmd.ini` as in the original, and put their art in the same folder.

### Add to every map

Some files are made to be added to a map, such as an AI's teams and triggers. Put such a
file, as its maker ships it, in a folder of its own in `Mods`, such as
`Mods/Stronger AI/Stronger AI.ini`, and [pick that folder](#pick-a-mod). The game then
adds the file to the map of every Skirmish and online match. Campaign missions,
cooperative ones too, and random maps play without it.

This works only while the folder holds nothing else the game reads. Besides INI files
with names the game doesn't use, it may hold notes and pictures (such as `.txt`, `.md`,
`.pdf` and `.png` files, or a `LICENSE`), subfolders, `mod.ini`, `veraui.ini` and
`mod-files.txt`. Names that end in `md.ini`, such as `rulesmd.ini` or a mission's
`all04dmd.ini`, belong to the game. A folder with such a file, or any other, such as an
archive, is an ordinary mod and adds nothing to maps. The game's log says which files it
adds.

- Where the map and the file set the same key, the file's value wins, as launchers merge
  such files. Everything else in the file is added, sections and keys.
- With more than one such file, they are added in the order of their names, so a later
  file's value wins.
- A list such as `[TeamTypes]` takes the file's entries under their own keys. An entry
  under a key the map already uses replaces the map's, so files made for adding give
  each entry a key of its own.
- Keys that only Ares reads work only in an [Ares](ares.md) game.
- The game reads the files once when it starts: restart it after you change one.
  Everyone in an online match needs the same files, as with any mod.
- If a file can't be read, the game names it when a match starts. Fix the file, or pick
  another mod.

### Add a map

Give the map a `.yrm` ending and put it in the `Maps` folder, or in your mod's folder if
the map needs the mod. As in the original, a map can change any rule for that match.

VERA20k adds two things. Open the map in a text editor to set them:

- **Bigger maps:** under `[Map]`, the width and height in `Size=` can add up to 1024.
- **More players:** under `[Basic]`, add `MaxPlayers=` with a number from 9 to 30. The
  map needs a start waypoint for every player, numbered from 0.

If you write a map editor or tool, the [map format](map-format.md) has the details.

### If a change doesn't work

- Restart the game and start a new match. A loaded save keeps the rules it was saved with.
- Check that the game uses your mod. The main menu shows its name, and near its start
  `logs/ra2.log` has a line beginning `Mod in use:`.
- Check your spelling. A wrong name is ignored with no error, and capitals count: `Cost=`
  works, `cost=` doesn't.
- `;` starts a comment. `#` doesn't.
- Look in `logs/ra2.log`, in the VERA20k folder.

## Playing with others

A match starts only if everyone has the same VERA20k version and build, the same map, and
the same `rulesmd.ini`, `artmd.ini`, `aimd.ini`, theater INIs (such as `snowmd.ini`) and
terrain tiles. **Terrain Deformation** and the **Superweapon Limit** don't need to match: a
room plays its host's. With a mod in use,
everyone also needs the same files in the mod's folder: every file there is compared by
its name and what it holds, except `mod.ini`, `veraui.ini` and `mod-files.txt`. The folder's own name
doesn't matter, and a file name with letters outside plain English counts only by what
the file holds. Terrain tiles you put in the game folder itself, such as `clear01.sno` or
an `isosnomd.mix`, are compared with the original game's, because they decide how high the
ground is and where units can go. Other pictures and sounds there aren't compared.
Players who don't have your map are asked whether to download it when they join your
online room. When a match can't start because the files differ, each player's game says
what it changes from the original game (its mod, or files such as a loose `rulesmd.ini`),
or that it plays the original, so the difference is in another player's game.

Quick Match pairs you with anyone on the same VERA20k version, so it plays the original
game, without anyone's Terrain Deformation or Superweapon Limit. With a mod in use it tells
you to pick **No mod** in the Mods menu first. It also names
any files in the game folder that change the original rules or terrain tiles, such as a
loose `rulesmd.ini` or `clear01.sno`, or an `expandmd99.mix` an old mod left there.

## Sharing your mod

- Give people your mod's folder, with only the files you made or changed in it. They put
  it in their own `Mods` folder and pick it in the Mods menu.
- Say in `mod.ini`'s `Terms=` what others may do with your work. The Mods menu shows it.
- Make a file list first ([Check a mod's files](#check-a-mods-files)), so people can
  check that their copy is complete.
- Everyone needs their own copy of the game. Don't share VERA20k together with the game's
  files.
- If you share a changed VERA20k program, you must share its source code too and give it
  your own name. See the [legal notice](../LEGAL.md).

## More detail

- [INI profiles](ini-roots.md): use your own rules, art and AI files under other names.
- [Interface layout](interface-layout.md): lay out the match screen with `veraui.ini`.
- [Ares](ares.md), [Phobos](phobos.md), [NPatch](npatch.md) and [VeraX](verax.md): what
  each extension covers and how to turn it on or build VERA20k with it. The official
  downloads hold all of Ares, which a game's data turns on, and none of the others.

## Something wrong?

Campaigns aren't finished yet, so test your mod in skirmish. If a mod
that only changes data files looks or plays differently here than in the original, that's
a bug. Tell us on [Discord](https://discord.gg/kmjRUn5m5F) or
[open an issue](https://github.com/YuriPlanet/vera20k/issues).
