# Terrain-Attached Anim Load Lifecycle and Side Effects — Ghidra Re-investigation

Date: 2026-08-31
Status: **COMPLETE for the bounded fresh active-YR authored/generated load corridor**
System: GSI-04.12 / GSI-04.13 / GSI-04.15 transaction-3 terrain-attached animation dependency
Mode: `/re-investigate`, exhaustive-slice; read-only Ghidra

## Verdict

`[ACTIVE-YR: YES]` Terrain-attached `AnimClass` instances created by
`CellClass::RecalcAttributes @ 0x0047D2B0` before the post-object boundary are
real objects, not discardable discovery descriptors. Each successful construction
consumes a fresh Scenario unique ID, enters the Object and Anim registries, runs
`ObjectClass::Unlimbo`, and, because the producer passes delay zero, runs
`AnimClass::Middle` synchronously before `RecalcAttributes` writes the terrain
markers, Z adjustment, and cell latch. `RandomRate`, when configured, consumes one
inclusive draw from `ScenarioClass+0x218` after ID assignment and registry insertion.
The Main RNG is not used anywhere in this constructor/Middle/Start corridor.

`[ACTIVE-YR: YES]` `MapClass::InitCellAttributes @ 0x00568BB0` does not call
the normal animation `Destroy`/`UnInit` path. It scans `g_AnimClass_Array` in live
order and invokes vtable slot `+0x20` with argument `1` for every animation whose
`Anim+0x197` terrain marker is set. The slot is the scalar-deleting destructor at
`0x00426590`: it runs `AnimClass::~AnimClass @ 0x004228E0` and frees the allocation
immediately. It does not enqueue a pending delete, create an `ExpireAnim`, or play
the configured `StopSound`. Its sound effect is narrower: release the two live
embedded sound-event handles and detach the two voice handles. Its owner cleanup
is conditional, but the terrain producer never assigns an owner, so that branch is
a checked no-op for these load-created objects.

`[ACTIVE-YR: YES]` The destructor does not ordinarily own this cell latch. After
all marked animations have been removed, `InitCellAttributes` performs a second
anti-diagonal whole-map sweep. For each cell it clears `Cell+0x140 & 0x20000` and
then calls `RecalcAttributes(-1)`, which recreates the surviving eligible set with
new IDs and repeats all active constructor side effects. The final set/order alone
is therefore not sufficient evidence: transient IDs, optional Scenario draws,
registry order, and Waterfall loop-sound start/release/restart are load-bearing.

`[ACTIVE-YR: CONDITIONAL]` Generated maps use the same mechanism after direct map
materialization. Their generator runs several whole-map Recalc sweeps and ends in
`InitCellAttributes(1)`. The first generator Recalc is after bridge/CABHUT work but
before `AddTechBuildings`, so animation IDs and optional `RandomRate` draws can
interleave between CABHUT and Neutral-Techno constructor trace events. Later
painting can create additional first-generation animations before the final
delete/recreate. A generated implementation that replays the complete construction
trace first and spawns only final tile-animation descriptors cannot match native
identity/RNG/sound order. Whether the earlier synthetic `Full_Init` itself finds
an eligible tile is map state/content dependent; zero must not be assumed without
capturing that staged state.

## Authority, claimed scope, and exclusions

- `[ACTIVE-YR: YES]` Authority is Ghidra program `gamemd.exe`, x86 PE image base
  `0x00400000`, executable
  `C:\Users\enok\Documents\Command and Conquer Red Alert II\gamemd.exe`, SHA-256
  `1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C`.
  This investigation used decompile, disassembly, xrefs, vtable bytes, and memory
  inspection only. No Ghidra metadata was changed.
- `[RETAIL-DATA]` Active theater INIs in the repository were scanned directly:
  `temperatmd.ini`, `snowmd.ini`, `urbanmd.ini`, `urbannmd.ini`, `desertmd.ini`,
  and `lunarmd.ini`, together with their referenced `artmd.ini` animation rows.
- `[LEAD ONLY]` OpenTS `code/cell.cpp:1359`, `code/anim.cpp:123..210`,
  `code/anim.cpp:289..342`, and `code/anim.cpp:1155...` were used to find the
  inherited cell-attachment, constructor, destructor, and Middle shapes. No OpenTS
  statement is parity authority; all material conclusions below were independently
  proved in active YR.
- Claimed scope is the fresh active-retail load path from each terrain-animation
  producing Recalc through the post-object `InitCellAttributes` deletion, unlatch,
  and recreation, including the generated-map ordering dependency.
- Out of scope are ordinary frame-age destruction, save restoration of already-live
  animations, multiplayer feedback animations, voxel animations, and presentation
  rendering after construction. They are mentioned only to exclude their paths.
- OOM/vector-growth failure is characterized where native behavior affects the API
  contract, but is not an ordinary-play acceptance target.

Confidence: **HIGH** for the bounded lifecycle, call order, RNG owner/order,
vtable deletion path, latch ownership, and stock data census; **MEDIUM-HIGH** for
opaque ancillary Object registry labels whose exact names are unnecessary to the
Rust handoff; **CONDITIONAL** for the exact number of eligible animations present
during the generated synthetic pre-materialization `Full_Init`.

## 1. Overview and integrated native timeline

### 1.1 Authored fresh load

