# Original Allied depot waiter comparisons

`building_repair.py --depot-waiters` extends the existing depot VM and native
input readers. The [golden payload](building_repair.depot_waiters.json) and
[sidecar](building_repair.depot_waiters.meta.json) pin
Unicorn 2.1.4, original entry points and declared substitutions.
Original text and the four actual Unit/UnitType/Building/BuildingType vtables
remain unchanged. Expected values come from native execution.

```sh
source ~/Documents/vera20k-dev/env.sh
VERA20K_DEPOT_SERVICE_INPUTS=.local-audit/war-miner-attack/inputs/extract \
  python -m tools.spatial_oracle.building_repair --depot-waiters --check
```

The physical input directory is the same as
[the service corpus](building_repair.depot_service.md#reproduction-and-preserved-ownership).
RULESMD, optional LANGRULE, Battle and AnyTown selected read slices run in
layer order; ARTMD is fixed. The additive reader establishes Enter controls,
MTNK Track/Crusher, GADEPT `NumberImpassableRows=1`, `CloseEnough=576`, and
the native PathDelay/BlockagePathDelay reads, including constructor defaults.
This selected reader does not certify a complete Rules/type ReadINI pass.
Unit74763F calls Techno712170 first. The additive reader preserves its selected
Strength → SpeedType → ManualReload → Cost → Crusher → MovementZone order,
then the derived Harvester/Weeder reads and SpeedType postpass. The legacy
corpora retain their independently bounded read slices and payload bytes.

Original x87/Object/Cell/Map CRT initializers execute before the first Cell
height query. An earlier exploratory fixture warmed a zero height cache before
Cell CRT and was rejected; it is not a golden or evidence that raised depots
skip Mark. The retained generator never resets that cache retrospectively.

At flat levels0/4, whole Building Mark `43F180(3)` produces nine Cell-list
entries and ground occupation `0x80`. Mark0 removes them without RNG draws.
Actual Unit `73F0A0` admits the eastern two columns and refuses the western
column: `[7,0,0]` on each row. Its caller is an unlinked Move with pending500;
it does not use a radio contact as permission to traverse those repair rows.

Busy destination → Enter dispatch selects outside Cell `(9,8)` at both levels,
changes the current mission to Move and retains500. The input explicitly
binds admitted playfield membership and LocalSize `[-16,-16,64,64]`, matching
the Rust comparison's production construction. A synthetic narrower rectangle
with different membership was rejected as an input mismatch.
The complete MissionAI dispatcher writes `[200,15]` with one Scenario cadence
draw. A busy retry preserves the request. After an explicit original BREAK
frees the incumbent slot, an outside retry admits Enter/depot NavCom and clears500.
Original Unit NULL destination preserves500. A free retry with the requester
already at foundation Cell `(8,11)` performs HELLO, CAN_LOAD refusal and BREAK,
then clears500 and both contacts. Native AL1 means the attempt was consumed;
it does not mean admission succeeded. Stack-frame-matched radio receipts reuse
`refinery_dock`'s existing observer.

The level4 blocked-goal control prepares another stationary friendly MTNK at
`(8,12)` through original CellPUT. Real Unit entry returns6 twice during
Drive `4B0500 → Process_Movement4B2630`. The single FindPath call supplies
only route `[4,4,4]` and AL1; no entry answer is substituted. Original distance
conversion returns256, NULL/Drive Stop clears NavCom and preserves500, and
movement/blocked timers become `[201,0]` / `[201,60]`. All three full RNG streams
remain unchanged. The subsequent original BREAK/retry reproduces foundation
refusal and request cleanup.
The corpus records the exact Foot/Drive state before and after: empty NavQueue,
absent head, selector−1, valid0, membership, path words, retries and path latch.
Native prestate timers `[200,0]` / `[200,60]` are imported explicitly in Rust;
construction at frame201 would otherwise produce different timer anchors.

[Rust comparisons](../../src/sim/docking/building_repair_waiter_oracle_tests.rs)
use production rules, Mark, Unit entry, mission/destination/radio and fresh
movement owners. The existing test seam now permits a supplied path with live
entry classification; no second admission implementation is added.
[The complete-frame regression](../../src/sim/docking/building_dock_frame_tests.rs)
separately constructs the depot and tanks, repairs all three and proves that
first admission follows live Logic order rather than issue or storage order.
[Retail profiles](../map_observation.md#allied-depot-waiters) cover ordinary
Queue/Place, combat damage, travel, service animations and departure.

Coverage is bounded. Object/House admission, flat map/class-height storage,
uniform zones, placeholder incumbent and the stationary peer's Unit fields are
prepared inputs. Complete constructors, map connectivity/path search, paid
track traversal, original multi-object Logic scheduling and the full crowded
retail tank trajectory do not execute here. Production captures are Rust
integration evidence, not native whole-match or pixel comparisons. No queue,
reservation or zone5 gameplay change is justified by these results.
