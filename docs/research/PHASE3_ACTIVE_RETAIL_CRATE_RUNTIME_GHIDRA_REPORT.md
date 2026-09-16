# Phase 3 active-retail crate runtime re-investigation

**Status:** COMPLETE for the active-retail crate mechanism: slot lifecycle,
pickup transaction, all stock-reachable effects, trigger and object ingress,
movement callers, presentation order, and installed-retail reachability.

**Confidence:** HIGH. Every active-stock branch below is grounded in live
decompilation/disassembly plus the installed `rulesmd.ini` and all 184 named
retail maps. The only remaining label uncertainty is the semantic naming of
some Unit-effect side/BaseUnit subvectors; their executed predicates and order
are fully observed and are an implementation contract, not an open behavior.

**Verdict:** the existing Rust scenario-start crate scatter is materially wrong
and the runtime mechanism is otherwise absent. The row remains **OPEN** until a
builder replaces the divergent behavior, all focused validation passes, and
fresh implementation critics return zero findings.

**Player impact:** high and ordinary. Retail skirmish enables `Crates=yes` by
default, stock random selection reaches eight live effects, and installed
skirmish maps additionally use action 108 and `CrateBeneath` ingress.
`CarriesCrate` exists in stock types but is disabled by every installed map's
scenario flags. Wrong slot, RNG, Mark, pickup, or effect order shifts every
later Scenario RNG consumer.

**Scope:** active `gamemd.exe`, stock `rulesmd.ini`, 184 named installed retail
maps, current Rust, and evidence-backed exclusions. Unsafe allocator failure,
native memory corruption, and unreachable malformed zero-weight data are not
deliberately emulated.

## Executive corrections

The re-investigation overturns these stale hypotheses:

- Initial count uses connected human/session nodes only, not human plus AI.
  Current Rust's human-only count is correct, but its unsigned rule parsing is
  not.
- A crate placement accepted by the pre-validator owns a slot and timer even
  when Overlay allocation, construction, Unlimbo, or Mark fails. Such a
  placement is a **ghost**, and it stops the random retry loop.
- Stock flat, unoccupied Water visibly Marks WCRATE: Mark recognizes the
  WaterCrateImg identity, selects Float passability, and retail Water Float is
  100%. Ghosts begin only at the exact post-precheck failures below.
- Placement does not directly stamp `OverlayGrid`; it goes through native
  Overlay construction and Mark.
- Radius effects iterate the live Ground display layer in exact 3D and do not
  filter to the picker owner.
- HealBase iterates owner-matching Logic objects; it is not an explicit
  building-only sweep.
- `[Powerups]` field three is the over-water eligibility byte, not a generic
  sound flag.
- `[CrateRules] FreeMCV` is parsed and retained, but the live pickup override
  reads the multiplayer session `Bases` option instead. Treating `FreeMCV` as
  the live gate is stale.
- `CrateBeneath` and trigger action 108 do not honor the global Crates option.
  `CarriesCrate` alone checks it, in addition to `TruckCrate`/`TrainCrate`.
- CrateTrigger has two distinct events: synchronous collector AttachedTag event
  49 and a separate next-Logic-rung global event-50 latch.
- Pickup's boolean return controls its locomotor caller; it is not a consumed
  flag. Unit-crate success and an event-49 collector death return zero, while
  most consumed outcomes return one.

## Retail data authority

Stock `[CrateRules]` is:

```ini
CrateMaximum=255
CrateMinimum=1
CrateRadius=3.0
CrateRegen=3
SilverCrate=HealBase
SoloCrateMoney=5000
UnitCrateType=none
WoodCrate=Money
WaterCrate=Money
HealCrateSound=HealCrate
WoodCrateImg=CRATE
CrateImg=CRATE
WaterCrateImg=WCRATE
FreeMCV=yes
```

Canonical powerup indices are fixed independently of INI declaration order:

| Index | Name | Retail weight | Retail anim | Over water | Retail data |
|---:|---|---:|---|---|---:|
| 0 | Money | 20 | MONEY | yes | 2000 |
| 1 | Unit | 20 | none | no | 0 |
| 2 | HealBase | 10 | HEALALL | yes | 0 |
| 3 | Cloak | 0 | CLOAK | yes | 0 |
| 4 | Explosion | 0 | none | yes | 500 |
| 5 | Napalm | 0 | none | no | 600 |
| 6 | Squad | 0 | none | no | 0 |
| 7 | Darkness | 0 | SHROUDX | yes | 0 |
| 8 | Reveal | 10 | REVEAL | yes | 0 |
| 9 | Armor | 10 | ARMOR | yes | 1.5 |
| 10 | Speed | 10 | SPEED | yes | 1.2 |
| 11 | Firepower | 10 | FIREPOWR | yes | 2.0 |
| 12 | ICBM | 0 | CHEMISLE | yes | 0 |
| 13 | Invulnerability | 0 | ARMOR | yes | 1.0 |
| 14 | Veteran | 20 | VETERAN | yes | 1 |
| 15 | IonStorm | 0 | none | yes | 0 |
| 16 | Gas | 0 | none | yes | 100 |
| 17 | Tiberium | 0 | none | no | 0 |
| 18 | Pod | 0 | none | no | 0 |

The active weighted total is 110. The reachable random stock set is exactly
Money, Unit, HealBase, Reveal, Armor, Speed, Firepower, and Veteran.
Their nonzero data qwords are Money `0x409F400000000000`, Armor
`0x3FF8000000000000`, Speed `0x3FF3333333333333`, Firepower
`0x4000000000000000`, and Veteran `0x3FF0000000000000`; the other three are
zero. These are binary64 rule values, not Rust literals to be recomputed in a
different numeric path.

`RulesClass__ReadPowerups @ 0x00673E80` owns the canonical table regardless of
INI declaration order. It parses weight through signed `atoi`, animation by
name, field three as a case-insensitive yes/no water byte, and data through
direct CRT binary64 `atof`. The native arrays are weights `0x0081DA8C`,
animation indices `0x0081DAD8`, data doubles `0x0089EC28`, and water bytes
`0x0089ECC0`.

These arrays begin as executable-image globals, not `RulesClass` constructor
fields: weights are `[50,20,1,3,5,5,20,1,1,10,10,10,1,3,1,1,1,1,1]`, all
animation indices are `-1`, and all data/water bits are zero. If the entire
section is absent, `ReadPowerups` returns without a store and preserves them.
If the section exists, however, it visits every canonical row using
`CCINIClass__ReadString @0x00528A10` with a 128-byte buffer and exact default
`"0,NONE,0"`. A missing canonical key therefore stores weight zero, resolves
bare `NONE` in the current animation registry, retains water because `0` is
neither yes nor no, and retains data because no fourth token exists. Stock has
no animation named `NONE`, yielding `-1`; a mod that allocated that literal
name before the pass stores its real index. This mixed fallback, not whole-slot
preservation, is repeated independently by every Process pass.

INI loading drops trimmed empty values and discards a section with no accepted
entries. Thus an otherwise empty `[Powerups]` section preserves all arrays,
whereas an empty canonical row in a section kept live by another nonempty row
is missing and takes the fallback. CRT `strtok` collapses leading, consecutive,
and trailing commas, shifting later fields left. Token one is direct decimal
`atoi` with signed wrapping and no hex mode. Token two is live case-insensitive
lookup; exact `<none>`, empty/unknown names store `-1`, and later allocation
does not repair an earlier miss. Token three writes only exact case-insensitive
`yes`/`no`; absent or malformed retains. Token four is direct CRT binary64
`atof`, not the Rules `ReadDouble` f32 intermediary. A `%` anywhere in it
multiplies through x87 by binary64 `0.01` bits `0x3F847AE147AE147B`; extra
tokens are ignored.

Native pass order is static globals, optional MISSIONMD Process at
`0x00686D35`, registry/rules reset at `0x006876AC` which does not clear these
globals, RULESMD at `0x00668A27`, optional LANGRULE at `0x00668B05`, selected
mode at `0x00668BAA`, scenario/map at `0x0068774F`, then conditional TMCJ4F at
`0x00687B76`. Installed MISSIONMD and TMCJ4F have no `[Powerups]`; LANGRULE is
absent; stock mode payloads and active YR map sources contain none. Legacy
retail `SOV07S` and `SOV08U` map passes do override all 19 rows, producing
weight arrays `[100,0,0,0,0,0,0,0,0,20,20,20,0,0,10,0,0,0,0]` and
`[100,0,0,0,0,0,0,0,5,20,20,20,0,0,30,0,0,0,0]` respectively.

`RulesClass__ReadCrateRules @ 0x0066B900` parses signed minimum, maximum, and
solo money; double regen; cell-range-scaled radius; the three image identities;
the three solo fixed-type mappings; heal sound; `UnitCrateType`; and
`FreeMCV`. Missing values preserve prior state. An unknown supplied fixed-type
name resolves to Money. `FreeMCV` is real parsed state, but the live
multiplayer pickup override does not read it: `0x00481B99..0x00481BFF` reads
the session `Bases` option byte at `0x00A8B258`.

The exact constructor state precedes every rules layer and differs materially
from installed retail:

| Field | Offset | `RulesClass__Constructor @0x00665650` |
|---|---:|---:|
| FreeMCV | `+0x040` | false (`0x006656B3`) |
| Wood/common/Water image pointers | `+0x0F8/+0x0FC/+0x100` | null/null/null (`0x006657DD..0x006657EA`) |
| HealCrateSound | `+0x718` | `-1` (`0x0066604B`) |
| SoloCrateMoney / UnitCrateType | `+0x1140/+0x1148` | `2000` / null (`0x00666DF2..0x00666E02`) |
| Silver/Wood/Water fixed mappings | `+0x1464/+0x1468/+0x146C` | `2/0/0` (`HealBase/Money/Money`) |
| CrateMinimum / CrateMaximum | `+0x1470/+0x1474` | signed `1/255` |
| CrateRegen | `+0x1678` | binary64 10.0, `0x4024000000000000` |
| CrateRadius | `+0x172C` | signed `640` leptons (`2.5` cells) |

Section absence returns zero without invoking a per-key parser or performing a
store. Section presence reads FreeMCV; Wood/common/Water images; Heal sound;
minimum; maximum; radius; regen; UnitCrateType; solo money; Silver; Wood; Water
in that exact order. Every call uses the live current field as default, so
missing keys preserve prior bits/identity across later Rules, language, mode,
and scenario passes. `INIClass::Put_String @0x00528660` omits/removes an entry
whose trimmed value is empty, so `Key=` is absence and preserves current state
for every reader here. Native layer order is constructor, base RULESMD,
optional LANGRULE, selected mode payload, then the map at `FullInit
@0x0068774F`. MISSIONMD precedes the rules reset and cannot win; the optional
later TMCJ4F pass is absent in ordinary stock. The installed LANGRULE, mode
payloads, and 184 maps have no nonempty CrateRules override, leaving the base
RULESMD result final.

`CrateRadius` calls `CCINIClass__ReadRange @0x00474620`. That helper calls
ReadDouble with sentinel `-1.0`, compares ST0 with the exact `-1.0` constant at
`0x007E4900`, and returns the supplied signed-i32 default when C3 is set—exact
`-1.0` or unordered/NaN. Otherwise it multiplies by exact binary64 256.0 at
`0x007E1710`, calls `Math__ftol @0x007C5F00`, and consumes EAX. No clamp exists.
ReadDouble first parses `%f` into binary32 and widens; a token containing `%`
then multiplies by binary64 0.01. Consequently `2.5 -> 640`, retail `3.0 ->
768`, `-0.5 -> -128`, `-1 -> prior`, `-1% -> -2`, and `-100% -> prior`.
Nonfinite/out-of-i64 conversion yields x87 integer-indefinite with low dword
zero; finite signed-i64 results outside i32 wrap through EAX. This corrects the
stale unscaled “5.9 -> 5” helper description elsewhere in the repository:
ReadRange stores lepton-scaled values.

CrateRegen calls ReadDouble directly and stores the returned qword. Missing
retains constructor/prior 10.0; retail `3` stores exact binary64 3.0
`0x4008000000000000`. Present malformed nonempty `%f` input exposes the native
address-layout-dependent scanf stack alias and is invalid-domain, not a stable
zero fallback.