| Order | Active in YR | Owner / address | Exact terrain-animation effect |
|---:|---|---|---|
| 1 | **Conditional** | Synchronous `OverlayClass::Mark(1)` calls into `CellClass::RecalcAttributes` while `ReadMapOverlayPacks @ 0x005FD2E0` walks OverlayPack | The first eligible Mark for a cell can construct and latch its animation in decoded source order. Later Mark/Recalc calls on that cell see the latch and suppress a duplicate. |
| 2 | **Yes** | `Full_Init @ 0x00686B20`, post-OverlayData whole-map Recalc call `0x00687A5A` | Visits every real cell in native anti-diagonal order. Cells already latched by Mark are skipped; otherwise eligible cells construct now. This first complete generation exists before authored map objects. |
| 3 | **Yes** | Full_Init Terrain `0x00687A74` | Authored Terrain objects construct after the transient tile animations and therefore receive later unique IDs. Terrain can also change live cell/overlay state before the final sweep. |
| 4 | **Yes** | Units `0x00687AA7`, Aircraft `0x00687ABF`, Infantry `0x00687ACB`, Structures `0x00687AEA` | Authored Technos construct after first-generation tile animations. Their IDs and registry insertion are between first and final tile-animation generations. |
| 5 | **Yes** | Smudge `0x00687B0E` | Authored Smudges load before deletion and final Recalc. |
| 6 | **Yes** | `MapClass::InitCellAttributes(0)` call `0x00687B92`; deletion loop in `0x00568BB0` | Every live `Anim+0x197` entry is scalar-deleted immediately in current Anim registry order. Array compaction is handled by rechecking the current index. |
| 7 | **Yes** | InitCellAttributes second anti-diagonal sweep, latch clear `0x00568CC7`, Recalc call `0x00568DF4` | Clears each cell's `0x20000` latch immediately before its Recalc; recreates only the now-eligible final set, with fresh IDs and repeated constructor/Middle/sound effects. |

`[ACTIVE-YR: YES]` The first whole-map and final whole-map Recalc sweeps each
make exactly `H*(2W-1)` calls over real cells for map width `W` and height `H`.
Both use the same anti-diagonal iterator. Consequently first-generation order is:

1. first successful eligible per-Mark occurrence, in decoded OverlayPack order;
2. remaining eligible cells, in the first anti-diagonal sweep;

whereas final-generation order is purely the second anti-diagonal sweep over the
post-object cell state.

### 1.2 Generated fresh load

`[ACTIVE-YR: YES]` `RandomMapGenerator::Generate @ 0x00598960` first calls
`InitMapFromSyntheticINI @ 0x00599650`, which reaches `ScenarioClass::Full_Init`.
The default synthetic `NewINIFormat=0` makes encoded OverlayPack bodies inert, but
the Full_Init Recalc and InitCellAttributes boundaries themselves are ungated.
It is unsafe to assert that these sweeps always construct zero animations; their
effect depends on the staged map state supplied to that Full_Init.

`[ACTIVE-YR: CONDITIONAL]` After the generator materializes terrain, its relevant
order is:

| Generator order | Native effect |
|---:|---|
| 1 | Water/region/river/bridge processing, including CABHUT construction attempts. |
| 2 | First generator whole-map Recalc, call site `0x00598E48`. Any currently eligible tile animation constructs here. |
| 3 | Start-point work, then `AddTechBuildings`; Neutral Techno constructors occur here. |
| 4 | Tiberium, then whole-map Recalc call sites `0x00598FE7` and `0x00599153`. |
| 5 | Hills, LAT, trees, and rocks. The temperate LAT helper can also call Recalc directly at `0x005A4259`. |
| 6 | Final generator whole-map Recalc at `0x0059937D`. |
| 7 | Final `InitCellAttributes(1)`: scalar-delete marked animation generation, clear latches, and recreate the surviving set. |

`[ACTIVE-YR: CONDITIONAL]` Retail waterfall tiles are recognized in active RMG
river/waterfall shaping and have `Tile##Anim` bindings. On maps that place them
before the first generator Recalc, their animation construction is between bridge
and Neutral-Techno construction. On maps where a later painting phase first makes
a tile eligible, its initial construction occurs in that later Recalc instead.
Therefore generated animation history must be recorded at native phase boundaries;
final cell contents cannot reconstruct all prior eligible generations.

### 1.3 Generated Building attempts also consume unique IDs

`[ACTIVE-YR: YES]` The generated construction replay must advance the shared
unique-ID sequence for every **actual `BuildingClass` construction**, even when a
later Neutral-Tech placement failure destroys the object. The precise owner/order
is important: `TechnoClass::Constructor @ 0x006F2B90` does not assign this ID. It
first consumes/stores its unconditional raw Scenario word at `0x006F3254`;
`BuildingClass::Constructor @ 0x0043B740` later calls
`AbstractClass::AssignUniqueID @ 0x00410230` at call site `0x0043BA15` after
installing the Building vtables. `AssignUniqueID` calls
`ScenarioClass::NextUniqueID @ 0x0068BCB0`, which pre-increments
`Scenario+0x214`. Both effects occur before the caller attempts Unlimbo/placement,
and destruction never rolls the counter back. Thus one generated Building event's
native side-effect order is **Techno Scenario word, then Building unique ID, then
placement outcome**.

`[ACTIVE-YR: YES]` Both Neutral-Tech owners construct before their up-to-100
placement loops: map types other than 2 call the Building constructor at
`0x005A96F8`, while the type-2 region owner calls it at `0x005954A1`. A failed
loop scalar-deletes the already-constructed object. Every emitted or discarded
Neutral-Tech trace event must therefore consume one stable ID; only an emitted
event binds that ID to a projected entity. Omitting the discarded event shifts all
later animation/object IDs even if the Scenario constructor word is replayed.

`[ACTIVE-YR: YES, NEGATIVE CABHUT BOUNDARY]` CABHUT is narrower. The repair-hut
helper `0x005904B0` searches for a qualifying cell **before** allocation and
construction. A failed search performs no constructor, consumes no Scenario word
and no unique ID, and must not create a trace event. Once it constructs CABHUT, it
calls Unlimbo, ignores that return, never deletes the Building on failure, and
returns success; the verified active stock 1x1 CABHUT path Unlimbos successfully.
Accordingly the active CABHUT trace contains constructed/emitted events, not a
native discarded-constructor class. The general replay rule is “every actual
constructor event consumes an ID,” not “every CABHUT search attempt consumes an
ID.” A design statement that says “all discarded CABHUT attempts” without this
qualification overclaims the native path.

## 2. Native layouts and ownership

The following offsets are proved only to the semantic precision needed by this
transaction.

