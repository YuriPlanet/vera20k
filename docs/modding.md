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
- The sidebar. You can replace its pictures and move its parts with an
  [interface layout](interface-layout.md). You can't yet add parts, leave parts out or
  put the sidebar on another edge.
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

VERA20k uses two folders: its own folder, and the game folder.

### The VERA20k folder

| System | Where it is |
| --- | --- |
| Windows | The folder you installed VERA20k in, next to `vera20k.exe` |
| macOS | `~/Library/Application Support/VERA20k/` |
| Linux | `~/.local/share/VERA20k/` |

| What you can put here | What it does |
| --- | --- |
| `VERARULES.INI` | Your own rule changes. The game creates this file for you. |
| A `Maps` folder of `.yrm` files | Adds your maps to the map list. Create the folder if it isn't there. Maps you download online go here too. |
| `vera20k.<language>.ini` | Adds a language, or changes the game's text. See [translating](../src/i18n/README.md). |

`config.toml` is here too. It says where the game folder is. The game writes everything
else itself, such as `VERA.INI` (your options and hotkeys), `saves`, `replays` and `logs`.

Art, sounds and files such as `rulesmd.ini` do nothing in this folder. They go in the
game folder. Don't share this folder with anyone, because it holds your online sign-in.

### The game folder

This is your Red Alert 2: Yuri's Revenge folder, the one with the `.mix` files in it.

| What you can put here | What it does |
| --- | --- |
| A file named the same as a game file, such as `rulesmd.ini` or a `.shp` | Replaces the game's copy. It must be directly in the folder, not in a subfolder. |
| An archive named `expandmd50.mix` | Replaces the game's copies of the files inside it. The number can be 00 to 99, and higher numbers win. Pick one that no other file uses. |
| `.yrm` files | Adds your maps to the map list. |
| `veraui.ini` | Moves the parts of the sidebar. See [interface layout](interface-layout.md). |

Don't change your real game. Copy the whole folder, then open `config.toml` and change
the `ra2_dir` line to the copy. Use forward slashes, even on Windows. With backslashes
the game can't read the file:

```toml
ra2_dir = "C:/Games/My Mod"
```

There's no mod menu. VERA20k plays whatever is in the folder `ra2_dir` points to, so keep
one copy for each mod and change `ra2_dir` to switch.

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

Put your version in your copy of the game folder under the same name, such as
`rulesmd.ini` or `aimd.ini`. To add new units, buildings or weapons, add them to
`rulesmd.ini` and `artmd.ini` as in the original, and put their art in the same folder.

### Add a map

Give the map a `.yrm` ending and put it in the `Maps` folder. As in the original, a map
can change any rule for that match.

VERA20k adds two things. Open the map in a text editor to set them:

- **Bigger maps:** under `[Map]`, the width and height in `Size=` can add up to 1024.
- **More players:** under `[Basic]`, add `MaxPlayers=` with a number from 9 to 30. The
  map needs a start waypoint for every player, numbered from 0.

### If a change doesn't work

- Restart the game and start a new match. A loaded save keeps the rules it was saved with.
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
game. Point `ra2_dir` back at your original folder, and put `;` in front of your changes
in `VERARULES.INI`.

## Sharing your mod

- Give people the files you made or changed, not the whole game folder. They put them in
  a copy of their own game folder. A `VERARULES.INI` goes in their VERA20k folder.
- Everyone needs their own copy of the game. Don't share VERA20k together with the game's
  files.
- If you share a changed VERA20k program, you must share its source code too and give it
  your own name. See the [legal notice](../LEGAL.md).

## More detail

- [Personal rules](personal-rules.md): everything about `VERARULES.INI`.
- [INI profiles](ini-roots.md): use your own rules, art and AI files under other names.
- [Interface layout](interface-layout.md): move the parts of the sidebar with `veraui.ini`.
- [Ares](ares.md), [Phobos](phobos.md), [NPatch](npatch.md) and [VeraX](verax.md): what
  each extension covers and how to build VERA20k with it. The official downloads don't
  include them, and no INI file can add them.

## Something wrong?

Campaigns aren't finished yet, so test your mod in skirmish. If a mod
that only changes data files looks or plays differently here than in the original, that's
a bug. Tell us on [Discord](https://discord.gg/kmjRUn5m5F) or
[open an issue](https://github.com/YuriPlanet/vera20k/issues).