Minimum, maximum, and solo money use signed ReadInt with no clamp. Decimal
mode is wrapping signed CRT `atoi`, accepts leading whitespace/sign and a
decimal prefix, ignores trailing junk, and returns zero when no digits exist.
A `$` marker or `h`/`H` form selects `%x`; failed hex conversion retains the
current default. FreeMCV uses the case-insensitive first-character table
`0/F/N=false`, `1/T/Y=true`, otherwise current. Absent (including authored
empty) image strings retain current; `none`/`<none>` stores null, known names
resolve, and unknown names allocate an OverlayType. Heal sound absent/unknown
retains the prior index. UnitCrateType absent retains, `none` stores null, and
unknown names use the normal UnitType find-or-allocation path. Fixed mappings
absent retain; their complete fixed table is `Money, Unit, HealBase, Cloak,
Explosion, Napalm, Squad, Darkness, Reveal, Armor, Speed, Firepower, ICBM,
Invulnerability, Veteran, IonStorm, Gas, Tiberium, Pod` = `0..18`; an unknown
nonempty name stores Money index zero. Installed retail's section
finally yields signed `1/255`, radius 768, regen qword 3.0, solo 5000,
`HealBase/Money/Money`, `CRATE/CRATE/WCRATE`, HealCrate, null UnitCrateType,
and FreeMCV true.

The seven crate spatial-sound slots at RulesClass `+0x1E4..+0x1FC` construct
to `-1`. Installed `[AudioVisual]` resolves effect-order identities
`CrateMoney`, `CrateReveal`, `CrateFirePower`, `CrateArmor`, `CrateSpeed`,
`CrateFreeUnit`, and `CratePromoted`; identity, not build-local Voc index, is
the portable contract. `C4Warhead` at `+0xFA8` constructs null and installed
retail resolves the existing `Super` WarheadType. It is ordinary active input:
every positive-weight HealBase pickup passes this exact identity to the real
damage receiver. Only the separate weight-zero random Explosion handler is
excluded from ordinary stock selection.

The installed-map census found:

- zero CRATE/WCRATE OverlayPack identities in all 184 named maps;
- 13 ordinary-skirmish action-108 calls: two in `xarena.map` and eleven in
  `xxmas.map`, all selecting positive-weight stock effects;
- zero ordinary-skirmish event-49 or event-50 conditions;
- zero ordinary-skirmish `TeamType Tag=` rows; all 144 retail occurrences are
  confined to eleven campaign maps;
- 72 `CrateBeneath` structure instances across 23 installed maps, of which 58
  across sixteen maps are in the strict ordinary-skirmish subset;
- four ordinary-map `TRUCKB CarriesCrate=yes` instances, but both scenario
  flags default false and every explicit installed `TruckCrate`/`TrainCrate`
  value is `no`, so none can produce a crate in retail data.

These facts exclude weight-zero handlers, CrateTrigger actions, and
`CarriesCrate` drops from the ordinary stock-skirmish closure, but not their
native parsing or synthetic regression coverage. Action 108 and
`CrateBeneath` remain active and required.

## Persistent slot lifecycle

`MapClass` owns `0x1000` inline bytes at `Map+0x158`: 256 ordered 16-byte
slots.

| Offset | Type | Meaning |
|---:|---|---|
| `+0x00` | `i32` | placement start frame |
| `+0x04` | `u32` | persisted timer auxiliary word |
| `+0x08` | `i32` | duration frames |
| `+0x0C` | packed `i16 x, i16 y` | cell; `(0,0)` means empty |

`MapClass__constructor @ 0x00565090` zeroes coordinates.
`MapClass::Init_Clear @ 0x005659F0` establishes the authoritative fresh/reset
tuple `{start=-1, aux=0, duration=0, x=0, y=0}` for every slot. Coordinate is
the sole occupied/empty discriminator.

`MouseClass__Save @ 0x005BE6D0` raw-writes the singleton body containing every
slot word. `MouseClass__Load @ 0x005BDF70` restores them verbatim, including
ghosts and paused timers. No post-load timer normalization was found.

The quick retail sync checksum at `0x0064DAB0` does not directly fold the Map
slot table. Rust snapshots must serialize it, and Rust's broader future-state
hash must include it, but `compute_retail_multiplayer_checksum` must not add it.

## Bootstrap and signed count

`ScenarioClass__Post_Map_Init @ 0x00686890` runs bootstrap whenever the global
Crates option is enabled; it has no game-mode gate. The signed count is:

```text
min(CrateMaximum, max(CrateMinimum, pregame_human_node_count))
```

Negative and inverted mod values are not pre-clamped. Each requested iteration
calls random placement exactly once; a failed call is not topped up.

`FUN_005E7460 @ 0x005E7460` copies the connected player-node count at
`0x005E7471..0x005E748B`. AI seats live in a separate global and are consumed
separately by House/start construction. A one-human/seven-AI skirmish therefore
requests one crate before `CrateMinimum`, not eight.

## Random and specific-cell placement

### Random placement

`MapClass__PlaceCrateAtRandomCell @ 0x0056BD40`:

1. Finds the first empty slot in ascending order. A full table returns false
   without RNG.
2. Retains that slot through at most 1,000 attempts.
3. Each attempt draws Scenario RNG X then Y within the active Map rectangle:
   `left + RandomRanged(0,width-1)`, then the analogous Y expression.
4. Water origins call FNPC with Float speed type 5; all others use Track 1.
5. The accepted destination runs the hard validator. A hard rejection retries
   and spends no timer RNG.
6. A precheck acceptance stops retries even if later Overlay/Mark work fails.
7. Acceptance consumes `RandomRanged(0,0x7ffffffe)` for the timer.

The verified FNPC tuple is Normal movement zone, no required zone, no bridge-
aware zone check, 1x1 footprint, no overlay/height/current-occupant/final
occupancy check, bridges allowed, zero target sentinel, and radius
`min(SizeW+SizeH,32)`. Preferred-candidate selection uses the current frame
modulo and consumes no Scenario RNG.

### Specific-cell placement

The helper at `0x0056BEC0` snaps the supplied origin once before scanning for
the first empty slot. Invalid snap or a full table returns false. It never
overwrites a live slot and does not deduplicate coordinates.

The full dword data parameter matters:

- exactly `0x14` performs no post-write: a visible crate keeps `0xff`, and a
  ghost preserves prior cell data;
- every other value writes its low byte after accepted placement, even for a
  ghost; `0x114` therefore writes `0x14` and is not the sentinel.

Pickup treats unsigned data below 19 as a fixed type. Data 19 and above uses
weighted random selection.

### Validator, Mark, and ghost acceptance

`CrateSlot__PlaceOverlayAndInitTimer @ 0x004A17C0` calls
`CrateSlot__ValidateCellAndCreateOverlay @ 0x004A18F0`.

Hard rejection is limited to:

- `MapClass__Is_Cell_In_Playfield(cell,1)` false; or
- a pre-existing overlay identity (`Cell+0x44 != -1`).

After those checks, the validator re-fetches the snapped destination Cell.
At `0x004A1944..0x004A198F`, exact `Cell+0xEC == 2` selects
`Rules+0x100 WaterCrateImg`; every other LandType value selects
`Rules+0xF8 WoodCrateImg`. `CrateImg` is never selected. This is independent
of the random origin Cell that selected Float or Track before FNPC, so a snap
across a land/water boundary uses origin movement classification but
destination image classification.

The validator allocates an OverlayClass and invokes its constructor. An active
ground `TerrainClass` found by `FUN_0047C550(cell,0)` makes the constructor skip
`ObjectClass__Unlimbo` and creates a ghost; ordinary Cell occupation does not
trigger that constructor gate. Reached `OverlayClass::Mark @0x005FC570`
rejects a stock crate ID only when slope byte `Cell+0x11C > 4`; slopes 0..4
remain eligible.

Mark compares overlay identity against current Rules pointers. WaterCrateImg
is checked first and selects SpeedType Float/5. Otherwise CrateImg or
WoodCrateImg selects Track/1, so Water wins if configured pointers alias. The
call to `CellClass__CheckCellPassability @0x004834A0` uses required zone `-1`,
required level `-1`, movement zone 0, `ignore_infantry=0`,
`ignore_vehicles=0`, and bridges allowed. With `Cell.Flags & 0x100`, it
selects the unmasked low byte of `AltOccupationFlags Cell+0x128`; otherwise it
selects unmasked `OccupationFlags Cell+0x124`. Exactly zero passes and any
nonzero bit rejects. A non-bridge selection also requires the selected
Float/Track terrain-row value to be nonzero. Selecting the bridge/deck field
bypasses the later zero-underlying-terrain-speed rejection. Allocation,
construction, Unlimbo, and Mark failure are not propagated. At
`0x004A1994..0x004A1A78`, accepted visible and ghost paths union the two object
rectangles and call `TacticalClass__DirtyScreenRect(..., force=0)`; ghost
paths supply the zero rectangles. Native restores the saved editor flag before
returning accepted. The outer helper then calls the snapped Cell redraw helper
`FUN_006DA7D0` at `0x004A184B`, before writing the slot coordinate and before
the timer RNG/word stores. The tail performs no radar-dirty operation.

`FUN_006DA7D0 @0x006DA7D0` enqueues only when the suppression global is zero,
Cell last-redraw frame `+0x5C` differs from the current frame, either explored
bit `Cell+0x12C & 8` or forced byte `+0x138` is set, the projected Cell
rectangle intersects the widened tactical viewport, and queue count is below
799. Success stamps `+0x5C`, clears `+0x138`, and sets the tactical redraw
flag. A hard rejection performs neither the screen-dirty nor cell-redraw tail.
Specific placement performs any non-`0x14` low-byte data post-write only after
the complete placement/timer tail and performs no second invalidation.

Therefore an allocation, terrain, slope, passability, occupation, Unlimbo, or
Mark failure is an accepted timed ghost. Only the two hard prechecks leave the
slot empty. A visible Mark writes data `0xff`; a ghost preserves the cell byte.
Installed WCRATE on flat empty Water is visible because `[Water] Float=100%`;
Water with Float zero ghosts, as does Land with Track zero, while nonzero rows
permit the corresponding zero-occupation Mark.

Native additionally registers a failed Overlay object/UniqueID in some Mark-
failure cases, but not in Logic, and the slot system never reads that identity.
This object-graph artifact is excluded from crate gameplay state; ghost slot,
timer, RNG, and cell-byte behavior are required.

## Exact timer and regeneration

On accepted placement:

```text
lower = CrateRegen * 450.0
upper = CrateRegen * 1800.0
draw = Scenario.RandomRanged(0, 0x7ffffffe)
value = lower + draw / 2147483646.0 * (upper - lower)
duration = x87 truncate-toward-zero(value)
start = current pre-increment frame
aux = high dword of upper's stored double
```

Retail `CrateRegen=3` yields 1350 at draw zero, 5400 at the inclusive maximum,
and aux `0x40B51800`. The direction of interpolation is load-bearing even
though reversing it preserves the distribution.

`MapClass__UpdateCrateRegenTimers @ 0x0056BBE0` runs only in nonzero game mode
with Crates enabled and scans all 256 slots ascending. Empty slots skip.
`start==-1` expires only when duration is zero. Otherwise signed/wrapping
`current-start >= duration` expires. Expiration clears the slot and calls
random placement once. First-free reinsertion can land at or before the
current index (not revisited) or above it (visited later in the same scan), so
modded zero/negative timers can cascade.

The sole caller is `LogicClass__PerTickUpdate @ 0x0055AFB0`. Order is live
objects, `FUN_0053D310`, AlphaShape purge, crate regeneration, Tactical, then
Factory and House arrays. The Rust insertion point is immediately before the
existing first factory sweep, after live-object/combat/effect work, using the
pre-increment absolute `binary_frame`.

## Clear and removal

`CrateSlot__ClearAndPreserveTimer @ 0x004A1750` returns false without mutation
for an empty coordinate. Otherwise it attempts overlay removal, clears the
coordinate regardless, preserves remaining duration using signed/wrapping
elapsed arithmetic, then sets start to `-1`. A pre-existing `start==-1`
preserves duration.