| Owner | Offset / global | Verified use in this path |
|---|---:|---|
| `CellClass` | `+0x11A` | Current sub-tile index compared with `IsoTileType+0x2D4 AttachesTo`. |
| `CellClass` | `+0x140`, bit `0x20000` | Per-cell terrain-animation latch. Recalc tests and sets it; final InitCellAttributes explicitly clears it. |
| `AnimClass` | `+0x100` | Producer-written terrain tile Z adjustment, written after constructor/Middle returns. |
| `AnimClass` | `+0xCC` | Attached owner pointer. Constructor initializes null; this producer never assigns it. |
| `AnimClass` | `+0x196` | Producer marker written to `1` after construction. |
| `AnimClass` | `+0x197` | Terrain-attached deletion marker written to `1` after construction; InitCellAttributes selects on it. It is not the cell latch. |
| `AnimClass` | `+0x198` | Middle StartSound suppression test. Fresh constructor state is false. |
| `AnimClass` | `+0x1A0`, `+0x1B4` | Embedded sound-event handles released by the direct destructor. |
| `IsoTileTypeClass` | `+0x2C8` | Tile animation type index; `-1` means none. |
| `IsoTileTypeClass` | `+0x2D4` | `AttachesTo` sub-tile selector. |
| `IsoTileTypeClass` | `+0x2D8` | Tile Z adjustment copied to `Anim+0x100`. |
| `AnimTypeClass` | `+0x2F8` | StartSound identifier tested by `AnimClass::Middle`. |
| `AnimTypeClass` | `+0x298` | Raw SHP frame-count/2 value used by Middle's zero test before `AnimClass::Start`; it is not an SHP pointer. |
| `AnimTypeClass` | `+0x355` | `IsVeins` conditional latch-clear behavior in the destructor; false for the active stock tile-animation types. |
| `AnimTypeClass` | `+0x357` | `TiberiumChainReaction` path in Middle; false for the active stock tile-animation types. |
| `ScenarioClass` | `+0x214` | Unique-ID counter, pre-incremented by `NextUniqueID`. IDs are not rolled back on destruction. |
| `ScenarioClass` | `+0x218` | Scenario RNG receiver used by `RandomRate` and every other constructor/Middle/Start random call in this corridor. |
| Global | `g_AnimClass_Array` | Ordered live Anim registry. Constructor appends; InitCellAttributes scans; destructor compacts on removal. |
| Global | `g_ObjectClass_Array` plus three ancillary Object vectors | Object constructor registers before Anim constructor body; Object destructor removes immediately. Some ancillary labels remain intentionally opaque. |

`[ACTIVE-YR: YES]` The latch is cell-owned. `Anim+0x197` tells
InitCellAttributes which live animation to delete, but it does not give that
animation ordinary responsibility for clearing `Cell+0x140 & 0x20000`.
`AnimClass::~AnimClass` has a separate `AnimType+0x355 IsVeins` conditional clear;
that condition is false for the stock terrain-attached rows and is not keyed from
`Anim+0x197`. The deterministic general clear is the later cell sweep.

## 3. Core logic

### 3.1 Exact Recalc producer

`[ACTIVE-YR: YES]` In `CellClass::RecalcAttributes @ 0x0047D2B0`, the
terrain-animation block is after the shared-dummy/valid-tile gate and operates on
the pristine IsoTileType head retained before LAT replacement. It is bypassed by
an earlier return when an overlay supplies the cell's attributes. For the block to
construct, all of these are true in order:

- `Cell+0x140 & 0x20000 == 0`;
- the current tile's animation type index is not `-1`;
- `IsoTileType+0x2D4 AttachesTo == Cell+0x11A`.

The producer computes Z as the signed cell level times the ground-level height.
It converts the tile pixel offset through `FUN_006D2360`, adds map-coordinate
cell units (`*256`) and the `+128` cell-center bias, allocates `0x1C8` bytes, and
calls `AnimClass::Constructor @ 0x00421EA0` with:

- the selected `g_AnimTypes_Array` entry;
- the computed world coordinate;
- delay `0`;
- loop `-1`;
- draw flags `0x1600`;
- constructor Z adjustment `0`;
- reverse `0`.

Only after the constructor returns does Recalc write `Anim+0x196=1`,
`Anim+0x100=IsoTileType+0x2D8`, `Anim+0x197=1`, and finally set the cell latch.
Thus Unlimbo and delay-zero Middle cannot observe the producer's terrain marker or
final Z adjustment. A null allocation is followed by native null-relative writes;
it is a crash/degraded path, not a successful silent skip.

### 3.2 Constructor ordering and RNG

`[ACTIVE-YR: YES]` `ObjectClass::Constructor @ 0x005F3900` runs first. It
constructs the Abstract base and appends the receiver to `g_ObjectClass_Array`
and three additional global DynamicVectors. `AnimClass::Constructor` then:

1. initializes fields, including `Anim+0x196=0`, `+0x197=0`, and owner `+0xCC=0`;
2. installs Anim vtables;
3. calls `AbstractClass::AssignUniqueID @ 0x00410230`;
4. `AssignUniqueID` calls `ScenarioClass::NextUniqueID @ 0x0068BCB0`, which
   pre-increments `Scenario+0x214` and stores the result at `Abstract+0x10`;
5. initializes both embedded animation sound events;
6. appends the receiver to `g_AnimClass_Array` and sets it active/alive;
7. if either `RandomRate` endpoint is nonzero and min is not above max, calls one
   inclusive `RandomRanged` on receiver `ScenarioClass+0x218`;
8. applies Normalized delay transformation without consuming RNG;
9. continues through placement/Unlimbo and delay-zero Middle.

`[ACTIVE-YR: YES]` This proves two ordering constraints: the ID is allocated before
the optional `RandomRate` draw, and the animation is already present in the Anim
registry when that draw occurs. Destroying the transient object does not decrement
or reuse its ID.

`[ACTIVE-YR: YES]` Bouncer/Meteor setup, Tiberium-chain child construction, and
crater/scorch branching can contain additional random calls for configured custom
types. Every inspected receiver is `ScenarioClass+0x218`; no Main RNG receiver is
used in the constructor/Middle/Start corridor. Those optional branches are absent
from all 20 active stock theater tile-animation types.

### 3.3 Unlimbo, registration, display, and occupancy

