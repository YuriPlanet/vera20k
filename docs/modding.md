# Modding VERA20k

VERA20k is a new game engine that reads the same files as Red Alert 2: Yuri's Revenge.
It isn't a patch on `gamemd.exe`. Mods supply data files; VERA20k implements the
behavior that reads them. A mod's compatibility depends on the keys, assets and
extension features VERA20k supports.

## How it differs from the original

| | Original game | VERA20k |
| --- | --- | --- |
| Program | `gamemd.exe`, for Windows | A new engine for Windows, macOS and Linux |
| Game files | MIX, INI, SHP, VXL, CSF and maps | The same files, searched in the same order |
| DLL mods and exe patches | Run inside the game | Can't run |
| Map size | Width plus height up to 512 | Up to 1024 |
| Players | 8 | Up to 120 player slots in offline Skirmish and up to 30 in LAN or online matches, on maps with enough starts |
| Player colours | 8 | 30, and a mod can [list its own](skirmish-colors.md) |
| Playing online | With the original game | Only with other VERA20k players |

Weapon and guard ranges can span the full map. VERA20k limits the number of
objects a single rules entry can create at once. `Cluster`, `ShrapnelCount`
(in magnitude) and debris counts are limited to 256, and `SpawnsNumber`
to 128. The game limits `CrateMinimum` and `CrateMaximum`
to 512 and `[AI] AIBaseSpacing` to −16 through 16 cells.

## What you can and can't do

**You can:**