`CrateSlot__RemoveCrateOverlayFromCell @0x004A1AA0` bounds-checks, then accepts
only an identity exactly equal to current Rules `CrateImg`, `WoodCrateImg`, or
`WaterCrateImg` at `0x004A1ACD..0x004A1AFE`. On a match it obtains and unions
the two Cell rectangles at `0x004A1B04..0x004A1BB8`, calls
`TacticalClass__DirtyScreenRect(..., force=0)` at
`0x004A1BBE..0x004A1BDC`, and only then writes `Cell+0x44=-1` at
`0x004A1BE1` and `Cell+0x11E=0` at `0x004A1BE8`. It changes no other Cell
field and performs no CellRedraw helper or radar dirty. A ghost/missing or
mismatched overlay emits no dirty request and changes no Cell, but the caller
still frees the slot and preserves/rebases its timer.

`MapClass__RemoveCrateAtCell @ 0x0056C020`:

- mode zero uses no slots and accepts any live OverlayType with native
  `Crate +0x2AA` true. Its inline `0x0056C0C8..0x0056C1D3` tail performs the
  same rectangle union, dirty-before-two-writes order, and no CellRedraw/radar;
- nonzero mode finds only the first ascending occupied slot with the packed
  coordinate at `0x0056C030..0x0056C087`, visible or ghost, and invokes slot
  clear. Slot clear calls the removal helper before coordinate/timer mutation.

Pickup ignores removal failure. In nonzero mode with Crates enabled, it calls
one immediate random replacement before the selected effect.

## Pickup transaction

`CrateClass__PickupDispatch @ 0x00481A00` receives `ECX=CellClass*` and one
collector argument. Exact prefix:

1. null collector, no overlay, non-crate overlay, or nonzero-mode passive
   owner returns one;
2. if `CrateTrigger=yes` and collector AttachedTag is non-null, synchronously
   call `TagClass__ProcessTriggerEvent @ 0x006E53A0` with event 49;
3. ignore callback return and re-read collector native-alive;
4. a dead collector returns zero immediately, leaving crate/latch/RNG intact;
5. otherwise set `Scenario+0x34BE=1`, even with no AttachedTag;
6. select, apply MP guards/remaps, remove, immediately replace, then dispatch
   the effect.

The latch is independent. At the next leading Logic rung,
`LogicClass__PerTickUpdate` walks global Tags in order with event 50, then
clears it. Ordinary stock maps contain no event-49/50 conditions, so the
latch remains future-affecting state only for custom/campaign data. The
ordinary stock implementation may evidence-exclude trigger actions, but must
not invent a different RNG/removal boundary.

## Selection and guard order

Data below 19 selects directly with no selection RNG. Otherwise native sums
the nineteen signed weights, draws inclusive `RandomRanged(1,total)`, and
chooses the first cumulative sum `>= roll`.

Mode zero bypasses MP guards. Only signed data zero enters the image override:
`CrateImg -> SilverCrate`, then `WoodCrateImg -> WoodCrate`, then
`WaterCrateImg -> WaterCrate`. Since retail CrateImg and WoodCrateImg both name
CRATE, Wood/Money overwrites Silver/HealBase. Mode-zero Money is fixed
`SoloCrateMoney` and spends no amount draw.

Nonzero-mode order is:

1. resolve side-compatible BaseUnit;
2. FreeMCV overrides to Unit when OwnedBuildings is zero, available funds are
   strictly above 1500, no side BaseUnit is owned, and the session `Bases`
   option is enabled; parsed `[CrateRules] FreeMCV` is not read here;
3. anti-stack remaps: Unit above 50 units, Squad above 100 infantry, existing
   Cloak/Armor/Speed/Firepower modifier, Aircraft Speed, non-firepower Techno,
   non-trainable/already-elite Veteran, and Unit/Squad on Water or Beach;
4. Water land plus the selected powerup's over-water byte false remaps Money;
5. exact-mode-four WOL bookkeeping;
6. remove and immediate replacement;
7. Squad remaps to Money;
8. effect dispatch.

### Exact-mode-four WOL bookkeeping — `0x00481D6B..0x00481D81`

`g_GameMode @ 0x00A8B238 == 4` is Westwood Online. Ordinary offline Skirmish
is mode 5 and LAN is mode 3. At the bookkeeping point native re-reads owner at
`collector+0x21C`, selects the embedded counter at `House+0x4B70`, and
increments the current selected/remapped powerup index. This occurs after all
free-MCV, anti-stack, terrain, and water remaps, but before remove
`0x00481D97`, immediate replacement `0x00481DB3`, late Squad-to-Money remap
`0x00481DB8..0x00481DC3`, and effect dispatch. Pre-count Money remaps count
index 0; Squad counts index 6 and executes Money. Event-49 death returns before
counting, while Event-49 owner transfer credits the owner re-read here. Later
effect failure/death never rolls the count back and the count consumes no RNG.

The embedded CounterClass has 512 signed i32 elements at `+0x000..+0x7FC`, a
signed logical length at `+0x800` (`House+0x5370`), and a network-order flag at
`+0x804`. Constructor `0x00748FD0` zeroes all 512, length 512, flag zero;
House constructor reset `0x00749060` zeroes them and fixes length 19. Increment
`0x00749020` uses signed `index < length` followed by raw x86 `INC`, so active
0..18 values wrap `INT_MAX` to `INT_MIN`; index 19+ is ignored. A negative
direct argument could underwrite but no pickup path produces one.

House raw save/load persists the entire counter, length, and flag. House CRC
`0x00502D60..0x0050303B` omits all three. Construction is its only match-time
reset; no copy path or gameplay/local-UI reader exists. The sole reader is WOL
postgame builder `0x006C6F50`, which converts all nineteen elements to network
byte order, trims after the last nonzero, and emits exactly `4*used_count`
bytes under `CRA<player digit>`; all-zero state emits length zero. Literal xref
census finds only dispatch, constructor/destructor setup, and that serializer.

This is an evidence-backed Phase 3 exclusion: the implementation milestone is
ordinary stock Skirmish versus AI, exact mode 5. WOL/session/postgame protocol
belongs to Phase 13. Phase 3 must distinguish the modes and must not gate a
partial counter on a generic nonzero/multiplayer boolean.

## Eight active-retail effects

All presentation follows mutation. The common tail at `0x004832F5` resolves
the animation from the final selected/remapped type and, when it is not `-1`,
allocates an Anim at crate-center ground Z plus 200 with constructor arguments
`(0,1,0x600,0,0)`. Allocation failure is silent. Unit placement success is the
only active-stock outcome that skips this common tail.

### Shared radius contract

Armor, Speed, Firepower, Veteran, and the stock-disabled Cloak handler iterate
the live Ground display-layer buffer in its current order, re-reading capacity
after each candidate. They do not snapshot, do not iterate the entity store,
and do not owner-filter: enemy candidates in range are modified. Armor, Speed,
and Firepower add no alive/limbo filter. Distance is exact 3D from crate
cell-center coordinates at computed ground Z to candidate virtual coordinates:

```text
Math__ftol(Sqrt_Approx(dx^2 + dy^2 + dz^2)) < Rules.CrateRadius
```

The boundary is strict, so stock distance 768 is rejected. Armor accepts
Techno with exact multiplier 1.0; Speed accepts Foot with exact multiplier 1.0
and excludes Aircraft; Firepower accepts Techno with exact multiplier 1.0 and
does not repeat the picker's firepower-capability test; Veteran requires the
marked/active byte, Techno lineage, a trainable type, and positive data.

### Money — `0x00482463`

Multiplayer converts data with `Math__ftol`, consumes one inclusive Scenario
RNG draw `[base, base+900]`, then calls `HouseClass__Add_Credits @ 0x004F9950`.
Stock adds 2000..2900. Addition is wrapping signed i32, with no cap or
saturation. The mode-zero fixed-money path uses `SoloCrateMoney` and consumes
no amount draw. In mode zero only, a local-human picker credits `g_PlayerPtr`;
otherwise the picker House receives the money. Credits mutate first;
picker-owner-local-human gates spatial `CrateMoneySound`; MONEY animation
follows.

### Unit — `0x00482041`

`UnitCrateType`, when non-null, overrides selection. Otherwise native repeats
`RandomRanged(0, UnitTypeCount-1)` until a type has `CrateGoodie=yes` and passes
BaseUnit eligibility; every rejected candidate still consumes its draw and
there is no retry cap. A BaseUnit is rejected when session Bases is off. With
Bases on, it is admitted only for a human-controlled house or the free-MCV
override. The chosen Unit constructor receives the picker owner.

Native first attempts Unlimbo at crate-center ground coordinates. On failure
it runs Foot FNPC with the chosen unit movement type and makes exactly one
nearby Unlimbo attempt. Success plays `CrateUnitSound` only for a human owner,
returns zero, and skips the common animation. Type/allocation failure consumes
the crate and reaches the common Unit tail (stock animation none). If both
placements fail, native destroys the candidate, remaps to Money, executes its
amount RNG/credit/sound, then creates the MONEY animation. This late fallback
retains Unit's already-loaded data value; stock Unit data is zero, so it draws
0..900. A pre-handler Unit-to-Money guard remap instead loads Money data and
draws the normal 2000..2900.

### HealBase — `0x00482B8F`

Picker-owner-human plays `HealCrateSound` before healing. Native then walks the
live Logic vector in exact order, re-reading its count after each candidate.
Each non-null candidate whose owner equals the picker owner receives virtual damage
`candidate.Health - candidate.Type.Strength`, distance zero, Rules
`C4Warhead` at `+0xFA8` (installed `Super`), and flags `(0,1,1)`. Negative
damage heals and the receiver clamps to
Strength; zero still calls the receiver. The IsTechno test is on the picker,
not each candidate, so this is not an explicit building-only sweep. HEALALL
animation follows the sweep.

### Reveal — `0x00481F9D`

`MapClass__BlackoutShroud @ 0x00577D90` runs first with the picker owner. It
sets a non-null House's `MapIsClear` byte `+0x241` before every later gate. A
remote non-null House then returns immediately from map work: no mode
predicate, Visionary mutation, Paranoid pass, cell write, radar refresh, or
tactical redraw. Local (or invalid null) execution computes the network-spare
predicate before testing Visionary:

```text
(g_GameMode == 3 || g_GameMode == 4)
&& selected_mp_mode != null
&& selected_mp_mode->vtable[1]() == false
```

Battle, ManBattle, Siege, Unholy, and FreeForAll return false and spare cells;
Cooperative returns true and spares none. A null selected-mode pointer also
spares none. This is raw `g_GameMode`, not an MPModes roster ID; Rust roster
IDs 3 Cooperative and 4 Unholy are unrelated.

With raw `[Map] Size` width `N` and height `M`, the three exclusions are
`(7,N+5)`, `(13,N+11)`, and `(M+13,N+M-15)`. There is no lookup, clamp,
sentinel, deduplication, or bounds check. Valid positive dimensions cannot
duplicate them; out-of-domain candidates simply are not returned by the map
iterator.

The iterator visits the allocated diamond once in native anti-diagonal order,
exactly `M*(2*N-1)` cells satisfying `N < x+y`, `x-y < N`, `y-x < N`, and
`x+y <= N+2*M`. Each non-spared cell receives, in order,
`Cell+0x130=0`, `Cell+0x134=0`, `Cell+0x12C |= 0x18`, and
`Cell+0x140 |= 0x03`; unrelated bits survive.

Local order is predicate, Visionary early return, `ParanoidRevealAll(0,0)`,
set `House+0x240 Visionary=1`, cell loop, `ParanoidUnrevealAll(0,0)`,
`RadarClass::RefreshRadar @0x00657CE0`, then
`GScreenClass::Flag_To_Redraw(2) @0x004F42F0`, including tactical redraw and
map draw-cache-generation increment. The Paranoid passes each snapshot Techno
count and walk forward; they can project ordinary sight onto directly spared
cells.

`CrateRevealSound` then plays through `VocClass::PlayAt @0x007509E0` without a
picker-human gate, including for remote or already-Visionary pickers. REVEAL
animation at crate-cell ground Z+200 is last; missing type/allocation suppresses
only the animation. Offline Skirmish raw mode 5 never spares cells.

