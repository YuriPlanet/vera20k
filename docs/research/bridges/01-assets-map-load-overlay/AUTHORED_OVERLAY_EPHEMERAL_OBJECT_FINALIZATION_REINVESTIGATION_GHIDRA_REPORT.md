# Authored OverlayPack Ephemeral Object and Shared Finalization Lifecycle — Active-YR Re-investigation

**Date:** 2026-08-31

**Corrected:** 2026-09-01. The later authored-wall reinvestigation proves that successful
`Full_Init` keeps `ScenarioInit @ 0x00A8E7AC` nonzero through `ReadMapOverlayPacks`.
Consequently the wall predicate call returns true immediately; the generic counter-zero wall-
rejection lifecycle below is active outside authored loading but is not an authored row outcome.

**Binary authority:** active retail Yuri's Revenge `gamemd.exe`

**Method:** fresh Ghidra decompile/disassembly, vtable/COL checks, active retail data and current Rust inspection

**Scope:** one bounded authored-OverlayPack object-lifecycle slice; no Rust or Ghidra metadata changes

## Verdict

`[ACTIVE-YR: YES]` An admitted, allocated authored OverlayPack row is not an
allocation-free cell stamp. The reader constructs a real `OverlayClass` in the
shared `AbstractClass`/`ObjectClass` identity and registry domain. The exact fresh
constructor order is:

1. allocate `0xB0` bytes;
2. construct the `AbstractClass` and `ObjectClass` bases;
3. append, best-effort and in order, to `g_ObjectClass_Array`, the Object pointer-
   expiration listener vector, the all-Abstract listener vector, and the Tag-removal
   listener vector;
4. install the Overlay vtables and type pointer;
5. preincrement the Scenario native unique-ID counter and store the returned ID;
6. append, best-effort, to `g_OverlayClass_Array`;
7. direct-call base `ObjectClass::Unlimbo`, whose virtual `Mark(1)` dispatch reaches
   `OverlayClass::Mark`.

`AssignUniqueID @ 0x00410230` does **not** perform a registry insertion. It only
obtains and stores the native numeric ID. Older wording that couples ID assignment
and global-array insertion into one operation is wrong: four base registrations
precede the ID, and the Overlay registry append follows it.

`[ACTIVE-YR: YES]` The common successful Mark tail recalculates the anchor cell,
sets `IsOnMap=0` and `InLimbo=1`, calls `ObjectClass::UnInit`, clears `IsAlive`, and
appends the still-registered object to the shared deferred-finalization queue. It
does not destroy or free the object inline. The next decoded Overlay row begins
while every earlier successful Overlay object is dead, queued, and still present in
all successfully joined Object/Overlay/listener registries. All optional CellAnim
and first-eligible terrain-tile `AnimClass` constructions caused by that row occur
between the Overlay's ID and the next row's Overlay ID.

`[ACTIVE-YR: YES]` Authored `Wall=yes` rows that pass the universal slope gate take this common
success lifecycle after their wall stamp, cardinal cleanup, and eight-neighbor count increments.
They do not take the generic wall-predicate-false UnInit branch during successful `Full_Init`.

`[ACTIVE-YR: CONDITIONAL]` The steep-slope rejection is materially different. Base
`ObjectClass::Mark(1)` has already set `IsOnMap=1`, set `NeedsRedraw=1`, and dirtied
the tactical view. `OverlayClass::Mark` then returns false for `Cell+0x11C > 4`
except type `0xB2`, before any Overlay cell write, Recalc, or UnInit. Base Unlimbo
restores only `InLimbo=1`. The object remains alive, registered, ID-bearing,
`IsOnMap=1`, and absent from the deferred queue. The reader's drain cannot select
it. It survives into the match and is finally destructed by later whole-scene
cleanup.

That survivor does **not** enter a Cell object list, Display, Logic,
`g_CurrentObjects`, or rendering. It is also absent from the active save stream's
class-array passes; the native live checksum paths do not enumerate it. Its
ordinary observable contract is therefore the consumed/nonrefunded ID plus
temporary/persistent registry and pointer-listener membership, not presentation.
The final `OverlayGrid` must continue to reject the row and must not render it.

`[ACTIVE-YR: YES]` `ReadMapOverlayPacks @ 0x005FD2E0` calls the shared
`DrainDeferredFinalizationQueue @ 0x00725C70` exactly once in its common epilogue,
after both OverlayPack identity and OverlayData bodies and after its temporary
pixel buffer cleanup. This call is outside the signed `NewINIFormat > 1` gate. It
therefore runs when the gate is false, when either body is absent or non-positive,
and on the ordinary generated `.SED` reader path whose default format is zero.
The drain completes before the reader returns, before the following network-service
call, and before Full_Init's first whole-real-map Recalc sweep.

`[ACTIVE-YR: YES]` The drain is shared, ordered, and live rather than Overlay-only
or snapshot-based. It skips alive queued objects in place, selects later dead
objects, removes every duplicate occurrence of a selected pointer before one
finalization, and processes the shifted successor at the same index. Its loop
rechecks the live queue count, so an entry appended during finalization can be
visited by the same call. For Overlay, `Release` returns one, none of the four
Techno RTTI restore arms apply, and the scalar-deleting destructor removes the
Overlay registry entry, the four base registry entries, and the allocation. The
native numeric ID is never refunded.

**Implementation readiness:** the lifecycle and the post-reservation ID transform
are closed. The absolute authored first-ID prefix is **not** closed: the map-read
snapshot `C_saved` is variable because native-ID-bearing House, Type, and Cell
constructors run after `Clear_Scene` seeds `1,000,000` and before the snapshot at
`0x004AD026`. A focused pre-map constructor-prefix investigation is a blocking
prerequisite for absolute end-to-end ID fixtures. Until that ordered prefix is
enumerated from current Rust inputs, this mechanism and its owning GSI row remain
open under the program's no-approximation rule.

## Authority, claimed scope, and exclusions

This report owns only:

- the real `OverlayClass` allocation, base construction, ID, and registry order for
  authored OverlayPack rows;
- base/derived Mark, synchronous child-Anim interleaving, UnInit, deferred queue,
  common reader drain, scalar destruction, and next-row visibility;
- the exact distinction among reader rejection, allocation failure, successful
  Mark cleanup (including authored walls), generic counter-zero wall failure cleanup, and steep-
  slope survival;
- the reader drain's position relative to identity, OverlayData, and the first
  Full_Init whole-map Recalc;
- generated-reader no-Mark plus unconditional shared-drain behavior;
- current Rust ownership and acceptance-test deltas.

The following are integration evidence, not duplicated algorithm ownership:

- exact high/low/ordinary Overlay cell writes and low procedural RNG;
- complete `CellClass::RecalcAttributes` LAT, CliffBack, zone, compact-cache, and
  tile-animation algorithms;
