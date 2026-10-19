# FV Guard acquisition across bridge layers

These original-executable corpora bound the ordinary empty retail FV passive
acquisition chain and the shared native cell/height queries it reaches. They do
not establish complete Unit scheduling, native map loading, bridge lifecycle or
whole-world parity. The original `gamemd.exe` SHA256 is
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.

## Reproduce

Run from the repository root with `VERA20K_GAMEMD_EXE` configured as described in
[the native harness reference](../native_oracle.md). The retail preparation uses
`VERA20K_PROJECTILE_RENDER_ASSETS` containing the physical files and SHPs required
by [the IFV preparation](../projectile_oracle/ifv_fire_coord.md). File hashes and
supplied boundaries are retained in JSON and metadata. No VERA-parsed gameplay
scalar seeds the native type fields.

```sh
PYTHONPATH=. python -m tools.spatial_oracle.bridge_target_layer --check
PYTHONPATH=. python -m tools.spatial_oracle.bridge_target_layer_inputs --check
PYTHONPATH=. python -m tools.spatial_oracle.bridge_target_composed --check
PYTHONPATH=. python -m tools.spatial_oracle.object_get_cell --check
PYTHONPATH=. python -m tools.spatial_oracle.object_flight_height --check
PYTHONPATH=. python -m tools.spatial_oracle.fire_error_cell_boundary --check
PYTHONPATH=. python -m tools.spatial_oracle.in_range_cell_boundary --check
```

Each module has a same-basename `.json` and `.meta.json`. The input directory
contains `RULESMD.INI`, `ARTMD.INI`, `MPBattleMD.ini`, `Hills.map` and the complete
1384-byte `DRAGON.SHP`. `Hills.map` is the 144458-byte inner payload of loose
`Hills.mmx`, SHA256
`780d5d6e6d3c81ac0d510a5df326848dd426b3d840ac290114f44faf8eab9e9e`;
it is not the archived `XHills.MAP` variant. `LANGRULE.INI` is absent in the
selected installation. Archive/file loading remains a supplied boundary.

Composed pointer receipts use symbolic Cell/entity identities. Loading DRAGON
shifts later bump allocations by 1392 bytes; normalizing only ObjectGetCell
pointer returns and target-pointer writes makes these receipts independent of
that allocation layout. This normalization changes no numeric gameplay output.
The Object/Techno CRT and FV AirRangeBonus corrections below do change native
outcomes and supersede the earlier joined goldens. Native evidence, Rust tests,
full retail validation, runtime and review are separate receipts.

On2026-09-27, all seven `--check` commands passed against the recorded complete
input set with independent `PYTHONHASHSEED=83` (109.373 seconds total). This was
after the consumed-field audit and MaxDamage/read-default correction. Comparing
the old and regenerated composed payloads with only `cases[].inputs` removed
gave exact equality across all22 histories/34 visits: no query, target, RNG,
timer, score or damage-estimate output changed. Python syntax, all14 JSON files
and the ledger's local links were also checked. These are native/tooling receipts;
production Rust and whole-goal validation are not implied.

| Corpus | Executed coverage |
| --- | --- |
| `bridge_target_layer` | 159 G27/getter controls: 144 raw-flag/layer pairs, four noncanonical layer-byte controls, eleven coordinate/dummy controls |
| `bridge_target_layer_inputs` | Original FV constructor/retained passive-reader layers, complete admission gates and Unit/Foot wrapper dispatch |
| `bridge_target_composed` | 21 passive histories/33 visits through real scan, evaluation, score, assignment/debit, plus one direct Evaluate control: 22 histories/34 visits total |
| `object_get_cell` | Ten complete ObjectGetCell calls, each with both physical-coordinate Map calls; mark, signed/alias and dummy cases |
| `object_flight_height` | 24 original height/low/high controls with original static initialization, ground/deck, mark, threshold and live terrain changes |
| `fire_error_cell_boundary` | Four complete TechnoGetFireError early-query controls and three Cell-center-to-Map leaves |
| `in_range_cell_boundary` | Six full FV InRange calls with sparse-map/dummy state, minimum-range and unlimited-range controls |

## Original order and state

G27 is `6F8682..6F86FE`; the former comment address `6F8672` is the preceding
mask branch. It calls Map565730 for stored attacker XYZ, then candidate XYZ,
retaining both Cell identities. **Both queries run before either flag test or
the layer comparison.** Only both raw Cell+140 bit100 set and unequal raw
OnBridge bytes reject. Flags200/400 alone do not. The isolated corpus includes
same-layer missing-cell and first-nonstructural controls so an early shortcut
would lose the second shared-dummy stamp.

