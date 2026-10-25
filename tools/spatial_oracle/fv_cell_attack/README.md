# Empty-FV paid bridge continuation

`paid_conditional.py` executes the original FV constructor, placement, Attack event,
mission dispatch, Approach, paid Drive movement, MoveSound/FireAt, Bullet flight,
bridge impact, Anim cleanup and live object-list mutations. It reuses the existing
`paid_world.Paid` and `anytown_damage.mission.Mission` owners. Native expected
values come from this execution; the vector generator only selects JSON values
and decodes little-endian actor, Bullet and Anim bytes.

The compact input declares the Scenario identity cursor **before** the actual FV
constructor, the original Scenario seed, GameOptions speed 3 and outside-world raw
RNG calls. The optional Stop command uses original TargetClass `6E6AB0` and simple
Event `4C65E0` constructors followed by full Event Execute `4C6CB0` before Logic 3. It
never writes a mission, target, movement, Bullet or impact outcome. Outside calls
execute original `65C780` and assert the observed word/indices; no RNG return is
substituted. Event-boundary RNG hashes are assertions, never state imports.

This is a selected-owner continuation under declared inputs. It does **not** prove
complete native Scenario startup/population, outside-object/global AI cadence,
keyboard or network event admission, renderer/audio output, every Unit/weapon, or
whole-bridge parity. The physical map/TMP/Cell/Terrain/navigation owner is rebuilt
natively on every replay; the separate native House/population boundary inherited
from Paid remains. The original isolated `paid_world` corpus and prior conditional packet remain
unchanged as historical evidence. Active conditional execution uses the current
`v26` outside-input boundary; Paid and huts use the corrected `drive_crt` packets
described below.
The separate [two-hut composition](huts/README.md) executes CABHUT construction,
collapse callbacks and admitted repair. It bounds the unaffected huts omitted by
the selected paid loop; full Scenario population and ordering remain outside it.

## Coverage and evidence

The six cases retain the original declarations and 336 states: healthy, damaged,
already-collapsed and repaired concrete bridges at seed 31; the historically
named `damaged_collapse` seed 3 control; and Stop during paid pursuit with an
already-launched missile. Under the current incoming schedule, damaged seed 31
collapses at Logic 32 (overlay 220 to 232), clears the target by next-frame 33,
and reaches Guard with all selected effects drained by next-frame 53. This is
the collapse/cleanup persistence witness. The unchanged seed 3 control instead
retains overlay 220 and Attack through next-frame 65, with shots at 1, 5, 55 and
60 and two live missiles at its declared horizon. It does not claim cleanup.
The other five cases drain their selected Bullet, Anim and deferred objects.
The packet retains 3,147 executed raw RNG words (492, 517, 511, 374, 629 and
624 respectively): 3,052 outside calls and 95 selected-owner calls.

The production FV/rules/restore tests include four snapshot checkpoints with
paired 64-frame continuations. Run them against the physical retail map, not
only the default library suite's non-retail fixtures.

The literal frame projection covers actor position/health, native identities,
mission/queue/status/counter, dispatch/rearm and passive-scan state, destination
versus accepted head, target presence, physical target-cell state, all live Bullet
velocity/arm/guidance fields, all selected Anim runtime fields and three RNG
buffer hashes. `track_state` additionally retains the Foot movement/blockage
timer start and duration, Drive residual, and raw native target-speed f64 bits.
Rust compares the latter through its documented SimFixed conversion; the native
packet retains the original unquantized bits. Timer gap dwords and raw heap
pointers are not gameplay comparison fields. The full compressed packet also retains raw actor/Drive/Bullet/
Anim state, complete RNG buffers, original ranged call results, every raw word,
ordered live callbacks, bridge-span snapshots, Event before/after and retirement.
The diagnostic comparison additionally checked full RNG buffers and every raw
word, stream, index and order.