- complete Anim constructor/Middle/sound and final delete/unlatch/recreate life;
- the full pre-map House/Type/Cell native-ID constructor prefix;
- save/load reconstruction outside the narrow negative OverlayClass enumeration
  check below.

OpenTS was inspected only as a navigation lead (`code/overlay.cpp`,
`code/object.cpp`, and `code/tracker.cpp`). No TS-only cleanup rule, tracker policy,
or class behavior is used as parity authority.

## 1. Bounded inventory and integrated native timeline

### 1.1 Candidate mechanisms and disposition

| Candidate | Active-YR disposition | Evidence |
|---|---|---|
| Reader pre-admission filters | Active | `ReadMapOverlayPacks @ 0x005FD2E0` |
| `operator_new(0xB0)` | Conditional per admitted row | reader assembly/decompile |
| Abstract/Object base construction | Active after allocation | `ObjectClass__Constructor @ 0x005F3900` |
| Base Object registries | Active, best-effort | constructor assembly |
| Scenario native unique ID | Active, shared, preincremented | `0x00410230`, `0x0068BCB0` |
| Overlay registry | Active, best-effort | `OverlayClass__Constructor @ 0x005FC380` |
| Constructor Terrain precheck | Present; normally inactive in fresh authored reader | `FUN_0047C550`; Terrain section is later |
| Base direct Unlimbo | Active | direct call `0x005F4EC0` from constructor |
| Base Mark/redraw | Active before derived branch | `0x005F5850`, `0x005F4D10` |
| Derived success Recalc/UnInit | Active for every Mark arm reaching common tail | `0x005FD1FA..0x005FD21C` |
| Ordinary `CellAnim` child Anim | Conditional | `0x005FD112..0x005FD1FA` |
| Recalc terrain-attached Anim | Conditional, first eligible Recalc per unlatch cell | `0x0047D2B0` integration evidence |
| Steep-slope early rejection | Conditional and active | `0x005FC5CD..0x005FC5E3`, false return `0x005FC784` |
| Wall placement rejection with UnInit | Active only for counter-zero callers; authored-unreachable in successful Full_Init | `0x005FC6F4..0x005FC705`, UnInit `0x005FC77C`; wall ScenarioInit report |
| Deferred queue | Active on UnInit success; best-effort growth | `0x005F65F0` |
| Per-row immediate scalar destruction | Excluded | no such call in reader/Mark/UnInit |
| Common reader drain | Unconditional | reader call `0x005FD692` |
| Generated `.SED` Overlay Mark | Inactive in ordinary generated reader | default `NewINIFormat=0` |
| Generated `.SED` common drain | Active | common reader epilogue |
| Overlay survivor render/save/hash | Excluded on proved path | failure return plus save/checksum consumer checks |
| Later scene-teardown cleanup | Active | `FUN_00534450`, `Clear_Scene @ 0x006851F0` |

### 1.2 Fresh authored loader order

Let `C_saved` be the native signed-dword counter value read from
`ScenarioClass+0x214` at map-read instruction `0x004AD026`, and let `T` be the
number of successfully allocated and constructed `[Tubes]` rows.

1. `Clear_Scene` seeds the native counter to `1,000,000`.
2. Full_Init performs earlier House/Type work and Map Resize/Cell construction,
   all sharing that counter. This makes `C_saved` input-dependent.
3. `Read_Map_Section_And_IsoMapPacks @ 0x004ACE70` saves `C_saved` before theater
   reload and unconditionally writes `wrap32(C_saved + 0x2710)` at `0x004AD05F`.
4. `MapClass::ReadTubesINI @ 0x007283C0` constructs `T` Tube objects. Each
   constructor preincrements the same counter once.
5. The first admitted, allocated Overlay row receives
   `wrap32(C_saved + 0x2710 + T + 1)`.
6. Each row completes synchronously. Its conditional CellAnim and terrain Anim IDs
   are allocated before the next row.
7. Earlier successful Overlay objects remain dead, queued, and registered through
   all remaining identity rows and the entire OverlayData body.
8. The common reader drain finalizes queued-dead objects.
9. Full_Init makes the first anti-diagonal whole-real-map Recalc sweep only after
   the reader and drain return.

The formula is exact relative to `C_saved`. An absolute value is not yet justified
because the complete ordered pre-snapshot constructor surface remains open.

### 1.3 One successful admitted row

For a fresh row that passes reader admission and reaches the common Mark tail:

1. reader saves the anchor's prior data byte and allocates `0xB0`;
2. base Object construction initializes the object and attempts four registry
   appends;
3. Overlay construction stores type, assigns the next native ID, and attempts the
   Overlay registry append;
4. constructor direct-calls base Unlimbo;
5. base Unlimbo clears `InLimbo`, writes the coordinate, and virtual-calls
   `OverlayClass::Mark(1)`;
6. derived Mark first calls base Mark, which sets `IsOnMap` and redraw state;
7. derived branch writes cell state and may construct a `CellAnim`;
8. common Recalc may construct the first eligible terrain-tile Anim for that cell;
9. derived tail clears `IsOnMap`, sets `InLimbo`, and calls UnInit;
10. UnInit sends pointer-expiration cleanup, observes Limbo already set, clears
    `IsAlive`, and appends the pointer to the shared queue;
11. base Unlimbo observes Mark success but dead state and returns before Display or
    Logic registration;
12. reader applies the four-high anchor restore when applicable, then advances to
    the next decoded row.

### 1.4 Steep-slope row

For slope `>4` and type other than `0xB2`:

1. allocation, four base registry attempts, ID assignment, and Overlay registry
   attempt already happened;
2. base Mark already set `IsOnMap=1`, `NeedsRedraw=1`, and emitted the tactical
   dirty call;
3. derived Mark returns false at `0x005FC784` before cell writes or Recalc;
4. base Unlimbo restores only `InLimbo=1` and returns false;
5. constructor and reader ignore that failure and continue;
6. the object is alive and not queued, so the common drain leaves it untouched;
7. the next row begins with the survivor present in every registry append that
   succeeded.

### 1.5 Generated source

The ordinary synthetic `.SED` Full_Init reader sees default `NewINIFormat=0`.
Accordingly it performs no encoded OverlayPack allocation, ID, Mark, or per-row
dirty effect. The reader still calls the common shared drain. Later generated
direct cell writes and Recalcs are not authored OverlayClass objects and must not
invent these lifecycle effects.

## 2. Native layouts and ownership

### 2.1 Relevant `OverlayClass` / inherited Object facts

