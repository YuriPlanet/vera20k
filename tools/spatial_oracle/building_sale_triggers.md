# Building sale completion and live Tags

This oracle executes the original non-undeploy completion from `0x44A1E8`
through `RecordKill(NULL)`, live Tag/Trigger actions, refund and payment. It
stops at `0x44A22C`, before the following Limbo call. It does not execute the
preceding Selling mission frames, build-down animation, crew creation or the
subsequent teardown. The existing `building_sale` oracle owns those separate
bounded observations; the Rust campaign integration test supplies an ordinary
SELL order and advances the whole world.

The original completion first writes `-1` to Building `+0x53C`, then invokes
virtual `+0xE0` (`0x702D40`). That call reaches actual Tag event 48 and, when a
live Tag remains, event 29. The NULL damage source excludes event 7. No callback
result, Tag state, refund, payment or owner transition is supplied by Python.
The refund virtual at `0x44A215` and the owner load at `0x44A21B` run after all
those callbacks. The action-14 control uses the real dynamic Techno scan and
Building/Techno ChangeOwner bodies, with a distinct target-country building
cost factor read by the original `0x511BC1..0x511BE0` reader.

## Inputs and observation boundary

Each row clones the shared physical Rules/ART reader VM, constructs a fresh
Building with `0x43B740`, and supplies an admitted, marked, alive, nonlimbo
GAPOWR in a clear 32-by-32 map. Original GrandOpening `0x445F80` runs before
the comparison, retaining `actually_placed=true`, damaged mode false and
the real GAPOWR_A in animation slot 3. The prepared Selling mission/status is 19/2;
the ordinary RecordKill comparison instead supplies Guard. Map cells have no
tile image (`TileIndex=-1`), supplied visible flags and the declared building
footprint. House lists/tracking counters and empty FactoryPlant membership
are supplied prior state; the existing original House factory-cost producer
computes neutral factors. This is not a native map-load or placement proof.

The native physical type reader supplies GAPOWR Strength 750 and Cost 800.
Both controlled Houses are human. OLD is player-controlled; NEW is not.
The runtime mode is 1. NEW's synthetic `CostBuildingsMult=0.75` is an explicit
input; original country construction and the exact scalar reader establish
its stored binary32 value. Per-row `native_inputs` records the measured type,
House and country fields used by the refund. No FactoryPlant, storage or
health-based price adjustment is introduced by the harness.

TriggerType, TagType, TEvent and TAction definitions pass through their original
constructors and counted INI readers. Actual Tag/Trigger construction and
Object Tag attachment execute. A small shared registry initializer supplies
capacity for the new Tag registries; it never resets the live Object/Techno
arrays created by the Building constructor. The Engineer fixture's deliberate
Tag-NULL assertion is bypassed only at the Tag entry. EIP, registers, stack
and native code remain untouched there.

Rows report ordered native call observations and the actual `+0x53C` write,
compact state before/after, native identity/allocation deltas, and complete
Main/Scenario/Mapgen RNG-state equality. A repeating Tag can execute an already
completed trigger again on the later event; a one-shot Tag can detach before
the later event. These are original results, not host dispatch rules.

The action-32 control executes the original attached-object scan, actual
physical C4/Super binding, Building ReceiveDamage and destruction effects.
It reaches zero health but retains `IsAlive=1` at this Selling-timer boundary;
no callback UnInit or destructor is observed. The outer sale still computes
and pays the refund. Its debris/explosion Anim constructors and RNG/identity
changes are retained as native evidence. `rust_comparison` explicitly marks
this row `native_only_selling_death`: the narrower Rust consumer does not
claim a full fatal C4/debris/ART comparison, and the row is not evidence of
a current Rust death-pipeline defect.

The physical ALL01 row uses the exact selected records for Tag `0BE0AB4C`,
trigger `061FF03C` (`SpySat_Boom`) and its linked trigger `0BE0B26C`, read from
`mapsmd03.mix` member `ALL01UMD.MAP`. It attaches those records to the declared controlled GAPOWR
and stops **before** the first `TAction::Execute` at `0x6DD8B0`, whose kind is
51 (Reshroud). That row proves the original sale-to-authored-action prefix;
it does not execute Reshroud, reveal waypoint 496, self-disable, refund or
payment. The actual-map Rust test separately uses the authored GASPYSAT at
(85,110), ordinary SELL/build-down and actions 51, 18 and 54. Its full-world
result is not a native whole-map or whole-match RNG comparison.

## Retained loss field

The original Building constructor initializes `+0x53C` to zero (`0x43B783`),
which this oracle observes. Separate static instruction inspection identifies
InitManagers `0x442C5C` as the current owner's Country-index producer and
nonnull-source ReceiveDamage `0x44298B` as the attacker's House-index writer.
Sale completion writes `-1`; RecordKill `0x70304D` and the House-source variant
`0x7033C3` test it before incrementing BuildingsLost. This is retained state,
not an inferred property of the current Selling mission.

The literal displacement census is not a proof that no alias writes exist.
The static save/load audit follows BuildingSave `0x454190` through the raw
Abstract serialization of the virtual size (`0x459E70` returns `0x720`) and
BuildingLoad `0x453E20` through raw restoration. The following NoInit
constructor `0x43B680` does not rewrite `+0x53C`. No native save/load roundtrip
is claimed by this corpus; Rust snapshot tests cover its retained authority.

## Replay and integrity

Use the existing Engineer joined-input directory containing the physical
Rules/ART/sound files and required assets. Set `VERA20K_GAMEMD_EXE` and
`VERA20K_ENGINEER_REPAIR_ASSETS`, then run:

```text
python -m tools.spatial_oracle.building_sale_triggers --check
```

`--write` is the explicit reference-writing operation. Import and `--help`
remain inert. Each row checks all executable PE sections against the loaded
original bytes and the declared Building/House vtable ranges. The measured
`native_image_sha256` identifies the replay input; it is provenance and is not
an acceptance gate for a particular build. The metadata also records the
canonical payload digest. Shared helper source hashes are not persisted as
acceptance requirements.

The inherited harness answers allocation/free, TLS, interlocked operations,
OS audio/clock and COM platform services. Construction's presentation leaves
`0x451F60`, `0x452170`, `0x456FB0` and `0x705D70` retain their documented
boundaries. The emitted per-row boundary list identifies those actually
reached. RecordKill, Tag/Trigger dispatch, actions, ChangeOwner, refund and
AddCredits execute original instructions; they are not presentation hooks.