### Armor — `0x00482D56`

Each shared-radius Armor candidate multiplies its native binary64 field by the
parsed 1.5. If any affected candidate owner satisfies
`HouseClass__IsHumanPlayer`, native emits `EVA_UnitArmorUpgraded` after the
loop. Independently, picker-owner-human gates spatial `CrateArmourSound`.
ARMOR animation follows. Order is mutation loop, EVA, spatial sound,
animation.

### Speed — `0x00482F36`

Each eligible non-Aircraft Foot multiplies native `Foot+0x580` by parsed 1.2.
If any affected owner has the native PlayerControl byte, native emits
`EVA_UnitSpeedUpgraded` after the loop. Picker-owner-human then gates
`CrateSpeedSound`; SPEED animation follows. The multiplier is persistent
binary64 state consumed directly by `FootClass__GetCurrentSpeed`; Rust's
locomotor `SimFixed` speed is not an equivalent owner.

The exact consumer is independently closed by
`FOOTCLASS_GET_CURRENT_SPEED_EXACT_GHIDRA_REPORT.md`. Under normal YR
53-bit/chop x87 control, with `ftol_low32` meaning signed-i64 FISTP followed by
low-dword consumption, `0x004DB1A0` executes:

```text
stage1_product = x87_mul53(
    i32_to_x87(native_type_speed_i32),
    exact_f32_to_f64(HouseClass.GetSpeedBonus(TechnoType)_bits)
)
stage1_product = x87_mul53(stage1_product, load_f64(Foot+0x580_bits))
stage1 = ftol_low32(stage1_product)
stage2 = HasWeaponAbility(FASTER)
    ? ftol_low32(x87_mul53(i32_to_x87(stage1), load_f64(Rules.VeteranSpeed_bits)))
    : stage1
stage3 = ftol_low32(
    x87_mul53(i32_to_x87(stage2), load_f64(Foot+0x578 current_fraction bits))
)
```

No conversion separates House and crate multipliers. The optional VeteranSpeed
conversion and final current-fraction conversion are distinct mandatory stage
boundaries. Stock `VeteranSpeed=1.2` is parsed through binary32 then promoted,
bits `0x3FF3333340000000`; stock crate 1.2 is binary64 bits
`0x3FF3333333333333`. Elite inherits a Veteran-list `FASTER` byte. The native
type-speed dword maps stock AMCV to 10 and MTNK to 17. Full-fraction derived
results are AMCV 10; MTNK rookie/veteran/elite 17/20/20; Speed-crated MTNK
rookie/veteran 20/24. A deliberately modded stage-1 real value 18.9 truncates
to 18, then stock VeteranSpeed produces 21; a fused formula would incorrectly
produce 22.

NaN, infinity, or a finite value outside signed-i64 range makes the x87 FISTP
write integer-indefinite `0x8000000000000000`; callers consume its low dword,
zero. Raw f32 signed-zero/subnormal/nonfinite country values therefore still
enter the exact f32-to-x87 path and are not parser-clamped or rejected by the
consumer.

`HouseClass__GetSpeedBonus @0x0050C050` calls the supplied TechnoType vslot
`+0x2C`. AircraftType return 3 selects `HouseType+0x130
SpeedAircraftMult`; InfantryType return `0x10` selects `+0x128
SpeedInfantryMult`; UnitType return `0x28` selects `+0x12C SpeedUnitsMult`;
every other return uses exact f32 one. Concrete type vtables/COLs and return
bodies are `AircraftType 0x007E2868 -> 0x0041CFB0`, `InfantryType
0x007EB610 -> 0x00524D40`, `UnitType 0x007F6218 -> 0x00748170`, and negative
BuildingType `0x007E4570 -> 0x00465D90` returning 7. Ship owners are UnitClass
and use SpeedUnitsMult; no naval/building speed multiplier exists.

`HouseTypeClass__Constructor @0x005113F0` writes f32 one to `+0x128/+0x12C/
+0x130`. ReadINI sites `0x00511BFF..0x00511C56` pass the current widened f32
as default through `ReadDouble @0x005283D0`, then narrow the result back to
f32. Missing section/key therefore preserves prior bits across rules layers.
No clamp exists. The `%f` path parses binary32, widens, applies binary64 0.01
for `%`, then narrows: `1.15` and `115%` both store `0x3F933333`; `-0` stores
`0x80000000`. Installed retail contains no active assignment of any of the
three keys, only a commented `SpeedUnitsMult=1.15`, so every stock HouseType
retains `0x3F800000`. A present malformed nonempty numeric is an address-layout
dependent scanf-output/argument alias accident; it is an unsupported invalid
input, not an implementation approximation. Empty values are omitted and
preserve the prior default.

Infantry alone overrides vslot `+0x538`.
`InfantryClass__GetMovementSpeed @0x00521D80` calls the common helper, returns
that i32 unchanged when `Infantry+0x6DB IsProne` is zero, and otherwise reads
`InfantryType+0xEBD Crawls`. Crawls returns wrapping
`N - trunc_toward_zero(N/3)`; non-Crawls returns wrapping
`N + trunc_toward_zero(N/2)`. There is no early return for zero/negative, no
saturation, and no x87/terrain/health/fear input in the wrapper. For positive
N the Crawls result happens to be `ceil(2N/3)`, but signed values make that an
unsafe replacement. Crawls constructs true; installed `WEEDGUY` omits the key
and retains true.

Walk `ProcessMovement` writes current fraction exact one at
`0x0075BFAC..0x0075BFB5`, calls the Infantry override once at `0x0075BFC0`,
and uses its returned integer for FILD/sin/cos displacement. Stop exits write
zero and return before the query. Walk terrain and damaged-health modifiers do
not enter this path. `WalkLocomotionClass::Is_Moving_Now @0x0075AB40` instead
checks its moving byte, ordered-positive owner fraction, then destination;
Walk body animation uses locomotion state plus WalkRate/IdleRate. Neither is a
speed-integer consumer.

The exhaustive 13-site `+0x538` call census also proves active Hover Unit
queries at `0x00514372` before multiplying by locomotor `+0x4C`, with a second
query at `0x005144A3` only for zero budget plus exact-facing startup reset.
Drive/Ship track sites are `0x004B1274/0x006A093C`, their moving predicates are
`0x004AFC71/0x0069F381`, base ApparentSpeed forwarding is `0x0055AD19`, and
Unit-only moving archive projection is `0x0070BD4C`. Mech
`0x005B14BE/0x005B1A31` and Tunnel `0x00728FCD/0x00729943` have no installed
YR ownership and are TS-dormant. The separate Jumpjet/Teleport census below
closes their non-common movement paths rather than pretending absence from
this call list is an exclusion.

That separate live census finds `Foot+0x580` has no direct locomotor reader:
the full operand search leaves `FootClass__GetCurrentSpeed @0x004DB1D5` as its
sole movement-value read. Installed Jumpjet owners are Vehicles
`DISK,HIND,SCHD,SCHP,SHAD,ZEP` and Infantry `JUMPJET,LUNR`; all store and
checksum the multiplier but do not apply it to displacement. Jumpjet
`Process @0x0054AEC0 -> State3_Translate @0x0054BFF0 -> UpdateCoordinates
@0x0054D0F0` ramps locomotor current `+0x70` toward target `+0x78`, converts
the current at `0x0054D55A..0x0054D575`, and applies facing sin/cos. It
publishes current/max through owner fraction setter `+0x544` at
`0x0054D19D..0x0054D1AE`, but never calls `+0x538`. Speed-crate presentation
still occurs; movement remains Jumpjet-owned.

Installed Teleport Vehicles `CMIN,CMON,SMON` and Infantry
`CCOMAND,CIVAN,CLEG` likewise persist/checksum the qword.
`StateMachineTick @0x007192F0` derives its timer from truncated 3D distance
divided by Rules `+0xBF4`, gated/clamped by `+0xBF8/+0xBFC/+0xC00`; position
commit `@0x00718260` is immediate. Neither path uses linear owner speed or
dispatches `+0x538`. Fly and Rocket are Aircraft-category pickup rejects:
the eight installed Fly types are `ASW,BEAG,BPLN,CARGOPLANE,HORNET,ORCA,
PDPLANE,SPYP`, and Fly independently computes
`ftol(Type+0x678 * Fly+0x48 CurrentFraction)` in `Process @0x004CD600`/
`0x004CFE20`. The three installed Rocket types `V3ROCKET,DMISL,CMISL` use
trajectory/acceleration state from `Process @0x006622C0`; neither family
consumes the crate field.

Parachute descent is not a locomotor. `ObjectClass::AI @0x005F3E70` computes
Z through vslot `+0x1D0` plus `Object+0x2C FallRate`, commits at `0x005F3F2C`,
then adjusts/clamps FallRate through Rules `+0x7B8/+0x7BC` at
`0x005F3FCB/0x005F3FEE`; no `+0x538` query occurs. An eligible Foot retains
the qword through descent and landed Walk begins consuming it at `0x0075BFC0`.
DropPod GUID `4A582745`, Tunnel `4A582743`, and Mech `55D141B8` have zero
installed retail locomotor bindings and are TS-dormant.

`LocomotionClass::Apparent_Speed @0x0055AD10` is only an owner-`+0x538`
forwarder. It is not called speculatively to retrofit Speed-crate movement into
the Jumpjet, Teleport, Fly, Rocket, or parachute paths, though a real apparent-
speed consumer observes the updated helper result. `TechnoClass::Resolve_
ArchiveTarget_Coords @0x0070BD00` also queries a moving Unit target at
`0x0070BD4C`; that targeting/leading projection can reflect the multiplier
without the target locomotor consuming it for displacement.

`FootClass__Constructor @0x004D31E0` initializes `Foot+0x578` to exact positive
zero. `TechnoClass::SetSpeedFraction @0x004D3710` first compares input with
1.0: ordered `>=` stores exact 1.0. Otherwise it compares with 0.0: ordered
`<=` or unordered stores exact positive zero. Only ordered strict interior
stores the original binary64 bits. Thus NaN and `-infinity` become zero,
`+infinity` becomes one, negative zero becomes positive zero, and positive
subnormal/interior inputs preserve all bits. Every result writes the low dword
then the high dword.

#### Drive/Ship target fraction producer

Drive `Process_Movement @0x004B2630`, tail
`0x004B357F..0x004B3E27`, and the instruction-equivalent Ship tail
`0x006A32D3..0x006A3476` are the ordinary target producer. Native forms a
signed reference level from the owner's current Cell `+0x11B`, plus four when
owner `OnBridge +0x8C` is set. A candidate whose signed level differs by at
least two uses Road land row 1; otherwise its `LandType +0xEC` selects the row.
The index is `land_row * 9 + SpeedType +0x67C`. The table f32 is loaded and
materialized as binary64, with ordered values above one capped to exact one.

Slope compares `GetGroundHeight` at the candidate coordinate with ground
height at the owner's exact XYZ, not the cell-level values used for row
selection. Only WhatAmI 1 Unit multiplies: uphill Track/non-Track uses Rules
`+0x768/+0x778`; downhill uses `+0x770/+0x780`. The product has an explicit
qword boundary. Equal height and non-Units skip slope. Exact zero or unordered
then becomes exact `0.5`, while negative nonzero survives. Health ratio at or
below `ConditionYellow +0x1700`, including unordered, next multiplies by exact
`0.75`. Selector `+0x58 < 64` stores the target qword. Selector `>=64` leaves
target unchanged and, unless the computed qword compares equal or unordered
to the owner current fraction, calls the shared owner setter.

The terrain-speed loader `@0x00674000` uses `ReadDouble`, caps only ordered
values at or above one, and stores f32. `ReadDouble @0x005283D0` scans with the
`%f` string at `0x00825BD8`, loads that binary32 at `0x0052855D`, and stores it
as binary64 at `0x00528569`. Percent tokens multiply the widened value by the
binary64 0.01 constant. The terrain loader's lower/unordered arm calls
`ReadDouble` a second time before its f32 store. A missing LandType section
performs no stores, leaving fresh BSS zero or retaining a prior reload row; a
present section gives absent keys one, forces Winged to f32 one, and stores
Buildable as a byte. Retail Tiberium Track 70% has parser intermediate
`0x3FE6666666666667`, stores f32 `0x3F333333`, and the movement producer later
widens it to `0x3FE6666660000000`.