| Offset | Meaning in this path | Evidence |
|---|---|---|
| `+0x10` | native unique ID | AssignUniqueID store |
| `+0x30` | `pNextObject`, initialized null | Object ctor / pointer-expired handler |
| `+0x34` | attached Tag pointer, initialized null | Object ctor / pointer-expired handler |
| `+0x74` | `IsOnMap` | base Mark and derived success tail |
| `+0x80` | `NeedsRedraw` | MarkNeedsRedraw / Limbo |
| `+0x81` | `InLimbo` | Object ctor, Unlimbo, derived tail |
| `+0x88` | parachute-like pointer, initialized null | Object pointer-expired handler |
| `+0x90` | `IsAlive` | Object ctor, UnInit, IsDead |
| `+0x98` | Logic membership flag | base destructor conditional unregister |
| `+0x9C/+0xA0/+0xA4` | world coordinate | base Unlimbo |
| `+0xAC` | `OverlayTypeClass*` | Overlay ctor/dtor |

The primary Overlay vtable is `0x007EF3D4`. Reading the complete-object locator at
`vtable-4`, its TypeDescriptor, and name resolves `.?AVOverlayClass@@`. Load-bearing
slots are:

| Slot | Binding | Role |
|---|---|---|
| `+0x08` | `AbstractClass::Release @ 0x00410310` | returns one |
| `+0x20` | `OverlayClass` scalar deleting dtor `0x005FDF70` | physical finalization |
| `+0x28` | inherited `ObjectClass::PointerExpired @ 0x005F5230` | clears three exact pointer fields |
| `+0x44` | `ObjectClass::IsDead @ 0x005F6690` | true iff `IsAlive==0` |
| `+0xD4` | `ObjectClass::Limbo @ 0x005F4D30` | conceal/Mark-remove path |
| `+0xD8` | Overlay Unlimbo override `0x005FD270` | not used by this constructor |
| `+0xF8` | `ObjectClass::UnInit @ 0x005F65F0` | logical death/queue |
| `+0x124` | `OverlayClass::Mark @ 0x005FC570` | cell transaction |
| `+0x1AC` | always-false placement stub `0x004264C0` | base Unlimbo placement test |

### 2.2 Registries and queue

| Storage | Data/count | Join/leave order |
|---|---|---|
| Object registry | `0x00A8E364` / `0x00A8E370` | base ctor first; base dtor first after queue |
| Object pointer-expiration listeners | `0x00B0F724` / `0x00B0F730` | base ctor second; base dtor second |
| all-Abstract listener vector | `0x00B0F674` / `0x00B0F680` | base ctor third; base dtor third |
| Tag-removal listeners | `0x00B0F61C` / count at `0x00B0F628` | base ctor fourth; base dtor fourth |
| Overlay registry | `0x00A8EC54` / `0x00A8EC60` | after ID; Overlay dtor before base dtor |
| deferred-finalization queue | `0x00B0F69C` / `0x00B0F6A8` | UnInit append; drain removes all selected duplicates |

All constructor appends are best-effort DynamicVector operations. A growth failure
does not roll back earlier appends, the ID, or later construction. This can produce
partial registration under allocation pressure. A faithful robust Rust policy may
hard-fail the load, but it must not silently treat the row as a normal pre-
construction rejection.

## 3. Core native logic

### 3.1 Reader rejection and allocation arms

Before Overlay allocation, the authored identity body requires:

- a decoded non-`0xFF` byte;
- an OverlayType whose virtual image lookup succeeds or whose `CellAnim` pointer is
  non-null;
- no nonzero-game-mode crate rejection;
- the native radar-diamond cell admission.

These ordinary reader rejections perform no Overlay allocation, no constructor, no
ID, no registry append, no Mark, and no queue entry. Native has no safe bounds/null
guard around an out-of-range OverlayType index; that malformed-data crash surface
is not a rejection arm.

If `operator_new(0xB0)` returns null, the constructor is skipped and no Overlay ID
or registry side effect occurs. The reader still executes the four-high owner
restore check, which simply restores the unchanged saved anchor byte. It then
advances to the next row.

The constructor's Terrain-object precheck can skip Unlimbo after ID/registration,
leaving an alive limbo object unqueued. In an ordinary fresh authored load, the
`[Terrain]` section is read later, so the relevant ground list has no TerrainClass
blocker. This arm is present but excluded from the normal active reader context.

### 3.2 Exact constructor order

`OverlayClass__Constructor @ 0x005FC380` direct-calls
`ObjectClass__Constructor @ 0x005F3900`. The latter:

1. calls `AbstractClass__Constructor_Full @ 0x00410170`;
2. initializes Object fields, voice handles, and Object vtables;
3. attempts the four registry appends in the tabled order;
4. sets the Object-class Abstract flag bit used by pointer-expiration dispatch.

The derived constructor then installs all Overlay vtables and `+0xAC`, calls
`AbstractClass__AssignUniqueID(&this+4)`, and only then attempts the Overlay registry
append. In active Full_Init, the Scenario exists, so AssignUniqueID calls
`ScenarioClass__NextUniqueID @ 0x0068BCB0`. That function increments the signed
dword at `Scenario+0x214` first and returns the incremented bits. Destruction never
decrements or refunds it.

After registry insertion, the constructor sets the temporary construction-owner
global, performs the Terrain precheck, and—when clear—direct-calls base
`ObjectClass::Unlimbo @ 0x005F4EC0`. It deliberately does not call the Overlay
override at vtable `+0xD8`.

### 3.3 Base Unlimbo and base Mark

On the fresh path base Unlimbo clears `InLimbo`, clears redraw state, writes the
coordinate, and virtual-calls Mark with argument one. Overlay's derived Mark begins
by direct-calling base `ObjectClass::Mark @ 0x005F5850`.

Fresh base Mark sets `IsOnMap=1`, then `MarkNeedsRedraw @ 0x005F4D10` sets
`NeedsRedraw=1` and calls the tactical dirty helper with argument zero. If derived
Mark returns false, base Unlimbo restores only `InLimbo=1`; it does not restore
`IsOnMap`, redraw state, coordinate, ID, or registries. If derived Mark returns true
after UnInit has cleared `IsAlive`, base Unlimbo returns success before its Display
and Logic submissions.

### 3.4 Successful derived tail and child-Anim interleaving

Every derived branch reaching `0x005FD1FA` executes:

```text
CellClass::RecalcAttributes(anchor, -1)
overlay.IsOnMap = false
overlay.InLimbo = true
overlay.UnInit()
return true
```

For an ordinary type with non-null `CellAnim`, `OverlayClass::Mark` can allocate and
construct an ordinary `AnimClass` before this Recalc. Recalc can then construct and
latch a terrain-attached Anim if the current tile is eligible and the cell latch is
clear. Therefore a configured row's shared native-ID order can be:

```text
Overlay ID -> CellAnim ID -> terrain-tile Anim ID -> next Overlay ID
```

Either child branch can be absent. A previously latched terrain Anim suppresses the
second child. The synchronous ordering, rather than merely the final live Anim set,
is the contract.

