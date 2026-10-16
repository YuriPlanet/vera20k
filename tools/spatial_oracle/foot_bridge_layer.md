# Foot navigation, bridge layer and reachability native comparison

The separate [golden](foot_bridge_layer.json) and
[provenance](foot_bridge_layer.meta.json) are produced by
[the existing Foot oracle](foot_navigation_coordinate.py). Its default
44-row `foot_navigation_coordinate.json` and metadata are unchanged.

```sh
python -m tools.spatial_oracle.foot_navigation_coordinate --check
python -m tools.spatial_oracle.foot_navigation_coordinate --bridge-layers --check
```

Configure the original executable with `VERA20K_GAMEMD_EXE` or `RA2_DIR` as
described in [native_oracle.md](../native_oracle.md). `--check` does not write;
`--bridge-layers --write` deliberately replaces only the new bridge golden
and its metadata. The shared oracle checks SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`
before mapping those same bytes. The sidecar also pins the payload and Unicorn
binding/core.

## Executed original bodies

`OriginalBridgeQuery` extends the existing `OriginalQuery` fixture. It retains
the original Infantry table `0x007EB058` for Walk and Unit table `0x007F5C70`
for Drive, Ship and Hover, and asserts their `+48`, `+4C` and `+BC` slots.
Neither vtables nor native instructions are replaced. Hooks observe calls,
returns and shared-Dummy coordinate writes; they do not answer calls.

The 416 independent layer rows execute `Foot4DBDF0`, followed by
`Foot4DDC40 → Object5F6A70`, except that an active Tube returns from the
original Foot override. Non-Tube rows reach original `Map578080` and
`Cell47B3A0`; the structural admission branch also reaches original
`Map565730`. Null-head fallback reaches original `Object5F65A0`.
`Cell47B3A0` initializes its own original double slope coefficients and uses
original `7C5F00` conversion. No Python height, bridge decision, zone decision
or slope-table reconstruction supplies an expected result.

Observed behavior in these rows:

- `Foot4DBDF0` chooses the locomotor's retained XYZ, falling back to physical
  XYZ only for native NullCoord. Its result is independent of the supplied
  unrelated or null `NavCom (+5A4)`. Tube state chooses the signed exit center.
- `Object5F6A70` asks for navigation ground first, then physical ground. When
  the actor is off the bridge, it asks for the navigation cell's structural
  `0x100` flag only if physical ground minus navigation ground exceeds 312.
  When the actor is on the bridge, navigation ground more than 312 above
  physical ground clears the returned layer. These calls preserve `OnBridge`.
- The strict boundary is exercised in both directions at 311, 312, 313 and
  416, with step 104. For example, level 3 / slope 1 at local `(0,0)` yields
  native ground 312 and local `(3,0)` yields 313. XYZ Z is an independent input
  and does not replace either ground query.
- Native misses stamp the actual shared Dummy's packed coordinate while
  retaining supplied level, slope and flags. Both missing coordinates can
  therefore observe one mutable Cell identity in different query order.
  `(-256,256)` also exercises the fixed-512-stride alias to allocated
  `(511,0)`. Negative fractions and signed extremes use original arithmetic.
- `Foot4DDC40` returns false for a Tube without calling `Object5F6A70`,
  `Foot4DBDF0`, or either ground query. The separate coordinate invocation
  still executes the Tube override with a null locomotor. A Tube exit
  `(-1,-32768)` returns XYZ `(-128,-8388480,0)`; original caller conversion
  then packs Cell `(0,-32767)` by signed division toward zero.

The corpus covers all four retained-head families, both `OnBridge` values,
all 21 valid slopes, signed level extremes, complete and partial NullCoord,
unrelated/null NavCom, structural and nonstructural flag contrasts, sparse
misses, retained-Dummy prefixes, aliasing, and Tube with/without a locomotor.
Candidate Foot, locomotor and allocated-Cell payloads are asserted unchanged;
the Dummy's level, slope and flags remain unchanged. Recorded instruction
spans are asserted unchanged after every row.

## Fresh Drive and Ship constructor controls

Six separate `constructor_rows` execute original Drive `4AF540` or Ship
`69EC50`, including their original `55A6C0` base constructor, before running
the Foot coordinate and bridge-layer queries. They use explicit `A5` backing
bytes, native NullCoord globals `(0,0,0)`, and frame 100, 0 or -1. The
constructor initializes both local `+34` destination and local `+40` retained
head from NullCoord, rather than relying on zero-filled fixture memory.

Each row records the original constructor's object, ILocomotion and
IPiggyback vtables, destination/head, initially null Foot link, cached frame,
selector/cursor pair and complete `0x6C` payload before linking. The live
Foot pointer at local `+0C` is then supplied; linking and the full Unit
constructor remain outside this execution. Original `4DBDF0` and `4DDC40`
resolve the linked Foot's physical XYZ for positive, zero and signed current
coordinates and preserve its supplied OnBridge verdict when the two ground
queries agree. This establishes a valid fresh native locomotor state; an
absent external Rust runtime must still distinguish that state from a null
native `Foot+674` interface pointer.

The original 416 layer rows, 28 response seams and seven harvest seams
retain their prior input/output values. Constructor observations add their
own rows and original instruction spans.

## Original consumer seams

The 28 response seams enter only the coordinate/layer portion of
`Techno708080`, with the earlier scan register/stack context supplied. The
victim is another concrete Unit with independent retained and physical XYZ.
These are the original call identities:

| Consumer | Victim `+4C` | Candidate `+4C` | Candidate `+BC` | `Map56D100` |
| --- | --- | --- | --- | --- |
| Infantry response | `70829C` | `7082DB` | `708319` | `70833C` |
| Unit response | `7084F4` | `708533` | `708571` | `708594` |
| Drive harvest probe | supplied clicked Cell | `4DCF31` | `4DCF6F` | `4DCF92` |

Response execution continues through original `56D100`, `56D230`,
`56DA10` and `578460`, stopping before threat scoring at `708345` or
`7085A1`, or at refusal `7083BC` or `708622`. Seven Drive harvest seams
execute the same protocol from `4DCF26`, including original Unit `+84`
type access at `4DCF7A`, and stop at `4DCF97`, before the LandType and
CanEnterCell continuation.

Each seam supplies explicit `Size=(8,8)`, `LocalSize=(0,0,8,8)`, a 17-by-17
base-group table, raw WORD labels, and active or missing bridge records.
It observes all six original reachability arguments and the native result.
Covered contrasts include active-record agreement, split groups, equal raw
labels across split groups, missing-record DWORD `-1` versus raw WORD 65535,
the source-fringe early return, null-head fallback, and Tube bypass. Ordered
receipts show target `GetZone` before source `GetZone` when both are needed,
plus the final shared Dummy after native reachability.

## Payload schema and limits

`rows[].input` supplies every fixture field: family, physical XYZ, retained
head XYZ, OnBridge, Tube, locomotor presence, unrelated NavCom XYZ, sparse
cell scalars, retained Dummy scalars, map capacity, level globals, FPCW,
conversion control word, and optional preceding map queries.
`rows[].output` contains native navigation XYZ, returned bridge verdict,
ordered world/ground query receipts with native selected-cell identity and
height, coordinate-call results, Dummy writes and final fields, plus retained
head/physical/OnBridge readback. Expected verdicts and heights come only from
original execution.

`response_seams[]` adds victim state, MovementZone and the zone fixtures;
`harvest_seams[]` adds the explicit clicked Cell instead of a victim. Both
outputs contain native source/destination packed Cells, all four scalar
arguments, their semantic low-byte booleans, reachability, ordered native
reach calls/results, Dummy at `56D100` entry, and final Dummy. Upper EAX bits
above a returned boolean AL are preserved in raw receipts for instruction
evidence; only AL is the native boolean contract.

This establishes these supplied-state query bodies and bounded consumer
seams and the six fresh Drive/Ship constructor controls. It does not execute
complete Unit/Infantry constructors, COM creation/link lifecycle,
retained-head/Tube/NavCom producers, map loading, bridge topology, zone flood fill, inactive-record
neighbor traversal, complete response admission/ranking/dispatch, harvest
eligibility/LandType/CanEnterCell/scan order, rendering, or a live game.
Signed/extreme inputs do not establish stock-map reachability. The whole
bridge goal needs its separate production integration and wider audit.


## Rust ownership and production checks

`movement/foot_coordinate.rs` owns Foot4DBDF0 and active locomotor coordinate
selection. `movement/ground_pose.rs::navigation_should_be_on_bridge` owns
Foot4DDC40/Object5F6A70's bridge-source verdict. `pathfinding/zone_map.rs::can_reach_native`
owns Map56D100's map bounds, Size shortcuts and target-before-source raw-zone
protocol. The base-response and ore-probe consumers call these owners.

The response-local NavCom coordinate, ground-height and bridge predicate were
removed, together with the response-local cached zone equality. That zone copy
also served ore scanning, so that consumer moved in the same change. An ore
probe now queries its source coordinate/layer for each admitted candidate,
then native reachability, then packed-cell LandType, then its class CanEnter
receiver. It no longer caches the object's OnBridge byte as ShouldBeOnBridge
or skips observable native queries just because the later LandType gate fails.
Fresh Drive/Ship runtime storage is lazy in Rust. Its absent payload represents
the constructor's null retained head while the active locomotor remains present;
the shared Foot owner falls back to physical XYZ. Original constructors 4AF540
and 69EC50, including base 55A6C0, establish that prerequisite on supplied storage.

Rust replay tests:

- `movement::bridge_layer_oracle_tests::native_bridge_layers_match_all_416_original_rows`
  compares source layers and final Dummy fields. The eight i32-extreme physical
  coordinates are scalar query witnesses: Position cannot represent them, so
  those rows execute the shared raw-coordinate projection rather than an entity
  storage round trip. Other rows query the live Foot receiver, including Tube.
- `movement::bridge_layer_oracle_tests::native_bridge_fresh_drive_ship_queries_survive_snapshot`
  compares all six constructor-backed physical/zero/signed coordinate and source
  layer receipts against fresh lazy payloads before and after snapshot reload.
- `combat::base_defense_response::tests::native_bridge_response_recruits_from_all_28_original_zone_seams`
  runs the complete Rust response transaction, compares responder admission
  with original seam reachability, and checks final Dummy and dispatch draws.
  Earlier admission and later scoring/mission behavior are Rust regressions;
  the new native corpus establishes only the bounded query seam.
- `combat::base_defense_response::tests::native_bridge_response_queries_precede_the_unit_slave_victim_gate`
  preserves those queries and Dummy writes before rejecting a Unit for a
  supplied slave victim. Native caller instructions 7085A4/7085A9/7085AD
  establish this later gate's ordering; the 28 original seam inputs establish
  its earlier queries. Stock SLAV has ToProtect false, so this supplied-state
  control does not establish an ordinary retail SLAV response trigger.
- `miner::ore_scan::tests::native_bridge_harvest_reach_matches_all_seven_original_seams`
  compares the actual ore-probe source producer, native raw-zone result and
  Dummy fields with original4DCF26..4DCF97. Existing `harvest_field` native
  replay tests exercise the connected production ring scan and ore selection.
- `combat::base_defense_response::tests::retail_bridge_base_response_runs_through_the_protected_damage_receiver`
  loads retail rules/art through production readers, spawns HARV/MTNK/HTNK,
  and enters the live ToProtect damage receiver. With supplied bridge/zone
  geometry it checks rejection across unequal raw zones and recruitment across
  equal raw zones. This is production integration, not retail-map-loading parity.

Fail-first control: `native_response_uses_the_paid_foot_head_instead_of_navcom`
failed on the old response coordinate (RustCell7,7 versus originalCell6,5),
then passed after migration to Foot4DBDF0's shared owner.

### Global replay fixture provenance

The global 600-tick skirmish previously supplied a width-zero playfield and no
Size or zone owner. Completing native ore-query admission exposed that incomplete
fixture. It now installs `Size=(64,64)`, raw `LocalSize=(2,2,60,56)`, a 129-square
backing grid, an empty bridge-map receipt retaining the same source Size, and the
zone cache. A common `(32,32)` translation preserves the authored scene's relative
geometry within the native Size diamond. It keeps the frame's existing PathGrid
owner. The mining tripwire now requires field acquisition, movement and cargo.

The paired Rust receipts (`tools/spatial_oracle/foot_bridge_layer.replay.json`, in git at 1b1a8b43e) isolate this fixture
change from the ore-query migration. The control restored only `ore_scan.rs` and
`zone_map.rs` from parent commit `b34f76e72c2afe614bf9e65fe42ecf07d166d4e0`;
all other candidate code, including the independently native-proved fresh Foot
prerequisite, and the completed fixture remained identical. A temporary observer
around the live frame used existing `rng::trace_draws` and serialized GameEntities.
It was removed after comparison. The receipts record both source hashes, fixture
inputs, per-tick hashes of the complete serialized entities, all three RNG stream
fingerprints, and raw draw values with their starting indices.
The captured ore/zone source files are preserved in commit `480dc6a1`; incoming
main's shared lepton conversions were integrated afterward. The comparison is
historical fixture provenance. The final integrated candidate reruns the baseline
and original query corpus rather than treating these earlier receipts as a new
native comparison.

Across all 600 ticks the two Rust executions matched every world hash, serialized
entity and RNG fingerprint, plus all 319 raw draws. The unchanged absolute RNG
pins and duel checks still pass. The final world hash `B3DF7D9407AF305B` then reflected
the completed fixture's map, coordinates and movement context (later composition-only
moves are recorded beside `GLOBAL_HARNESS_FINAL_HASH`). These are Rust
regression and baseline provenance checks, not a native skirmish comparison.
The current baseline and strengthened mining checks are reproducible with:

```sh
VERA20K_REQUIRE_RETAIL_INI=1 python -m tools.cargo_run -- test -p vera20k-core --lib \
  sim::world::global_parity_harness_tests::global_skirmish_replay_is_deterministic_and_baseline_stable