- Change costs, health, damage, speed and other stats.
- Add new units, buildings and weapons.
- Replace pictures, models, sounds and music.
- Change the AI, the game modes and the game's text.
- Make larger maps with up to 120 starting positions. Offline Skirmish can use all 120. LAN and online matches can use up to 30.
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
- Ares and Phobos. Only some of their keys work. A mod can declare that it requires
  Ares in `mod.ini`, before its files load. A selected mod's `ares.mix` also enables
  Ares reading with the default setting. Phobos needs a build you make yourself.
  See [More detail](#more-detail).
- Large conversions. Selecting their data does not establish support for all their
  extension features.
- New behaviour in code. A mod or map can carry a [Wasm module](wasm-modules/guide.md), a
  sandboxed program that changes the game through its own rules. It is a preview.

**You can't:**

- Load DLLs, Syringe or a patched `gamemd.exe`.
- Add new behaviour with a DLL, script or plugin outside the [Wasm modules](wasm-modules/guide.md),
  which have no files, network or clock.
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
| A `Maps` folder of `.yrm`, `.map` or `.mpr` files | Adds your maps to Skirmish. Mods with their own client catalog use that catalog. Create the folder if it isn't there. Maps you download online go here too. |
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

In the desktop game, open **Options → Mods → Add Mod** to import a download.
Select all the ZIPs or extracted folders for one mod, including any patches. On
macOS, hold Command while clicking to select several items. Use **Add More** to
select another part, or drop parts onto the window on desktops that support it.
The Linux chooser uses Zenity or KDialog. For an extracted mod, select the folder
that contains the mod's game files.

The preview starts in filename order, ignoring letter case and the ZIP extension.
Select a row and use **Move Up** or **Move Down** to follow the mod's install order.
Later parts replace files at the same path.

**Install & Play** makes one ready-to-use folder in `Mods` and restarts into it.
**Install Only** adds the mod to the list for later. Both keep the original downloads.

To extend an installed mod, select it and choose **Add Mod → Add parts**.
Pick the patch ZIPs or folders and confirm **Append**. Existing filenames match
regardless of letter case. The mod keeps its own `mod.ini` and Android shortcut.
If you are already playing that mod, restart the game to load the added files.

You can also install a mod manually.

1. Make a folder inside `Mods` and give it your mod's name, such as `Mods/MyMod`.
   Or press **New Mod** in the Mods menu, which makes one with the original game's
   files in it: see [Start a new mod](#start-a-new-mod).
2. Put the mod's files in it. Keep the original filenames, or declare other names in
   [`mod.ini`](#modini).
3. [Pick the mod](#pick-a-mod).

The game then reads the mod's files as if you had copied them on top of the game folder.
A mod made to be copied into the game folder works the same from its own folder.

A classic Red Alert 2 mod can keep its original filenames. When the selected mod
supplies the RA2 rules, art, AI or multiplayer mode files, VERA uses the RA2
file family for Skirmish. A mod can replace just part of that family. For
example, one that supplies its own `rules.ini` and `mpmodes.ini` can use the
installed game's `ai.ini`. See [how VERA chooses the files](ini-roots.md).

With **No mod** selected, an `ares.mix` already in the game folder is ignored, unless
the [Ares setting](ares.md#the-ares-setting) is On; set to Off, no `ares.mix` is used.
A selected mod can use that copy or provide its own, which takes precedence.
The optional `langrule.ini` rules override is also skipped with **No mod**,
whether loose or inside an archive. Selected mods keep that layer, from the
installation or the mod, and its normal Digest check. This does not skip the
`.csf` files used for translated text.
Other changed rules and terrain files still need to match between players.

A mod is an external data package. VERA20k reads its files and never runs its
executables, DLLs or launcher. A manifest selects resources and supported extensions;
it does not install new engine code.

A mod's own terms still apply. Some authors don't allow their mod in other engines:
check a mod's terms before you use it here. VERA20k doesn't support such mods.

| What you can put in a mod's folder | What it does |
| --- | --- |
| A file named the same as a game file, such as `rulesmd.ini` or a `.shp` | Replaces the game's copy. It must be directly in the mod's folder, not in a subfolder. |
| An archive named `expandmd50.mix` | Replaces the game's copies of the files inside it, except the ones in `langmd.mix` and `language.mix`, which the original game reads first. An Ares game reads the expansion archives before those two as well. The number can be 00 to 99, and higher numbers win. `[Archives]` can select another expansion prefix. An archive with the same name as one in the game folder takes that archive's place. |
| `ares.mix` | With the Ares setting on Auto, a selected mod mounting this file becomes an [Ares](ares.md) game. It mounts after expansion archives and before the game's own. Its `ares.csf` and `ares_<language>.csf` add text, and so do `stringtable00.csf` to `stringtable99.csf` in the mod, later files replacing a label's earlier text. A text written `NOSTR:text` where the game expects a label, such as `UIName=NOSTR:Heavy Tank`, shows as written. A mod can also declare an Ares requirement without shipping this archive. |
| An archive whose name starts with `ecache` or `elocal`, such as `ecachemd01.mix` | Adds the files inside it, as the original game does. All `ecache` archives load before all `elocal` ones, each group by name. They load after the `expandmd` archives and the game's main, cache and local archives, which keep a file name they share, and before the game's art, cameo and theater archives, whose copies they replace. |
| Maps | Adds them to the map list while the mod is in use. |
| Only INI files the game doesn't read, such as an AI another author made | Adds them to the map of every Skirmish and online match. See [Add to every map](#add-to-every-map). |
| `veraui.ini` | Lays out the match screen: which edge the sidebar and the command bar sit on, where the sidebar's parts sit, which it has, and how big it is. See [interface layout](interface-layout.md). |
| `mod.ini` | Supplies display information, resource filenames and extension requirements. Optional. |
| An `hd_art` folder | An HD art pack: 2x pictures the game draws instead of the original art when HD art is on. Players need not share it. See [HD art](hd-art.md). |
| `mod-files.txt` | Lists the mod's files, so players can check their copy. The game writes it for you: see [Check a mod's files](#check-a-mods-files). Optional. |

#### Mods distributed in parts

In the desktop game, put the downloads for one mod inside one `Mods` folder
entry. Each part can be a ZIP archive or a folder containing the files from
that archive. For example, with ZIPs use `Mods/My Mod/01 Base.zip` and
`Mods/My Mod/02 Patch.zip`. If already extracted, use `Mods/My Mod/01 Base/`
and `Mods/My Mod/02 Patch/`.

VERA sorts the parts by name, ignoring letter case and the `.zip` extension.
A later part replaces files at matching paths. Keep the shipped names if
they already sort in install order, or prefix the parts with `01`, `02`,
and so on. Choose the containing mod in the **Mods** menu as usual. VERA
assembles its files for the launch and keeps the downloads in place.

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

The file and its sections are optional. Save it as UTF-8, at most 64 KiB. Section and
key names ignore case. `;` starts a comment, so don't use one in your text. Display
values keep their first 300 characters; resource names are validated instead.

- `Name` and `Version` are what the game calls the mod. Without them it uses the
  folder's name.
- `Author`, `About` and `Terms` show in the Mods menu when the mod is picked: who made
  it, what it is in a line or two, and what others may do with your work.
- `Needs` is the oldest VERA20k version that can play the mod. An older game says so and
  plays without the mod, and its Mods menu shows what the mod needs. Leave it out unless
  the mod uses something a newer version added.

To use other resource names, add loading declarations. These fictional names show
the format; the mod must supply the named files:

```ini
[Files]
Rules=example-rules.ini
Art=example-art.ini
AI=example-ai.ini
Modes=example-modes.ini
MapCatalog=example-maps.pkt

[Archives]
ExpansionPrefix=expand_example
MapPrefix=maps_example
Multiplayer=example-multiplayer.mix
Theme=example-music.mix
MoviePrefix=movies_example

[Extensions]
Ares=yes
```

- `[Files]` names the INI roots and catalogs. Its presence starts from stock defaults;
  omitted keys keep those defaults, rather than inheriting custom host filenames.
  Without the section, the mod inherits the host's `[ini]` settings.
- `[Archives]` replaces names in existing startup slots, preserving their order and
  fallback rules. Omitted keys keep stock names. It does not scan for other archives.
- `[Extensions] Kratos=yes` makes this launch play Kratos, as if the mod shipped it. With the
  player's Kratos setting Off, VERA20k refuses the mod and says how to turn Kratos back on.
- `[Extensions] Ares=yes` requires Ares reading from startup. With the player's Ares
  setting on Auto, it enables that reading for this launch without changing the saved
  setting. With Off, VERA20k refuses the mod and tells the player how to enable it.
  `Ares=no`, the default, declares no requirement; it does not force Ares off.

The [complete filename table](ini-roots.md#a-mods-files) includes sound, EVA, theater,
theme and campaign files. Names are plain basenames, not paths or wildcards. Mode and
map catalogs still use the native MPModes and PKT formats. These declarations neither
run a DLL nor implement unsupported extension keys. Unknown keys are ignored; invalid
recognized loading values refuse the mod. Restart after changing them.

#### Pick a mod

Start the game, open **Options** and press **Mods**. The list shows the original game
as **No mod**, then every folder in `Mods`. Pick one and press **Use This Mod**. The game
restarts with the mod, and the main menu then shows the mod's name. To go back to the
original game, pick **No mod** the same way. **Open Mods Folder** opens `Mods`.

A mod can brand the main menu and its other movie pages with `ra2ts_l.bik` or
`ra2ts_s.bik`. Put the movie directly in the mod folder or in one of its loaded
MIX archives. The game tries the slot for the current window size first, and it
can use the other size if the mod ships one. The movie needs to fit the menu's
picture area, so its width and height must be within a factor of two. Brief,
wide launcher messages are skipped when looking for menu artwork.

Compatible mod artwork is tried before stock in the startup splash, setup shell,
loading screen, match controls and score screen. Shape replacements can use
their matching palette. Supply a loose file or put it in a loaded mod MIX
archive. The built-in style supplies fallbacks where the mod has no usable art.

Skirmish keeps your saved choices where the selected game supports them. A saved
faction that the current rules do not offer comes back as **Random**.

The folder name is only the choice saved by the menu. Its loading rules come from
`mod.ini` or the files the mod already ships.

An extracted CnCNet client folder can go straight into `Mods`. Put the folder
containing `Resources/ClientDefinitions.ini` and the game archives there, then
choose it as above. VERA uses its `MPMapsPath` catalog. A client whose files
use other names declares them in `mod.ini`; VERA doesn't guess them. A folder
that ships the Ares DLL and its injection list uses VERA's own Ares reader.
VERA also uses the client theme's `splashScreen.png` and skirmish lobby
background when present. It reads the configured `BackgroundTexture` from
`SkirmishLobby.ini` or looks for `gamelobbybg.png`, and draws its setup controls
over the artwork.

This import opens offline Skirmish with the supported maps and modes. It applies
their plain mode INIs and uses VERA's game options. Client-only forced settings,
random maps, campaigns specific to the client, and multiplayer through the
imported catalog are outside this path. An explicit loading section in
`mod.ini` takes precedence over the client folder's files.

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
For a mod distributed in parts, this checks the assembled files.
Two copies with the same code hold the same compared data files: the code leaves out what
[playing with others](#playing-with-others) leaves out, such as a translated `.csf`, though
the list and the count name every file. Also keep their
`mod.ini` loading declarations in sync; the display metadata can differ.

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
- If the manifest's loading declarations are invalid, fix the named entry and restart.
  If the mod requires Ares while the setting is Off, choose Auto or On in Game Options,
  restart, and select the mod again. The refusal leaves the saved Ares preference alone.
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

[ModEnc](https://modenc.renegadeprojects.com/) explains what the keys do. The stock
selection reads `rulesmd.ini` and `artmd.ini`; other root names need a declaration in
`mod.ini`.

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
`artmd.ini` as in the original, and put their art in the same folder. A mod using other
root names declares them in `[Files]`. A sound or music track can also be replaced by an
Ogg Opus file named after it, such as `igisea.opus`; see [Opus sounds and music](opus-audio.md).

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
`all04dmd.ini`, belong to the game, as do the files selected by `[Files]`. A folder with
such a file, or any other, such as an archive, is an ordinary mod and adds nothing to
maps. The game's log says which files it adds.

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

Put your `.yrm`, `.map` or `.mpr` file in VERA20k's `Maps` folder and restart the
game to find it in Skirmish. Skirmish also finds loose multiplayer `.mpr` and
`.yrm` files directly in the game folder or the selected mod's folder. Put a
map that needs a particular mod in that mod's folder. As in the original, a
map can change any rule for that match.

New map exports and downloads use `.map`. If you already have an identical
copy, VERA20k reuses it under its existing name, including `.yrm` and `.mpr` files.

VERA20k also supports these map settings:

- **Bigger maps:** under `[Map]`, the width and height in `Size=` can add up to 1024.
- **More players:** under `[Basic]`, add `MaxPlayers=` with a number from 9 to 30. The
  map needs a start waypoint for every player, numbered from 0.
- **Breakable ice.** Set `VeraIce=yes` under `[Map]` on a snow map, then paint
  the crossing with the bundled ice tiles. Players only need your map file.
  See [ice tile numbers, editor setup and Ice Lake](ts-ice.md#use-the-bundled-ice-in-a-map).

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
the same selected Rules, Art and AI. **Terrain Deformation**, the **Superweapon Limit** and the **Building
Production Queue** don't need to match: a room plays its host's, and a match a launcher
starts plays without the queue. With a mod in use,
everyone also needs the same theater INIs (such as `snowmd.ini`) and terrain tiles, and the same
files in the mod's folder: every file there is compared by
its name and what it holds, except `mod.ini`, `veraui.ini` and `mod-files.txt`, and except files that
only show, play or tell you something: text (`.csf`), fonts (`.fnt`), movies (`.bik`, `.vqa`), pictures
only a screen draws (`.pcx`, `.png`, `.jpg`, `.jpeg`, `.bmp`, `.gif`, `.ico`, `mouse.sha`) and documents
(`.txt`, `.md`, `.pdf`, `.rtf`, `.htm`, `.html`, `.url`, `.log`). So a mod with its text translated plays
with the original. Sounds, music, palettes and every `.mix` archive are still compared. The folder's own name
doesn't matter, and a file name with letters outside plain English counts only by what
the file holds. Terrain tiles decide how high the ground is and where units can go, so a
mod's are compared with the original game's. Without a mod, terrain files you put in the game
folder itself, such as `clear01.sno`, an `isosnomd.mix` or a tile pack in `expandmd06.mix`,
are left out of an online match on a map that uses only the original game's tiles: every
player's game reads the original terrain there. A map that uses other tiles needs everyone
to have the same terrain files, and the match says so when it starts. Other pictures and
sounds there aren't compared.
Players who don't have your map are asked whether to download it when they join your
online room. When a match can't start because the files differ, each player's game says
what it changes from the original game (its mod, or files such as a loose `rulesmd.ini`),
or that it plays the original, so the difference is in another player's game.

Quick Match pairs you with anyone on the same VERA20k version, so it plays the original
game, without anyone's Terrain Deformation, Superweapon Limit or Building Production Queue. With a mod in use it tells
you to pick **No mod** in the Mods menu first. It also names
any files in the game folder that change the original rules, such as a
loose `rulesmd.ini`, or an `expandmd99.mix` an old mod left there. Terrain files there
don't matter: its matches read the original terrain.

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

- [INI roots and archive names](ini-roots.md): select your own resource filenames.
- [Interface layout](interface-layout.md): lay out the match screen with `veraui.ini`.
- [Wasm modules](wasm-modules/guide.md): build a `rules.wasm` or a `ui.wasm`, put it in a mod
  or a map, and declare its triggers in `rulesmd.ini`; tutorials for
  [simulation](wasm-modules/rust-tutorial.md) and [client](wasm-modules/rust-ui-tutorial.md) modules
  and the [Rust SDK reference](wasm-modules/rust-sdk.md).
- [Ares](ares.md), [Phobos](phobos.md), [NPatch](npatch.md), [VeraX](verax.md) and
  [Kratos](kratos.md): what each extension covers and how to turn it on or build VERA20k with
  it. The official downloads hold all of Ares, which a game's data turns on, and none of the
  others. Kratos is at its first pieces: the per-game switch and the prone firing offset.

## Something wrong?

Campaigns aren't finished yet, so test your mod in skirmish. If a mod
that only changes data files looks or plays differently here than in the original, that's
a bug. Tell us on [Discord](https://discord.gg/kmjRUn5m5F) or
[open an issue](https://github.com/YuriPlanet/vera20k/issues).