### 3.5 Steep-slope survivor and consumer visibility

Assembly at `0x005FC5CD..0x005FC5E3` reads `Cell+0x11C`, compares it to four, and
branches directly to the false epilogue at `0x005FC784` when the type index is not
`0xB2`. It bypasses the common tail.

Final state after base Unlimbo handles that false result:

| Fact | State |
|---|---|
| `IsAlive` | `1` |
| `InLimbo` | `1` |
| `IsOnMap` | `1` |
| `NeedsRedraw` | `1` |
| coordinate | retained |
| native ID | consumed and retained |
| base/Overlay registries | retained where append succeeded |
| deferred queue | absent |
| Overlay cell identity/data | untouched by this row |
| Recalc | not called by this row |
| Display/Logic/current object | never joined |

Because base Unlimbo returns before Display/Logic, and derived Mark returns before
any Overlay cell write, the survivor cannot render. `Save_Game_Content_To_Stream @
0x0067D300` serializes Map/cell state and OverlayType objects but has no
`g_OverlayClass_Array` pass. `OverlayClass::Save @ 0x005FD950` has only its vtable
data reference. The live recording sum consumes `g_CurrentObjects`, which this
object never joins; the physically present Object ComputeCRC family has no active
per-frame sync enumerator. Final OverlayGrid rejection remains correct.

While alive, the survivor is visited by the Object pointer-expiration listener
dispatch. Its inherited handler at `0x005F5230` can only clear exact matches in
three Object pointer fields, all initialized null on this path. This is real
registry iteration but has no proved player-visible output. `FUN_00534450` /
`Clear_Scene` later scalar-deletes the object during scene teardown.

### 3.6 UnInit, generic counter-zero wall rejection, and queue-append failure

`ObjectClass::UnInit @ 0x005F65F0` performs, in order:

1. conditional Bomb/passenger work;
2. direct `DispatchPointerExpiredCleanup` while the object is still alive and
   registered;
3. virtual Limbo;
4. `IsAlive=0`;
5. best-effort append to the shared deferred queue.

On the normal Overlay success tail, Mark pre-set `InLimbo=1`, so Limbo returns
without Destroy/Mark-remove work. The object receives one pointer-expiration
broadcast at UnInit and another from its later Overlay destructor.

The Wall branch has a distinct post-construction rejection for callers with
`ScenarioInit==0`. If its placement predicate at `0x0047C620` fails, derived Mark calls virtual UnInit at
`0x005FC77C` while `InLimbo=0` and `IsOnMap=1`, then returns false. Limbo therefore
runs its full Destroy/Mark-remove corridor, including an additional pointer-expired
cleanup. The object becomes dead and queued, so the reader drain finalizes it. It
is not the steep-slope survivor arm. Successful authored `Full_Init` keeps the counter nonzero,
so `Is_Clear_To_Build` returns true before its ordinary body and this rejection cannot be selected
by an authored OverlayPack wall.

If queue growth fails, UnInit has already broadcast expiration and cleared alive
state, but the pointer is not present in the queue. The reader drain cannot discover
it through registries and therefore cannot finalize it. This native degraded/OOM
arm should be an explicit hard load failure in Rust rather than a silent normal
outcome.

### 3.7 Exact shared drain algorithm

`DrainDeferredFinalizationQueue @ 0x00725C70` uses a forward live index:

1. read the pointer at the current index;
2. virtual-call `IsDead`;
3. if alive, increment the index and leave the entry in place;
4. if dead, repeatedly find and stable-left-erase every occurrence of that same
   pointer;
5. call `Release`;
6. perform Building/Unit/Infantry/Aircraft RTTI restore checks;
7. virtual-call scalar deleting destructor with flag one;
8. do not increment the index; process the shifted successor;
9. re-read the current live count at the loop condition.

Overlay's Release returns one and it matches none of the four restore classes.
Thus every queued-dead Overlay selected by the drain is physically finalized.
Alive entries remain, but they do not stop later dead entries from being selected.

The drain is not a snapshot. Because count and data are live, a destructor or
callback that appends another queue entry can extend this same drain. This follows
directly from the assembly loop; no Overlay destructor in the ordinary path appends
a new entry.

### 3.8 Physical finalization and registry removal

The Overlay scalar destructor at `0x005FDF70`:

1. installs Overlay vtables;
2. dispatches pointer expiration again;
3. stable-erases its Overlay registry entry;
4. calls base Limbo when game-active (already-limbo Overlay rows early-return);
5. clears the type pointer;
6. calls `ObjectClass__Destructor @ 0x005F3B80`;
7. frees the allocation because the drain passed deleting flag one.

The base destructor stable-erases the first receiver occurrence from the deferred
queue, then removes it from the Object registry, pointer-expiration listeners,
all-Abstract listeners, and Tag listeners, in that order. The drain already erased
all selected duplicates, so its first queue erase is normally a no-op. It then
clears/detaches Object resources and conditionally unregisters Logic only if the
membership flag is set; authored successful Overlay objects never reached Logic.

No destructor path writes back, decrements, recycles, or otherwise refunds
`Scenario+0x214`.

### 3.9 Exact reader-drain boundary

Reader assembly proves this order:

```text
ClearSectionCache
if signed NewINIFormat > 1:
    optional positive-length OverlayPack identity traversal
    optional positive-length OverlayData traversal
temporary PixelBuffer cleanup
DrainDeferredFinalizationQueue  // call at 0x005FD692
return
```

Full_Init calls the reader at `0x00687A34`. Only after it returns does Full_Init run
`Network_ServiceLoop`, initialize the cell iterator at `0x00687A3E`, and call
`RecalcAttributes` at `0x00687A5A`. Therefore queued successful Overlay objects are
gone before the first whole-map sweep, while slope survivors remain.

When the format gate or both bodies are absent, the drain still processes every
dead object already in the shared queue. It is incorrect to implement it as
"finalize only the Overlay objects this reader created."

## 4. Retail activation and negative data authority

- Active retail authored maps use `NewINIFormat=4`, so the identity/data reader and
  this object lifecycle are ordinary active behavior when rows pass admission.
- The gate is a signed integer comparison against one, not presence of the two
  section keys.
- `[Tubes]` is parsed before OverlayPack and conditionally advances the same native
  ID stream once per actual Tube construction.
- Ordinary accepted `.SED` launch data defaults `NewINIFormat=0`; its reader has no
  authored Overlay objects but retains the common drain.
- No retail INI key configures the queue or registry order. These are compiled
  engine mechanics.
- OpenTS deferred deletion and Overlay construction were navigation leads only.
  No TS-only generated overlay, Vein, or tracker rule is imported.

## 5. State-transition table