`[ACTIVE-YR: YES]` `Main__PrepareSession @ 0x0052D9A0` sets `g_GameActive=1`
before this load corridor. `ObjectClass::Unlimbo @ 0x005F4EC0` therefore takes the
active path. It clears limbo/redraw state, stores the coordinate, dispatches
`AnimClass::Mark(1) @ 0x004238B0` through vtable slot `+0x124`, submits the object
to a Display layer, and—because the AnimType LogicVisible default is true—registers
it in Logic in ordinary modes.

`[ACTIVE-YR: YES]` The Mark ultimately reaches `ObjectClass::Mark @ 0x005F5850`.
On success it sets the Object's on-map state and performs redraw/cell notification
work. It does not insert this animation in Terrain or Techno ground-occupancy lists,
and the Recalc producer does not set an owner or cell-occupation pointer. Rust must
model the live Anim/Logic identity effects without inventing entity occupancy.

### 3.4 Delay-zero Middle and StartSound

`[ACTIVE-YR: YES]` Because the producer passes delay zero, the constructor calls
`AnimClass::Middle @ 0x00424CE0` synchronously before returning. Middle:

1. calls `Mark(2)`;
2. when `Anim+0x198` is false and `AnimType+0x2F8 StartSound` is valid, obtains
   the animation coordinate and calls `VocClass::PlayAt @ 0x007509E0` using the
   embedded handle at `Anim+0x1A0`; otherwise it stops/clears that handle;
3. always stops/clears the second handle at `Anim+0x1B4`;
4. calls `AnimClass::Start @ 0x00424F00` only when the raw SHP frame-count/2 field
   at `AnimType+0x298` is zero;
5. conditionally executes the `TiberiumChainReaction` branch when
   `AnimType+0x357` is true.

`[ACTIVE-YR: YES]` The `+0x298` identity was rechecked in
`AnimTypeClass__LoadImageAndResolveFrameBounds @ 0x00427B50`; stale material that
calls it an SHP pointer is wrong. For the loaded stock tile animations, the SHP
frame count is present, so Middle is real but its conditional `Start` call is not
a stock side effect. The four stock Waterfall `01` types do execute StartSound.

`[ACTIVE-YR: NO for stock; CONDITIONAL for custom]` `AnimClass::Start` can spawn
a particle, place scorch/crater/debris/Tiberium effects, or consume a Scenario RNG
draw on configured rows. None of those keys/flags is present on the 20 active stock
tile-animation types. A custom theater AnimType can activate them and therefore
must follow normal AnimClass semantics; they must not be misreported as unconditional
effects of terrain attachment.

### 3.5 Exact post-object deletion path

`[ACTIVE-YR: YES]` At entry, `MapClass::InitCellAttributes` walks
`g_AnimClass_Array` from index zero. When `Anim+0x197` is set it calls the object's
primary vtable slot `+0x20` with argument `1`. Vtable `0x007E3354` resolves that
slot to `AnimClass::ScalarDeletingDestructor @ 0x00426590`, which calls
`AnimClass::~AnimClass @ 0x004228E0` and then `operator_delete` because argument
bit zero is set. The removal compacts `g_AnimClass_Array`; the loop adjusts/rechecks
the current index and reloads array/count, so no shifted entry is skipped.

The direct destructor performs the following relevant work synchronously:

- immediately dispatches pointer-expiry cleanup;
- under active game state, conditionally handles an attached owner: scan the Anim
  array for another Anim sharing that owner; if none remains, call owner vtable
  `+0x17C` and clear owner state `+0x84`; then clear `Anim+0xCC`;
- conditionally clears a cell latch only for `AnimType+0x355 IsVeins`;
- calls `ObjectClass::Limbo @ 0x005F4D30`;
- removes the receiver from the Anim registry, the Object registry, and ancillary
  object/global vectors while preserving survivor order through compact-left removal;
- releases both embedded SoundEvents at `+0x1A0/+0x1B4` through
  `SoundEvent__Release @ 0x00406060` and detaches both voice handles;
- clears its type/owner fields, chains through the Object/Abstract destructors,
  and frees the scalar allocation before the vtable call returns.

`[ACTIVE-YR: YES]` For this producer specifically, owner cleanup is not an active
mutation: the constructor initializes owner null and Recalc never sets it. Design
or implementation prose may describe the destructor's conditional owner branch,
but must not claim that a terrain-attached load animation has an owner that needs
cleanup.

`[ACTIVE-YR: CONDITIONAL, dormant on fresh stock path]` The destructor calls a
type helper at `0x00428DE0`. Its image-release body requires both
`AnimType+0x35E` and `+0x35F`. The AnimType constructor initializes both false,
and the fresh-path writer scan found no active writer for `+0x35F`. This is not a
load-time stock tile-image free/reload effect and should not be implemented as one.

### 3.6 What direct scalar deletion deliberately does not do

`[ACTIVE-YR: YES]` `ObjectClass::UnInit @ 0x005F65F0` would call pointer cleanup,
Limbo, clear alive, and append to the deferred-finalization vector. It is not called
by InitCellAttributes. `AnimClass::Destroy @ 0x004255B0` would run owner handling,
release the current sound, conditionally play `StopSound`, and then use
`ObjectClass::UnInit`. It too is not called. Therefore this transaction has:

- no pending-delete insertion or later frame drain;
- no configured `StopSound` playback;
- no `ExpireAnim` construction;
- no interval in which the selected transient remains live but merely inactive;
- no ID rollback.

`[ACTIVE-YR: YES]` `ObjectClass::Limbo` still performs Deselect, object destroy/
pointer-expiry work, Mark(0), Display removal, base sound stop, conditional Logic
unregistration, dirty-extent work, and limbo/redraw flag changes. `ObjectClass`
destruction then removes the object from all live registries. Thus “immediate
scalar delete” does not mean “drop only the AnimStore row”; it means synchronous
normal destructor cleanup without the ordinary UnInit/pending-delete policy.

### 3.7 Sound lifecycle

`[ACTIVE-YR: YES]` `VocClass::PlayAt` binds the live StartSound event/loop handle.
`SoundEvent__Release` releases/stops a valid active event and clears its handle.
The scalar destructor does not consult or play the AnimType `StopSound`. Final
recreation then calls delay-zero Middle again and starts the final sound handle.

