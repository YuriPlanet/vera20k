# Engineer bridge-hut action, object click and display cursor

The [caller harness](engineer_bridge_cursor_caller.py), [122 native comparison rows](engineer_bridge_cursor_caller.json)
and [provenance](engineer_bridge_cursor_caller.meta.json) execute retail
`gamemd.exe` instructions. They establish the caller's hut admission, action
arithmetic, object-click composition and selected display cursors, with the geometric `587410` answer
supplied explicitly. They do not establish the geometric answer or complete repair.

```sh
python -m tools.spatial_oracle.engineer_bridge_cursor_caller --check
```

The checked run reproduces **34 action-branch rows, 26 object-click rows, four
display-dispatch rows, 24 paired controllability/click controls (48 comparisons)
and ten Undeploy action controls**. Eleven original launch-count preflights
also retain their native return values.
The additive prerequisite packet retains **80 Foundation comparisons** and all
22 original table names/widths/heights. Constructor writes and six sequential
boolean-reader layers establish Building `Repairable=true`,
`BridgeRepairHut=false` and Infantry `Engineer=false` defaults. Those packet
sections supply type fields and INI caches; they do not execute the complete
physical rules loader or certify every key in its type readers.
Every row verifies unchanged mapped `.text`; action/click rows also verify unchanged
fixture object/type/house bytes. The original 64 action/click/display rows are retained unchanged.
The metadata also pins the harness and shared native helpers. No Rust output
determines a golden.

Building constructor `45DF13` writes Foundation index0. Its reader
`461225..46125D` makes two calls to `ReadFoundation474DA0` against ART
`887180`: effective RULES `Image` first, stored type ID second. The first call
receives the existing index as default; the second receives the first result.
The first result is always stored, and the second replaces it only when nonzero.
Consequently an explicit type-ID `1x1` cannot replace a nonzero image Foundation.
Unknown present names resolve0; missing reads retain their supplied default.
Both section and `Foundation` key are exact-case, while the 22 name-table
comparisons ignore ASCII case. Native `char[32]` copy/CRT trimming controls
include the31-byte content cap and Latin-1 NBSP, which does not count as trim.
The oracle supplies raw cached values for those string-reader controls; authored
text undergoes the separate INI lexical trim before entering the cache.

RULES `Foundation` is a poison control, not a native input. Effective Image
comes from ObjectType `5F9320..5F933B`, a25-byte RULES read into Type+1F8;
the later visual ART `Image` read uses a separate local buffer. Rust's shared
Foundation reader and retained pass owner are compared with this packet in
`rules::native_processing::tests::building_foundation_pass_matches_original_art_reads`,
through rules projection, ART installation and the Undeploy consumer.

Object-route entry is the interior `51E49E` after upstream WhatAction/modifier
processing. It runs original Engineer `InfantryType+EC3`, Building RTTI,
actor locality `50B6F0`, Building `+80`/`457620→465D40`, Repairable `Type+CCC`,
and hut `Type+16B6` gates. Ordinary supplied CABHUT has no UndeploysInto.
The query uses the target GetCoords result and native signed truncation by
256, followed by packed-word narrowing. GetCoords itself is a declared seam.
The four coordinate controls cover fractional positive/negative leptons and
packed-coordinate extremes; they are arithmetic controls, not stock positions.

Cell-route entry is interior `51F9F4`, after supplied Building discovery.
It requires Engineer, discovered Building and actor locality. The hut branch
queries before its non-hut Repairable test, so `Repairable=false` suppresses
the object-route branch but still reaches the cell-route hut query.

For every admitted object/cell case, query false produces action **32**;
query true produces action **29**. Both arms terminate their selected hut
branch without falling through to ordinary capture/repair predicates.
For object route the actual arithmetic is `51E551..51E55A`; the cell twin is
`51FA75..51FA7B`. Target-owner friendship is not an admission gate: native
`4F9A90` establishes each fixture's self/allied/hostile relation in a separate
preflight, and the measured hut branch never calls it. All three relations
reach identical query/action decisions.

Original actor locality is mode-dependent: `50B6F0` reads campaign actor
House `+1EC/+1ED` when game mode is zero; otherwise it compares the actor House
with `PlayerPtr`. The rows include nonlocal rejection and both campaign flags.
Rejected cases stop before the later ordinary WhatAction continuation; the
harness does not invent its final action.

