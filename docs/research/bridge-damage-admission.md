# Bridge cell aim and damage admission

This evidence bounds the ordinary force-fire Cell target path: launch coordinates,
the impact-coordinate ladder, and the four bridge admission blocks in area damage.
It establishes their native arithmetic and callback order and compares those
results with the production Rust owners. It does not establish complete bridge
destruction, repair, or whole-bridge parity.

## Reproduction and corpus

The [harness](../../tools/spatial_oracle/bridge_damage_admission.py),
[284-case payload](../../tools/spatial_oracle/bridge_damage_admission.json), and
[provenance](../../tools/spatial_oracle/bridge_damage_admission.meta.json) use
Unicorn 2.1.4 and the retail `gamemd.exe` with SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
No executable is distributed. From the repository root, with the executable path
configured and no competing Cargo process:

```sh
export VERA20K_GAMEMD_EXE='/path/to/retail/gamemd.exe'
python -m tools.spatial_oracle.bridge_damage_admission --check
python -m tools.projectile_oracle.bridge_cluster_order --check
VERA20K_REQUIRE_RETAIL_INI=1 cargo test -p vera20k --lib bridge
```

`--check` is read-only; `--write` regenerates both evidence files and requires
review of any changed values. Follow the [native comparison workflow](../../tools/native_oracle.md).

| Rows | Executed original instructions | Rust comparison |
| ---: | --- | --- |
| 149 | Area-damage bridge blocks `0x00489E87..0x0048A2C4`, actual cell lookup and Scenario RNG | [damage_dispatch_tests.rs](../../src/sim/bridge_state/damage_dispatch_tests.rs): `all_original_bridge_damage_blocks_match_live_callbacks_and_rng_continuation` |
| 63 | Inner bridge selector `0x005871A5` to its selected leaf entry or no-match tail | Same file: `inner_driver_reselects_live_overlay_and_family_like_original_587180` |
| 10 | Ordinary FireAt and Bullet::Fire Cell coordinate calls | [bridge_launch_tests.rs](../../src/sim/combat/bridge_launch_tests.rs): `cell_launch_aim_matches_original_fireat_and_bullet_fire` |
| 14 | Actual Cell/Bullet impact-coordinate ladder `0x00468D80` to first detonation | [projectile_collision.rs](../../src/sim/world/projectile_collision.rs): `bridge_cell_impact_ladder_matches_original_ground_and_deck_receivers` |
| 48 | BridgeStrength read/store with three retained initial values | `damage_dispatch_tests.rs`: `bridge_strength_reads_signed_constructor_and_layer_values_from_native_corpus`; also executes the constructor's default store |

The native read-only checks passed. The production joined regression compares
all 72 caller rows, and the nested-receiver and signed-strength/save regressions
passed. The final full library run reported **9507 passed, 0 failed, 147 ignored**;
ignored tests provide no validation unless explicitly run. `cargo clippy -p
vera20k --lib` passed with 966 warnings, unchanged from the previous run. Both
commands used retail `ini/` and `VERA20K_REQUIRE_RETAIL_INI=1`.

The retail Grizzly regression `retail_grizzly_forcefire_freezes_the_native_cell_aim`
loads production rules/art and follows ForceAttackCell to the frozen shell target.
The release example [bridge_forcefire.rs](../../examples/bridge_forcefire.rs)
loads unmodified Hills through the production loader and ordinary runtime:

```sh
cargo run --release --example bridge_forcefire -- /path/to/retail
```

On seed `0x0B21D6E5`, MTNK at `(64,72)` fires at bridge body `(64,69)` with deck aim
Z1040 and retail BridgeStrength1500. The release run observed 121 Cannon shells,
flight, bridge collapse and target release at frame 7327. A separate release
`parity-digest` run loaded Hills and committed120 ordinary frames. These are Rust
production composition checks; no native whole-flight, whole-match or rendered
comparison is claimed. The ignored `bridge_live_chain_tests` regression follows
the same firing path and passed validated save/load against the pristine terrain
template, checking every retained bridge flag, runtime surface, collapsed
navigation, released target, and subsequent Cell aim.

Ghidra comments at `0x00489280`, `0x00486890`, `0x00587180`, `0x0066BBB0`,
`0x0070D4A0` and `0x00468D80`
were appended with this evidence and its limits, saved to `gamemd.exe`, and read
back exactly. Existing accurate function labels were retained.

## Cell ground and deck coordinates

Cell vtable `+0x48`, `GetCoords` at `0x00486840`, returns ground coordinates using
`ComputeGroundHeightAtCoord` at `0x0047B3A0`. Cell vtable `+0x58`,
`GetTargetCoords` at `0x00486890`, adds 416 leptons exactly when the Cell's current
raw flags at `+0x140` contain `0x100`. It does not consult walkability or an
object's OnBridge state. The original deck initializers execute with the supplied
104-lepton height scalar.