| Row/path | ID/registry | Mark/cell | End of row | Reader drain |
|---|---|---|---|---|
| reader pre-admission reject | none | none | no object | none for this row |
| Overlay allocation null | none | none; high restore still checked | no object | none for this row |
| constructor Terrain blocker | ID + registries | no Unlimbo/Mark | alive, limbo, unqueued | not selected |
| common successful Mark | ID + registries; optional child IDs | cell writes + Recalc | dead, limbo, queued | finalized |
| generic wall placement false (`ScenarioInit==0`) | ID + registries | base Mark then full UnInit/Limbo; no wall stamp | dead, queued | finalized; not an authored Full_Init row |
| slope `>4`, non-`0xB2` | ID + registries | base Mark/dirty only; no cell/Recalc | alive, limbo/on-map contradiction, unqueued | survives |
| queue-growth failure | ID + registries | path-specific | dead, unqueued | cannot discover |
| generated default-format reader | no authored Overlay ID | no authored Mark | no Overlay object | still drains shared queue |

## 6. Current Rust correspondence and exact delta

### 6.1 What already matches or is reusable

- `src/sim/world/mod.rs::allocate_stable_id` provides one shared collision-free
  runtime handle namespace for modeled Object analogues.
- `src/sim/world/substrate.rs` owns an ordered `pending_delete` vector shared by
  multiple object stores.
- `src/sim/world/lifecycle.rs::process_pending_delete` already preserves alive
  entries, collapses all selected duplicate IDs, finalizes once, and processes the
  shifted successor.
- `src/sim/overlay_grid.rs::from_native_overlay_packs` rejects steep-slope identities
  from the final cell projection, which is presentation-correct.
- `src/sim/anim_class.rs` already stores `native_unique_id` separately in the Anim
  object shape, although it currently derives it from the runtime stable handle.

### 6.2 Verified mismatches

| Rust owner | Current behavior | Required delta |
|---|---|---|
| `src/map/resolved_terrain.rs` Overlay loop | applies raw/high projections before Simulation exists | drive the one synchronous authored transaction through a sim-owned effect sink with native-ID and registry ownership |
| `src/sim/overlay_grid.rs::from_native_overlay_packs` | final-cell filter/stamp only; no Overlay object life | consume only finalized cells; do not make OverlayGrid own ephemeral objects or render slope rejects |
| `src/app/loading/init.rs` | runs authored Overlay finalization before creating Simulation | create/borrow the sole load orchestrator early enough to own shared native-ID, registry, queue, and child Anim effects |
| `ObjectSubstrate::next_stable_object_id` | `u64`, next-to-return, saturating; collision-free handle | keep it as runtime handle; add one separate wrapping signed-dword native-ID cursor shared across every native constructor |
| `AnimClass::spawn_anim_at_world` | chooses RandomRate before runtime ID, then casts stable ID to native ID | allocate/register native identity before optional Scenario RandomRate, using the shared native cursor |
| terrain-tile Anim load | spawns only final descriptors after map entities | retain the separately verified per-Mark/first-sweep/final-sweep lifecycle and its ID interleaving |
| pending-delete drain | production owner runs at ordinary frame tail only | expose the same shared drain at the reader boundary, after OverlayData and before first sweep |
| object stores | no lightweight Overlay load object | represent successful queued Overlay objects and slope survivors without inserting either into `GameEntity`, occupancy, Logic, Display, or OverlayGrid |
| state hash | hashes runtime handle cursor and pending queue | hash the native cursor where future constructor IDs require deterministic continuation; do not invent native render/save membership for slope survivors |

The load-owned Overlay record can remain narrow, but it must be able to prove:

- native ID and collision-free handle are distinct;
- alive/limbo/on-map/redraw facts;
- ordered membership in the five constructor registries and shared queue;
- pointer-expiration and destructor event order;
- successful removal versus persistent slope survival.

It must not be represented as a normal `GameEntity`, because that would incorrectly
create occupancy, Display/Logic, save, or render authority.

### 6.3 Blocking pre-map prefix prerequisite

The present Rust load does not derive `C_saved`. The exact prerequisite must emit an
ordered native-ID-constructor trace from the fresh Full_Init seed through
`0x004AD026`:

```text
NativeIdSeed(1_000_000)
AssignUniqueID { class, type/source ordinal }  // every actual pre-map ctor
...
Snapshot(C_saved)
ReservePlus0x2710
Tube constructors in source order
Overlay/child-Anim constructors in native order
```

The trace must cover every actual HouseClass, native-ID-bearing TypeClass, and
CellClass construction in native order, plus any additional constructor found by a
fresh AssignUniqueID reachability pass. Map dimensions, House rows/session context,
and type-registry reconstruction are inputs. A fixed `1,000,000` or
`1,010,000` Overlay seed is disproved.

This prerequisite is required for absolute Overlay and Anim IDs. The focused
Overlay lifecycle can be tested with explicit `C_saved`, but the GSI mechanism may
not close until the integrated prefix produces it from the same current Rust load
inputs.

### 6.4 Cross-document consequence: RMG preview Cell prefix

The same open prefix owner affects the progressive RMG preview report. A fresh
check of `RandomMapGenerator::InitMapFromSyntheticINI @ 0x00599650` proves a
two-branch matrix after `ScenarioClass::Set_Defaults @ 0x00683610` resets the
native cursor at call site `0x00599B23`:

- When the prior storage snapshot exactly matches the requested dimensions, mode,
  and storage key, the zero flag branches at `0x00599BFB` around full cleanup and
  at `0x00599D2D` around Resize. The later iterator resets existing Cell payloads;
  no `CellClass` constructor advances the newly reset cursor on this branch.
- When that snapshot is missing or any key differs, the flag remains one. The path
  calls `FUN_00534450` at `0x00599C62`, then calls the Resize wrapper at
  `0x00599D48`; wrapper `0x00653F50` directly calls `MapClass::Resize @ 0x00565C10`
  at `0x00653F64`.

On the ordinary successful-heap changed/missing path, Resize constructs the real
Size-diamond cells in row-major order. Each admitted empty slot calls
`CellClass::Constructor @ 0x0047BBF0` at `0x005663D6`; an already populated
admitted slot is re-constructed in place at `0x005663FC`. The constructor calls
`AbstractClass::Constructor_Full` first at `0x0047BBF6`, initializes its fields,
installs all four Cell vtables, clears both embedded arrays, and only then calls
`AbstractClass::AssignUniqueID(this+4)` at `0x0047BD8F/0x0047BD90`. Resize also
unconditionally reconstructs its shared dummy Cell at `0x005670F2`, consuming one
more ID. Thus a changed/missing preview consumes one ID per real Cell plus one dummy
Cell after the reset; a matching-key preview consumes none of those new Cell IDs.