For a stock Waterfall `01` tile present in both generations, native observable
ordering is:

1. first-generation `StartSound=WaterfallLoop` starts during construction;
2. scalar destructor releases/stops that live handle, with no StopSound identity;
3. final-generation Middle starts `WaterfallLoop` again.

An implementation that emits the first start but omits its synchronous release can
leave a dead loop alive when the app drains events. An implementation that suppresses
the entire transient generation loses both sound lifecycle and unique-ID effects.

## 4. Retail data census

`[RETAIL-DATA]` Across the six active theater INIs there are exactly 20 active
`Tile##Anim` names:

- `TUNTOP01`, `TUNTOP02`, `TUNTOP03`, `TUNTOP04`;
- `WA01X` through `WA04X`;
- `WB01X` through `WB04X`;
- `WC01X` through `WC04X`;
- `WD01X` through `WD04X`.

`[RETAIL-DATA]` Every one has zero/absent `RandomRate`. None declares StopSound,
ExpireAnim, SpawnsParticle, Scorch, Crater, TiberiumChainReaction, Bouncer, Meteor,
or another constructor/Middle/Start random branch. Consequently a stock load
consumes zero Scenario RNG and zero Main RNG for each actual tile-animation
construction, while still consuming a unique ID and performing registration and
sound effects.

`[RETAIL-DATA]` The 16 Waterfall rows loop (`LoopStart=0`, `LoopEnd=8`,
`LoopCount=-1`, `Rate=320`), are flat/demand-loaded cell-drawer animations, and
only `WA01X`, `WB01X`, `WC01X`, and `WD01X` declare
`StartSound=WaterfallLoop`. The other twelve waterfall rows start no sound.
`TUNTOP01..04` loop from frame zero with infinite loop count and rate zero; two
explicitly carry ground/Y-sort configuration and the others use their defaults.

`[ACTIVE-YR: CONDITIONAL]` A valid custom theater can bind another AnimType through
`Tile##Anim` and configure `RandomRate` or other active AnimClass options. Such a
row consumes one Scenario draw per actual construction when its RandomRate range
passes the native endpoint test. A survivor normally constructs at least twice in
the authored lifecycle—first generation and final generation—and therefore draws
twice, with object construction and possible other marked cells interleaved.

## 5. State transition table

| Stage | Cell latch | Anim marker `+0x197` | Registries / ID | RNG | Sound / owner | Lifetime |
|---|---|---|---|---|---|---|
| Eligible Recalc entry | clear | no object | none | none | none | candidate |
| Object/Anim ctor before RandomRate | clear | `0` | Object vectors + Anim registry; fresh Scenario ID assigned | none yet | owner null; sound handles initialized | live |
| Optional RandomRate | clear | `0` | unchanged | one Scenario inclusive draw; no Main draw | unchanged | live |
| Unlimbo | clear | `0` | Display/Logic registered as applicable | only configured special ctor branches | no cell occupancy, no owner | live/on-map |
| Delay-zero Middle | clear | `0` | unchanged | configured custom Middle/Start branches only | optional StartSound binds live handle | live |
| Producer post-write | set | `1` | unchanged | none | Z adjustment installed; owner remains null | terrain-marked live |
| Repeated Recalc before final boundary | set | `1` | duplicate suppressed | none | unchanged | same object survives |
| InitCellAttributes scalar dtor | normally still set | selected | removed immediately; ID remains consumed | none | active sound handles released; owner branch sees null; no StopSound | allocation freed before call returns |
| Final sweep before cell Recalc | explicitly clear | no object | none | none | none | eligible again |
| Final surviving Recalc | set | `1` on new object | fresh later ID; appended after surviving unrelated objects/anims | optional new Scenario draw | optional StartSound starts again; owner null | final live object |
| Final non-survivor Recalc | clear | no object | no recreation | none | none | absent |

## 6. Current Rust correspondence and exact delta

### 6.1 What already matches

- `src/map/resolved_terrain.rs:384..398` has the needed final tile-animation
  descriptor fields: cell, type name, coordinate, and Z adjustment.
- `src/map/resolved_terrain.rs:2421` sorts final descriptors by native
  anti-diagonal key `(rx+ry, rx)`.
- `src/sim/runtime.rs:399..` supplies constructor delay zero, loop `-1`, flags
  `0x1600`, constructor Z adjustment zero, and writes the descriptor Z adjustment
  through the spawned animation state.
- `src/sim/anim_class.rs` already has Scenario-owned Anim rate selection,
  AnimStore/Logic registration, Unlimbo-equivalent reveal, Middle, and sound-event
  primitives adequate for ordinary AnimClass construction.
- `src/sim/world/mod.rs:3973` owns the shared monotonic stable-ID allocator, the
  correct architectural analogue for the Scenario unique-ID word.

### 6.2 Verified mismatches

| Rust owner | Current behavior | Native-required delta |
|---|---|---|
| `src/map/resolved_terrain.rs:2090..2421` | Discovers one final descriptor set during a row-major resolved-grid build, then sorts it. It has no per-Mark generation, latch history, or post-object second generation. | Move lifecycle ownership into an explicit load transaction that can represent first eligible Mark creation, remaining first-sweep creation, immediate deletion, latch clear, and final-sweep recreation. A final projection remains useful but cannot be the only history. |
| `src/sim/runtime.rs:655..663` | Spawns only final descriptors after map objects. | Construct the first generation before authored Terrain/Technos, preserve its IDs/side effects, delete after Smudge, then recreate survivors. Keep final spawn after objects, but not as the only spawn. |
| `src/sim/anim_class.rs:543,550` | `choose_anim_rate` runs before `allocate_stable_id`. | Allocate/register the Anim analogue before the optional RandomRate draw for this and, if shared spawn ordering is corrected, all native Anim constructors that use the same path. |
| `src/sim/anim_class.rs:833..` | `destroy_anim` follows ordinary owner/deactivate/sound/StopSound/conceal/pending-delete policy. | Do not reuse it for InitCellAttributes. Add a narrow immediate scalar-delete primitive/equivalent: release the currently bound sounds with StopSound forced absent, unregister Logic/live Anim storage immediately, perform conditional owner cleanup only if actually present, and never enqueue pending delete. |
| `src/sim/world/lifecycle.rs:2805..` | `process_pending_delete` drains later. | No drain is part of this deletion. Tests must prove the pending-delete collection remains empty and the transient is absent immediately. |
| `src/map/construction_trace.rs:34..` | `RmgConstructionTrace` carries only a flat vector of Building constructor attempts/phases. | Generated load needs explicit native phase boundaries or equivalent staged animation events/state so first-Recalc animations can interleave after CABHUT and before Neutral Techno, later painting generations can be retained, and the final scalar-delete/recreate can run. Final map cells alone are insufficient. |
| `ResolvedTerrainGrid: Clone` and `tile_animations()` | Retains cloneable descriptor authority. | The mutable lifecycle/history must have one transaction owner. A cloneable final render descriptor may remain only as a projection after the lifecycle owner has established final state; it cannot authorize repeat construction. |