Rules constructor `0x00665650` writes low zero/high `0x3FF00000` to all four
qwords at `0x006660C3..0x006660ED`, so each starts at exact one. `ReadGeneral`
call/store sites are TrackedUphill `0x0066F22F/0x0066F234`, TrackedDownhill
`0x0066F256/0x0066F25B`, WheeledUphill `0x0066F27D/0x0066F282`, and
WheeledDownhill `0x0066F2A4/0x0066F2A9`. Active rules supply
`1.0,1.2,1.0,1.2`, so downhill is widened-f32
`0x3FF3333340000000`. TechnoType constructor stores exact binary64
deceleration/acceleration defaults at `0x00710BBC..0x00710BDA`; ReadINI calls
and stores them at `0x007124A3/0x007124A8` and
`0x007124C4/0x007124C9`. A missing key returns the supplied current qword
unchanged, while an explicit override stores the widened-f32 parser result.
The target chain is invoked by actual `Process_Movement`, not by a generic
every-tick update.

#### Drive/Ship current fraction ramp

Drive `Process_Drive_Track @0x004B0F20`, block
`0x004B0F69..0x004B1295`, and Ship `@0x006A05F0`, block
`0x006A0639..0x006A095D`, have the following verified priority:

1. `Accelerates +0xDBD == false` sends the target qword directly through the
   owner setter, skips passive/selector/ramp/convoy work, then queries speed.
2. Accelerated Unit with UnitType `Passive +0xE0C`, or selector `>=64`, skips
   all fraction writes but still queries speed. Passive constructs false and
   active retail data contains no `Passive=` assignment.
3. The destination is the stored locomotor XYZ, except Cell flags
   `+0x140 & 0x100` replace its Z with ground height plus 416. Wrapping signed
   i32 deltas are squared in exact inline order `dz^2 + dy^2 + dx^2`, stored
   as qword, passed through `Sqrt_Approx`, then chopped. Planar 100 plus bridge
   Z 416 therefore yields 427, not 516, and enters stock slowdown 500.
4. Strict signed distance below `SlowdownDistance +0x2F8` computes
   `C - f64(type_speed) * DeaccelerationFactor +0x300`, then ordered-maxes
   against promoted-f32 0.3 (`0x3FD3333340000000`). Otherwise owner state
   `+0x3CD` computes the same shape with promoted-f32 rate 0.0015
   (`0x3F589374C0000000`) and floor 0.1 (`0x3FB99999A0000000`).
5. `CurrentlyCrushing +0x6B5` overrides either brake candidate with ordered
   min of target and binary64 0.2 (`0x3FC999999999999A`), unordered choosing
   target, writes that result back to target, and invokes the setter.
6. Otherwise a selected brake candidate invokes the setter. Without braking,
   `C < T` or unordered adds `AccelerationFactor +0x308`, caps at target with
   unordered choosing target, and invokes the setter. Ordered `C > T`
   subtracts type-speed times deceleration and floors at target only when
   ordered target is greater. Exact equality performs no owner setter call.

Stock acceleration is binary64 `0.03` (`0x3F9EB851EB851EB8`), deceleration is
`0.002` (`0x3F60624DD2F1A9FC`), and slowdown distance is 500 when their INI
keys are absent. Explicit overrides use the widened-f32 parser result. Drive
obtains type speed through virtual slot `+0x38C`; active Unit binding reaches
`TechnoClass::GetTypeSpeed` and Type `+0x678`. Ship reads `+0x678` directly,
so all active-retail Unit results are equal despite that structural source
difference.

Installed overrides resolve to exact qwords: DNOA, DNOB, V3, and SQD
AccelerationFactor `0.01` become `0x3F847AE140000000`; DRON and CAOS factors
`5` become `0x4014000000000000`; SMIN DeaccelerationFactor `.2` becomes
`0x3FC99999A0000000`. All other installed types retain the constructor qwords
for missing keys.

Unit vslot `+0x544` binds the fraction setter. After the ordinary accelerated,
nonpassive, selector-below-64 arm, both Drive and Ship propagate the owner's
normalized qword through `Unit+0x6C8` before owner vslot `+0x538`
`GetCurrentSpeed`. Native applies the initial linked member, advances, and
stops on null or before applying a newly reached self-linked terminal; an
initial self-linked member is applied once. Nonaccelerated and skipped arms do
not propagate. Same-Process retry repeats target/current work, propagation,
and the speed query, but masks the fresh query contribution before adding the
retained residual. An active track schedules Track(0), optional
`Process_Movement`, then Track(1); no-track state schedules
`Process_Movement` then Track(0).

Sinking clears path head and target without owner setter. Fully idle state
sets owner current to zero only for ordered current above zero. Terminal
ProcessMovement abort clears selector/path head and unconditionally sends zero
through the owner setter without clearing target. ForceTrack success sets only
the locomotor target qword to exact one; it does not set owner current. Land
war-factory selectors 66 set owner 0.5, tank-bunker install/undock/eject
selectors 67..71 set owner one, and Action 128 relocation/PerformDeploy use
selector -1 without a direct owner write. `ReleaseDockedHarvester` is a bunker
reciprocal-release path, not refinery unload.

The remaining owner-setter callers are exact-gated: EnterIdle zero only with
empty NavQueue; nonmoving Move reissue one only with result 2 and non-null
NavCom; eligible visible Unit/Infantry Wave zero before rocking/damage; Chrono
one after Unlimbo/destination and before facing/occupation.

Tube binds the same setter through Unit vslot `0x007F61B4` and Infantry vslot
`0x007EB59C`, with category-specific order. Blocked Infantry calls zero at
`0x0051B8F8..0x0051B8FC`; blocked Unit calls zero at
`0x00735F66..0x00735F6A`. Both retain Tube state and return to the wrapper.
Unit success calls one at `0x00736047..0x0073604F` after `+0x18C`; ordinary
Infantry success calls one at `0x0051BA79..0x0051BA81` before `+0x18C`. The
Infantry `+0x5A4`-equal arm instead invokes virtual `+0x174` at `0x0051BA6F`
and preserves the prior current-fraction qword. Every branch leaves target
unchanged, and Tube owns the complete object turn so no generic same-tick
movement writer follows. Low-bridge auto-Tube remains reachable without
explicit `[Tubes]` data.

The full direct-store scan finds only constructor zero and the setter's three
low-then-high arms. `FootClass__ComputeChecksum @0x004DBAD0` consumes both
current-fraction dwords separately from the following Speed-crate pair.

After stage 3, Unit with `Unit+0x6CC != -1` receives signed division by two
toward zero. That field is the active YR CTF flag-owner index, but stock
`CaptureTheFlag=no`; Phase 13 owns its off-default multiplayer state machine.
The ordinary Phase-3 path retains sentinel `-1` and does not halve. Terrain,
slope, health, and acceleration affect only the upstream stored fraction and
are not multiplied again here. Same-Process retry repeats fraction update and
the full query, then masks only the fresh returned integer before residual.

### Firepower — `0x00483125`

Each eligible Techno multiplies native `Techno+0x160` by parsed 2.0. If any
affected owner satisfies `IsHumanPlayer`, native emits
`EVA_UnitFirePowerUpgraded` after the loop. Picker-owner-human then gates
`CrateFireSound`; FIREPOWR animation follows. The persistent binary64 value
feeds the attacker damage path. It does not activate the `GetROF` branch at
`Techno+0x2E4`: that field is the unrelated bunker reciprocal partner pointer,
and the handler writes only `+0x160`. Radius candidates do not undergo the
picker's capability-vslot gate. The verified arithmetic consumers are
`0x006FDBD3` and `0x006FE343`; both multiply owner-House firepower by
`Techno+0x160` and base damage before one `Math__ftol`.

### Veteran — `0x00482972`

For each eligible candidate, native loops integer `i=0` while
`(double)i < data`. Each iteration applies the ordered native helpers:
Veteran-to-Elite, standard-Rookie-to-Veteran, then negative-Rookie reset.
Stock data 1 advances exactly one tier. No EVA is emitted.
Picker-owner-human gates `CratePromoteSound` after the loop; VETERAN animation
follows.

RNG order is selection, replacement X/Y attempts, successful replacement
timer, then effect-specific draws. Predetermined content omits selection.

## Active ingress

### Trigger action 108

`TriggerAction__Execute @ 0x006DD8B0`, case `0x6C` at
`0x006DF69B..0x006DF6CD`, resolves waypoint `TAction+0x44`, passes the full
dword `+0x90` to specific placement, has no mode/Crates gate, and returns the
helper boolean including true for a ghost.

`TActionClass__Constructor @ 0x006DD000` initializes `+0x44=0` and `+0x90=0`.
`TActionClass__Read @ 0x006DD5B0` consumes each counted chunk as
`ActionID,ParamType,Param3,Param4,Param5,Param6,Param7,WaypointCode` and clears
`+0x90` on every read. The signed i32 operand materialization is exact:

| ParamType | `TAction+0x90` |
|---:|---|
| 0 or 11 | native `atoi(Param3)` |
| 5 or 9 | native `atoi(WaypointCode)` when present, otherwise zero |
| 6 | dialog registry index, `-1` unknown |
| 7 | sound registry index, `-1` unknown |
| 8 | theme registry index, `-1` unknown |
| other | retained zero |

The registry lookups are `0x00753250`, `0x007514D0`, and `0x00721210`.
Waypoint `+0x44` receives the two-letter decoder result for every ParamType
except 5/9/11; those retain constructor zero (type 11 separately writes token
eight to `+0x48`). A missing optional token eight retains zero. Mandatory
ActionID through Param7 are unconditional `strtok -> atoi`; missing tails are
native invalid-domain null dereferences, and consecutive commas collapse.
Native decimal parsing accepts leading whitespace/sign, stops at the first
non-digit, returns zero without digits, and wraps to i32.

The Action reader at `0x0072753C..0x007275A3` stores the first Action at
`TriggerType+0xB0` and appends successors through `previous+0x28`. Actions
therefore execute in textual CSV-chunk order. `TriggerClass__Spring @
0x007265C0` invokes every Action without short-circuiting and returns the OR of
all Action results. `TagClass__ProcessTriggerEvent` ignores that OR at all
three callsites; Tag return and repeat-zero cleanup depend on condition/repeat
state. Its two other Spring callers also discard the OR.

All 44 Action-108 chunks across the active 184-map corpus use ParamType zero;
13 are ordinary standard-skirmish calls. `xxmas.map` executes
textual `CA..CK`, cells `(63,67)`, `(64,62)`, `(71,64)`, `(74,73)`, `(69,78)`,
`(82,77)`, `(79,85)`, `(71,86)`, `(80,94)`, `(91,82)`, `(88,72)`, with data
`0,10,9,0,14,11,0,10,9,0,14`. `xarena.map` left uses `CB` `(69,114)`, data 2;
right uses `CA` `(99,52)`, data 8. The full dword matters: exact 20 suppresses
the specific-placement post-write; 276 is not the sentinel and writes low byte
`0x14`.

### Ordered Tag/Trigger ownership

`TagTypeClass` construction/parsing at `0x006E5B60/0x006E6080` proves `[Tags]`
field zero is repeat, field one is name, and field two is the head TriggerType
ID. The Trigger reader writes `[Triggers]` field three at
`0x007273D8/0x007273E1` as the enabled flag (`value != 0`); fields four, five,
and six are Easy/Medium/Hard admission; field seven is not repetition.

The trigger difficulty authority is signed `ScenarioClass+0x60C`.
`ScenarioClass__Set_Defaults @0x00683610` initializes it to one and
`ScenarioClass__Full_Init @0x00686B20` copies the campaign launch value
`DAT_A8EB64` or, for noncampaign games, raw multiplayer-dialog
`DAT_A8B278`. Stock `[MultiplayerDialogSettings] AIDifficulty=0` therefore
selects Trigger field four (Easy) in OfflineSkirmish. The literal Trigger
mapping is `0=Easy, 1=Medium, 2=Hard`; the fact that House-AI terminology names
its separate zero value Hard does not invert this Scenario field. Raw values
outside `0..=2` are preserved. Scenario raw save/load carries `+0x60C`, and
the Scenario quick CRC at `0x0068BBD0` folds it.