Evaluate selects its weapon and probes GetFireError at `6F7CE8`, before estimated
health, actual health and the other cheap predicates. Only Illegal5 rejects here.
Moving that probe later changes native cell-query side effects for rejected
candidates. The direct health0/missing-cell control executes real GetFireError,
returns Facing2 and stamps dummy41,41 before `6F7DA3` rejects health0.

TechnoGetFireError's unconditional target virtual48→Map565730 call at `6FC197`
follows T10 and precedes the lifted/limbo checks. Its retained Cell identity is
the sensor receiver at `6FC26F`. The boundary corpus executes Temporal-held-target
Rearm3 with no query, lifted and limbo Illegal5 with one direct query, and a
cloaked-target Cant6 with actual sensor receivers (return addresses703956 and
6FC274). The FV control has equal center and physical XY; differing centers are
not established by this sample.

ObjectGetCell5F6960 executes **two** physical XYZ→565730 lookups per invocation,
independent of marking. The ready opposite-layer composed case has thirteen
565730 calls inside common GetFireError: direct T11, four for the two T40 target
getters, four for the two T58 firer getters, four for its two target getters.
T58 then returns Illegal5, so that ordinary ready case never reaches G27.
With a supplied active original rearm timer, GetFireError returns Rearm3 before
T58. Source on deck1040 and target on ground624 then pass InRange and G27 rejects
independently. The forward ground-to-deck case fails InRange before G27.

Original Object CRT entries `8141D8..81420C` establish level104 and bridge416.
GetHeight5F5F40 subtracts live Map578080 floor and, when OnBridge is nonzero,
bridge416 from stored Z. Marked objects are low below208 (including height0 and
negative height), high at/above208. Unmarked objects are neither. The composed
flat level6 source/target are at ground624 or deck1040 and have actual native
height0/HighFlying0. Earlier exploratory results with an omitted Object static
initializer are superseded; no golden uses that zero-threshold fixture.

Techno has separate static constants: original scalar CRT entries815040..815074
establish B0EB34=104 and B0EB24=416. InRange first calls target high, then target
coordinates and low. Low targets on structural cells snap to floor+416 even
when their OnBridge byte is false. Thus the two structural ground objects fail
its under-bridge test at6F7629..762F and Evaluate rejects at6F81B8, despite the
early GetFireError returning OK0. Earlier composed results that omitted these
Techno initializers are invalid. Corrected JSON preserves those rejected cases
and includes adjusted InRange source/target coordinates and native returns.

The direct InRange companion supplies one real source Cell40,41 and missing
target Cell41,41 with Dummy level1. Its range/minimum fields are explicit controls:
baseline1280/0, MinimumRange1000 and unlimited Range-512. They are not physical
HoverMissile tuning; each row separately records the original retail readback
1536/256. Full native6F7220 snaps a marked low target to104 with clear Dummy flags
and520 with structural100; both remain within the supplied1280 range.
Missing source42,42 below a structural dummy deck
rejects and leaves Dummy42,42. A supplied MinimumRange1000 rejects before the
source query; Range-512 returns before either flight getter or any cell query.
An unmarked target invokes high then low but neither getter queries height.
Ordered565730 and578080 receipts distinguish direct world lookups from
Map578080's inline table lookup/dummy stamp (5780D0..5780E3). These are supplied state controls, not map admission.

## Passive scanner and ring lookups

The composed entry is `6FA65A`, after earlier AI/mission phases; it ends at
`6FA6F5`, after passive-byte publication and before remaining AI. It executes
709290→709820→743190→4D9920→6F8DF0, original ScanCell/Evaluate/GetFireError,
70CD10 scoring, 6FCDB0 assignment, 6FDB80 estimate and the outer passive write.
No claimed runtime callee return is supplied.

Scanner709820 first writes last-scan frame, then performs Scenario Ranged(0,2)
and arms its timer before target retention or a fresh search. Seed31/frame173
executes raw2026076499 then2287577493, returning1 and arming27+1=28. The next due
visits in the retained history execute raw2225548056→0 and1502837926→2. The JSON
retains full before/after RNG bytes, not just counts. Native cursor29 remains
unchanged; it is a partial preparation prefix, not a whole-world identity claim.

