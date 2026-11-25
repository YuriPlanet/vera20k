# INI roots and archive names

A mod can name its Rules, Art, AI and catalogs in `mod.ini`. The game reads
those names through the existing loose-file and MIX loaders. Archive names
are separate: changing an INI name does not change which archives mount.
There are no built-in named-mod profiles or guesses based on a folder's name.

## A mod's files

Put `mod.ini` directly in the mod's folder and [select the mod](modding.md#pick-a-mod).
For example:

```ini
[Mod]
Name=Example Mod

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

Each section is optional. A mod with no loading declarations can use its
conventional files as described below. With `[Files]`, omitted entries use the stock names
below, independent of the host's custom names. Rules, Art and AI must then
all resolve to nonempty, readable files before loading continues.

| `[Files]` key | Default | Read by |
| --- | --- | --- |
| `Rules` | `rulesmd.ini` | Ordered scenario Rules loading |
| `Art` | `artmd.ini` | Fixed Art definitions |
| `AI` | `aimd.ini` | AI definitions, extended by the map |
| `Sound` | `soundmd.ini` | Unlayered sound definitions |
| `Eva` | `evamd.ini` | Unlayered EVA definitions |
| `TheaterSuffix` | `md` | Theater control INIs, such as `snowmd.ini` |
| `Theme` | `thememd.ini` | Theme catalog |
| `Battle` | `BATTLEMD.INI` | Campaign catalog |
| `Mission` | `MISSIONMD.INI` | Mission metadata |
| `MapSelect` | `MAPSELMD.INI` | Campaign map-selection stages |
| `Modes` | `MPModesMD.ini` | Skirmish and network mode roster |
| `MapCatalog` | `missionsmd.pkt` | The initial multiplayer map catalog |

These are plain basenames: no directory paths or wildcards. `MapCatalog`
requires a `.pkt` name; the other file entries require `.ini`.
`TheaterSuffix` accepts only letters and digits. It replaces the `MD` in
theater control INI names, not the theater's MIX names. If that selected
theater INI is absent, the existing loader tries the stock name.

The catalog formats stay the same. `Modes` reads the native MPModes sections
and rows; `MapCatalog` reads a PKT's `[MultiMaps]` and per-map sections. Naming
a launcher's unrelated settings file does not convert its format. Loose
Skirmish checks loose PKT catalogs, YRO archives and multiplayer maps after
the initial catalog.

Optional catalogs keep their existing missing-file behavior. Sound and EVA
can produce empty catalogs; absent campaign or mission files do not become
stock campaign data. Selecting a different root does not add support for an
unimplemented gameplay key.

## Classic Red Alert 2 folder mods

For a selected mod without loading declarations or a CnCNet client catalog,
VERA looks for `rules.ini`, `art.ini`, `ai.ini` or `mpmodes.ini` in the
mod itself, including standard numbered `expandNN.mix` archives. It chooses
the RA2 family only when the package has no corresponding Yuri’s Revenge
roots or numbered YR expansion archives. An explicit custom INI profile
takes precedence.

In that family, `rules.ini`, `art.ini` and `ai.ini` resolve through the normal
mod and installation lookup. `mpmodes.ini` names the actual mode INIs, and
Skirmish uses `missions.pkt` and `multi.mix` for the classic map catalog.
Selecting **No mod** returns to the configured installation defaults.

## Archive names

`[Archives]` changes names in the existing startup slots:

| Key | Default | Files selected |
| --- | --- | --- |
| `ExpansionPrefix` | `expandmd` | Prefix plus `99.mix` down to `00.mix` |
| `MapPrefix` | `mapsmd` | Numbered map media, or the prefix's `*.mix` files in CD mode |
| `Multiplayer` | `multimd.mix` | Multiplayer archive |
| `Theme` | `thememd.mix` | Expansion theme archive |
| `MoviePrefix` | `movmd` | Numbered movie media, or the prefix's `*.mix` files in CD mode |

Prefixes accept ASCII letters, digits, `_` and `-`. The two filenames use
the same characters followed by `.mix`, and cannot name Windows devices.
Paths, empty names and wildcards are refused.

Mount order and fallback rules do not change. Expansions still mount in
descending numeric order; the native language, master, cache, local, audio,
conquer and cameo slots keep their names. Ares retains its own archive-order
rules. Numbered map and movie media keep their base `mapsNN.mix` and
`moviesNN.mix` fallbacks, and VERA still adds `theme.mix` after the selected
expansion theme archive. No directory-wide archive scan is added.

`[Extensions] Ares=yes` declares a requirement before startup reads any game
INI. With the player's Ares setting on Auto, it uses Ares reading for this
launch. Off refuses the mod and explains how to enable it. The saved player
setting is unchanged. See [Ares](ares.md#the-ares-setting). A declaration does
not load a DLL or supply missing files.
Ares is currently the supported declaration. Any other extension marked as
required is refused instead of being silently ignored.

The process asset owner freezes the chosen files and archive names. Recovery
uses that same choice rather than rereading a changed manifest or guessing
from newly available files. Restart after changing a mod's definition.

## Host configuration

Without a mod's `[Files]` declaration, `config.toml` can select INI roots:

```toml
[ini]
profile = "custom"
rules = "example-rules.ini"
art = "example-art.ini"
ai = "example-ai.ini"
sound = "example-sound.ini"
eva = "example-eva.ini"
theater_suffix = "example"
theme = "example-theme.ini"
battle = "example-battle.ini"
mission = "example-mission.ini"
mapsel = "example-mapselect.ini"
modes = "example-modes.ini"
map_catalog = "example-maps.pkt"
```

`yr` is the default profile and uses the stock triplet. `custom` requires all
three Rules/Art/AI names. `auto` validates the stock triplet; it does not
discover a mod. The optional entries use the same defaults and validation
as `[Files]`. Host `[ini]` settings do not select archive names.

## Inspection

Use the same definition with the asset tool:

```text
asset ini-roots --ra2-dir <game directory> --mod-dir <mod directory>
asset ini-get TEST Voxel --domain art --reader bool --default false --ra2-dir <game directory> --mod-dir <mod directory>
```

`--mod-dir` reads that folder's `mod.ini`, overlays its files and applies its
extension requirement. It does not use the desktop's selected mod.
Without it, INI commands use the host's `[ini]` settings unless
`--ini-profile yr|auto|custom` replaces them as a unit. A CLI custom profile
must supply `--rules-ini`, `--art-ini` and `--ai-ini` together.

`ini-roots --all-mixes` remains an explicit tooling-only wider search, not the
game's mounting policy. Source names and hashes describe consumed inputs;
they do not establish compatibility with a whole mod.

## Source and scope

baixiong443 adapted root selection from the ra2-mono files
`Ra2.Shared/GameConfig.cs`, `Ra2.IO/GameIniLoader.cs` and `Ra2.IO/IniMerger.cs`.
VERA keeps one root per family. The author contributed the adaptation under
VERA's GPLv3. [Original contribution](https://github.com/YuriPlanet/vera20k/commit/abe98da40b836f73de99902bc109f1aa43ce14af)

[Include and inheritance](ini-directives.md) run at the selected source
boundary when the game reads the [Ares way](ares.md), or in a `phobos` build.
Resource selection itself needs no extension build feature. Native archive
and layer ordering remain owned by the existing loaders.
