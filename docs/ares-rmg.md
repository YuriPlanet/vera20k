# Optional Ares random-map generator extensions

Build with `--features ares`. Ares 3.0 (`Misc/RMG.cpp`) widens the random-map
dialog, adds urban areas to generated maps and fixes three generator bugs.
Default builds read none of the keys below and keep the stock dialog and
generator.

```ini
; RMGMD.INI, read on the process's first RMGMD.INI read only
[General]
GenerateUrbanAreas=no   ; the Urban Areas box's first value

[Urban]
Structures=CABUNK01,CABUNK02,CAARMY01,CAARMY02,CAARMY03,CAARMY04,CACHIG03,CANEWY01,CANEWY14,CANWY09,CANWY26,CANWY25,CATEXS07
Infantry=CIV1,CIV2,CIV3,CIVA,CIVB,CIVC
Vehicles=TRUCKA,TRUCKB,COP,EUROC,SUVW,SUVB,FTRK,AMBU
```

The lists split on commas alone (`VectorNames::Tokenize`): empty entries
vanish and spaces stay in the names. Retail `RMGMD.INI` sets none of these
keys, so the defaults above apply.

## What it does

- The landform list starts with Archipelago and the theater list gains
  Desert after Snow, in every dialog (offline Skirmish keeps VERA's Urban
  after them).
- The Urban Areas box turns urban areas on. Surprise Me picks the landform,
  then the theater, from the listed entries, sets the box from the low bit of
  one raw Main draw, then draws the rest as stock does.
- With the box set, a generated map gets up to ten attempts at an urban
  area, right after the region reset. Each grows a paved district of 100 to
  2000 cells, lays east-west roads, then north-south roads with crossings and
  medians, places neutral `[Urban] Structures` on paved ground, one to five
  neutral `[Urban]` infantry or vehicles, and paved plazas.
- Fixes, unconditional in an Ares build:
  - A low-bridge deck longer than it is wide is placed without
    `ValidateLowBridgeDeckArea`.
  - Bridge and urban road ends always take the road-end piece; the coin that
    could swap in a straight road piece is never drawn.
  - A theater without `[General] Medians=` (SNOWMD.INI) reads tileset 71,
    its paved road bits. This also changes the Pave LAT exemptions of every
    snow map, generated or not.

## Hooks

| Hook | Ares.dll | Name | Disposition |
|---|---|---|---|
| `0x545904` | `1006A920` | IsometricTileTypeClass_CreateFromINIList_MediansFix | Ported: `map/theater.rs` `medians_ordinal` |
| `0x58FA51` | `1006ADB0` | RMG_PlaceWEBridge | Ported: `rmg/phases/bridge_deck.rs` `unvalidated_axis` |
| `0x58FE7B` | `1006AD90` | RMG_PlaceNSBridge | Ported: same |
| `0x58FBDD` | `1006AD50` | RMG_FixPavedRoadEnd_Bridges_East | Ported: `bridge_deck.rs` `stamp_end` |
| `0x58FCC6` | `1006AD80` | RMG_FixPavedRoadEnd_Bridges_West | Ported: same |
| `0x59000E` | `1006AD60` | RMG_FixPavedRoadEnd_Bridges_North | Ported: same |
| `0x5900F7` | `1006AD70` | RMG_FixPavedRoadEnd_Bridges_South | Ported: same |
| `0x596786` | `1006A9C0` | MapSeedClass_DialogFunc_SurpriseMe | Ported: `rmg/randomize.rs` `randomize_from_combos` |
| `0x596C81` | `1006A940` | MapSeedClass_DialogFunc_GetData | Ported: the box is `RmgOptions::urban_areas` itself (`toggle_urban_areas`) |
| `0x5971EA` | `1006A980` | MapSeedClass_DialogFunc_SetData | Ported: same; the dialog draws the field |
| `0x596FFE` | `1006ACF0` | RMG_EnableArchipelago | Ported: `ui/skirmish_shell/state/random_map_setup.rs` list |
| `0x5970EA` | `1006AD10` | RMG_EnableDesert | Ported: same |
| `0x5982D5` | `1006ABF0` | MapSeedClass_LoadFromINI | Ported: `rmg/settings.rs`, latch in `app/frontend/skirmish_session.rs` |
| `0x598FB8` | `1006ABD0` | MapSeedClass_Generate_UrbanAreas | Ported: `rmg/pipeline.rs` stage `UrbanAreas`, `rmg/phases/urban_areas.rs` `run` |
| `0x5A5C6A` | `1006AA90` | MapSeedClass_Generate_PlacePavedRoads_RoadEndNE | Ported: `urban_areas.rs` `place_paved_roads` |
| `0x5A5D6F` | `1006AAC0` | MapSeedClass_Generate_PlacePavedRoads_RoadEndSW | Ported: same |
| `0x5A5F6A` | `1006AAA0` | MapSeedClass_Generate_PlacePavedRoads_RoadEndNW | Ported: `urban_areas.rs` `place_north_south_road` |
| `0x5A6464` | `1006AAB0` | MapSeedClass_Generate_PlacePavedRoads_RoadEndSE | Ported: same |
| `0x5A65CA` | `1006ABA0` | MapSeedClass_Generate_PlaceUrbanStructures_Start | Ported: `urban_areas.rs` `place_structures` |
| `0x5A6619` | `1006AB70` | MapSeedClass_Generate_PlaceUrbanStructures_Loop | Ported: same |
| `0x5A66B0` | `1006AB90` | MapSeedClass_Generate_PlaceUrbanStructures_SanityCheck | Ported: same |
| `0x5A6998` | `1006AAD0` | MapSeedClass_Generate_PlaceUrbanFoots | Ported: `urban_areas.rs` `place_foots` |

