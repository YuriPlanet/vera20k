# VERA20k map format

For people who write map editors and map tools. A VERA20k map is an ordinary
Yuri's Revenge map file (`.map`, `.yrm` or `.mmx`). VERA20k reads every map the
original game reads, the same way. It also supports bigger maps, more than eight
players, and snow maps with VERA's bundled breakable ice. The map's values
select these features.

If you only want to make a map by hand, [Modding VERA20k](modding.md#add-a-map) is
shorter.

## Which maps still play in the original game

A map plays in both games when:

- the width plus height in `[Map] Size=` is 512 or less,
- it has at most eight start waypoints (0 to 7),
- all waypoints fit the original range (0 to 701), and
- its painted terrain exists in the original game.

Maps outside these limits require VERA20k.

## Bundled ice for snow maps

`[Map] Theater=SNOW` with `VeraIce=yes` selects VERA's ice tiles for this map.
Use `[Basic] NewINIFormat=4`. Write ordinary eleven-byte `IsoMapPack5` cell
records with the 32-bit little-endian tile number at byte offset 4, sub-tile 0
at offset 8, height at offset 9 and the per-cell ice growth flag at offset 10.
Tile IDs 4096, 4160 and 4224 are solid ice. The original snow tile IDs keep
their normal meanings. See [tile numbers and editor setup](ts-ice.md#use-the-bundled-ice-in-a-map).

## Map size

`[Map] Size=X,Y,Width,Height`: width plus height may be up to **1024**. A map whose
width plus height is more than 1024, or whose width or height is 0, is refused.

The width plus height also picks how the overlay packs are laid out:

| Width + height | Layout | `[OverlayPack]` and `[OverlayDataPack]`, after decoding |
| --- | --- | --- |
| up to 512 | Original | 512 × 512 bytes, cell at `ry * 512 + rx` |
| 513 to 1024 | Extended | 1024 × 1024 bytes, cell at `ry * 1024 + rx` |

Pick the layout from the size alone, never from where the overlays are. A map
of 300 × 300 uses the extended layout even when all its ore sits below cell 512.

Both packs hold one byte per cell, as in the original: the overlay type in
`[OverlayPack]` (255 for none), the frame in `[OverlayDataPack]`. Encoding is
unchanged (LCW blocks, base64 over numbered lines). Write `NewINIFormat=4`.
VERA20k does not read two-byte overlay types (`NewINIFormat=5`): such a pack is
twice the size VERA20k expects and is dropped, so the map loads without overlays.
A map can use at most 255 overlay types.

`[IsoMapPack5]` is unchanged: each cell record already holds its own 16-bit `rx`
and `ry`. Object lists (`[Structures]`, `[Units]`, `[Infantry]`, `[Aircraft]`)
and `[Smudge]` are unchanged too, since they write X and Y as separate numbers.

### Cells with `rx` of 1000 or more

`[Waypoints]`, `[Terrain]` and `[CellTags]` name a cell by one number,
`ry * 1000 + rx`, as in the original (`[Terrain]` and `[CellTags]` use
`ry * 128 + rx` when `NewINIFormat` is below 4, which is too small for these
maps). A cell whose `rx` is 1000 or more can't be written that way: the number
reads back as a different cell. `ry` has no such limit. Editors should not let
waypoints, terrain objects or cell tags go there. This only matters on maps whose
width plus height is near 1000 or more, and only for cells along their far edges.

## More than eight players

`[Basic] MaxPlayers=` is a key the original game already reads for its map list.
VERA20k reads it the same way, and a value from 9 to 120 also makes a map for that
many players. VERA20k uses it only when the map has a start waypoint for every
player, numbered from 0: for `MaxPlayers=12`, waypoints 0 to 11. A waypoint missing
from that range turns the setting off, and the map is treated as an ordinary map
with up to eight starts. Values below 9 or above 120 don't add players.

Without `MaxPlayers=`, VERA20k counts waypoints 0 to 7 as starts, as the original
does.

VERA20k's own random maps write both spellings, `MinPlayer`/`MaxPlayer` and
`MinPlayers`/`MaxPlayers`, with the same numbers.

### Start waypoints are still waypoints

A start is an ordinary waypoint that also marks where a player begins. Triggers,
team scripts and everything else that takes a waypoint can still use it, as they
can use waypoints 0 to 7 on any map. On a map for 30 players, waypoint 12 is player
13's start and can also be where a trigger sends reinforcements. Network rooms
still seat at most thirty players; the larger counts are for offline Skirmish.

What a map can't do is put a waypoint below `MaxPlayers` anywhere other than a
start. On a new map, number other points from `MaxPlayers` up. An older map
that uses waypoints 8 and up for its own points needs those points renumbered,
everywhere they are used, before it can take more players. Editors should warn
when a trigger, team or script uses a waypoint that more players would turn into
a start.

`[Waypoints]` accepts decimal keys from `0` through `18277`. Alphabetic fields,
such as trigger destinations and a team's `Waypoint=` or `TransportWaypoint=`,
use `A` through `ZZZ` without regard to case. `ZZ` is `701`, `AAA` is `702`,
and `ZZZ` is `18277`. Three-letter labels allow surrounding whitespace.
Maps and saves that rely on `AAA` through `ZZZ` need a VERA20k build with
extended waypoint support. The home cell defaults to `699` and can be set
with `[Basic] HomeCell=`.

### Houses for players 9 to 120

The original names its eight start houses `<Player @ A>` to `<Player @ H>`, with
the numbers 4475 to 4482. VERA20k carries on to one hundred twenty: `<Player @ I>` to
`<Player @ Z>`, then `<Player @ AA>` to `<Player @ AZ>`, `<Player @ BA>` to
`<Player @ BZ>`, `<Player @ CA>` to `<Player @ CZ>`, and `<Player @ DA>` to
`<Player @ DP>`, with the numbers 4475 to 4594. A team type's `House=` takes the name, and a trigger event or action that
takes a house takes the number.

### The start markers in the map list

The map list draws numbered start markers from `[Header]`: `StartX`, `StartY`,
`Width`, `Height`, `NumberStartingPoints` and one `WaypointN=x,y` line for each
start, from `Waypoint1`. `NumberStartingPoints` may be up to 120. A header with any
of the five fields missing, a `Width` or `Height` of 0 or less, or a count of 0 or
less or over 120, is skipped and the map is
listed without markers.

## Playing online

Offline Skirmish can seat all 120 players. A network room can use the same authored
map with up to 30 players. Random-map seeds with more than 30 starts play offline.

A big map is sent to players who don't have it like any other map, up to the
lobby's 8 MiB limit for a map file ([lobby protocol](lobby-protocol.md)).
Everyone in a match runs the same VERA20k version, so all of them read the map the
same way.
