# Native Anytown navigation composition

This packet executes the SHA-pinned original `gamemd.exe` for the physical `XMP03T4.MAP` navigation state before damage, after the first concrete-low damage phase, after collapse, and after repair. It derives the class/height inputs through original readers, constructors, terrain placement and Cell recalculation. It supplies no VERA class planes, graph IDs, graph edges or CanEnter answers. The independent replay receipt is in `navigation_receipt.json`. `navigation_comparison.json` records full production equality for loaded, first-damaged, collapsed and repaired states in the `concrete-v8` exports.

## Reproduce

Run from the repository root using the dependencies in [native_oracle.md](../../native_oracle.md), including `cryptography` for encrypted MIX indexes and liblzo2 for MAP packs. Keep the exact sparse Shrapnel bootstrap directory described in [retail_manifest.json](retail_manifest.json); the Anytown input directory must contain its SHA-matching `XMP03T4.MAP`. `RA2_DIR` supplies the physical archives listed below. Proprietary files are local inputs and are not bundled.

```sh
export VERA20K_SHRAPNEL_INPUTS=/path/to/shrapnel-native-inputs/extract
export VERA20K_ANYTOWN_INPUTS=/path/to/anytown-native-inputs/extract
export RA2_DIR=/path/to/retail-game
export VERA20K_GAMEMD_EXE="$RA2_DIR/gamemd.exe"
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.anytown_damage.navigation --check
```

`--check` is also the default. `--write` is explicit and still rejects any change to the independently frozen original payload. The original and published payload hashes are pinned in `navigation_promotion.json`; its original external receipt is preserved. The shared compressed-publication owner is used with a navigation-specific promotion sidecar, leaving the existing six-witness manifest untouched until its owner refreshes it.

`navigation_inputs.py` uses the package input/theater helpers, the shared MAP pack decoder and the existing sidebar stock MIX reader. Original emulation depends only on repository helpers and explicit local retail inputs. No external research Python import is needed. `navigation_receipt.json` records source and artifact hashes. `navigation.json.gz` is deterministic-gzip canonical JSON; metadata records the payload hash, original executable identity, Unicorn version, boundaries and original entry addresses. Original `.text` is checked after initialization and every bridge stage. No native code is replaced. The payload contains numeric authored input projections and native output tables, not original MAP/TMP/INI file bodies or copied lexical INI dictionaries; the existing shared publication projection is still applied.

To repeat the saved production comparison after producing compatible exports:

```sh
python -m tools.spatial_oracle.anytown_damage.navigation_compare /path/to/concrete-v8 --repaired /path/to/concrete-v8.repaired.json
```

The comparator reads only the native reference, the three `PREFIX.{loaded,damaged,collapsed}.json` exports and, when supplied, the explicit `--repaired` export. The original three-state mode remains available. It checks every plane, raw movement row, graph ID/padding slot and complete ordered graph record, then all allocated live level/slope values and the exported representative Cell fields. It reports mismatches and fails rather than reducing comparison to record counts. It can write a comparison receipt only with explicit `--output`.

## Physical input identity and construction