Consequently, the preview report's absolute `first Building = 1,000,001` oracle is
valid only for a branch whose complete post-reset prefix has independently proved
that no constructor intervenes. It is disproved for changed/missing storage: even
before later preview objects, the cursor has advanced by all real Cell constructors
and the dummy Cell constructor. The exact first-Building value belongs to the same
focused constructor-prefix prerequisite (and must include any non-Cell constructor
between Resize and Building placement); it is not safe to infer it from
`Set_Defaults` alone.

## 7. Coverage ledger

| Surface | Verification | Status |
|---|---|---|
| `ReadMapOverlayPacks 0x005FD2E0` | full decompile, tail assembly, Full_Init call order | closed |
| Overlay vtable/COL | raw vtable, COL, TypeDescriptor, load-bearing slots | closed |
| `OverlayClass` ctor `0x005FC380` | full decompile/disassembly | closed |
| `ObjectClass` ctor `0x005F3900` | full decompile, registry order | closed |
| Assign/Next ID `0x00410230/0x0068BCB0` | decompile/disassembly semantics | closed |
| base Unlimbo/Mark/redraw | full decompile and branch order | closed |
| derived Mark slope/wall/common tails | full decompile, assembly anchors | closed |
| CellAnim before Recalc | derived Mark body | closed |
| terrain Anim within Recalc | fresh integration check plus focused current report | closed for ordering |
| UnInit `0x005F65F0` | full decompile | closed |
| Limbo/Destroy distinction | full decompile | closed |
| drain `0x00725C70` | full decompile/disassembly | closed |
| Overlay/Object destructors | full decompile | closed |
| slope render/Logic/cell negative | branch and base-Unlimbo return order | closed |
| slope save negative | active top-level save enumeration plus Overlay Save xrefs | closed |
| slope live hash negative | current-object and ComputeCRC consumer evidence | closed for active path |
| generated no-Mark/shared drain | format/provenance plus reader common epilogue | closed |
| post-reservation first-ID transform | map-read, Tubes, Full_Init order | closed relative to `C_saved` |
| preview matching-vs-rebuild Cell prefix | branch assembly, Resize wrapper/call sites, Cell ctor order | closed for Cell contribution |
| absolute `C_saved` constructor prefix | only variable sources proved; full order/count not enumerated | **blocking open** |
| OOM partial-registry exact heap reproduction | native degradation identified | deliberately hard-error policy candidate |

## 8. Open-question log

### Resolved in this investigation

1. `[RESOLVED]` Does AssignUniqueID insert into a global registry? **No; it only
   preincrements/stores the ID.**
2. `[RESOLVED]` Do base registries join before or after the ID? **All four attempts
   precede it.**
3. `[RESOLVED]` Does the Overlay registry join before or after the ID? **After.**
4. `[RESOLVED]` Does the constructor use the Overlay Unlimbo override? **No; it
   direct-calls base Object Unlimbo.**
5. `[RESOLVED]` Does a successful row destruct inline? **No; UnInit queues it.**
6. `[RESOLVED]` Is the queued success object still registry-visible to the next
   row? **Yes, through identity and data traversal until the common drain.**
7. `[RESOLVED]` Can child Anim IDs occur before the next Overlay ID? **Yes; CellAnim
   precedes common Recalc and first-eligible terrain Anim can occur inside Recalc.**
8. `[RESOLVED]` Does UnInit's Limbo repeat Mark removal on common success? **No;
   derived Mark pre-sets InLimbo.**
9. `[RESOLVED]` Does slope rejection call UnInit? **No.**
10. `[RESOLVED]` Does base Unlimbo roll back OnMap/redraw/ID/registries after slope
    rejection? **No; only InLimbo is restored.**
11. `[RESOLVED]` Is the slope survivor queued or dead? **Neither; it is alive and
    absent from the queue.**
12. `[RESOLVED]` Does the slope survivor render? **No; it joins no cell list,
    Display, Logic, or current-object list and writes no Overlay cell.**
13. `[RESOLVED]` Is the slope survivor saved by the active class-array stream?
    **No OverlayClass instance pass exists.**
14. `[RESOLVED]` Is generic counter-zero wall placement failure the same as slope failure? **No;
    it UnInits, fully Limbos, dies, queues, and drains. Successful authored Full_Init cannot select
    it because ScenarioInit short-circuits the predicate true.**
15. `[RESOLVED]` What happens if queue append growth fails? **Death/cleanup remains
    committed but the object is unqueued and undiscoverable by the drain.**
16. `[RESOLVED]` When does the reader drain run? **After identity, data, and temp
    cleanup; before reader return and first whole-map sweep.**
17. `[RESOLVED]` Does the drain run on `NewINIFormat<=1`? **Yes.**
18. `[RESOLVED]` Does the drain run with missing/empty pack bodies? **Yes.**
19. `[RESOLVED]` Does generated default-format reader execute authored Mark? **No.**
20. `[RESOLVED]` Does generated default-format reader still drain? **Yes.**
21. `[RESOLVED]` Is the drain Overlay-only? **No; it consumes all queued-dead
    shared Object-derived entries.**
22. `[RESOLVED]` Does one alive queue entry stop later dead entries? **No.**
23. `[RESOLVED]` Are duplicate dead entries destructed multiple times? **No; all
    duplicates are erased before one finalization.**
24. `[RESOLVED]` Is the queue a snapshot? **No; the loop rechecks live count.**
25. `[RESOLVED]` When are registries removed? **Overlay registry in derived dtor,
    then queue/Object/listener registries in base dtor.**
26. `[RESOLVED]` Is the native ID refunded? **Never.**
27. `[RESOLVED]` Does map-read reserve `+0x2710` before Overlay IDs? **Yes,
    unconditionally.**
28. `[RESOLVED]` What loader-owned constructors lie between reservation and first
    Overlay? **One TubeClass per successfully constructed `[Tubes]` row.**
29. `[RESOLVED]` Is `C_saved` always `1,000,000`? **No; pre-map House/Type/Cell
    constructors advance it.**
30. `[RESOLVED]` Does ordinary Full_Init construct Cells before the map-read
    snapshot? **Yes; `Read_Map_Section_And_IsoMapPacks` invokes Map Resize through
    vtable `+0x70` at `0x004ACF0D`, before `0x004AD026`.**
31. `[RESOLVED]` Does preview `Set_Defaults` imply the next Building receives
    `1,000,001`? **Not generally. Exact-match storage skips Cell construction, but
    changed/missing storage calls Resize after the reset and consumes one ID per
    real Cell plus the dummy Cell before later preview objects.**

### Bounded-open / blocking

1. `[DEFERRED-BLOCKING]` Enumerate every pre-`0x004AD026` AssignUniqueID constructor
   count and order from current authored-load inputs. This is a focused prerequisite
   for the absolute ID oracle, not permission to approximate.