`[RUST DELTA]` Rust has no raw Display-list or native ancillary-vector analogue,
and presentation is intentionally separated. Exact parity does not require
inventing those containers. It does require the observable shared effects Rust
does own: stable-ID cursor, live Anim order, Logic visibility, sound events,
owner link if any, immediate removal, and no entity/cell occupancy insertion.

`[RUST DELTA]` For sound, the existing event vocabulary can represent destructor
release as `SimSoundEvent::AnimationStopped { stop_sound_id: None }`. The load
scalar-delete primitive must not pass the AnimType's configured StopSound to the
ordinary destruction helper.

### 6.3 Existing focused test gap

`src/app/loading/init_helpers.rs:786`
`gsi_13_04_post_map_object_spawn_preserves_descriptor_order_state_and_sound`
proves only that final descriptors spawn after a map object with expected final
state and StartSound. Selector tests in `resolved_terrain.rs` prove final admission
and anti-diagonal sorting. They do not prove:

- per-Mark versus first-sweep first-generation order;
- transient unique-ID consumption before Terrain/Technos/Smudge;
- ID-before-RandomRate ordering;
- immediate scalar deletion rather than generic pending destruction;
- StartSound release without StopSound and final restart;
- latch clear/recreation after object mutation;
- generated CABHUT/animation/NeutralTech interleaving;
- preservation of unrelated Anim survivor order.

The current final surviving-set/order fixture is therefore necessary but not
sufficient for this lifecycle.

## 7. Coverage ledger

| Mechanism | Trigger | Native evidence | Active verdict | Rust verdict |
|---|---|---|---|---|
| Per-Mark early creation/latch | Eligible tile reached by Overlay Mark Recalc | `RecalcAttributes 0x0047D2B0`; synchronous Mark owner | **Conditional, active content** | **Missing** |
| First post-data full creation | Every fresh authored Full_Init | call `0x00687A5A` | **Yes** | **Missing as lifecycle; final descriptor only** |
| ID and registry before RNG | Every successful Anim construction | Object ctor; Anim ctor `0x00421EA0`; AssignUniqueID `0x00410230` | **Yes** | **Wrong order for RandomRate** |
| RandomRate receiver/count | Custom AnimType with passing endpoints | ctor receiver `Scenario+0x218` | **Conditional; stock count zero** | **Receiver matches; staging/order missing** |
| Main RNG exclusion | All constructor/Middle/Start branches inspected | receiver audit | **No Main draws** | **Must remain excluded** |
| Unlimbo/Logic/Display/Mark | Every successful construction | `ObjectClass::Unlimbo 0x005F4EC0` | **Yes** | **Logic/reveal largely present** |
| Cell/entity occupancy | Terrain-attached animation construction | Mark/Unlimbo/producer audit | **No raw occupancy insertion** | **Must not add** |
| Delay-zero Middle | Every producer construction | ctor argument and `Middle 0x00424CE0` | **Yes** | **Present only for final spawn** |
| StartSound | Four stock Waterfall `01` types; custom types | Middle + retail data | **Conditional; common generated visual** | **Initial/final lifecycle missing** |
| Conditional Start | Raw SHP count/2 equals zero | Middle + loader `0x00427B50` | **Conditional; not stock tile effect** | **Do not overclaim** |
| Post-object deletion selection/order | `Anim+0x197` set | InitCellAttributes scan | **Yes** | **Missing** |
| Immediate scalar free | Every selected terrain Anim | vtable `0x007E3354 +0x20 -> 0x00426590` | **Yes** | **Missing primitive** |
| Generic Destroy/StopSound/pending exclusion | Same | contrast `AnimClass::Destroy 0x004255B0`, `ObjectClass::UnInit 0x005F65F0` | **Explicitly absent** | **Generic helper conflicts** |
| Owner cleanup | Direct destructor owner non-null | `AnimClass::~AnimClass 0x004228E0` | **Conditional globally; no-op for producer** | **Do not invent terrain owner** |
| Explicit unlatch | Every final InitCellAttributes sweep cell | `0x00568CC7` | **Yes** | **Missing** |
| Final surviving recreation | Eligible post-object cell | Recalc `0x00568DF4` | **Yes** | **Final-only spawn approximates state, not history** |
| Generated staged lifecycle | Eligible RMG tile during generator Recalcs | Generate call sites `0x00598E48`, `0x00598FE7`, `0x00599153`, `0x0059937D`; final InitCellAttributes | **Conditional and active** | **Flat trace/final spawn cannot represent** |

No mechanism in this table is closed by final descriptor equality alone. The row
remains open until its native side-effect sequence and negative exclusions pass.

## 8. Open-question log

### Resolved in this investigation

1. **Who owns the latch?** The cell owns `+0x140 & 0x20000`; the final sweep
   explicitly clears it. `Anim+0x197` is the deletion selector, not latch ownership.
2. **Does deletion use Anim Destroy/UnInit?** No. It invokes the scalar-deleting
   destructor directly through vtable slot `+0x20` with argument `1`.
3. **Immediate or deferred?** Immediate; the allocation and live registries are
   gone before the call returns. No pending-delete entry is created.
4. **Does it play StopSound or ExpireAnim?** No. It releases live handles only.
5. **Does owner cleanup matter for these tile animations?** The destructor owns a
   conditional branch, but this producer leaves owner null, so no owner mutation occurs.