A same-deck target is assigned and estimated HP200 becomes183 through original
EstimateDamage. Two newly added deck-origin histories establish actual retention:
ready opposite layers clear a held target through T58; a held ground target under
Rearm3 remains held by a deck source without GreatestThreat/G27. After an explicit
original AssignTarget(null), a fresh scan under that rearm timer reaches G27 and
rejects it. The older ground-origin histories now correctly remain unassigned.
A not-due visit performs no scanner/RNG operation.
The inactive middle timer word comes from reused native stack scratch and is
recorded for transparency; only start/duration carry the tested timer meaning.

The ring walk is stateful even at empty cells. Native Map5657A0 at6F8984 executes
at ScanCell entry, **before** its zone test and before list lookup. Its returned
Cell pointer is retained in EBX; deck+E8 wins when nonempty, otherwise ground+E4.
Fixed-stride aliases must therefore use the returned Cell's occupancy. A dummy
miss still stamps coordinates even if both lists are empty. Outer ring sites
first apply original usable-diamond568300, then call ScanCell; do not replace
that ordering with an occupancy-only early return.

Ring0 preserves both coincident top/bottom ScanCell visits. The AA path also
resolves the scanner center at6F91C8 before querying AirTracker, even with no
airborne objects. The selected physical HoverMissile Range6 and MinimumRange1
are1536/256 leptons. Original FV AirRangeBonus read71479A..7147B4 consumes4 and
stores1024 leptons at Type+68C; GreatestThreat's fallback resolves range0 and
scanner radius11. The earlier corpus omitted that reader and used radius7.

In the corrected supplied geometry, accepted cases execute27 Cell-coordinate
queries (one center plus26 ScanCell), while exhausted scans execute312 (one
plus311). The last exhausted query is20,29 and stamps the shared dummy
accordingly. `cell_lookups` saves full order/identity/dummy state; `lookups`
saves world-coordinate queries, and both carry instruction sequence indexes.
These counts are bounds of this geometry, not universal scan constants.

After selecting a hostile deck head, its InRange rejection does not expose a
ground occupant or later occupant in that same list. These three older rows
therefore prove the shared no-retry boundary, not a composed G27 rejection. A
legal hostile on a later nonstructural ground cell can still win and receive the17
estimate debit; the structural-ground alternative fails InRange. The same-list
tail intentionally supplies a ground-pose/OnBridge0 object in the deck list and
does not establish ordinary admission for that inconsistent membership.

## Retail preparation and coverage limits

Original UnitType/FV weapon preparation is extended with retained physical
Armor/Strength/Immune, passive keys, GuardRange/AirRangeBonus, ThreatPosed and threat-coefficient
reads. Original Rules constructor plus Process runs the selected physical General
keys before each type coefficient layer. Physical RULESMD, optional LANGRULE,
MPBattleMD and Hills data yield Strength200, Armor3, ThreatPosed10, GuardRange0,
SpecialThreatValue1, delays27/36 and type coefficients200,-200,200,-200,-10.
The original non-null HouseType constructor arm4F643B..6455 sets House+1FB, so
scoring reads those type values rather than the separate Dumb General family.

An executed memory-read audit of accepted, rejected and retained/rearm histories
found one further input prerequisite: EstimateDamage489180 reads Rules+16C8.
The original Rules constructor sets MaxDamage1000, but physical
`[CombatDamage] MaxDamage=10000`. The composed preparation now executes section
admission526810 and the retained MaxDamage block66CE2C..66CE57 on each present
physical layer. It records constructor1000, first-layer10000 and retention across
the absent mode/map sections. Earlier fixtures retained1000. The selected damage25
is below either cap, so that correction changes no target or17-point estimate
debit; it is required to establish the actual consumed retail input.

The remaining reached type defaults now have original reader execution too:
LegalTarget/Insignificant, LandTargeting/VHPScan, IsTrain, OpenTopped, MobileFire,
OpportunityFire, SprayAttack, Natural, Invisible, NoAutoFire, HunterSeeker,
Organic, AttackFriendlies, DeployToFire/IsSimpleDeployer, SmallVisceroid/
LargeVisceroid and NonVehicle. Native section admission skips absent FV layers.
These keys are absent in physical FV: LegalTarget and MobileFire retain1, the
other recorded fields retain0. Fixed FV ART has no FiringSyncFrame0/1 keys;
original loop747AA2..747B03 retains the UnitType constructor's-1/-1. This closes
the inspected field inputs for these ordinary, empty, non-elite FV controls;
it does not establish a complete type loader or Unit lifecycle.

