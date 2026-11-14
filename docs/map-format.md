# VERA20k map format

For people who write map editors and map tools. A VERA20k map is an ordinary
Yuri's Revenge map file (`.map`, `.yrm` or `.mmx`). VERA20k reads every map the
original game reads, the same way. On top of that it reads two things the original
can't: bigger maps and maps for more than eight players. There is no new file
type and no version number to write; the map's own values say which rules apply.

If you only want to make a map by hand, [Modding VERA20k](modding.md#add-a-map) is
shorter.

## Which maps still play in the original game

A map plays in both games when:

- the width plus height in `[Map] Size=` is 512 or less, and
- it has at most eight start waypoints (0 to 7).

Write such a map exactly as the original game expects. A map past either limit
plays only in VERA20k.

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
VERA20k reads it the same way, and a value from 9 to 30 also makes a map for that
many players.
VERA20k uses it only when the map has a start waypoint for every player, numbered
from 0: for `MaxPlayers=12`, waypoints 0 to 11. A waypoint missing from that range
turns the setting off, and the map is treated as an ordinary map with up to eight
starts. Values below 9 or above 30 don't add players.

Without `MaxPlayers=`, VERA20k counts waypoints 0 to 7, as the original does.
Waypoints 8 and up are then ordinary waypoints for triggers and scripts, as they
always were. On a map for more players, waypoints 0 to `MaxPlayers - 1` are starts and
the ones from `MaxPlayers` up are ordinary waypoints.

VERA20k's own random maps write both spellings, `MinPlayer`/`MaxPlayer` and
`MinPlayers`/`MaxPlayers`, with the same numbers.

### The start markers in the map list

The map list draws numbered start markers from `[Header]`: `StartX`, `StartY`,
`Width`, `Height`, `NumberStartingPoints` and one `WaypointN=x,y` line for each
start, from `Waypoint1`. `NumberStartingPoints` may be up to 30. A header with any
of the five fields missing, a `Width` or `Height` of 0 or less, or a count of 0 or
less or over 30, is skipped and the map is
listed without markers.

## Playing online

A big map is sent to players who don't have it like any other map, up to the
lobby's 8 MiB limit for a map file ([lobby protocol](lobby-protocol.md)).
Everyone in a match runs the same VERA20k version, so all of them read the map the
same way.