The object-click rows execute original `51F190` with the preceding native
action supplied at its WhatAction call boundary. Its original tables
`51F20C/51F1FC` map action29 to action9 and preserve action32. Original
`4D74E0` maps action9 to `4D7716`, runs original `700C40` IsControllable, then
reaches the actual `6FFBE0` QueueMegaMission entry with **mission8 Capture,
target=null, destination=hut, fourth argument=0**. Action32 returns true with
no queue call; fixture bytes remain unchanged. Signed positive EMP `+504`
suppresses the action29 queue through the original order-admission receiver.
Supplied zero flags/links and stopped timer start=-1/duration=0 bound the
other order-admission branches.

The additive Undeploy controls supply `BuildingType+408` and foundation index
`+EF0`, then execute Building virtual `+80` (`457620→465D40`) through the
object hut branch. Original `465D40` first requires a nonnull UndeploysInto
pointer, then compares both original foundation tables (`8192B8` widths,
`819310` heights) with1. Index0 is1x1, index1 is2x1, index2 is1x2 and index3
is2x2; their original dword bytes are retained in the JSON. Only a nonnull
pointer with1x1 prevents the object hut query at `51E4E1`; a null pointer,
width2 or height2 still reaches it. The paired cell route has no such gate
and queries in all five controls.

The paired controllability controls execute full original `700C40` and the
`51F190→4D74E0` object-click route with action29 supplied from the previously
established hut result. The original Infantry vtable `7EB058+A0` points to
`700C40`; its stale Ghidra name IsIdleForAutoTarget and the former harness
name CanAcceptOrder do not identify a different function. Its gates run in
this order:

| Native check | Original receiver or field | Control result |
| --- | --- | --- |
| Actor locality | `50B6F0`, actor House pointer `+21C` | Earlier action/click rows retain mode/locality coverage |
| EMP | virtual `+37C→70EFD0`, signed `+504>0` | Positive refuses; negative allows |
| Bunker | pointer `+2E4` | Nonnull refuses |
| Spawned | virtual `+84→6F3270→+88`, Type byte `+D54` | True refuses |
| Paralysis | virtual `+380→4DE770`, timer `+6A0/+6A8` | Pending, future-start and stopped nonzero refuse; expired allows |
| Warp | virtual `+1E0→70C5F0`, bytes `+270/+271` | Either byte refuses |
| Slave | pointer `+2DC` | Nonnull refuses |
| Offline latch | byte `+1C8` | Supplied positive native-save control refuses |
| Launched missiles | manager `+2D0→6B7D80`, Type `+D5C` | Positive partial count refuses; zero or full count allows |

Every refused gate prevents the object-click queue boundary; every allowed
control reaches mission8 with null target and hut destination. These are
pre-enqueue player controllability checks. The synchronized event's separate
actor/target lifecycle admission is not this receiver, and `700C40` itself
adds no health, object-alive or limbo check.

The launch-count controls run original `6B7D80` both in a separate preflight
and inside the native gate. Supplied manager `+3C/+48` and slot `+0/+4`
establish the vector/state inputs. State1 counts; state2 counts only with a
nonnull, nonlimbo child whose original type getter returns `MissileSpawn`
byte `+D68=true`. Ready, null-child, limbo and nonmissile controls return0.
Counts0,1,2 and capacities1,2,3,-1 are retained; signed capacity-1 admits
count1. Child Infantry vtables and supplied type fields are control fixtures,
not a claim that retail constructs missile-spawning Infantry.

Ordinary Infantry EMP/offline initialization and producer bounds are
**instruction evidence**, separate from the executed gate controls. Original
Techno constructor `6F2B4B` zeroes EBX; `6F3112` writes `+504=0` and `6F2C9A`
writes `+1C8=0`. EMPulse Apply `4C575E..4C5787` admits RTTI1/2 before its
`4C5829` positive EMP write; original Infantry RTTI `523340` returns15.
The other positive Apply write `4C5718` belongs to the Building arm.
Techno AI `6FAF0D..6FAF24` only decrements a positive EMP value.
Arming constructor `4C52B0` calls Apply at `4C5356` and has no incoming
Ghidra references; that search does not prove general EMP unreachability.
The COM factory `6BFB10→4C5370` uses a zero-field load constructor;
`4C5A30` loads its raw state without calling Apply. `4C54A0`, called from
Logic `55B5F6`, only expires pulse objects.