`TagClass__Constructor @ 0x006E4DE0` constructs one independent TriggerClass
for each TriggerType in the Tag's linked chain and push-fronts it, so instance
order is the reverse of the TriggerType chain. Event parsers push-front, so
Event runtime order is reverse textual CSV-chunk order. Action readers append,
so Action runtime order remains textual CSV-chunk order.
Reusing one Tag ID on several attached objects/cells reuses the first TagClass
runtime. Different Tag IDs that point to the same TriggerType own independent
TriggerClass state. `FUN_00684C30` walks TagTypes in `[Tags]` source order and
appends global Tag instances to `DAT_008B40CC`; global polling is therefore
source order, not Trigger ID sort order.

The minimum live Tag state is its TagType identity, first/ordered
TriggerInstance list, signed attachment-reference count at `+0x2C`, attached-cell sentinel at
`+0x30`, disabled/uninitialized byte at `+0x34`, busy byte at `+0x35`, global
registration, and pending-finalization state. Each TriggerInstance owns its
TriggerType/next identities, raising House at `+0x2C`, pending delete at
`+0x30`, semantic timer start/duration at `+0x34/+0x3C`, satisfied mask at
`+0x40`, and enabled at `+0x44`. The intervening `+0x38` word is inert residue,
as reconciled below. Event and Action nodes belong to the referenced
TriggerType, not the TriggerInstance. Therefore every instance of one
TriggerType shares its native-order nodes and the mutable Event
`last_raising_owner` at `TEvent+0x54`, while instance enable/pending/timer/mask/
raising-House state remains independent.

`TriggerActionEntry__EvaluateConditions @ 0x007264C0` first rejects disabled or
pending-delete instances. Repeat two skips the Event list and succeeds.
Otherwise it visits every Event in runtime order without short-circuiting. Event
index `i` maps to `1 << (i & 31)`, so indices above 31 alias. A latched Event is
not reevaluated. Every successful or prelatched Event may provide its `+0x54`
owner and the last non-null owner in runtime order wins the TriggerInstance
raising-House field. The shared persistence byte begins true for repeat two;
Event 1 sets it. Qualifying persistent Events latch unless classified
nonlatching. Events 49/50 are latch-eligible; Event 1 is explicitly
nonlatching; Event 8 is not latch-eligible. All-true plus persistence rearms
the timer. Events 1/8/49/50 consume no RNG.

`TagClass__ProcessTriggerEvent @ 0x006E53A0` rejects editor mode, busy,
disabled/uninitialized, or null-type Tags, then visits every TriggerInstance
without stopping after a match. `TriggerClass__Spring @ 0x007265C0` invokes
every Action synchronously in native list order; individual Action booleans do
not stop the list. Repeat zero marks each sprung TriggerInstance pending-delete
and queues it. After the full list native clears busy, detaches the triggering
object if still attached, detaches a passed non-sentinel cell, logically
unregisters the Tag, queues physical finalization, and returns whether anything
sprang. The late finalizer frees later. Repeat two springs enabled/unfired
instances without condition evaluation or detach/queue and remains registered.
Active 184-map Tag occurrences are 1,926 repeat zero, 16 campaign repeat one,
and 584 repeat two, including two standard-skirmish repeat-two Tags.

#### Fresh-load and registry order

`ScenarioClass__Full_Init @0x00686B20` first reads TriggerTypes then TagTypes in
source order, but runtime Tag order is first materialization. Exact ensure/reuse
order is valid unoccupied CellTags (`0x004AD2AA`), Units (`0x00743475`),
Aircraft (`0x0041B2E8`), Infantry (`0x0051FD34`), Structures (`0x0044FB3E`),
then category-mask 4/0x10/8 postpass calls at
`0x00684D6C/0x00684DD5/0x00684E3F`. `FUN_006E52A0` forward-reuses the first
Tag with matching TagType. Object/Cell setters increment the shared signed
attachment count; each successful CellTag overwrites `Tag+0x30`, so the last
successful attached cell wins.

Tag construction appends the Tag master before constructing the linked
TriggerTypes head-to-tail. `TriggerClass__Constructor @0x00725FA0` appends each
global Trigger registry entry in that forward order while Tag local insertion
pushes front, yielding reverse evaluation. Event-13/51 timers initialize after
registry append and before the final enable gate; Event51 spends Scenario RNG
in this construction order. The constructor begins enabled, resets timers,
then sets enabled to field three AND the selected Easy/Medium/Hard field; raw
difficulty outside `0..=2` uses field three alone. A Tag with no attachment and
no category bit 4/8/0x10 has
no runtime. Polling registry `DAT_008B40CC` is separately appended by the mask-
0x10 postpass in Tag source order.

Common timer reset walks Events head-to-tail (reverse textual chunks). Event
13 writes current frame as start, `scalar.wrapping_mul(15)` as duration, and
clears its aliased satisfied-mask bit. Event 51 calls Scenario
`RandomRanged(0, scalar)`, uses signed truncation for `scalar/2`, writes
`(scalar/2 + draw).wrapping_mul(15)`, and clears its aliased bit. Every Event
51 draws even when another timer Event later overwrites the shared timer or the
constructor's final gate disables the instance. Native `TriggerClass+0x38` is
uninitialized stack residue: raw object save carries it, but no semantic read
or quick-CRC fold exists. Rust must normalize it rather than emulate garbage.

Team construction directly constructs a no-reuse Tag group at runtime and can
therefore add duplicate TriggerType instances later. Closed constructor/factory
xrefs plus installed tile-animation data prove no Team creation interleaves
the initial order in the 184 active maps. Their TriggerType graph is a forest:
3,186 types, 2,526 Tag heads, 659 links, 3,185 owned nodes, no fan-in/cycle/
self-link/dangling reference, one inactive unowned node, and maximum chain 30.

#### Repeat-one and polling mutation correction

`Tag+0x2C` is a signed wrapping attachment-reference count, initialized zero
at `0x006E4DE0` and maintained by Object setter `0x005F5B50`, Cell setter
`0x00485250`, and Team member add/remove. For repeat one,
`0x006E5442` compares exact `count == 1`. Any other count Springs nothing and
detaches only supplied matching sources. Exact one Springs/queues every
matching instance, logically expires the Tag, synchronously clears remaining
references, and defers physical destruction. A detach reaching one does not
reevaluate; zero stays live and inert.

The earlier cursor-repair claim was wrong. `LogicClass__PerTickUpdate
@0x0055AFB0` resets polling index zero, processes `polling[index]`, then
unconditionally increments. Repeat cleanup synchronously stable-erases the Tag
through `LogicClass__PointerExpired @0x0055B8A0`; physical destruction at
`0x00725C70` is later. Thus `[A,B,C]`, A retires -> `[B,C]`, index one next
processes C, and B waits until the following tick.

#### Migrated active Event and Action substrate

The active `[Events]` format is variable arity: after its count, ParamType zero
uses `kind,param_type,scalar`, while ParamType two adds `type_name`. The active
corpus contains 3,540 triples and 78 quads. Fixed-three parsing desynchronizes
mixed rows, e.g. `all03umd 0C7B443C` Event60 plus Event11. The current Rust
evaluator also reads ParamType rather than scalar.

Live Event contracts are: 27/28 signed global index 0..49 set/not-set; 36/37
signed local 0..99 set/not-set; 47 scalar <= signed frame/15; 60 case-sensitive
TechnoType resolution followed by backward global-Techno scan, signed threshold
and no owner/alive/limbo filter; 61 the same scan and exact absence. All are
nonlatching. Native invalid variable indices address adjacent stack bytes;
active values are valid, so typed Rust rejects the invalid domain. Changed
global/local writes set the corresponding dirty delivery and rearm matching
Event51 timers in global Trigger registry order, spending RNG immediately;
unchanged writes do neither. Later Tags see changed values in the same polling
walk, while completed Tags are not revisited.

Action22 at `0x006DF0E7` scans every matching TriggerInstance forward and
calls Spring synchronously, bypassing Events, Tag busy/repeat cleanup, queue,
and dedupe. Enabled/pending Spring gates remain; forced repeat-zero instances
are not consumed. Actions53/54 at `0x006DF137/0x006DF164` scan all matches.
Enable ignores current enabled, pending-delete, and authored field three. Raw
Scenario difficulty `0..=2` admits only the matching Easy/Medium/Hard flag;
out-of-range difficulty admits every match. Each admitted instance is enabled
before timer reset, including RNG and mask clearing; ineligible instances are
unchanged. Disable only clears enabled for every exact match and performs no
difficulty, timer, mask, or RNG work. Both return true for null/empty/no-match/
all-ineligible cases, and neither evaluates Events.
Actions28/29 and 56/57 use the materialized scalar, not ParamType, and perform
the dirty/rearm path above. Action40 remains synchronous. All Actions resolve
the executing TriggerType owner House before dispatch; a nested Force uses the
target TriggerType's House.

The 184-map active-retail census contains 3,186 Trigger rows. Their thirteen
distinct field3/Easy/Medium/Hard combinations yield initially enabled totals
Easy=1,293, Medium=1,318, Hard=1,330. It contains 1,807 Action53 occurrences,
admitting 1,668/1,701/1,710 targets and resetting Event51 RNG 57/60/60 times
under Easy/Medium/Hard respectively; 98 occurrences target a Trigger whose
field three is zero, proving Action53 ignores that field. It also contains
1,502 Action54 occurrences, 1,693 Event13 rows, and 73 Event51 rows. Of 3,186
TriggerTypes, 3,185 are owned by installed Tag chains and the one unowned row
has no timer RNG path.

#### Camera Actions 48/112

Action48 `0x006DEDFF` and Action112 `0x006DF795` use the full 702-slot waypoint
table without a validity check: an in-range missing slot is cell `(0,0)`, while
negative/>701 native indices are OOB and excluded by typed rejection. Both
build cell-center XYZ with slope-aware ground Z plus 416 when cell flags contain
`0x100` or `0x400`, project it, have no House/human/local gate, and return true.

Action48 arms Tactical glide state with Param3 selector 0..4 and f32 speeds
stored at `0x008428EC`: `0.0015,0.003,0.0075,0.03,0.06`, completing in
667/334/134/34/17 steps. Action112 writes committed/requested center instantly
without cancelling glide. Consequently 48->112 resumes the pre-snap glide next
AI step; 112->48 captures the snapped start; a second 48 replaces pending
glide; a second 112 wins the immediate position.

`TacticalClass__AI @0x006D2540` advances at most once per binary frame after
trigger commands, gates replay/scenario-active, uses `(0,0)` target as disabled,
performs f32 progress add/cap and f64 axis lerp with x87 chop/clamp, and clears
glide only at completion. Raw Tactical save/load preserves glide, progress,
centers, and last frame; load clamps committed, copies it to requested, and
cannot double-step the load frame. Tactical CRC omits camera fields and replay
records committed center separately. Camera motion is saved local presentation
state, not Simulation/hash/checksum state.

#### Result Actions 67/68/69

`TriggerAction__Execute @0x006DD8B0` cases at
`0x006DDD77/0x006DDD93/0x006DDDAF` use only ActionID, always return true, and
call the session `g_PlayerPtr`, never the Trigger owner or raising House.
Action67 invokes `HouseClass__Flag_To_Win(1) @0x004FC9E0`; Action68 invokes
`Flag_To_Lose(1) @0x004FCBD0`; Action69 invokes `FUN_004FCDC0`.

Win accepts only with pending/win/loss all clear, sets win, and argument one
preserves the shared result timer. Loss clears win first, then pending or prior
loss suppresses a new loss/timer/output transition; otherwise it sets loss and
preserves the timer. Action69 calls Win(0) only when neither terminal byte is
set; pending can reject it. With win/loss present it repairs only a `start==-1`
timer by storing current frame. It produces no immediate MissionResult.