The ordinary Cell-target FireAt branch calls `+0x58` at `0x006FE1FF`; Bullet::Fire
calls it at `0x00468707` and freezes the result in Bullet `+0x140`. For flat level 2
the executed results are ground Z 208 and aim Z 624 with raw `0x100`; raw `0x400`
keeps aim Z 208. The corpus also covers a slope and signed level boundaries.
Out-of-retail-domain levels characterize accepted arithmetic, not map reachability.

The impact ladder differs: its initial strict `<32` snap and `DistanceTo <42`
comparison use `+0x48`; only the final nearby-target replacement uses `+0x58`.
At level 2, locations with Z 208, 239, 240, or 249 resolve to 624; Z 250 stays 250.
The 14 rows exercise seven locations for both real and supplied shared-dummy Cells,
including locations at deck height. Original Cell/Bullet vtables and DistanceTo
execute; duplicated dummy scalar state excludes intervening lookup mutation.

## Damage classification, height, and live continuation

`Apply_area_damage` at `0x00489280` reaches the measured block after its ordinary
receiver and Rocker phases. The measured prefix resolves the impact Cell before
checking Scenario flag `0x8000` and Warhead `Wall` at `+0x144`.

The four blocks are independent and retain native A, B, C, D order:

| Block | Admission | Called entry |
| --- | --- | --- |
| A | Anchor overlay `0x18/0x19`, or relative concrete tile in either BridgeMiddle band | `0x00587180` |
| B | Retained anchor's current overlay `0xED/0xEE`, or current relative wood tile in either BridgeMiddle band | `0x00587180` |
| C | Current impact overlay `0x4A..0x63` | `0x0057BAA0` |
| D | Current impact overlay `0xCD..0xE6` | `0x0057CCF0` |

A captures `(tile - concrete_base) + 1` before anchor lookup. When raw `0x100` is
set, raw `0x80` selects the impact Cell as anchor; otherwise the anchor allocation
comes from Cell `+0x2C`. Its current packed coordinate feeds actual GetCellClass
`0x005657A0`. The resulting allocation is retained across A's callbacks and B.
Each BridgeMiddle band includes offsets 0 through 3. There is no extra role or
axis admission gate. B reads live tile/anchor state after A.

Only raw structural `0x100` enables the A/B height check. With Cell `+0x11B`
interpreted as a signed byte, impact Z in leptons must satisfy:

```text
(level - 2) * 104 + 416 < impact_z <= (level + 1) * 104 + 416
```

The corpus executes both exact boundaries and adjacent leptons. A/B without
`0x100`, and all C/D calls, have no such height check. Both A and B call the same
inner selector, which re-reads live state; an outer block does not force its own
concrete/wood driver family. The 63 selector rows execute actual lookup/selection
to concrete `0x00576BA0`, wood `0x00571490`, either direct driver, or no-match
`0x00587388`, before executing the selected driver body.

Each admitted ordinary block calls original Scenario `+0x218` RandomRanged
`0x0065C7E0` with `(1, signed BridgeStrength)` and accepts only `roll < signed
damage`. Equality fails. The Rules `+0xFF0` Ion warhead identity bypasses that
draw. Ordinary admitted blocks call once; Ion A/B try the driver up to four
times, stopping on success. Ion C/D still call once.
Success calls Detach `0x0070D4A0` on the original impact Cell. A/B dirty the
screen once after their attempt loop even when every call returns false. Success
does not skip later outer blocks.

For seed 31 and strength 1500 the first two native rolls are 340 and 406. Signed
strength cases include 0 and -1 and pin their original rejection-loop draws.
Every row compares raw draw count, RNG indexes, and four following original RNG
values. Callback transcripts compare lookup, driver, supplied writes, detach,
and dirty ordering. No timer writes occur in these bounded admission/selection
blocks; timers inside excluded drivers and receivers are not covered.

## Ordinary bullet integration and RNG order

The separate [72-case full caller corpus](../../tools/projectile_oracle/bridge_cluster_order.json),
[harness](../../tools/projectile_oracle/bridge_cluster_order.py), and
[provenance](../../tools/projectile_oracle/bridge_cluster_order.meta.json) execute
original `0x00468D80` through `DetonateAtCoord 0x004690B0` and the full
`Apply_area_damage 0x00489280`, then return through cluster placement. This catches
an ordering error that isolated bridge-block comparisons cannot: the bridge
strength draw must finish before cluster distance `0x00469057` and direction
`0x00469067`, including `Cluster=1`. Recursive damage receivers finish before
their own area's bridge continuation; the outer area resumes afterward.

For seed 1/strength 1500, both damage 2000 and damage -1 draw bridge 1199, then cluster
466 and direction raw 3879883985; the next four raw values are 615944872,
594350709, 1828753889, 1900114041. Damage 0 skips the bridge draw. Native area entry
`0x004892BE` excludes **zero only**; negative nonzero damage still enters and
consumes admission RNG before the signed comparison rejects it.

The Rust receiver now calls one bridge continuation immediately after its area's
synchronous receivers, before animation and cluster continuation. Ordinary
bullets and nested death blasts share this owner. `DeathEffects` carries only a
collapse notification; no deferred bridge-event queue remains. Bullet AI, the
live object turn and the combat tail forward that notification to the frame
result while bridge navigation has already been published synchronously.