6. **Does construction consume an ID?** Yes, before optional RandomRate; the ID is
   permanently consumed even when the object is deleted later in the same load.
7. **Which RNG owns RandomRate?** Scenario `+0x218`; one inclusive draw per actual
   construction when either endpoint is nonzero and min <= max. No Main RNG.
8. **Does Middle run before terrain markers/ZAdjust?** Yes. Delay zero makes it
   synchronous inside the constructor; producer post-writes follow the return.
9. **Does Middle always call Start?** No. Start is gated by raw SHP frame-count/2
   `+0x298 == 0`; the stock loaded tile animations do not satisfy that condition.
10. **Does Unlimbo occupy the cell?** No Terrain/Techno ground-list occupancy is
    inserted. The object does receive on-map/Display/Logic registration effects.
11. **What stock rows draw RNG?** None of the 20 active stock TileAnim types. Four
    Waterfall `01` rows do start a loop sound.
12. **Can final cells reconstruct generated history?** No. Native Recalcs occur at
    multiple generator phases, including between CABHUT and Neutral-Tech construction.

### Still bounded-open / explicitly deferred

- **OQ-A:** The exact eligible terrain-animation set during the synthetic
  pre-materialization `Full_Init` is state/content dependent and was not enumerated
  for every generator seed/mode. Implementation must preserve/capture this boundary
  or prove zero for its exact staged state; it must not hard-code zero from the
  `NewINIFormat=0` pack gate.
- **OQ-B:** Exact human-readable identities of all three ancillary Object constructor
  vectors are unnecessary to this Rust delta and remain intentionally unlabelled.
  The verified observable requirements are Anim/Object/live order, Logic, sound,
  owner, and identity effects.
- **OQ-C:** The `AnimType+0x35E/+0x35F` guarded image-release path is dormant on the
  fresh path proved here. Restore/raw-loaded type state is outside this transaction.
- **OQ-D:** OOM and DynamicVector growth failure can leave native partially
  registered state or crash on producer post-writes. A Rust hard error/panic is a
  defensible safety policy; silently skipping an eligible animation is not native.
- **OQ-E:** Save/restore and pause/replay of already-live tile animations are not
  part of this fresh lifecycle. Fresh replay load uses this same creation sequence;
  restore-specific object hydration needs its own evidence if later routed here.

## 9. Implementation handoff

### 9.1 Required behavioral contract

1. Represent one load-owned tile-animation lifecycle, not merely a cloneable final
   descriptor list.
2. On authored input, construct an eligible cell at its first native Recalc:
   per-Mark source order first, then the post-data anti-diagonal sweep for remaining
   cells. Suppress duplicate construction with the cell latch.
3. For each construction, preserve native ordering: Object/Anim registration and
   stable unique ID first; optional Scenario RandomRate draw second; reveal/Logic;
   delay-zero Middle/StartSound; then terrain markers, Z adjustment, and latch.
4. Run Terrain, Unit, Aircraft, Infantry, Structure, and Smudge construction while
   the first generation remains live.
5. At the post-object boundary, scan terrain-marked Anim entries in live Anim order
   and remove them synchronously through a scalar-delete analogue. Release active
   sound with `stop_sound_id=None`; do not call generic Anim destroy, do not enqueue,
   do not create ExpireAnim, and do not play StopSound.
6. Treat owner cleanup as conditional generic destructor behavior; terrain producer
   instances should have no owner and no owner cleanup mutation. Never insert them
   into entity/cell occupancy.
7. Clear each cell latch immediately before the second anti-diagonal Recalc and
   recreate the post-object eligible set with new IDs and all constructor effects.
8. For generated maps, preserve generator phase boundaries. At minimum, the
   construction transport must express animations constructed by the first Recalc
   between CABHUT and NeutralTech, later Recalc creations, and the final
   InitCellAttributes delete/recreate. Do not replay a flat complete Building trace
   and then infer history from final cells.
9. Replay every actual generated Building constructor with its native per-event
   order: Techno Scenario word, Building stable ID, then outcome. Discarded
   Neutral-Tech events consume both but bind no entity. CABHUT pre-search failures
   consume neither; active constructed CABHUTs are emitted rather than discarded.
10. Missing referenced AnimType/allocation must fail explicitly rather than silently
   omit an otherwise native-eligible object.

### 9.2 Focused acceptance tests

1. **Authored source/sweep/ID/RNG fixture.** One tile first becomes eligible in a
   per-Mark Recalc and another only in the first full sweep. Give both a custom
   valid RandomRate. Assert source-order then anti-diagonal creation, ID assignment
   before each draw, IDs below Terrain/Techno/Smudge objects, immediate deletion,
   final anti-diagonal recreation with fresh later IDs, and the exact Scenario RNG
   cursor. Assert the Main cursor is unchanged.
2. **All-stock data fixture.** Enumerate all 20 active stock TileAnim types and
   prove each actual construction consumes an ID but zero Scenario/Main draws.
3. **Waterfall lifecycle fixture.** For `WA01X`, assert first StartSound, scalar
   release event with `stop_sound_id=None`, then final StartSound; assert no pending
   delete and immediate absence of the transient. Repeat a non-`01` waterfall row
   and prove no StartSound events.
4. **Configured StopSound negative.** A custom tile AnimType with StartSound and
   StopSound must release its active handle at the post-object scalar deletion but
   never emit the configured StopSound.
5. **Latch/survivor fixture.** Prove repeated Mark/Recalc calls do not duplicate a
   latched object. Mutate/clear one tile or overlay between generations; prove it is
   not recreated, while survivors recreate in final anti-diagonal order.
6. **Owner/occupancy fixture.** Assert producer Anim owner is null, scalar deletion
   does not mutate an owner slot, and terrain animation never appears in raw entity/
   cell occupancy while it does appear then disappear in live Anim/Logic storage.
7. **Registry survivor fixture.** Mix non-terrain Anim entries before/between the
   marked entries. Assert compaction preserves their order and final recreated
   terrain animations append after surviving entries.