Two discovered defects are protected by executed behavior. After collapse,
Mission_Attack queues Guard while Techno AI still reads committed Attack at
`6FA697`; the later Unit Commence owns promotion. A cleared target is not an early
Guard transition. Stop Event 6 clears target/destination but retains the mission
and accepted Drive head, so its original paid movement and existing missile
continue. Stop does not assign mission 13. `stop_event_sources.py` pins the exact
producer/receiver bytes and vtable/switch links; the conditional Stop case proves
its exercised runtime result. The independent [Temporal Stop consumer](../temporal_stop.py)
also executes the deferred beam-release deadline through the original Attack handler.

Production firing now runs at each Unit's original post-movement slot, before
its second Ready/Commence. That required placing Infantry firing in its own
slot too: otherwise a later Unit could kill an earlier Infantry before its
shot. The [original Infantry caller controls](../infantry_ai_order.md) establish
second Ready/Commence, fear, fire and sequencer ordering; mixed-object production
regressions exercise firing, fatal consequences and Temporal consumers. A new
Inviso Bullet detonates at its later live-list visit, after pre-existing actors.
Current main owns Infantry action, firing latch and Ready through its native
Infantry receiver and shared FireVisit owner. The full retained AttackMove
mechanism and remaining global AI compatibility hosts remain separate work.

The projection and full packet have immutable promotion digests. The compressed
publisher removes copied lexical INI text in favor of SHA256 and normalizes
checkout references; it refuses changed native output even with `--write`.

## Drive startup correction and preserved evidence

The native CRT caller at `7CD8B4` enters `7CBDAF`, which passes
`[812000,815DA4)` to original dispatcher `7CBED3`. The Drive slice
`[812D2C,812D64)` contains fourteen initializers `4AF330` through `4AF520`.
`Paid.attach_world` executes that original table before FV construction. Original
`4AF400` writes level height 104 to `8A07D0` at `4AF42B`; `4AF4A0` derives
bridge scale 416 at `8A07C4`. The packet records the table bytes, entries and
before/after values. No scale constant or native result is injected.

The prior fixture left both globals image-zero. This made the reached-destination
height test at `4B2196` reject arrival on frame 7 of five selected cases. Correct
startup avoids a redundant destination reissue, 2000 Cell admission calls (1840
for the already-collapsed case), two mark callbacks and one paid-points call.
Movement timer `[7,0]` becomes `[1,0]`, blockage timer `[7,60]` becomes `[1,60]`,
Drive residual becomes 12 rather than 0, and target speed remains 1.0 rather than
0.30000001192092896. Stop during pursuit has none of these differences. A current
helper control without the initialization exactly reproduced the entire old
healthy packet's gameplay/raw state and traces, isolating this prerequisite from
unrelated imported helper changes.

All 336 previously projected states, selected RNG words, shots, impacts and
cleanup results remain unchanged across the six corrected native executions.
The added track fields expose the previously omitted owner state. The complete
CABHUT gameplay projection also remains unchanged after corrected startup,
including all hut bytes, 104 AI visits, detach callbacks and admitted repair.

The CRT-corrected historical conditional packet is
`paid_conditional_drive_crt.json.gz`. Active packets are
`paid_conditional_v26.json.gz`,
`paid_world_drive_crt.json.gz`, and `huts/hut_joined_drive_crt.json.gz`; their
literal projections are `paid_conditional_v26_vectors.json`,
`paid_drive_crt_vectors.json`, and `huts/hut_drive_crt_vectors.json`. The old
packets, projections and promotion digests remain immutable historical evidence.
The pre-CRT packets used the superseded image-zero fixture; the corrected
conditional packet retains its valid historical outside-input boundary. Existing generator modules and shared
publication owners target the corrected versions; there is no compatibility
mode retaining the invalid initialization. Range/Approach/admission evidence
that does not use Paid is unchanged.

## Reproduction