2. `[DEFERRED-NONPARITY-POLICY]` Choose and document hard-load-error behavior for
   native heap/vector growth failures. Reproducing partial registration and null-
   relative crashes is not recommended, but silent omission is forbidden.
3. `[DEFERRED-LOW-IMPACT]` Human-readable names for the two non-Tag ancillary
   listener vectors are not needed for order or behavior; addresses and dispatcher
   roles are exact.

The deferred share is 3/34 and only item 1 blocks the absolute GSI closure.

## 9. Implementation handoff

### 9.1 Required behavioral contract

1. Add one wrapping signed-dword native-ID cursor distinct from the collision-free
   Rust runtime handle allocator. It must be shared by every native constructor.
2. Treat `C_saved` as an upstream cursor value, apply the unconditional `+0x2710`
   reservation, then Tube and Overlay/Anim preincrements in exact order.
3. Execute one lightweight Overlay object construction for every reader-admitted,
   successfully allocated authored row. Do not allocate for reader rejects.
4. Model base registry joins before native ID and Overlay registry join after it.
   A hard error may replace native registry-growth degradation.
5. Run base Mark before every derived branch. Preserve the one tactical dirty event
   even for the later steep-slope rejection.
6. On common success, perform all cell/child-Anim/Recalc effects synchronously,
   then set OnMap false/Limbo true, UnInit, clear alive, and append without duplicate
   suppression.
7. Keep queued successes resolvable in all registries through the next rows and
   OverlayData. Child Anim IDs must precede the next Overlay ID.
8. On slope rejection, retain an alive, limbo/on-map, unqueued lightweight survivor
   with consumed ID and registry membership. Do not place it in OverlayGrid,
   GameEntity, occupancy, Display, Logic, rendering, or the native save projection.
9. On an authored wall that passes the slope gate, execute wall success and then the common
   UnInit/dead/queue lifecycle. Do not route authored loading through generic counter-zero wall
   rejection; preserve that generic rejection lifecycle for its ordinary runtime callers.
10. Invoke the shared deferred drain once after OverlayData even when the format gate
    or bodies are absent. It must run before the first whole-map Recalc.
11. Drain all shared queued-dead objects in stable live order, preserve alive entries,
    collapse all selected duplicates, and finalize once.
12. Remove Overlay registry before the four base registries during physical
    destruction. Never refund native IDs.
13. Generated default-format reader performs zero Overlay constructions/Marks/IDs
    but still invokes the shared drain.
14. Keep final OverlayGrid as sole Overlay presentation authority; lifecycle records
    are not render objects.
15. Leave the mechanism/GSI row open until the upstream pre-map constructor-prefix
    trace produces exact `C_saved` from current Rust inputs.

### 9.2 Focused acceptance tests

1. **Constructor and prefix-relative ID order.** Seed explicit `C_saved`, configure
   two Tube rows, and admit one Overlay. Assert reservation, two Tube IDs, four base
   registry joins, Overlay ID, then Overlay registry join. Assert the first Overlay
   formula exactly.
2. **Success/child/slope/next-row fixture.** Admit a success with CellAnim and an
   unlatch eligible terrain Anim, then a steep-slope row, then another success.
   Assert exact ID chain, registry state at every next-row boundary, slope survivor,
   and zero slope cell/render/Logic membership.
3. **Data-before-drain fixture.** Observe dead successful Overlay pointers still in
   all registries during every later identity row and the entire data pass; assert
   their destructor/removal happens only in the common epilogue.
4. **Authored-wall-versus-slope fixture.** Keep ScenarioInit nonzero, admit one slope-accepted wall,
   then a slope failure. Assert wall success effects followed by the common two-broadcast
   UnInit/death/queue/finalize lifecycle versus slope alive/unqueued survival. Keep a separate
   counter-zero unit fixture for the generic wall three-broadcast/full-Limbo rejection path.
5. **Mixed shared queue fixture.** Seed `[alive A, dead B, B, alive C, dead D]`.
   Assert A/C remain in place, B duplicates collapse before one destructor, D also
   finalizes, and shifted successors are processed without skipping.
6. **Gate/body absence fixture.** Seed one dead shared object, use
   `NewINIFormat=1`, then separately format 4 with absent/empty identity and data.
   Assert the common drain still runs before the first sweep.
7. **Allocation/rejection fixture.** Reader rejects and Overlay allocation-null
   consume no IDs/registries; high allocation-null still performs a no-op restore;
   post-constructor slope rejection consumes and retains its ID.
8. **Queue-growth policy fixture.** Force the Rust hard-error policy at queue append
   or prove the exact dead-unqueued degraded record. Silent normal completion is
   forbidden.
9. **Generated negative fixture.** Default-format `.SED` reader produces no
   Overlay object/Mark/dirty/ID, but drains a seeded shared dead entry.
10. **Presentation/save negative fixture.** A slope survivor does not appear in
    final OverlayGrid, render manifest, Logic/occupancy, current-object checksum, or
    native save-class enumeration; its native ID cursor effect remains.
11. **Integrated absolute prefix prerequisite.** After the focused pre-map report,
    trace every pre-snapshot native-ID constructor from seed `1,000,000`, assert the
    exact `C_saved`, then assert absolute Tube/Overlay/Anim IDs. This test is required
    before the owning GSI row closes.
12. **Preview prefix branch matrix.** After `Set_Defaults`, assert that matching-key
    storage reuses Cells without constructor IDs, while changed/missing storage
    emits one row-major ID event per real Size-diamond Cell plus the final dummy-Cell
    event before any later preview object. The absolute first-Building fixture must
    be derived from that full trace rather than hard-coded to `1,000,001`.

### 9.3 Design/contract corrections

- Replace “AssignUniqueID and insert into global registry” with the exact split:
  **four base registry attempts -> native ID -> Overlay registry attempt**.
- Replace “ephemeral Overlay is immediately destroyed” with:
  **successful rows are deferred until the common post-data drain; steep-slope rows
  survive alive and unqueued until scene cleanup**.
- Keep “rejected row must not render” and final OverlayGrid sole presentation
  authority. The slope survivor proves lifecycle/ID state, not cell presentation.
- Replace any Overlay-only or body-conditional drain with the unconditional shared
  reader epilogue.
- State next-row ordering explicitly: previous success objects remain registered,
  and all synchronous child Anim IDs occur before the next Overlay ID.
- Do not initialize authored native IDs from `1,000,000` or `1,010,000`. The design
  must accept the variable pre-map `C_saved` from one common constructor-prefix
  owner, apply `+0x2710`, then Tubes.
- Do not reuse preview `first Building = 1,000,001` as a universal reset oracle.
  Matching-key storage skips Resize, while changed/missing storage consumes all
  real-Cell IDs and the dummy-Cell ID after `Set_Defaults`.
- Do not use saturating `u64` runtime stable IDs as the native numeric counter. Keep
  collision-free handles separate from wrapping signed-dword identity.