`TooBigToFitUnderBridge=true` is not an admission prerequisite for this chain.
A fresh whole-program instruction search found its five direct Type+E16 accesses:
constructor747143, reader/default747747 and store747778, then draw consumers
73B1B0 and73CE0D. The draw branches alter sprite clipping/depth arguments after
the bridge-edge predicates703B10/703E70. The complete UnitCanEnter73F0A0 body has
no E16 access; its bridge/ground list decision is driven by cell flags and carried
height. No movement restriction is inferred from the key's name. The broader
rendering behavior remains a separate whole-goal audit item.

Supplied boundaries remain explicit: sparse fixed-stride map cells and native
Size20x20, empty AirTracker extent64x64, hostile human House/Country memory and
unit combat factors1, on-map Guard Unit memory, host frame changes, placement,
list links, flags and OnBridge changes. Original Drive constructor and original
Unit ammo/health/respawn initialization slices execute, but full Unit/House/Map
constructors, Unlimbo and bridge mutation do not. Full selected General Process
is not full retail type discovery; unrelated sections are omitted.

Production validation joins the shared canonical getters, early fire probe,
G27, retained occupancy selection, normal passive timer/RNG and target owner as
recorded below. Other acquisition
masks, aircraft/jumpjet overrides, disguise RNG, entire AI scheduling and broader
combat chains are not certified by these ordinary FV controls.

## Rust and production validation

On 2026-09-27 the final candidate, merged with origin/main8309fa17 (including
PR567 open-topped fire and PR563 pause ownership), passed `VERA20K_REQUIRE_RETAIL_INI=1 cargo test
-p vera20k --lib`: 9583 passed, 0 failed, 169 ignored (62.96 seconds). The final
`cargo clippy -p vera20k --lib` exited 0 with 954 warnings. The earlier focused
selection passed 145 tests, with four ignored. Native-derived checks live in
`greatest_threat_bridge_tests`, `fire_error_cell_tests`, `in_range_cell_tests`,
`movement::object_flight_tests`, and
`world::techno_ai::bridge_target_layer_tests`; the latter compares timer words,
complete RNG state, target/debit outcomes and retained Cell-query effects across
the composed histories. Cursor checks exercise isolated query state and verify
that presentation does not change the simulation hash or shared Dummy.

Five older tests initially failed because their fixtures omitted inputs now read
by the shared native queries. Their expected results were preserved: AoE and
jumpjet fixtures now supply on-map marking and physical Z; the persistent-Z
projectile fixture supplies the actual supporting terrain; the Desolator AreaFire
fixture supplies a real Cell identity; the Dog-release fixture supplies a
structural bridge and owner-maintained layer occupancy. All five focused checks
then passed before the final full suite. These are fixture corrections, not
additional native comparisons.

The explicitly ignored production test
`sim::world::techno_ai::bridge_target_layer_tests::retail_hills_passive_bridge_layers_survive_movement_and_restore`
was run separately against the physical installation and passed in 7.49 seconds.
It loads Hills through the app's production loader, moves FV961 from `(64,72)`
onto deck `(64,69)` at Z1040 in 21 frames, and moves FV962 from `(62,69)` toward
`(68,69)`, stopping beneath the bridge at `(66,69)` and Z624 in 46 frames. Both
use ordinary Move/Stop admission. Ready and busy opposite-layer scans remain
unassigned. Direct live-to-restored checks preserve actor position/layer, target,
passive/rearm timer words, estimated/actual health and lifecycle, plus both Cells'
height/raw bridge flags. Native loading reseeds Scenario RNG to zero and Map
resize reconstructs the shared Dummy, so an uninterrupted future is not expected
to match a restored future. Two restored futures match for 40 frames, and
bridge-receiver collapse plus two restored collapsed futures match for four
further frames. This is Rust
production integration; supplied receiver damage is not an ordinary firing proof.

A release example, `bridge_target_layer_scene` (removed on 2026-10-08), closed that
firing boundary separately on a loose copy of the exact inner Hills payload identified
above. It preserved the ordinary Move/Stop scene, supplied hostility and an active
rearm timer for the G27 boundary, then used an ordinary bank FV and repeated
ForceAttackCell commands through the existing target/weapon/projectile owners. It
recorded 52 projectiles and collapse after
1400 frames; the former deck and ground actors were both retired. The intact save
is frame69/hash`286e28169efa00f8`, the collapsed save frame1509/hash`3d10a7cf8f09e45a`,
map hash`c045c269668aa87e`, rules hash`9bf711eab7834933`. Snapshot compatibility is
219; the hash layout remains217.