Fresh House construction zeroes the three bytes, sets timer start to creation
frame, and duration zero. House update later in the same Logic tick processes
win, loss, then pending and owns terminal routing; pending expiry clears the
byte and scatters units. Textual order therefore yields 67->68 loss, 68->67
loss, terminal->69 start repair only when paused at -1, and 69->68 loss with
the normally armed win timer retained. Independent pending/win/loss plus shared
start/duration are required; a single outcome enum is not equivalent.

Native House raw save carries all result fields. Active network quick checksum
`0x64DAB0` folds only `House+0x241 map_is_clear`; it omits the three result
bytes and TimerClass remaining time. That remaining-time fold belongs to the
distinct full House CRCEngine `0x502D60`. Authoritative Rust world hash still
includes all future-affecting result state. Current trigger-global
`last_announcement` and hardcoded deferred result effects have no native owner
and must be removed. Active retail has exactly two Action68 rows (`all01umd`,
`all04dmd`) and zero Action67/69; both target the local player despite authored
`Americans` Trigger owners.

The sole active Action22 edge `all02umd 0926703C -> 0916204C` is non-self and
acyclic. Native Force has no recursion guard, so typed compilation may reject
synthetic self/cycles but must execute this edge synchronously. Action12 is a
separate later campaign-trigger gap: its 27 active campaign calls do not
co-occur with Action108 and are not introduced or regressed by this crate
prerequisite.

### Active Event 1/8/49/50 paths

Retail `xarena.map` uses Event 1 on two tagged `CATECH01` structures to place
fixed HealBase and Reveal crates. The active producer is engineer capture in
`InfantryClass__Mission_Capture @ 0x005202F0`: after the engineer is within
strictly 128 leptons, native reads the target Building's AttachedTag and raises
Event 1 with the engineer as the object. Event 1 is object-raised, filters the
owner only when its data is not exact `-1`, writes the engineer owner's House,
sets evaluator persistence, and is nonlatching. The synchronous action-108 call
finishes before target Guard, target Limbo, capture EVA/detach, ownership
transfer, Building tag replacement, and engineer destruction. Both installed
rows use `-1`, so either player's engineer passes.

Retail `xxmas.map` has Event 8 and eleven action-108 calls selecting Money,
Speed, Armor, Veteran, and Firepower. No `Process(8)` caller exists. Instead,
the first pre-object Logic trigger rung unconditionally delivers Event 13 after
earlier optional latches clear; Event 8 evaluation ignores the raised ID and
returns true. Its repeat-zero Tag springs the eleven Actions synchronously in
textual `CA..CK` order, then unregisters and is physically finalized late.

Crate pickup delivers Event 49 only to the collector AttachedTag before the
liveness reread, Event-50 latch, any RNG, or crate removal. Events 49 and 50
require their exact raised IDs and non-editor mode but have no House/data gate,
persistence write, or raising-owner write. An Event-49 Action that kills the
collector causes the pickup early return and preserves crate/latch/RNG. A
surviving pickup sets the global Event-50 latch. On the next leading trigger
rung, Event 50 is attempted first for every Tag in global source order. A
firing Tag skips only its own later event ladder; the global walk continues,
and the latch is always cleared afterward.

Current Rust aggregates state by Trigger ID in a `HashMap`, sorts trigger IDs,
incorrectly retains textual Event order, short-circuits conditions with `.all()`, and
tracks disabled/fired state globally by Trigger ID. It lacks per-Tag instances,
native latch/timer/owner state, mutation-safe repeat cleanup, Events 1/8, action
108, and map-object Tag columns. `ActionEntry` also lacks the exact signed
`+0x90` projection and `apply_action` returns `()`, losing native Spring OR.
Exact retail ingress requires replacing that ownership model, not adding
isolated event/action cases to the aggregate.

Native save/load serializes the future-affecting Tag, TriggerInstance, and
shared TriggerType/Event state above and swizzles TagType, TriggerType,
next-instance, raising-House, and
attachment identities. Tag quick-checksum identity covers TagType, first
TriggerInstance, and `Tag+0x34`. TriggerInstance checksum identity covers its
TriggerType, next, pending-delete, enabled, remaining timer, and satisfied
mask. Raising-House and Event `+0x54` owner memory are serialized despite being
omitted from parts of that native checksum. One shared Event owner cell must be
restored for all instances of its TriggerType rather than cloned per instance.
Busy is normally false at a legal
save point, but native state ownership provides no basis for dropping it from
the Rust snapshot/world hash. Scenario difficulty raw state is independently
saved and quick-CRC-folded and must enter Rust persistence/world identity;
`TriggerInstance+0x38` is the exception because it is semantically unread
native stack residue and must remain normalized/hash-neutral.

### Building `CrateBeneath`

The older [BUILDINGCLASS_ON_DESTROYED_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_ON_DESTROYED_GHIDRA_REPORT.md) and
[BUILDINGCLASS_MASTER_GHIDRA_REPORT_V3.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MASTER_GHIDRA_REPORT_V3.md) claim this is Iron-Curtain-only;
live exhaustive caller reconciliation proves that claim false.

`BuildingClass__Place_OccupyMap @ 0x00441F60` has exactly two callers:
`BuildingClass__ReceiveDamage @0x004426A2` and zero-health
`BuildingClass__Update @0x004400E5`. It has no construction, placement,
Unlimbo, sale, or generic teardown caller. Fatal result-4
`BuildingClass__DestructionEffects` sets duration zero for mission `0x13` or
`Explodes=yes`, otherwise duration eight. All active retail CrateBeneath types
have default `Explodes=no`, so at elapsed zero ReceiveDamage synchronously
calls `ObjectClass__UnInit @0x005F65F0` then `0x00441F60`. Duration-zero or
already-expired cases defer to Update: Limbo, SpawnSurvivors, UnInit, then
`0x00441F60`. Lethal damage while Selling eventually reaches the path;
voluntary sale alone and damage result 5 do not. The body executes once after
UnInit while the Building is still allocated.

When foundation data exists, `0x00441F60` first walks ordered foundation
deltas, obtains render coordinates, converts them to cells, merges/dirty
screen and radar rectangles, writes `Cell+0x44=0xEF, Cell+0x40=0`, recalculates
attributes, assigns orphaned zones, incrementally rebuilds zone graph, detaches
target references/restores missions, restores the origin BuildingType pointer,
and queues origin redraw. Foundation-data failure skips this refresh but does
not skip the crate tail.

The tail at `0x004421A4..0x00442221` reads
`BuildingType+0x1767 CrateBeneath`, re-reads vslot `+0xAC`, then calls specific
placement. Vslot `+0xAC` is `BuildingClass__GetRenderCoords @0x00459EF0`,
returning `(LocationX-128, LocationY-128, LocationZ)`, not foundation-center
`GetCoords`. Each render axis `r` converts as
`(r + ((r >> 31) & 0xFF)) >> 8` with wrapping i32 arithmetic and low-i16
storage: signed division by 256 toward zero. The request is the northwest
foundation anchor and is independent of foundation dimensions. Examples:
`Location=256*A+128 -> A`; locations `0,-1,-128,-129` request cells
`0,0,-1,-1`.

`CrateBeneathIsMoney` passes zero; otherwise exact `0x14`. The return is
ignored. There is no Crates, mode, owner, House, alive, foundation-size, or
player gate. The specific helper performs one FNPC, one ascending free-slot
scan, and at most one placement; full/out-of-playfield/existing-overlay
failure never retries.

Fourteen retail types set the flag. The ordinary census contains 58 placed
instances across sixteen maps: 55 Money and three random. Ordinary destruction
of those props is therefore active player-visible ingress, while construction,
normal placement, Unlimbo, capture, direct despawn, and voluntary sale are
negative paths.

### Unit `CarriesCrate`

`UnitClass__ReceiveDamage @ 0x00737C90`, death block
`0x0073838A..0x00738452`, requires `CarriesCrate`, global Crates, and the
scenario `TruckCrate` flag for non-trains or `TrainCrate` for trains. Both map
flags default false. It performs an outer FNPC/invalid-cell check, then the
specific helper snaps again, passing exactly `0x14`.

Only stock `TRUCKB` has `CarriesCrate=yes`. Four ordinary maps instantiate it,
but every explicit installed `TruckCrate`/`TrainCrate` value is `no`; neither
producer gate is active in retail data. This ingress is required parsing and
synthetic regression surface, but evidence-excluded from ordinary-stock
runtime closure.

## Complete pickup caller closure

The live xref set to `CrateClass__PickupDispatch` is exactly thirteen calls in
eleven bodies: `0x5153E9`, `0x5B1894`, `0x4B405D`, `0x4B46E6`, `0x4B0D1B`,
`0x6A3689`, `0x6A3D15`, `0x6A03EB`, `0x71972E`, `0x75C56C`, `0x6A1401`,
`0x4B1DBE`, and `0x54C9F6`. The continuations are not interchangeable.

Drive ForceTrack `0x004B0C40/@0x004B0D1B` and Ship ForceTrack
`0x006A0310/@0x006A03EB` write selector `+0x54`, track index `+0x58=0`, and
install the original request before dispatch. Return zero or limbo clears
destination/validity only if the collector is alive; death retains state.
Return one plus unlimbo performs no alive test: it raw-applies the original
request, writes original XYZ to head-to `+0x30..+0x38`, and writes speed
`+0x4C/+0x50=1.0`. It does not rewrite destination, so callback retarget
survives; Explosion death still receives these raw writes.

Drive ProcessDriveTrack `0x004B0F20/@0x004B1DBE` and Ship
`0x006A05F0/@0x006A1401` capture the owner's raw current-fraction low/high
dwords before candidate classification, CanEnter, setup, destination install,
or pickup. They then write `+0x60=0`, new selector `+0x58`, and `step_count-1`
to `+0x5C`, and install the original candidate. Return zero or limbo clears it
only while alive. Return one plus unlimbo again has no alive test: it raw-applies
the original candidate, passes the saved pre-dispatch qword to the exact speed
setter without rereading any speed field, copies 23 dwords
`Foot+0x5E4..0x63C` to `+0x5E0..0x638`, sets `Foot+0x63C=-1`, and advances the
cursor/paid-point tail. Callback movement is ignored, retarget survives, and
dead/unlimbo still receives those writes. A callback current-fraction mutation
is overwritten only on this success/unlimbo path; the distinct Speed-crate
multiplier at `+0x580` persists.

Drive ProcessMovement first pickup `0x004B405D` and Ship `0x006A3689` occur
only when descriptor flag bit 3 is set. Before dispatch native has already
written movement selector state and computed the candidate; the call neither
clears nor installs live destination. Return zero plus unlimbo forces CanEnter
result 7; dead returns immediately, alive enters native result handling.
Return one or limbo returns immediately when dead; alive computes the second
curve cell from the original endpoint, not callback-moved XYZ, calls CanEnter,
and rechecks alive. Result zero shifts 22 dwords `Foot+0x5E8..` to `+0x5E0..`,
writes `Foot+0x638=-1,+0x68B=1`, and finalizes. Result 2 recurses with force;
4/5 clear `Foot+0x5E0` and locomotor `+0x58`, null the local endpoint, recurse,
and return while retaining a callback destination into recursion;
1/7/other rejection performs those clears and reaches common finalization.
That finalization clears any live callback destination, writes
`Foot+0x63C=-1`, packed-null endpoint `Foot+0x558`, `Foot+0x68A=0`, locomotor
`+0x5C=0`, and stops speed. Results 3/6 remain the native crush/block branches.
In particular, zero/unlimbo/alive becomes result 7; it is not an invented
stop/recurse continuation.

Drive ProcessMovement final pickup `0x004B46E6` and Ship `0x006A3D15` occur
after `Foot+0x63C=-1`, packed endpoint `+0x558`, `+0x68A=0`, and locomotor
`+0x5C=0` are written and a non-null local endpoint is installed. Return one
plus unlimbo performs no alive check and applies occupation mode 1 to the
original endpoint, then returns immediately without moving the object,
resetting selector/path, or stopping speed. The exact live sequences are
`PickupDispatch @ 0x004B46E6 -> Drive Apply_Track_Occupation_Mode @
0x004B4705` (callee `0x004B0AD0`) and `PickupDispatch @ 0x006A3D15 -> Ship
Apply_Track_Occupation_Mode @ 0x006A3D34` (callee `0x006A01A0`). Callback
retarget and callback movement survive; death still receives the raw
occupation-mode call. Return zero or limbo clears destination only while
alive, then regardless of alive writes locomotor `+0x58=-1`,
`Foot+0x5E0=-1`, speed `0.0`, and returns. A null endpoint skips dispatch and
performs that cleanup.