The offline writer `70FC90→70FD62` is reached from House lost-power/removal
(`50E144`, `502718`) and Unit PerCell (`739F59`). The House paths walk the
**all-Techno** array `A8EC7C/A8EC88` (populated by Techno constructor
`6F3183..6F31D2`), but require the object's type pointer to equal `PowersUnit`
`+40C`. Original TechnoType reader `7132DF` supplies key `844164`, `7132F3`
reads its string, `713300` calls `7480D0`, and `713309` writes that pointer.
`7480D0` searches UnitType array `A83CE4` or constructs `7470D0`; the latter
writes vtable `7F6218` at `7471BF` and inserts into that array at `747230`.
Original RTTI proves the family: `7F6214→COL80CD28→TypeDescriptor845980`,
whose name at `845988` is `.?AVUnitTypeClass@@`. Thus a normal InfantryType
pointer cannot match this reader's UnitType pointer. The generic type-pointer
walk `50E1C0` has no incoming static references; it is not proved unreachable.

Native save loading remains outside the ordinary initialization bounds:
`70BF50→65AB80→5F5E80→410380` reaches the raw IStream read, using Infantry
Size virtual `+30→5232F0`, which returns `6F0`. That byte range includes
`+504` and `+1C8`, without subsequent zeroing of either field. These paths
were read from original instructions, not executed by this harness. The
positive gate controls therefore represent supplied native-save state;
they do not establish a normal retail Infantry producer. Native raw-save
import and broader EMP/powered-center implementation remain separate work.

The display rows execute the original `4AAE90` interior dispatch, entering
`4AB449` for the normal cursor or `4AB366` for the minimap. Supplied EAX is
action29/action32, ESI is a receiver, and `[ESP+34]` is cursor argument0/1.
The original switch tables run until the receiver's virtual `+48` boundary;
the prepared row and unchanged argument are observed there. Each selected
28-byte descriptor is read from the original table at `82D028` and retained
as raw bytes and seven signed dwords in the JSON.

| Action | Surface | Cursor row | Frame start | Count | Rate |
| --- | --- | --- | --- | --- | --- |
| 29 | Normal | 33 (`21`, Repair) | 150 | 20 | 4 |
| 29 | Minimap | 0 (default) | 0 | 1 | 0 |
| 32 | Normal | 35 (`23`, NoRepair) | 190 | 1 | 0 |
| 32 | Minimap | 35 (`23`, NoRepair) | 190 | 1 | 0 |

Thus the normal admitted hut action selects Repair frame150, rather than
Enter frame89 or EngineerRepair frame170. The minimap action29 case takes
the original default branch (`4AB78F` return); it does not select Repair.
Normal Repair returns at `4AB4B0`, and both NoRepair cases at `4AB3C4`.
The supplied receiver's `+48` method is a terminal sink: cursor setter state,
animation timing and SHP drawing are outside these display comparisons.

Execution stops before QueueMegaMission's body. Upstream mouse object discovery,
base WhatAction/modifiers, full cell click, network/event enqueue, locomotion,
PerCell2 Engineer entry, rebuilding, sounds, visibility and final rendering are outside
these comparisons. There are no code patches. Query and GetCoords return seams
and the cross-fixture WhatAction seam are recorded explicitly in every affected
trace. RNG calls and a later alliance receiver fail closed if unexpectedly reached.

The shared selection dispatch is separate **instruction evidence**:
`Selection__DispatchMultiUnitOrder` enters at `004AE750`; `004AE844` is its
object-click virtual call. The [original static instruction packet](engineer_bridge_selection_dispatch.json)
is reproducible with `python -m tools.native_inspect disasm 0x4AE750 --bytes 0x253`.
In the object loop, `004AE829` loads `g_CurrentObjects_Data[ESI]`, `004AE83A`
calls that object's WhatAction (`+74`), and `004AE844` calls its ClickedAction
(`+144`). `004AE851` increments the index; `004AE858..004AE85A` compares it
with the live count and loops back. Thus capability grouping in Rust must retain
original selected-object order for the resulting single-object events. This is
not an execution comparison of the entire mouse handler. The adapter leaves
aggregated commands such as VERA's multi-producer SetRally in their existing
dispatch order; those commands cannot be interleaved at one actor index.