The native caller corpus covers four seeds, signed damage, Cluster 1/3, zero and
negative cluster counts, bridge/warhead/scenario gates, signed strengths and
repeated admissions. It supplies empty object and AirTracker lists and zero
special-warhead, Shrapnel, Rocker, AnimList and MaxDebris fields. Original math,
Cell virtuals, lookup, animation selection, air collection and RNG execute.
Only bridge-driver false returns and rendering projection/dirty sinks are
substituted. Thus it establishes ordinary empty-receiver caller/RNG composition;
it does not demonstrate nested-death or successful-collapse native execution.

The nested Rust regression uses a negative inner death blast followed by a parent
with damage 29000 or 31000. The [scalar RNG fixture](../../tools/projectile_oracle/bridge_nested_draws.json)
executes original ranges 1..65536 from seed 1: 28374 then 29871. Only the second result
may gate the parent's bridge attempt. This distinguishes inner-first from
parent-first execution without a callback hook; it remains scalar native evidence
plus a production Rust nested-receiver regression. BombClass's direct area call
`0x004387A3` likewise precedes animation and has no Weapon requirement, so it uses
the same bridge continuation.

## BridgeStrength reader

Rules constructor store `0x006675DA` sets `Rules+0x1740` to **1000**. The
CombatDamage reader at `0x0066CD66` supplies the current field as the default for
the exact key `BridgeStrength`, calls original ReadInt `0x005276D0`, and stores
EAX unchanged at `0x0066CD86`. It is a signed 32-bit value with no clamp or
narrowing. Retail RULESMD supplies **1500**; a missing later-layer key retains the
earlier value. The production field and damage inputs therefore preserve signed
values rather than clamping into `u16`. A production reader → runtime constructor
→ saved/restored bridge state → live dispatcher regression compares strengths
-1,0,65536 against the native RNG cursors and four-value continuations.

The 48 rows cover three initial values and 16 absent, decimal, hexadecimal,
overflow, malformed, or blank inputs. Original cached INI indexes are supplied
using the original key CRC; original ReadInt and its callsite/store execute.
45 rows compare the production RulesMd-to-Scenario layered reader. The other
three supply an already-cached empty string, which ReadInt turns into zero: the
physical INI loader drops blank values, so those rows compare only the production
scalar parser and do not claim physical-file reachability.

## Substitutions and remaining dependencies

The outer oracle supplies interior registers/locals, flags, tile globals, explicit
Cell/anchor data, and the input impact. Actual cell lookup and RandomRanged run
unchanged. Driver entries `0x00587180`, `0x0057BAA0`, and `0x0057CCF0` return
declared booleans and apply only declared scalar callback writes. Those mutations
prove the outer continuation's live reads, not stock driver reachability. Detach
and screen-dirty calls are recorded sinks; CoordsToClient supplies `(0,0)`.
Consequently this corpus proves neither detach lifecycle effects nor rendered
dirty rectangles. The separate inner-selector rows have no substituted selector
calls, but begin after its dirty-vector clear and stop before any leaf body.

The first chain leaves the whole-bridge goal open. Required surrounding work
includes the following; its existence is not evidence that these paths are rare
or safe to omit:

- Direct/ramp and wood drivers still need complete synchronous publication and
  occupant/fallout comparison. The current world host retains deferred
  `StateOutcome` paths alongside the concrete body publication owner.
- Collapse `MetallicDebris` construction, constructor RNG, and landing damage
  remain missing in the orchestrator. Existing slot draws do not substitute for
  creating the debris and following its effects.
- Tagged bridge-collapse Event 31 delivery remains a no-op; untagged rim
  comparisons cannot demonstrate scenario-trigger consequences.
- The bridge-hut no-overlay fallback retains a starter/anchor heuristic; native
  `0x00574C20` / `0x00574000` and repair/rebuild consequences require completion.
- The bridge-layer passive-target gate and the area-damage Rocker selected-plane
  producer require their own connected admission/effect comparisons.
- Other area producers require closure. The lethal `SpawnsTiberium` Terrain path
  reaches a 100/C4 call at `0x0071BABF`; ordinary tree deaths do not. Its current
  Rust collector lacks the bridge tail and uses cell-level height rather than
  the retained Object location. A permitting C4 warhead can therefore omit
  bridge RNG/effects when that path runs. Missile area producers also need their
  caller/coordinate and bridge continuation audit.
- Map topology, movement/placement, rendering/visibility, AI, persistence, and
  objects on or beneath each bridge family still require the whole-bridge audit.

The authoritative implementation owners are
[damage_dispatch.rs](../../src/sim/bridge_state/damage_dispatch.rs), its
[world host](../../src/sim/world/bridge_damage_dispatch.rs), the
[projectile collision owner](../../src/sim/world/projectile_collision.rs), and
[bridge_orchestrator.rs](../../src/sim/world/bridge_orchestrator.rs). The oracle's
callback boundary must remain explicit when extending claims beyond admission.