- A load-local shared drain may reuse the general pending-delete algorithm, but it
  must run at the reader boundary rather than waiting for the first gameplay frame.

## Sources

### Native functions and data

- `ScenarioClass::Full_Init @ 0x00686B20`
- `RandomMapGenerator::InitMapFromSyntheticINI @ 0x00599650`
- `ScenarioClass::Set_Defaults @ 0x00683610`
- `Read_Map_Section_And_IsoMapPacks @ 0x004ACE70`
- `MapClass::Resize @ 0x00565C10`
- Resize wrapper `0x00653F50`
- `CellClass::Constructor @ 0x0047BBF0`
- `MapClass::ReadTubesINI @ 0x007283C0`
- `ReadMapOverlayPacks @ 0x005FD2E0`
- `OverlayClass::Constructor @ 0x005FC380`
- `ObjectClass::Constructor @ 0x005F3900`
- `AbstractClass::Constructor_Full @ 0x00410170`
- `AbstractClass::AssignUniqueID @ 0x00410230`
- `ScenarioClass::NextUniqueID @ 0x0068BCB0`
- `ObjectClass::Unlimbo @ 0x005F4EC0`
- `ObjectClass::Mark @ 0x005F5850`
- `ObjectClass::MarkNeedsRedraw @ 0x005F4D10`
- `OverlayClass::Mark @ 0x005FC570`
- `CellClass::RecalcAttributes @ 0x0047D2B0`
- `ObjectClass::UnInit @ 0x005F65F0`
- `ObjectClass::Limbo @ 0x005F4D30`
- `ObjectClass::Destroy @ 0x005F5280`
- `ObjectClass::IsDead @ 0x005F6690`
- `DrainDeferredFinalizationQueue @ 0x00725C70`
- `OverlayClass` scalar deleting destructor `0x005FDF70`
- `ObjectClass::Destructor @ 0x005F3B80`
- `ObjectClass::PointerExpired @ 0x005F5230`
- `DispatchPointerExpiredCleanup @ 0x007258D0`
- `Save_Game_Content_To_Stream @ 0x0067D300`
- `OverlayClass::Save @ 0x005FD950`
- `Clear_Scene @ 0x006851F0`
- global registry/queue addresses listed in section 2.2
- Overlay vtable `0x007EF3D4`, COL `0x00807638`, TypeDescriptor `0x00833458`

### Retail data and prior integration reports

- active retail `rulesmd.ini`, `artmd.ini`, theater data, and authored format-4 map
  corpus inspected by the sibling focused reports below
- `AUTHORED_OVERLAYPACK_INLINE_TRANSACTION_REINVESTIGATION_GHIDRA_REPORT.md`
- `AUTHORED_OVERLAY_WALL_SCENARIOINIT_ACCEPTANCE_REINVESTIGATION_GHIDRA_REPORT.md`
- `TERRAIN_ATTACHED_ANIM_LOAD_LIFECYCLE_SIDE_EFFECTS_REINVESTIGATION_GHIDRA_REPORT.md`
- `AUTHORED_MARK_LOAD_CONTEXT_SOURCE_PROVENANCE_REINVESTIGATION_GHIDRA_REPORT.md`
- `LOW_OVERLAY_MARK_FIXED_MAP_STAMP_RNG_TRANSACTION_GHIDRA_REPORT.md`
- [OVERLAY_CLASS_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OVERLAY_CLASS_SYSTEM_GHIDRA_REPORT.md) (navigation/staleness comparison only)
- [OBJECTCLASS_UNINIT_DEATH_CLEANUP_ORDERING_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECTCLASS_UNINIT_DEATH_CLEANUP_ORDERING_RESWARM_20260528.md)
- [PENDING_DELETE_DRAIN_DESTRUCTOR_TIMING_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PENDING_DELETE_DRAIN_DESTRUCTOR_TIMING_RESWARM_20260528.md)
- `RMG_PREVIEW_ANIM_BUILDING_IDENTITY_LIFECYCLE_REINVESTIGATION_GHIDRA_REPORT.md`

### Current Rust inspected

- `src/map/resolved_terrain.rs`
- `src/sim/overlay_grid.rs`
- `src/sim/world/mod.rs`
- `src/sim/world/substrate.rs`
- `src/sim/world/lifecycle.rs`
- `src/sim/anim_class.rs`
- `src/sim/runtime.rs`
- `src/app/loading/init.rs`
- `src/sim/world/world_hash.rs`

## Stale or misleading findings encountered

1. “Overlay placement calls the Overlay Unlimbo override” is wrong for this
   constructor. It direct-calls base `ObjectClass::Unlimbo`.
2. “AssignUniqueID registers the object globally” is wrong. Registration and ID are
   separate operations with base registries before and Overlay registry after.
3. “OverlayClass is ephemeral, so all instances are gone when Mark returns” is
   wrong. Successes remain queued/registered through OverlayData; slope rejects
   survive much longer.
4. “A rejected Mark has no lifecycle side effects” is wrong for post-construction
   rejection. Slope consumes ID/registries and leaves OnMap/redraw state; generic counter-zero wall
   failure queues and destructs. That generic wall rejection is not authored-Full_Init behavior.
5. “The reader drains only the Overlay objects it just created” is wrong. It invokes
   the shared global drain even when no Overlay body runs.
6. “Generated no-Mark means no reader finalization boundary” is wrong. The common
   drain remains active.
7. “The slope survivor's `IsOnMap=1` means it can render” is wrong. It never joins
   cell, Display, Logic, or current-object presentation lists.
8. “Fresh Full_Init Overlay IDs begin at 1,010,001” is wrong. The reservation is
   relative to a variable `C_saved`, and Tube constructors intervene before
   OverlayPack.
9. “Preview `Set_Defaults` means the first Building is always `1,000,001`” is
   wrong. Changed/missing storage reconstructs the real Cells and dummy Cell after
   reset; only the exact-match branch skips that Cell prefix.

## Final zero-add pass

After the inventory was reopened for slope consumer visibility, the map-read ID
prefix, and the preview cross-document consequence, fresh decompile/disassembly
passes were repeated over the reader,
Overlay/Object constructors, AssignUniqueID/NextUniqueID, base Unlimbo/Mark,
derived Mark slope/wall/common tails, UnInit, shared drain, both destructors,
Full_Init ordering, map-read reservation, Tubes, preview storage branches,
Map Resize/Cell construction, save enumeration, and pointer-expiration dispatch.
The pass added the variable-`C_saved` blocking prerequisite and its preview branch
matrix, then produced no further material lifecycle questions.

The later authored-wall report reopened only the reachability classification: it proves the generic
counter-zero wall-rejection mechanics remain active but cannot occur during successful authored
Full_Init. The lifecycle facts above are retained under that corrected caller boundary.