Use the original `gamemd.exe`,
Unicorn 2.1.4 and local retail assets as described in
[`tools/native_oracle.md`](../../native_oracle.md). Set `VERA20K_GAMEMD_EXE`
(or `RA2_DIR`), `VERA20K_ANYTOWN_INPUTS`, `VERA20K_SHRAPNEL_INPUTS` and
`VERA20K_PROJECTILE_RENDER_ASSETS` to the corresponding existing local fixture
roots. No proprietary assets are bundled and no machine-specific path is a
runnable default in this package.

From the repository root:

```sh
python -m tools.spatial_oracle.fv_cell_attack.paid_conditional --check
python -m tools.spatial_oracle.fv_cell_attack.paid_conditional_vectors --check
python -m tools.spatial_oracle.fv_cell_attack.stop_event_sources --check
python -m tools.spatial_oracle.fv_cell_attack.paid_world --check
python -m tools.spatial_oracle.fv_cell_attack.paid_vectors --check
```

The production consumer is
[`bridge_fv_pursuit_tests.rs`](../../../src/sim/world/bridge_fv_pursuit_tests.rs),
`retail_fv_paid_pursuit_fire_impacts_and_cleanup_match_native`, using the ordinary
retail loader and simulation. Its optional `VERA20K_FV_PAID_EXPORT` diagnostics
support auditing, but are not inputs to the native generator and are not bundled
as goldens.

## Input provenance boundary

`paid_conditional_v26.input.json` contains only declarations, assertion hashes
and outside-call receipts. Its source catalog and capture receipt are sealed to
retained build `bridge-fv-current-v26`, commit `10c01f0e` and executable
SHA256 `9f61c3df261986cb9798c1ed094b891ffd4090ce958eec5de0ba0725b72f407d`.
The previous `paid_conditional.input.json` and its `bridge-fv-mission-v11a`
receipts are retained. All six declarations, seeds and frame horizons are
preserved; only the newly observed outside inputs define this new comparison.

Current main moved Infantry idle calls from a global suffix to the mission's
object turn. The existing [executed native idle controls](../anytown_damage/foot_missions.md)
establish the original `51CDB0` / `5216D0` / `7099E0` receiver's timer,
Doing, moving, prone and firing-latch admission. Guard caller `4D51A6` and
AreaGuard caller `4D6F2B` precede their cadence draw. These establish the producer
and gates, not native whole-world scheduling. The current capture's first idle
calls occur at frame 27; the old input's unconditional frame-one global suffix
is therefore a stale boundary for current production.

Repaired preparation advances the world before resetting command clocks/RNG;
TerrainSpawner phases remain retained. Current trees 858 and 699 first draw at
their Active midpoint on frames 5 and 17, then resume probability rolls. The
[Terrain owner](../../../src/sim/terrain_spawn.rs) skips the probability
draw while Active. Historical captures had different retained phases. The
capture does not establish the exact pre-reset cause or activation frames;
this is an explicit outside boundary, not newly inferred native terrain timing.

Actual test-only Logic-object context classifies the selected FV, its Bullets
and new Anims; their calls remain native, including FV Guard calls after
collapse. Only established outside global producers may have a null context.
Interleaving an outside call inside the selected native pass is rejected.
No producer whitelist was widened for the new capture.

The original native runner does not import production diagnostics, the extraction
helper, or Rust gameplay outcomes. Changing this boundary is a new comparison,
not permission to regenerate native expected behavior from Rust.

`capture_conditional_inputs.py` can audit the historical capture without reading
later checkout sources as the old build. It verifies the production export and
build-manifest identities, uses the sealed source receipts, reclassifies calls by
observed Logic ownership, and compares the complete reconstructed input:

```sh
python -m tools.spatial_oracle.fv_cell_attack.capture_conditional_inputs \
  --production-dir /path/to/retained/exports \
  --manifest /path/to/retained/build/manifest.json \
  --check-input tools/spatial_oracle/fv_cell_attack/paid_conditional_v26.input.json
```

A new capture uses `--declarations declarations.json --out new-input.json` in
place of `--check-input`. It requires the current source fingerprint to match its
retained build. Both modes remain separate from native execution and projection.