```

These source queries draw no RNG, write no actor timer and perform no detach.
They retain the shared map Dummy coordinate writes. The response's existing
assignment RNG draws and cooldown writes remain with their original owners;
response transaction tests protect those outputs. No new authoritative state,
cache or writable simulation field was added.

Ghidra program `gamemd.exe`: corrected the misleading Foot4DDC40 and Object5F6A70
NavCom/Tube descriptions, and named708080 `TechnoClass__RespondToBaseAttack`
with the executed consumer identities and coverage bounds. Saved and read back
all three comments and the708080 label on2026-09-29. Existing accurate4DBDF0
labels/comments were retained and extended with the fresh constructor receipts;
Drive4AF540 and Ship69EC50 comments now cite those controls. All three additions
were saved and read back exactly.

Required wider bridge work remains open. Cached graph reachability clients,
inactive-record traversal and map/topology producers need their own traced
chains; this query refactor does not establish their parity. Campaign ore
shroud admission remains an explicit existing residual in ore_scan.
The response scoring/dispatch mechanism also remains required under issue
[759](https://github.com/YuriPlanet/vera20k/issues/759): production budget,
score and assigned sum still consume Cost instead of native vt+2C0/708B40
ThreatPosed. Original 7080BA, 4D97BE/4D9825/4D984C and 708734 establish the
reads. This affects ordinary AI defender rankings, budget and potentially
dispatch draws/cooldown timing; retail MTNK is Cost 700 / ThreatPosed 15 and
HTNK is 900 / 40. The existing greatest_threat::live_threat_posed port is the
owner to consolidate during that separate response mechanism, including its
remaining Building+2E4 source substitution. These query comparisons do not close it.

## Bridge-response prerequisite replay provenance (2026-09-30)

The `bridge_ai_prerequisite_followup` entry in
the same Rust receipt owner (`tools/spatial_oracle/foot_bridge_layer.replay.json`, in git at 1b1a8b43e) records real spawn
and `advance_tick` calls after the class-target, Foot mission and idle changes.
It compares missing-ART inputs against explicit E1 `Image=GI` and the fixed
GI/sequence excerpt bound through the production reader. These are Rust
regression observations, not native skirmish goldens.

The global replay's first difference from the historical receipt is frame0:
draws7..13 match, but the four former global idle-tail draws14..17 are absent.
Native mission-site idle precedes the first stationary Ready action. Bound ART
admits Ready after that mission call, without adding a draw. It first changes
idle callers at global frame14 and bridge frame16. Full Main/MapGen states are
unchanged in every captured frame; the receipts explicitly bound their coverage.

Slice6's new admitted Walk head is `(1728,1088,0)` with facing8315. The five
historical native paid-step rows supplied `(1728,1216,0)` with facing10855;
their caller premises therefore do not match this replay. All40 original rows
remain unchanged in the sole numeric receiver comparison. The replay retains
head/progress, health, mission-counter and per-frame determinism gates and now
pins live counters directly, removing the historical counter inversion.

The final absolute pins include the newly retained state and supplied ART.
They protect this Rust fixture only. Ground infantry's firing/absolute-stage
integration and ordinary ground reach/Approach remain required behavior;
this replay provenance does not close either chain or the whole-bridge goal.


## Current response and Infantry action dependencies (2026-09-30)

The response chain now uses the private Infantry Doing/Stage owner through
firing, movement, completion and terminal cleanup. Shared consumers, clock
initialization, live deployed Guard, the production Unload dispatcher and
snapshot267 are migrated together. The final replay contexts in
`techno_stage264_followup` retain complete actors, all three RNG objects and
ordered draw/caller observations; these are Rust world receipts, not native
whole-world goldens. The historical observations above are retained as history.

The [wet-action generator](infantry_water_action.py),
[356 native entries](infantry_water_action.json) and
[metadata](infantry_water_action.meta.json) execute original DoAction51D6F0,
constructor517A50 and physical GHOST/TANY rules/ART/sound readers. The original
151 controls remain byte-identical. Native constructor517AC2 initializes signed
water state2; zone3 physical Cell lookup, wet raw records20/21 and transition cues
precede admission. Fixed sound-name reads retain earlier valid IDs across
root/LANG/mode/map. Audio requests execute through the supplied disabled gate;
wet Unlimbo, occupancy, whole native death AI and audible playback are excluded.

`sim::movement::infantry_action::water_tests::` compares all action/reader
receipts and checks ordinary tick progression to WetDie UnInit without a legacy
AnimationState. With production ART/sequence binding installed, the old DoAction
refuses physical GHOST Health0/Water/Die11 while the original accepts20. Restoring
the corrected sole action owner makes all five tests pass. Water state also has
save/load and state-hash coverage. [Ghidra save/readback](infantry_water_action.ghidra.json)
records the exact selected program, original spans and corrected comments.

The [production profiles](foot_bridge_layer.profiles/) use the existing map
observation owner and ordinary retail loading/commands. To reproduce Hills:

```sh
python -m tools.cargo_run --label bridge-response-release-UNIQUE -- build -p vera20k --release --bin vera20k
python -m tools.map_observation --build-label bridge-response-release-UNIQUE \
  --profile tools/spatial_oracle/foot_bridge_layer.profiles/hills-staging2400.json \
  --contract "$(pwd)/src/app/diagnostics/tactical_capture/contract.v2.json" \
  --cwd "$(pwd)" --output /absolute/new/observation-directory