* Original executable SHA256: `1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
* Original mapped `.text` SHA256: `4cd5557a7490debc493ff965afc4483d8d2f1065f434f6b665cbb8fc4835b0cc`.
* Physical map SHA256: `7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`, `multimd.mix` entry `CD0DDEF2`, 156269 bytes. Size `80,85`, LocalSize `5,5,70,73`, NewINIFormat 4, TEMPERATE. The map contains 13515 allocated diamond cells, 573 Terrain entries, and no Tube or CellTag entries.
* The primary TEMPERATE TMP bytes come from physical `ra2.mix/isotemp.mix`. The packet records both archive hashes and all 838 requested file names, MIX hashes, byte counts and content hashes. All 266 reached native primary TMP heads have resident physical data. The reader asserts no requested primary `.tem` name in the eight selected override archives recorded in `assets.selected_override_checks`, or as a loose asset. This is a bounded physical cache projection, not execution of the entire native archive loader or every presentation variant.
* Applicable scalar/type layers are physical `RULESMD.INI`, absent `LANGRULE.INI`, physical `MPBattleMD.ini`, then physical `XMP03T4.MAP`. Full file hashes and admitted sections are stored in `readers.layers`. The shared reader bootstrap first reads its frozen Shrapnel inputs; `readers.bootstrap_layers` records that explicitly. MPBattleMD and XShrapnel admit no relevant bootstrap sections, and the Anytown passes then read the physical data through the original readers. `ARTMD.INI` is a separate unlayered input with its own hash.
* Original `47BBF0` constructs every Cell and Dummy. Physical tile/subtile/level/ice/overlay/frame fields are supplied at the loader-to-Cell boundary. All `LastTilesInSet` reads are native `-1` (or current count), establishing the legacy tile conversion identity for this input.
* Original overlay constructor/readers, land reader `674000`, TerrainType constructor `71DA80` and full TerrainType reader `71DEA0` provide the runtime type input. Selected original Tiberium Value/Image blocks derive all four Tiberium categories used by final `568BB0`; this does not claim the unrelated full Tiberium reader. Native building constructor/reader slices establish both class-affecting LaserFence/FirestormWall bytes are zero for all 48 physical building types.
* Original TileType constructor `5447C0` runs for all 838 heads; native integer/string readers provide shadow and tile-animation fields. Physical TMP offset-table pointers are relocated by the harness. Original `47D2B0` reads those headers and `483C80` derives the zone class. No class/land/slope answer is substituted.
* Original map iteration `578350`/`578290` gives the first recalculation sweep order. Each Terrain instance runs the actual constructor `71BB90`, its placement-coordinate callback `71E0D0`, `5F6940` and `5683C0` Place_Down in physical source order. This reaches real `47E8A0` ground-list insertion and `71C110` occupation; all 32 physical Terrain types have a native one-cell foundation and unchanged Image name. Final `568BB0(0)` recalculates every Cell and returns native ore value 133775.

Original `483C80` ignores ordinary Unit/Infantry ground-list categories. The ordinary building types are class-neutral under the executed gate reads above. Their runtime instances are omitted here; Terrain instances are present. Consequently `cells.has_ground_object` describes the bounded native Terrain membership, not all production objects. Wall owner assignment, visibility and discovery are not claimed.

## Executed navigation and bridge sequence

Raw navigation storage uses physical stride **166** (`(80+85)+1`) and 166 squared slots. Exported class/level/base-ID planes are 165 squared (27225) entries. Each graph also exports 331 padding IDs so the entire native graph-ID plane is retained.

1. Actual `56C510` constructs base connectivity. Actual `581F90` builds levels 2, 1, 0, followed by actual `42C1C0` pathfinder scratch refresh. Vector constructors execute; their final vtable/growth/header stores use the established caller-boundary projection.
2. Already-admitted `57CCF0` is called at `(87,54)` twice. The first call changes the nine concrete-low overlay cells and returns AL 0, without a connectivity rebuild. The second returns AL 1, reaches collapse endpoint notification, recalculates, rebuilds base connectivity and incrementally patches the full hierarchy.
3. Actual concrete-low repair `573540` starts at `(85,58)` and returns AL 0. It rebuilds base connectivity and incrementally patches the hierarchy. This is a direct repair-controller boundary, not an Engineer mission proof.

| State | Base zones | Graph record counts, levels 0/1/2 |
| --- | ---: | --- |
| Initial healthy | 359 | 3355 / 1336 / 704 |
| First damage | 359 | 3355 / 1336 / 704 |
| Collapse | 358 | 3363 / 1356 / 732 |
| Repair | 359 | 3370 / 1372 / 756 |

The repair retains native appended/dead graph records. Restored connectivity does not imply that all graph IDs or record arrays return to their initial values. Every record's parent, type and ordered outgoing edges is preserved. Collapse and repair each reach one `56C510`, one `586990` batch, both reverse Cell passes, four `584550` patches and four `42C1C0` scratch refreshes. Ordered trace events and reached-body counters are saved, not inferred from the final graph.

No live damageable object occupies the bridge in this graph packet. Actual Cell occupant callbacks see empty bridge lists. Resident/moving-head consequences are covered by the separate frozen [`anytown_occupants`](anytown_occupants.py) packet; they must be composed by the production owner, not inferred from this empty-span run.

## Movement and RNG bounds

The original UnitType constructor and selected original readers produce physical MTNK Strength 300, Crusher 1, SpeedType 1 (Track), MovementZone 0. The actual Unit constructor prefix `7353C0..7354CE` and Drive constructor/initializer run. Owner, health, life/limbo, current coordinates and selected ordinary actor fields are supplied; the probe actor is unmarked and absent from Cell/Logic lists.

Original Unit virtual `+1AC = 73F0A0` executes seven requests per state. Source coordinates are `(87*256+128, 53*256+128, 416)`; candidates are `(87,53)`, `(86,54)`, `(87,54)`, `(88,54)`, `(87,55)`, `(85,54)`, `(89,54)`. Each request supplies direction 4, height 4, previous Cell NULL, fifth argument 1. The five bridge Road candidates return 0 before damage, after first damage and after repair; the neighboring Water candidates return 7. After collapse the center row `(86..88,54)` also returns 7, while `(87,53)` and `(87,55)` remain 0. All queries preserve all three RNG states.

These are bounded direct CanEnter calls. Some supplied candidates are not an adjacent step from the declared source. Ordinary mission dispatch, AStar route production, locomotor head publication, occupancy competition and actor movement are not established by these queries.

Each stream begins with the complete original seed-0 state. The Unit constructor prefix performs one Scenario `65C780` Next draw; its complete before/after states and call trace are retained under `actor_constructor`. No range request accompanies that constructor draw. Main and Scenario then remain unchanged throughout all three bridge stages; first damage and collapse use no MapGen draws, while repair makes three MapGen range requests and three underlying Next draws. The complete state arrays and indices are retained, along with the range results. This is a supplied scenario-seed boundary, not the RNG position after an entire native match load. FPCW and native initialization receipts are saved.

## Explicit execution seams and exclusions

* Successful bounded allocation, free, CRT registration and TLS support are supplied.
* Physical file extraction and lexical INI cache construction run on the host; original case-sensitive numeric/scalar parsing and runtime readers execute against those caches.
* Two waterfall Anim constructors are encountered during the initial sweep and two during final `568BB0`. `421EA0` returns the allocated receiver; caller-side Cell `0x20000` flag and animation coordinate fields execute. Animation registration, art playback, lifetime and any hidden constructor RNG are excluded. No bridge-stage Anim constructor is reached.
* Shroud queries return hidden, skipping Terrain discovery. Original Cell membership and occupation still execute. Screen/radar/dirty-rectangle sinks are inherited from the frozen bridge geometry harness.
* AutoTube construction and an unexpected TMP-loader fallback fail closed; neither is reached. Native warnings also fail closed.
* This packet does not execute the whole scenario/file loader, general building/mobile world lifecycle, high bridges, visibility/AI, full projectile admission, renderer, or full gameplay load RNG. Its claim is the listed physical inputs, original Cell class derivation, native graph builders/updates and bounded MTNK admissions.

`initial` and `stages[*].state` contain `navigation`, three `graphs`, `cells`, `dummy`, `movement_admissions` and complete `rng`. Stage records add the original return byte, ordered trace, RNG before/after and per-stage reached counters. Production export comparisons must check all planes/rows and complete ordered graph records, with padding retained, rather than treating the record counts as sufficient.

### Authored CAFNCB / AP wall component

`python -m tools.spatial_oracle.anytown_damage.mtnk_attack --authored-fence --write`
(and `--check`) adds the consumed `authored_fence.json` comparison. The normal
mtnk_attack corpus and all Navigation defaults remain independent and unchanged.
The opt-in uses the same physical XMP03T4 Cell/TMP/Terrain runtime and reader,
full native Rules constructor, physical Overlay ART and stock MTNK/105mm/AP
readers. Actual decoded OverlayPack admission `5FD41A..5FD508`, constructor/Mark,
then second data pass `5FD61A..5FD646` establish wall identity and retained +122
before Terrain. The shared bridge saved-byte restoration `5FD4F3..5FD508` remains
a distinct bridge-only operation. Original nested ScenarioInit writers bracket
this setup; original Main zero/active-game stores precede live damage.

Each independently seeded history calls full original AreaDamage `489280` at
physical Cell43,81 center with its native-read stock damage. Every reached wall,
real `480CB0/480630/480510`, Recalc, connectivity/hierarchy edits, radar dirty
writer, pointer expiry/detach and deferred drain execute. The compact output
retains native admissions, changed cells/owner/data/+122/material/zone, all three
full RNG states and draw/callback order. The explicit null attacker and empty
Techno/listener registry do not prove ordinary MTNK target clearance; the normal
World shot/Cell-target regression is a separate integration consumer. Full native
Scenario bootstrap, authored buildings/Technos and whole-World RNG are excluded.
An uncovered live animation fails closed rather than supplying a gameplay result.

`python -m tools.spatial_oracle.bridge_shadow_render --authored-fence --write`
(and `--check`) consumes those native states into `wall_render.json`. It uses the
existing physical image/ColorScheme/TMP Lighting/native raster owners, with actual
map/type identity checks and observed A readback. Native initial source, partial
neighbor, changed endpoint and removed source states select the wall frames. The
existing sole ReadColor helper uses its small INI Fixture; selected ColorScheme
constructors, material conversion and raster execute in the existing raster VM. Sparse TMP/Cell registration,
current Player/House scheme index, local projection/clip/background/A/Z are explicit
component priors. Actual Cell lighting produces the top scalar; its floor pixels
are cleared before the separate wall raster. The erased row reuses the existing
Tactical loop's actual `-1` admission and observes no body/shadow calls; its map
and visibility transport boundaries remain explicit. This does not claim a whole
Tactical frame or GPU composition. Sidecars record the binary and reproduction command;
validation receipts remain external.

`python -m tools.spatial_oracle.anytown_damage.mtnk_attack --authored-fence-driveover --write`
(and `--check`) writes the separate `fence_driveover.json` component corpus.
It reuses the authored-fence VM, actual physical wall/type/map initialization,
entry observers and deferred-delete owner. Independent seed0 is installed on all
three streams after setup. Original `480CB0(-1)` executes first on authored
`43,82`, then `44,82`; the second call uses the actual first call's resulting
grid. Each `steps` row retains the existing `before/events/changed/before_drain/
after/source` fields and adds `source_cell`. Full RNG states, draw requests,
ordered callbacks, current radar storage, runtime cursor and queue drain remain
observed. The selected forced path must remove both receivers without RNG draws.
It still executes the accepted-hit dirty/data-increment prefix before removal.
This direct Unit route has no AreaDamage `70D4A0` detach.

The same MTNK reader additionally executes admitted layered
`TiltsWhenCrushes` reads at `7153DA..7153FB`, using the retained constructor
default at type `D2B`. The corpus records physical layer identities, source
lines, admission and before/after values under `input.mtnk_tilts`. No Rust value
or inferred stock default supplies that byte. Historical AP/default modes do
not execute these extra reads and retain their existing corpus.

This is a Cell component, composed with the existing 296
`unit_per_cell_overlay` caller/numeric controls and ordinary World integration.
Those caller controls replace map/damage/audio queries and explicitly exclude
sound playback RNG. The direct Cell mode creates no Unit and does not execute
UnitPerCell `739EC0`, its retained physical `+1B8` query, live sound `+48`
sample, Drive `+64` pre-tilt, `+334` recoil or `+6B5` lifetime. Selected physical
`WallCrushBlack` has random samples/pitch/volume, so draw-free forced Cell damage
does not establish draw-free Unit/audio behavior. Empty native listeners do not
prove positive target expiry, and no absolute normal movement frame is claimed.

`python -m tools.spatial_oracle.anytown_damage.mtnk_attack --concrete-wall --write`
(or `--check`) uses that same runtime and writes `concrete_wall.json`.
The opt-in `Inputs` selection additionally enrolls GAWALL's fixed ART section
and physical `GTWALL.SHP` through `bridge_shadow_render.physical_assets`' existing
archive/cache loop. Native constructor/current-default and layered full
`5FE770` outputs retain Strength, Wall, Image, fixed ART DamageLevels and theater
flags; original `5FE620` and lazy GetSHP must admit the actual image.
Historical selections use empty extra-type/material arguments.

Four component constructor inputs use physical flat cells `33,88`, `33,87`,
`33,89`, `33,90`. Each executes the existing decoded-construction caller
`5FD41A..5FD508`, real Overlay constructor/Unlimbo/Object Mark/Overlay Mark,
connectivity/navigation/122 writes and deferred drain. The corpus retains
construction events and both construction/independent entry-seed boundaries.
These cells are a separate ordinary paid witness's legal coordinates; GAWALL
is not map-authored there, and this component does not execute paid factories,
autofill or ordinary Building/Foot population. It supplies no damage nibble,
overlay identity store, Mark answer or favorable random word.

Independent seeds0/1 then execute stock MTNK/105mm/AP65 through complete
`489280`, repeating until the source wall clears under a bounded impact count.
Coverage requires actual accepted/rejected65 and original stage2 inline200
caller `480EB4`, including an admitted child. Complete three-stream RNG states,
request/raw order, inline calls, changed cells, dirty/navigation/radar, retained
122, expiry/detach and drain remain in the shared history schema.

The selected ordinary GDI-negative query controls use original BuildingType
FindOrAllocate/full constructors for GAPILE/GAPOWR and original layered General
gate/WallTower readers `66F450..66F589`. Original `526810` admits the General
section first; each name uses ReadString's empty default, then retains the
current field pointer when no name is returned. Separate
query-only positive-health Building shells supply unchanged native vtables,
constructed type pointers and linked-list priors. Complete original `480510`
traverses them; it supplies no RTTI or predicate answer. The temporary Cell
head is restored. This does not place or construct Buildings in the damage
history, prove a positive gate/Nod branch, or establish whole native paid-scene
owner/122/RNG equivalence. Production integration must preserve those contexts.