8. **Generated phase fixture.** Assert stable IDs and custom Scenario RandomRate
   cursor interleave as CABHUT trace effects -> first-generator-Recalc animations ->
   NeutralTech effects -> later-paint animations -> immediate final delete/recreate.
   Include a discarded Neutral-Tech constructor: it consumes its Techno Scenario
   word and Building stable ID but binds no entity. Assert a failed CABHUT cell
   search consumes neither, while every constructed stock CABHUT consumes both and
   is emitted. A stock Waterfall control must consume no RNG but retain ID and sound
   ordering.
9. **Immediate-path fixture.** Poison the generic pending-destroy/StopSound helper
   and prove InitCellAttributes never calls it; pending-delete is empty before and
   after the scalar deletion.
10. **Failure policy fixture.** Missing referenced type/allocation cannot silently
    continue with an incomplete generation; an explicit load failure is acceptable.

### 9.3 Design-review correction bundle

The transaction design may retain “delete/unlatch/recreate,” but these exact words
need the following constraints:

- replace broad **“sound/owner cleanup”** with **“release/detach current sound
  handles without StopSound; run conditional owner cleanup, which is a no-op for
  Recalc-created owner-null terrain animations”**;
- retain **“delay-zero Middle”**, but state that it runs before producer terrain
  markers/ZAdjust and that `AnimClass::Start` is conditional on raw SHP frame-count
  `+0x298 == 0`, not an unconditional Middle substep;
- extend generated-path wording beyond “no authored Mark replay”: native generated
  materialization still has several Recalc generations and a final
  InitCellAttributes lifecycle, with a first animation boundary between CABHUT and
  NeutralTech construction. Final surviving-set/order coverage alone cannot pass.

## Sources

### Native functions and data

- `CellClass::RecalcAttributes @ 0x0047D2B0`
- `AnimClass::Constructor @ 0x00421EA0`
- `AbstractClass::AssignUniqueID @ 0x00410230`
- `ScenarioClass::NextUniqueID @ 0x0068BCB0`
- `TechnoClass::Constructor @ 0x006F2B90`
- `BuildingClass::Constructor @ 0x0043B740`, unique-ID call site `0x0043BA15`
- `RandomMapGenerator::PlaceBridgeRepairHut @ 0x005904B0`
- `RandomMapGenerator::AddTechBuildings @ 0x005A95B0`
- region-scoped Neutral-Tech owner `0x00595400`
- `ObjectClass::Unlimbo @ 0x005F4EC0`
- `ObjectClass::Mark @ 0x005F5850`
- `AnimClass::Mark @ 0x004238B0`
- `AnimClass::Middle @ 0x00424CE0`
- `AnimClass::Start @ 0x00424F00`
- `VocClass::PlayAt @ 0x007509E0`
- `MapClass::InitCellAttributes @ 0x00568BB0`
- Anim vtable `0x007E3354`, scalar-deleting slot `+0x20 -> 0x00426590`
- `AnimClass::~AnimClass @ 0x004228E0`
- `AnimClass::Destroy @ 0x004255B0` (negative-path contrast)
- `ObjectClass::Limbo @ 0x005F4D30`
- `ObjectClass::UnInit @ 0x005F65F0` (negative-path contrast)
- `ObjectClass::~ObjectClass @ 0x005F3B80`
- `SoundEvent__Release @ 0x00406060`
- `AnimTypeClass__LoadImageAndResolveFrameBounds @ 0x00427B50`
- `ScenarioClass::Full_Init @ 0x00686B20`
- `RandomMapGenerator::Generate @ 0x00598960`
- `InitMapFromSyntheticINI @ 0x00599650`

### Prior active-YR reports used as integration evidence

- `AUTHORED_OVERLAYPACK_INLINE_TRANSACTION_REINVESTIGATION_GHIDRA_REPORT.md`
- `AUTHORED_MARK_LOAD_CONTEXT_SOURCE_PROVENANCE_REINVESTIGATION_GHIDRA_REPORT.md`
- `OVERLAYPACK_SHARED_DUMMY_FINAL_RECALC_FIELDS_REINVESTIGATION_GHIDRA_REPORT.md`
- [docs/research/ANIM_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIM_CLASS_GHIDRA_REPORT.md)
- [docs/research/ANIMCLASS_GLOBAL_REGISTRATION_SAMEPASS_SCHEDULER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_GLOBAL_REGISTRATION_SAMEPASS_SCHEDULER_GHIDRA_REPORT.md)
- [docs/research/ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md)
- `docs/research/skirmish-ui/RMG_MODE34_WATER_BRIDGES_TECH_GHIDRA_REPORT.md`

### Current Rust inspected

- `src/map/resolved_terrain.rs`
- `src/map/construction_trace.rs`
- `src/map/rmg/pipeline.rs`
- `src/map/rmg/phases/bridge_deck.rs`
- `src/map/rmg/phases/tech_buildings.rs`
- `src/sim/runtime.rs`
- `src/sim/anim_class.rs`
- `src/sim/world/lifecycle.rs`
- `src/sim/world/mod.rs`
- `src/app/loading/init_helpers.rs`

## Stale or misleading findings encountered

- A prior Anim layout table labels `AnimType+0x298` as an SHP pointer. The active
  loader proves that field is the raw SHP frame-count/2 value used by Middle's zero
  test. This report uses the corrected identity.
- “Terrain animation cleanup calls AnimClass::Destroy” is false for
  InitCellAttributes. The selected vtable call is the scalar-deleting destructor.
- “Destructor cleanup plays StopSound” is false for this direct path. StopSound is
  an `AnimClass::Destroy` behavior, and that path is not used.
- “Owner cleanup is an active tile-animation load side effect” overstates the
  producer. The destructor checks it, but Recalc-created instances remain owner-null.
- “TechnoClass assigns the generated Building unique ID” is imprecise. The Techno
  base consumes its raw Scenario word; the derived Building constructor calls
  `AssignUniqueID` afterward, still before placement. Discarded Neutral-Tech
  constructions consume both effects, in that order.
- “Generated maps bypass authored pack replay, therefore need only final animation
  spawn” is incomplete. The generated direct path owns multiple Recalc boundaries
  and the same final delete/unlatch/recreate lifecycle.