`0x5FED00` (OverlayTypeClass_GetRadarColor, `1006ACC0`) is not part of this
chain; see [remaining work](ares-remaining.md).

## Where VERA20K owns it

- `map/rmg/phases/urban_areas.rs` ports gamemd's urban pass, which stock Generate
  never calls: `0x005A5020` (driver), `0x005A5360` (area growth),
  `0x005A7640` (bounds), `0x005A5B50` (roads, with `0x005A6D80`,
  `0x005A7070`, `0x005A76E0` and `Tile_Is_Median @ 0x00486730`),
  `0x005A6510` (structures), `0x005A6920` (foots), `0x005A78D0` with
  `0x005A7B40` (plazas). `AreaIsPavedClear @ 0x005A7250`'s override bytes
  live in `phases/area.rs`.
- The urban switch is `RmgOptions::urban_areas`, written to the seed as
  `UrbanAreas=` (a VERA key), so the launch regeneration and every network
  peer generate the same map. A seed without the key takes
  `GenerateUrbanAreas=`.
- Urban buildings, infantry and vehicles are emitted as Neutral entities
  after the bridge huts and tech buildings; each constructor is a
  `RmgConstructionPhase::UrbanAreas` trace row, replayed as one Scenario
  word. They follow VERA's own towns (`phases/urban.rs`, `VeraUrban=`), a
  separate extension that may run in the same map.
- `rules/native_processing.rs` `registers_urban_type` answers the type
  lookups; `rmg/tech_catalog.rs` `resolve_ares_urban` resolves the lists, with
  footprints from art `Foundation=` like the tech buildings.
- The dialog's Urban Areas box is `RandomMapSetupControl::UrbanAreas5000`
  at Ares' resource rect, with `GUI:UrbanAreas` and `STT:RMGUrbanAreas`. VERA's
  Advanced view puts its seventh slider there, so the box shows in the basic
  view only.

## Residuals

- Ares calls `0x005A5020` with ESI as `this`. At `0x598FB8` ESI is the region
  the free loop just deleted (`operator new(0x50)`), so the urban count
  `+0x60` is read from heap memory past that block. VERA reads
  `MapSeed+0x60` (UrbanPresence), the receiver Ares' source names. With that
  receiver the dirt-road branch (`0x005A89D0`, `0x005A82E0`, `0x005A8720`)
  is unreachable: presence 0 gives a target of 0, any other presence takes
  the urban branch. So a Mountainous map (UrbanPresence 0 by its range) and
  any other map that rolls presence 0 get no urban area. Frequency: every
  urban map; the count is undefined in native Ares.
- Ares' foot picks draw the process Main stream. The dialog's preview does
  the same. The launch regeneration draws a Main stream seeded with the match
  seed, as VERA's launch seeds Main, but the match's own Main stream does not
  continue past those draws. Effect: post-launch Main draws (presentation)
  are offset from native by the foot count.
- An `[Urban]` foot name that is no type leaves native Ares with a null
  object, dereferenced at its first eligible cell after the facing draw (a
  crash); VERA spends the same draws and skips that foot.
- Building and foot `Put` are taken to succeed after the native gates, as
  for the tech buildings (Generate holds ScenarioInit). Foots are emitted at
  sub-cell 0, the cell centre `Put` uses.
- The structure gate reads the Rock land type of the last full recalc
  before the starts; later stamps do not refresh it, as in native.
- The checkbox is drawn as the shell's owner-draw checkbox; Ares' dialog
  resource asks for a flat Win32 box (`BS_AUTOCHECKBOX | BS_FLAT`).
  Toggling it neither rerolls nor invalidates the preview, as in native, so
  Use Map after a toggle keeps the previous preview while the launch
  regenerates with the new value.
- Native Ares keeps the switch process-global, so its network peers can
  disagree; VERA's seed key keeps them in step, and the effective network
  digest (`map::source::effective_network_map_digest`) covers it.
- Urban foot constructors are charged one Scenario word each (TechnoClass
  `0x006F3254`), as for the buildings. The Infantry (`0x00517A50`,
  `0x00517CC0`), Unit (`0x007353C0` and its direct callees) and Foot
  (`0x004D31E0`) constructors draw nothing else; the foot `Put` path
  (`0x0051DFF0`, `0x004D7170`) was not traced for draws.