```

Use a new label/output and the checkout's machine-local retail config. Retain
`run.json`, profile, contract, config and exact `child-output` bytes. The prior
61595 release receipts remain in this checkout's ignored
`logs/bridge-response-stage264-validation`, with hashes in its receipt.json.
Hills2400 places MTNK943/E1945 on the high deck at624 leptons, ground944/947 at208,
and endpoint942 at624 offbridge; the GPU frame was inspected. AnyTown captures
exercise real Infantry fire/latch/retaliation and MCV construction. Attackers were
intercepted before yard damage, so those runs do not demonstrate the production
yard-damage→recruit→Rescue/home-cleanup route. Supplied native receiver tests and
Rust integration checks remain separately bounded.

The whole-bridge goal remains paused and open. Required broader map/path authority,
collapse/Tags/huts, full arcing geometry, complete Building scheduling and native
visibility/GPU comparisons are not closed by these response/action dependencies.

## Incoming-main replay composition

The complete incoming-main receipt (`tools/spatial_oracle/foot_bridge_layer.main986.replay.json`, in git at 1b1a8b43e)
retains all819 before/final/control observations. Its lossless comparison accounts
for all2,734 field differences: two buildings retain exact floor Z=0 rather than
None (1,202 fields), their hash views change (1,201 fields), and331 raw caller
strings contain only twelve exact source-line replacements. The201 bridge rows
are byte-identical. Every other actor, input, command, queue and object-store
field, and all three full RNG states and ordered draw values, match.

Original BuildingType `0x00464A70`, installed at type virtual+0x6C, copies input
XY and returns original `Map578080` floor Z. Active Object Unlimbo dispatches that
type virtual at `0x005F4F88`, then commits its result before Mark(PUT). The receipt
retains pinned original-byte packets and explicit-program Ghidra readback. Ground
arithmetic remains established by [the original ramp vectors](../ramp_height_vectors.json);
this body/caller proof does not establish whole Building AI or native world parity.

The uncommitted control excludes only Structure exact Z from the hash; it leaves
gameplay XYZ untouched. All601 prior global hashes and600 frame-result hash views
recover exactly, with all819 final/control gameplay and raw RNG fields identical.
Thus the global Rust pin moves from `CE21A562A1295C86` to `B80AAAB95187A8F9`;
bridge `4477C32418EDA27F` and Slice6 `77B7FEF300222AB2` stay unchanged. The receipt
preserves complete source, installation/restoration and binary identities, actual
old-pin failure logs, control logs and the comparison recipes. None is a native
whole-world golden. The earlier full replay contexts remain unchanged in
the historical receipt (`tools/spatial_oracle/foot_bridge_layer.replay.json`, in git at 1b1a8b43e).

The final strict-retail library run passes9,463 tests (211 ignored); lib clippy
finishes with765 existing warnings. Field ownership is2,646 versus2,661 on
integrated main. Final ordinary release label
`bridge-response-runtime-20260930-stage267-main986` has binary SHA256
`e3549a23bd935f463298665c3b891d99c9527c18fca4a7904a8c7a243fe98f11`.
The same committed profiles revalidate Hills2400 (2,401 observations),
AnyTown800 (801) and AnyTown1184 (1,185). Hills943/945 are on the high deck
at Z624 and944/947 remain on ground Z208, all alive. AnyTown1383 fires with
Doing4 from step912; hostile1386 ends at7HP and the yard remains1,000HP.
Therefore yard damage→Rescue/home remains unobserved. GPU frames were inspected.
Raw local evidence is retained at `logs/bridge-response-stage267-main986-validation`
with receipt SHA256
`4f21ae65fbd18f9a26ab55465497c137e9194bf2ba3d33ceecdb3e300c759554`;
its receipt hashes every raw child output and input copy. These release checks
validate connected production output; their native parity certification is NONE.

## Final main474 integration

The same replay receipt (`tools/spatial_oracle/foot_bridge_layer.main986.replay.json`, in git at 1b1a8b43e) adds complete
main474/HEAD95d observations after integrating the shared Aircraft CanEnter and
Object SetHeight owners. All819 actor/hash/state/RNG records agree with the
preceding candidate. Only331 raw caller strings change, through seven exact
source-line replacements; the201 bridge rows remain byte-identical. No pin
changes or difference whitelist are needed. The existing original
[ramp/SetHeight vectors](../ramp_height_vectors.json) establish `0x005F5FA0`
and its ground evaluator; this merge calls that sole owner rather than
recreating the removed ground-height wrapper. The Sonic bridge fixture uses
SetHeight(0) between occupancy removal/re-addition; its level-two deck Z is624.

The corrected final strict-retail library passes9,466 tests (211 ignored),
clippy completes with765 repository warnings, and the field ratchet remains
2,646 versus2,661 on main474. Complete source/build/execution and comparison
receipts are preserved; original replay contexts remain intact. These results
retain the previous bounds on native world and production Rescue coverage.

The final ordinary release label `bridge-response-runtime-20260930-stage267-main474`
has binary SHA256
`0ae9a62af1e315bfa1d0940901a49a490acf29c277bfb2362a814e4b0402a455`.
The three committed profiles again pass Hills2400 (2,401 observations),
AnyTown800 (801) and AnyTown1184 (1,185). Hills943/945 remain alive on
the high deck at Z624;944/947 remain alive on ground Z208. AnyTown1381/1383
stage at low-bridge Z416, and1383 begins Doing4 firing at step912. Hostile1386
ends at7HP; yard1408 remains1,000HP, with no observed Rescue mission. GPU
frames were inspected. The local raw-output/input receipt in
`logs/bridge-response-stage267-main474-validation/receipt.json` has SHA256
`96eb9813a679bb31123406fa2c89334a7cfc72d84a43b0ea8d0f9498d3ce4c9f`.
These production checks carry native parity certification NONE.