Hover helper `0x00514F70/@0x005153E9` installs resolved next-cell center and
ground/bridge Z before dispatch. Zero plus unlimbo, while alive and status
`+0x8D==0`, clears path/destination, performs the stop call, zeroes Hover
`+0x50/+0x48/+0x54/+0x4C`, writes speed `0.0`, and returns status 7. Dead or
nonzero status returns 7 retaining state. One or limbo re-reads alive, limbo,
and status; any failed gate returns 7 retaining state. Otherwise it recomputes
the next coordinate from current post-callback collector XYZ and enters native
CanEnter handling without reinstalling the original candidate, so callback
retarget survives.

Jumpjet helper `0x005B17B0/@0x005B1894` adjusts the request Z, releases the old
reservation without first clearing stored destination, resolves the local, and
dispatches. One plus unlimbo overwrites stored destination with the original
adjusted local and does not test alive, discarding callback retarget. Zero or
limbo writes Null, after which dead returns. The tail reservation-installs a
live non-null destination and returns one; with Null it re-reads current
post-callback XYZ, installs that, and returns zero.

Jumpjet State4 descend `0x0054C550/@0x0054C9F6` calls after successful
zero-altitude landing has stopped movement, cleared target, and updated
bridge/fog/cell state. It ignores return, alive, and limbo, then raw-writes
collector `+0x6AE=1,+0x427=0,+0x425=0`, locomotor `+0x90=0`, and under the
native UnitType gate collector `+0x134=0`. Callback retarget survives; kill,
move, or limbo suppresses nothing.

Walk FindSubCellDest `0x0075C240/@0x0075C56C` stores the exact native
subcell/deck result, resolves it, and dispatches; Null input stores Null and
skips dispatch. Only zero plus unlimbo clears live destination, then dead
returns. Zero/limbo and every one preserve live destination with no alive
test. Non-null live destination is reservation-installed and returns one;
Null re-reads current post-callback XYZ, installs it, and returns zero.

Teleport arrival `@0x0071972E` dispatches after arrival state work and ignores
return, alive, and limbo. It raw-calls collector stop, allocates a `0x1C8` Anim,
uses current post-callback collector XYZ for it, raw-writes collector
`+0x280=0`, and returns false. Callback movement changes animation position,
retarget survives, and kill/limbo suppresses no postwrite.

The shared Rust transaction must release entity borrows, retain a stable ID or
tombstone, and re-fetch after every callback/effect. A deferred queue cannot
preserve same-stack continuation. No Rust safety cleanup may erase the native
raw-write/deferred-lifetime outcomes above.

Ship ForceTrack is load-bearing for SQD. `ParasiteClass::Attach @ 0x0062A980`
snapshots victim XYZ, calls Ship ForceTrack selector `-1`, ignores its result,
then writes victim backlink followed by manager victim even if pickup killed or
uninitialized the limboed SQD. Rust must not sanitize that deferred-lifetime
state.

## Return-value matrix

- Prefix rejection/no crate: one.
- Event-49 collector death before selection: zero, crate intact.
- Unit exact or nearby placement success: zero for human and AI; human only
  adds sound; common animation is skipped.
- Unit type/create-null: common Unit animation tail, one.
- Unit created but both placements fail: destroy candidate, remap to Money,
  execute Money, common tail, one.
- Every other consumed effect, even Explosion after collector death: common
  tail, one.

## Rust divergence

Current `src/sim/crates.rs`:

- owns a discarded local `[bool;256]` rather than persistent slots;
- parses unsigned/clamped min/max and lacks regen/full rules/Powerups;
- draws within the wrong abstraction instead of exact active Map bounds;
- directly stamps overlays and asserts that occupation/Mark is bypassed;
- has a visible-water assertion but lacks the native identity-specific
  Float/Track, terrain, slope, occupation, and bridge predicates that make the
  installed flat-empty Water case correct;
- discards timer state after spending its draw;
- has no specific placement, clear, pickup, replacement, regeneration,
  effects, ingress, caller barrier, save, or hash ownership.

Current `src/sim/movement/movement_commands.rs` additionally calls
`update_drive_speed_fraction(..., target=1, ...)` immediately when a Drive path
is installed. For `Accelerates=false` it also writes current immediately. No
native Move-command path-install writer exists: ordinary target production is
confined to Drive `Process_Movement @0x004B2630`, tail
`0x004B357F..0x004B3E27`, and consumption to
`Process_Drive_Track @0x004B0F20`, apart from the separately enumerated
sinking/crushing/ForceTrack writers. The Rust call must be deleted; path
installation alone preserves both raw qwords until the scheduled movement
rung.

Current object and overlay rules omit `CrateGoodie`, `CarriesCrate`,
`CrateBeneath`, `CrateBeneathIsMoney`, and `CrateTrigger`. Current scenario
data omits `TruckCrate`/`TrainCrate`. Current trigger runtime lacks the active
retail action/event/tag ingress. `GameEntity` has Armor multiplier only; stock
crate Firepower and Foot Speed need independent persistent native-bit fields
and consumer integration.

`ScenarioSession` retains only `game_mode_nonzero`, losing raw 3/4/5 identity.
Production currently constructs only campaign and offline sessions; network
menu routes do not reach loading/Simulation. `HouseState` persists/hashes
`MapIsClear` but lacks the separate Visionary latch, and
`FogState::reveal_all_for_owner` does not reproduce the four exact per-cell
writes, their order/OR preservation, or redraw-generation side effect.

Current trigger parsing also interprets `[Triggers]` field three as an enabled
bit, which matches native, but hardcodes medium difficulty and derives
repetition from Trigger field seven instead of `[Tags]` field zero. The latter
two owners must be corrected as part of the active retail action-108 ingress.

## Evidence-backed exclusions

- No installed ordinary map authors a CRATE/WCRATE OverlayPack identity.
  Nonzero-mode loader filtering plus independent OverlayData decoding must be
  preserved, but authored overlay-to-slot bootstrap is excluded because native
  deliberately does not do it.
- No ordinary stock map defines events 49/50 or TeamType tags. The native latch
  state and ordering are verified, but full campaign/custom trigger action
  behavior is outside ordinary stock-skirmish implementation closure.
- `CarriesCrate` is parsed and its native death gate is verified, but every
  installed retail `TruckCrate`/`TrainCrate` flag is false. No ordinary stock
  unit-death crate is active.
- Weight-zero handlers are not selected randomly and no ordinary stock action
  108 supplies them. They remain parsed/indexed and testable but are excluded
  from the active ordinary effect implementation.
- Phase 3 admits raw modes 0/5 only and must reject raw LAN/WOL 3/4 before
  Simulation. Native network Reveal's exact predicate and cells are documented
  above, but are unreachable under that boundary. MPModes roster IDs 3/4 are
  not raw game-mode identities and remain valid offline selections.
- Failed Overlay allocation's orphan object-array identity and UniqueID are not
  read by crate gameplay or the quick checksum. Ghost state and RNG are
  required; exact native orphan graph identity is not.
- Native allocator OOM, zero-total malformed Powerups, invalid pointer graph,
  and memory corruption are invalid-domain behavior.

## Required builder validation

The implementation design carries the exhaustive executable matrix. At
minimum it must cover exact CrateRules constructor state versus installed
overrides; section/key layered retention; signed integer/no-clamp and lookup
fallbacks; ReadRange f32-widen/percent/`*256`/sentinel/NaN/nonfinite/i64-to-i32
low-dword cases; CrateRegen constructor/retail qword bits; Powerups executable
baseline, absent/empty-only preservation, present-section `0,NONE,0` fallback,
empty-row/unknown-section retention, live literal-NONE animation lookup,
collapsed-comma token shifting, decimal atoi wrap/no-hex behavior, exact
yes/no retention, direct-binary64 atof/percent bits and 127-byte boundary,
original-pass typed layering despite equal flattened projections, complete
RULESMD arrays, and SOV07S/SOV08U map arrays; exact slot
defaults/serde/hash; signed count; no-RNG full
table; hard rejection versus accepted ghost; land/water Mark behavior; timer
goldens; origin-versus-snapped destination image classification; accepted
visible/ghost screen-dirty then gated cell-redraw ordering; zero ghost rect,
no hard-reject invalidation, no radar dirty, and no second specific-data
invalidation; clear/pause/wrapping; matched/ghost/mismatch removal,
dirty-before-identity/data writes, all-other-Cell preservation, no removal
CellRedraw/radar, and mode-zero arbitrary `Crate=yes`; ascending regen
reinsertion; predetermined and
weighted selection boundaries; replacement/effect RNG order; all strict MP
guard thresholds; eight active effects; Unit success/fallback; action-108
signed operand/default/registry boundary, textual retail table, full-dword
20/276 distinction, and false/true Spring-OR permutations;
raw-mode 0/5 admission and 3/4 pre-Simulation rejection without MPModes-ID
confusion; Reveal MapIsClear/Visionary/remote gates, exact mode-5 cell writes,
Paranoid/radar/redraw order, and ungated sound/animation;
Speed setter ordered clamp/NaN/infinity/raw-interior-bit table, owner/member
write order before the current-speed query, target row/level/ground-slope/
health/selector chain, 70%-f32 widening, absent-versus-present terrain rows,
exact acceleration/braking/crush bits, strict 499/500 slowdown boundary,
caller-specific structural z/y/x distance 427, Drive/Ship source equivalence,
convoy self-link termination, scheduling/retry order, forced-track target-only
semantics, path-install no-fraction-write timing for accelerating and
nonaccelerating types, raw-0.375 Unit/Infantry Tube zero/one/preserve timing
with target unchanged and no same-tick overwrite, all three House speed
category f32s plus other-category one fallback/layer retention/no clamp,
Infantry signed wrapping prone branches including negative and overflow cases,
Walk fraction-one/query/displacement order plus ready/animation non-consumer
truth tables, Hover one-query and aligned-zero two-query paths, and unchanged
uncrated movement; Jumpjet/Teleport exact multiplier persistence with unchanged
movement but changed real ApparentSpeed/Unit-leading projections, parachute
unchanged descent then landed-Walk consumption, Aircraft Fly/Rocket pickup
exclusion plus mod-cross-assignment category/locomotor separation, and dormant
DropPod/Mech/Tunnel;
same-ID Tag sharing versus distinct-Tag instance-state independence plus
shared TriggerType Event-node/owner memory, including cross-Tag owner
propagation and save/load/hash; source/reverse native list orders;
no-short-circuit condition side effects; persistence/latch/last owner;
repeat-zero mutation-safe cleanup and late free; repeat-two retention;
Event-49 kill preservation; Event-50 per-Tag ladder skip/global continuation;
Tag/Trigger/Event save-load; parser enabled/repeat/difficulty ownership;
constructor and Action53 Event13/51 timer/RNG/mask order; field-three-zero and
pending Action53 targets; out-of-range difficulty fallthrough; Scenario
difficulty save/hash; inert `+0x38` normalization; CrateBeneath;
CarriesCrate; Event-1/Event-8 retail ingress; all caller continuations; SQD
ForceTrack; and the scheduler rung immediately before factories.

## Primary evidence

- Active retail `gamemd.exe`, image base `0x00400000`, live Ghidra
  decompilation/disassembly/xrefs at every address cited above.
- Installed retail `rulesmd.ini` extracted from the active VFS.
- 184 named retail maps extracted from the active installation and scanned by
  decoded INI/OverlayPack content.
- Current Rust `src/sim/crates.rs`, `src/rules/ruleset.rs`,
  `src/sim/trigger_runtime.rs`, `src/map/entities.rs`, movement families,
  `src/sim/game_entity.rs`, snapshot, and world-hash owners.

No Ghidra metadata was modified during this investigation.