The ordinary release app loaded both saves through Skirmish → in-game Load Game.
The intact view shows the selected deck FV and hostile ground FV beneath it; the
collapsed view shows the missing central span, absent former occupants and the
bank FV on the surviving approach. Both cursor-free GPU screenshots use normal
fog: SCRN0019.pcx SHA256`4675ef2509d8cd88c64dcaa0e757957f73d7e281211b69128be3d6b93fc03f4e`
and SCRN0020.pcx SHA256`e82674c16afc2c5051551609424b86400a9bfa1a6c44c7051a1cdd06c1597087`.
Presentation-only full visibility and terrain diagnostics helped locate the
bridge, then were disabled before capture. This is visible Rust smoke validation,
not native image parity. The initial intact load showed black at the inherited
camera position; temporary full visibility revealed distant Cell(72,125). Panning
to the saved actors and restoring normal fog produced the captured view. No
rendered claim uses that initial black frame.

Local logs, retained binaries, saves' generating output, PCX/PNG captures and
`visible-v219-captures.json` are under
`~/Documents/vera20k-dev/bridge-target-evidence-20260927`.
Release app SHA256`cfa250702c4011c69a5cbcd3f2c719d918d9b3aa28e88c8cf14dee6fec6c80b1`;
example SHA256`43210b8692b2c0c11abda54ae418de5160dec77645e024b047bfafd02d0479e9`.

## Required subsequent chains

The supplied `0x100 → 0x400 → 0x100` target history establishes reaction to live
flags; it does not establish Engineer rebuilding. The ordinary Engineer entry
chain owns that follow-up. It establishes that the initial diagnostic source
`(69,74)` is a cliff rejected by native Foot/Map too, then uses the legal source `(66,76)` to trace AStar,
Walk, PerCell2 and repair through the authored CABHUT at `(68,74)`. Exact
captures and native comparison receipts are retained in the local evidence
directory's `engineer-entry-followup/`. No claim here closes all bridge types,
native map/lifecycle construction, bridge rendering or the whole-bridge goal.

## Single critic pass

The fresh read-only critic found no confirmed Rust defect in the candidate based
on547d2d578. It independently reproduced the complete composed native corpus with
`PYTHONHASHSEED=131` and passed13 relevant checks using the retained test binary.
Its one confirmed finding was the stale6F7CE8 Ghidra ordering comment, corrected
as recorded below. The report is retained locally as `critic.md`.

The critic also recommended comparing an uninterrupted future with a restored
future, since two restored copies cannot detect a shared deterministic omission.
The final candidate adds direct actor-state and bridge-Cell comparisons across
loading; the continuation receipt remains bounded because native loading
intentionally reseeds Scenario RNG and recreates Dummy. Integration with PR567
preserves its passenger schema and applies the open-topped range bonus before
coordinates, MinimumRange and the arcing split for Abstract and retained Cell
targets. Snapshot219 rejects the two incompatible earlier218 states. The merged
full retail suite and Clippy passed, including the native walk-cell/cargo-range
controls. All four explicitly ignored Battle Fortress production tests passed
(15.30 seconds). The single critic was not repeated.

## Local annotation receipt

The root owner reports saving and reading back Ghidra notes for ObjectGetCell
5F6960, low/high5F6B60/5F6B90, actual G27 start6F8682 and ScanCell lookup6F8984.
Existing function labels were retained. Program base400000 and the87-byte
ObjectGetCell body matched the SHA-verified image. These annotations index the
executed evidence above; they are not additional parity proof. The root also
created and read back TechnoStaticInit__LeptonsPerLevel6F2970 (49 bytes) and
TechnoStaticInit__BridgeHeight6F2A10 (39 bytes), indexing actual table entries
815058/815064 and the invalidity of the omitted-Techno-CRT exploratory fixture.
The original43 bytes at66CE2C also matched the verified image before a focused
MaxDamage reader note was added at66CE46, saved and read back from
RulesClass__ReadCombatDamage66BBB0. The existing BridgeStrength plate and
PlayerScatter note were preserved.
The single critic found the old6F7CE8 EOL still claimed the fire probe could run
after pure gates. Root replaced that claim with the actual pre-health query
ordering, verified the original33 bytes at6F7CE0, saved, and read back the exact
new EOL. `ghidra-early-probe-readback.json` preserves old and new comments locally.
