# Combat parity acceptance and continuation

## Objective and acceptance

Make VERA20k combat reproduce active-retail `gamemd.exe`: targeting, attack
execution, weapons, projectiles, damage, special warheads and destruction,
including all required dependencies. Missing behavior and contradictory evidence
keep the goal open. Commit, publish and merge validated increments.

Before implementing each mechanism, identify native entry/callers, retail inputs,
observable outputs and the Rust production route. Acceptance covers the complete
chain: prerequisite state and ordering, action admission, effects and downstream
consumers, cleanup and save/restore continuation where retained state is affected.
Native execution witnesses and production regressions must declare their coverage;
samples and isolated helper tests do not certify the whole combat system.

The final owner audit must account for combat orders/acquisition/retaliation,
movement and facing prerequisites, weapon selection/reload/bursts/fire timing,
launch coordinates/scatter/guidance/collision, damage and armor, each active special
warhead chain, and destruction/detachment/secondary effects. Audit retail infantry,
vehicles, aircraft and buildings and their interactions. Resolve required omissions
and incorrect behavior; recording them is not acceptance. Follow AGENTS.md for
numeric policy, validation and the single pre-PR critic pass.

## First mechanism: infantry facing at fire-sequence start

Baseline: `bf422762`; branch `feature/combat-native-parity` in the task-owned
`engine-ownership-boundaries/ra2-rust-game` worktree. The primary checkout's local
instruction edits are preserved separately.

Native evidence inspected: `InfantryClass::AI` calls `Fire_At_Target` at
`0x0051BF59`; `Fire_At_Target @ 0x005206B0` tests the pending-fire byte `+0x68D`
and GetFireError before selecting a fire action. At `0x00520904..0x00520925`, it
sets the byte, derives direction through `0x005F3DB0` (both objects' `+0x48`
coordinate getters) and calls `FacingClass::UpdateFacing @ 0x004C9300` on body
`+0x388`. A pending sequence bypasses that update; emission later tests the art
fire frame and rechecks legality. Ghidra's `bHasReachedDock` field rendering is
misleading for this infantry use; confirm and annotate the role without inventing
a universal shared-field identity.

Acceptance before implementation:

- Infantry attack/force-fire orders and retaliation do not snap facing early.
- A legal new fire sequence publishes the lepton-precise body facing through the
  existing FacingClass owner, including standing, prone and deployed sequences.
- Pending fire frames keep the start-facing; refusal/cooldown does not start a
  turn. Zero-delay shots use the new facing immediately in FLH and fire events.
- Entity and cell targets use their proper coordinates, including building
  foundation centers. Existing vehicle/structure/aircraft behavior is preserved.
- Reproducible original-code comparison covers the facing update and its guards;
  production combat/frame regressions cover order, sequence and emission paths.
- Relevant Ghidra annotations are saved and read back. Focused tests, the full
  library suite and library Clippy pass before the one independent PR review.

## Accepted first mechanism (PR #439)

The implementation moves Infantry facing from entity/cell orders and
retaliation into the fire-start receiver. It updates the existing body FacingClass
and the current emission snapshot. Fire coordinates now retain the snapshot's
full body heading instead of discarding its low byte before FLH.

`python -m tools.spatial_oracle.infantry_fire_start --check` passes: 59 original
prefix cases, with executable SHA-256 and fixture substitutions recorded in its
sidecar. Thirty-three heading cases feed Rust receiver comparisons, including
building/cell coordinate getters and a live-turn equality edge. The remaining
native witnesses do not certify the supplied legality/action receivers.

`cargo test -p vera20k --lib` passes: 9,092 passed, zero failed, 134 ignored.
All ten added tests pass, including full-frame firing/damage and both immediate
and pending-action save/restore continuation. The fixtures bind sight, action
sequences and flat terrain through existing owners; restore validates object
membership and rebuilds map-derived caches. Existing global replay pins pass
without rebaselining. Output: `%TEMP%/vera20k-combat-lib-tests.log`.
`cargo clippy -p vera20k --lib` passes for the corrected candidate (1,031 warnings),
recorded in `%TEMP%/vera20k-combat-clippy.log`.
Source checkpoint: `3621bac4`. The module map was regenerated for that commit;
its production dependency edges are unchanged. The single critic pass completed.

The critic found one required prerequisite defect: a new fire sequence could turn
and fire during an accepted Walk step retained by an attack order. Confirmed the
production chain from `finish_ordered_walk_attack` through the retained head and
the Foot speed writer in `walk_head::finish_fresh_head`. The firing receiver now
checks the existing `FootSpeedState.applied_fraction` against native's strict
`> 0.1` predicate, including the pending fire-frame refusal cleanup. No additional
stored speed or movement boolean substitutes for it. The original-code corpus
`infantry_fire_speed` passes eleven comparison/return witnesses, including both
adjacent fixed-point values and binary64 threshold neighbors. Added Rust checks
cover the eight representable inputs, pending cancellation and a real movement
order followed by attack during the accepted step. Focused and full library tests
pass for the correction. The owner validates fixes; do not request a second critic pass.

The critic also identified an ownership improvement: several older snap callers
publish the requested byte heading instead of sampling `body.current()` after
the native equality branch. No ordinary production equality trigger was established
through those callers. Audit `track_host`, `walk_head`, `animation`, `infantry` and
`jumpjet_cruise` when consolidating body-snap/mirror publication; the new fire-start
writer already samples the actual resulting heading.

Ghidra: `ObjectClass__DirectionToTarget @ 005F3DB0` named and its plate, plus the
`InfantryClass__Fire_At_Target` plate, saved and read back. Original Infantry
vtable `007EB058+3CC` points to `0051DF60`: the raw-byte writer at `0051DF70`
clears `+68D` before the call to `TechnoClass::Fire_At`. This confirms the
pending-sequence reset on emission. Its label/comment and the snap equality
comment at `004C937B` are saved and read back. Also labeled/commented the original
body-facing getter `004E0150` (Infantry vtable `007EB058+2A8`), which reads `+388`
through FacingClass::Current and feeds GetFLH. Saved and read back that annotation.
Corrected the old research report which called infantry fire-start a smoothed Set.

Other open leads: special detonation effect bodies, launch/guidance inputs,
fire-legality residuals, scanner and locomotor prerequisites. Native
`InfantryClass::GetFireError @ 0051C8B0`, selected by vtable `+3C0`, checks common
Techno legality first, then refuses when Foot `+578` exceeds binary64 0.1
(`0051C9B8..0051C9C9`, error 7 at `0051CAFA`). This speed gate is the review fix
above; its Ghidra label/comment were saved and read back. NavCom/action interruption
and locomotor-specific gates remain open. WalkHost supplies completion/boundary
transactions and `finish_fresh_head` sets applied speed to one; completion clears it.
At PR439, the paid step still reached `movement_tick`'s `advance_lepton_position` adapter,
contradicting `movement_step`'s native-polar-step claim. Porting its original numeric
step remains required. These are leads, not a complete inventory.
FacingClass::snap's live-angle equality discrepancy is now corrected
against original-code execution: cancel the timer while retaining the old target.

Published and merged as PR #439, merge `d47a9247`; final source correction
`c0b854a9`. No Cargo process or review remains pending for that increment.

## Next mechanism: paid Walk step used by combat approach

Current branch `feature/combat-walk-step`, based on refreshed `origin/main`
`d47a9247`, in the same owned worktree. The whole-combat goal remains open.

Native `WalkLocomotionClass::ProcessMovement @ 0075AEC0` takes the paid-head
arm at `0075BD25`. Outside the `<17` completion arm and owner movement refusal,
`0075BFA9` sets Foot speed fraction to one, queries Infantry's movement speed
through `+538 -> 00521D80`, computes the head direction and calls the actual
Walk facing setter at `0075C035`. `0075C067..0075C0CB` computes signed-heading
sine/cosine displacement and truncates the final world coordinates. The result
selects the existing same-cell or boundary transaction. At the branch baseline,
Rust reached `advance_lepton_position` and a direct normalized vector instead.
The source comment claiming a live native-polar WalkHost dispatch was incorrect.

Acceptance before implementation:

- Original instruction witnesses cover the numeric step, speed input, direction
  update and resulting same-cell/boundary decision, including diagonals and small
  speed/offset contrasts; distinguish supplied speed from the full getter chain.
- Production Walk pays this step once per native visit through existing Foot,
  facing and coordinate owners, preserving completion, occupation and height
  transactions. Remove the competing generic Walk calculation and false claims.
- Reuse/complete live speed prerequisites, including Infantry's prone override;
  do not duplicate a track-owned speed calculation as another Walk authority.
- Demonstrate approach-to-fire behavior and save/restore through production
  commands/frames; validate deterministic SimFixed math and documented rounding.
- Native corpus checks, affected production tests, full library tests and Clippy
  precede one fresh critic for this new PR. Integrate accepted work and continue.

## Accepted Walk increment (PR #440)

Task-owned worktree: `C:/Users/enok/.codex/worktrees/engine-ownership-boundaries/ra2-rust-game`.
Branch `feature/combat-walk-step`, source HEAD `c80821f3`, including the runtime
port `fe7df21a` and the validated critic correction. Fetched main remains `d47a9247`.
The primary checkout's local instruction edits are untouched.

Production Walk now uses `walk_step::advance`: set Foot fraction, resolve live
speed plus the Infantry prone override, clear the blocked latch, snap the full
body heading and pay native table displacement. FacingClass survives arrival.
The normalized-vector Walk arm and its old budget helpers are deleted. Shared
speed resolution moved from Track/Drive to `foot_speed`; Track, Walk and Tube
consumers use it. Other locomotors retain their own execution paths. Existing
same-cell height and boundary/completion owners remain in place.

Native `walk_paid_step --check` passes 40 original-code vectors. Its first 35
rows are unchanged, including cardinal transverse rounding and live-turn equality
(the body may retain its old destination while displacement uses the new SI
heading). Five additional native outputs carry the production Slice6 head
through five consecutive steps. The Rust full-frame replay asserts those
coordinates/headings. `walk_direction_table --check` exhausts all 65,536 direction
words, comparing original lookup indexes, table bits and the extracted retail
table. Rust's integer lookup test passes; no new runtime x87 emulator is added.
The corpora supply the movement-speed integer and do not certify full Process
admission, placement or the complete Foot getter.

The initial full library run had 9,089 passes, three failures, 134 ignored.
The new native tests and production move-to-fire/save-restore regression passed.
Two prone expectations used stale request speed 11 instead of live type speed 10;
they now expect native prone results 7/15 and verify the Foot cache remains 10.
The Slice6 hash change was independently attributed against a rebuilt ec27dc26:
only E1 changes from frame 12, tanks/RNG match on all 16 frames, and replacing only
E1 restores both prior hashes exactly. See
[the receipt](../research/COMBAT_WALK_REPLAY_ATTRIBUTION.md). Its pins are updated
with native per-frame assertions; the global replay pin required no change.
After the single fresh read-only critic pass and owner correction, the final
full library run passes: 9,093 passed, zero failed, 134 ignored. Library Clippy
passes with 1,027 warnings. Final logs are preserved in the owned worktree's
`.local/walk-validation/lib-tests-final.log` and `clippy-final.log`.

The critic found a competing completion-facing writer: the generic next-cell
configuration changed the displayed byte toward the next cell while leaving the
body FacingClass unchanged. Walk completion now advances only the execution path
cursor. Original75BD70..75BF82 contains no movement turn; fresh acceptance75BC97
and the paid step75C035 own it. The new regression runs world completion through
Mark/PerCell, then a refused and accepted next-head selection at a corner, checking
both heading representations. Native completion (42 rows), paid step (40 rows)
and exhaustive direction selection were rerun successfully. The75BD97 annotation
is saved and read back. No second critic pass is needed or requested.

Ghidra comments at 75BFA9/75C067 preserve the paid-step comparisons; the exhaustive
lookup note is saved/read back. The movement-refusal prerequisite also exposed a
false source comment: BoomerTorpedo uses APSplash2, Robogun uses AP, and stock
EMPuls is annotated disabled. Techno70EFD0 reads +504>0; Unit746C90 also tests
+6D8!=-1 (not the claimed DeployTarget+6CC). Its corrected plate is saved/read
back. EMPulse::Apply4C54E0 writes +504 but its constructor4C52B0 has no observed
incoming Ghidra references. This does not prove unreachability; creation/load and
+6D8 identity remain open. The cloak comment no longer asserts invented stock
EMP effects. No EMP behavior was added or declared complete.

Speed prerequisite work remains: House50C050 selects HouseType +0x128 (Infantry),
+0x12C (Unit) or +0x130 (Aircraft) multipliers; Foot4DB1A0 also consumes crate+580 and conditionally
halves Unit flag-carrier speed. Current `foot_speed` preserves the existing
fixed projection and FASTER handling; those missing production inputs are not
closed by `track_speed_native`'s isolated oracle. Infantry521D80 then applies
prone adjustment, which uses the existing `infantry::apply_prone_speed` owner.
NavCom/action legality, launch inputs, special warhead effects and the final
whole-combat audit also remain required; this increment does not close the goal.

Published and merged as PR #440, merge `a37e8118`; final PR head `20bf1af0`.
The local
comparison checkout `.local/walk-baseline-ec27` retains only a temporary test
probe; it is not a second implementation to publish.

## Current dependency: Aircraft firing and Fly state

Task-owned worktree: `C:/Users/enok/.codex/worktrees/engine-ownership-boundaries/ra2-rust-game`.
Branch `feature/combat-foot-speed`, based on merged PR440 (`a37e8118`).
Current validated source: `4a32f4c3bd6a29d1ca9b531cac71b1fe6143853b`: shared fire emission separated from
admission, with original Aircraft release/entry witnesses and corrected native
class identities. This is preparation for the open aircraft mission firing and
pending-ammo migration, not a shot-behavior completion claim. Previous checkpoint:
`0cbfa458`. Range source `d7c155501e7f260024dc965b415b2c3aa8431979` remains intact,
including native0x0 foundations. No tracked implementation WIP remains after this
checkpoint commit. The dependency map is retired by user instruction: do not use
or refresh it, regardless of the older branch contract text. Facing source
`be8e2d62ce1fcdfc6d968954204878670f6ebd38` and evidence
`ae5b3042e66091c584b1bc2335272fca6f360cbc` remain intact, including80 original
Fly takeoff callbacks. Aircraft attack-state, emission and Fly prerequisites below
remain open; the range comparison is not whole-aircraft parity.
Carryall move decision source: `450c408bd78490e5a2f42b0b392144ceb91271b1`;
Display fixture correction: `f4347849`;
integer Fly height source: `fa615a67a48be364e890b2801e52cdaf28283cdc`.
Prior FlightLevel source: `bae495bff341bcccc905a3b97d8f13068adf0a9e`.
`c3e4876f` migrated Ground rendering/picking to retained Display;
`17520993` implemented the current animation Display lifecycle.
No PR or critic pass for this branch. Production pickup and the complete Display
consumer migration remain unfinished. Preserve the primary checkout and untracked
`.local/`. The prompt-writing request is finished; continue the active combat goal.

Acceptance remains movement pickup through native selection, eligibility,
trigger, removal/replacement, effect and subsequent movement/combat, with matching
RNG, returns and save/restore. Display must preserve registration, resubmission,
removal and single-pass Ground ordering through actual consumers. Isolated crate
effects or partial registration do not complete pickup.

### Implemented dependency state

`FootSpeedState` owns binary64 +580, initialized to1.0; the speed resolver consumes
it before FASTER with separate native truncations. Drive/Ship, paid Walk, Hover
and tube exit use the live getter. Walk attack survives production save/load.
Native `crate_speed_effect` has23 recipient loops plus Foot queries; `crate_pickup`
has28 dispatch cases; `track_speed_native` has75 getters and116 Drive/Ship prefixes.
Selection/removal helpers still lack their production movement caller.

`world/display_layers.rs` owns five private ordered vectors and a rebuilt lookup
index. Submit4A9720 removes prior membership, inserts Ground before the first
strictly greater GetYSort, and appends other layers. Remove4A9770 preserves order.
MainTick55DBC8 performs one adjacent Ground sort551A30 before Logic55DC9E.
Entity Reveal/Conceal and Jumpjet Process use this owner; Jumpjet compares live
queries before/after Process, independently of cached membership. Terrain and
particles register Ground, Flat bullets Surface, other bullets/debris/waves Air.
Mixed comparisons borrow live stores, including shared Building render coordinates.
FireAt/shrapnel carry effective Flat into admission; retirement removes Display.

Snapshot184 adds Anim marking, retained instance YSortAdjust and Display history.
Ordinary and map-load constructors copy type+340 to instance+104 (422137); both
admit through Display with actual Rules and the currently bound load ART.
GetCoords422BE0 is shared by ownership, sound and sorting. GetYSort422BC0 adds
retained +104 to owner-resolved X+Y with wrapping integer arithmetic. Presentation
now consumes this retained adjustment. Next424801 replaces the type without
recopying +104; Start's Mark(REDRAW) is not parent Display resubmission. Current
type layer and registered layer can differ; animation routing now reads retained
Display membership rather than querying the current type again.

SetOwner424B50 gates old-owner Remove/Submit on entry +74; attachment always
removes, stores owner-relative coordinates and submits Ground. Expiry425150
instead removes Display, clears+CC, sets+19B and Mark(REMOVE), leaving relative
coordinates unchanged. Scalar destructors clear ownership without SetOwner or
intermediate resubmission (422961). Feedback remains outside synchronized Display.

Building damage fires now follow owner expiry before destructor Destroy43BDE8.
Recovery43FCAC still destroys attached fires. Owner callback710410->5F6DA0 does
not clear damage-fire slots; the Anim's own expiry reaches Building44EA45.
A derived reverse link comes from Building+5C8 slots, is rebuilt/validated on
restore, and is neither serialized nor hashed. This avoids scanning all entities
for every muzzle-animation deletion. Slot targets must be distinct live Anims.

Anim Layer now uses native five-name lookup477050/48E050, capacity128: default
Air3 (ctor4276D4), invalid/numeric tokens -1. YSortAdjust uses ReadInt5276D0 at
428147 with ctor default0. Neither unknown nor omitted Layer silently means Top.

Prior source `3aee9c65` ports per-pass Bullet ART Flat: current raw Image read
with empty default/capacity25 at46C1E8 gates Flat46C28D; ctor false and retained
prior on missing/invalid values. No type-ID fallback or ART Image redirect.
The existing rules processor owns retained Flat across registry handoffs and
feeds the shared RuleSet constructor/config hash. Trailer shares the Image gate;
ObjectRead Voxel remains a distinct binding. Other projectile ART readers and
flattened rendering Image are not certified by this change.
Global JumpjetControls CruiseHeight: ctor665C3A default400, reader67447E, retail500;
Object5F4260 uses this Rules field while Jumpjet54B8D0 uses linked loco+2C.

### Current Display consumers

Simulation and SimView expose read-only Display vectors separately from the
explicit `logic_order` used by the existing radar consumer. Ground builders
use only the Ground vector; the planner restores its ranks after class/atlas
emission. Removed presentation coordinate/YSortAdjust copies, the full Y-sort,
and entity-store fallback. Class key calculation remains in `display_registry`.
Entity rendering/picking traverse Display; band-box preflight retains its separate
bulk live-Building exception. Conceal and equal-key resubmission affect consumers.
Animation iteration/canopies use Display, and destination uses historical layer.
Top SHP ranking now reads Top membership, but absent-member fallback and separate
upper VXL/SHP/effect buckets still make upper rendering incomplete.

Original Tactical Draw6D3D10 calls6D8DB0 at6D465F. The latter reads the five
vectors at8A0360 (stride18) in forward order6D8F19..6D95A9, without GetLayer or
Y-sorting. Building postpass after Ground and the later object+110 pass remain
separate. Annotation6D8F39 was saved/read back. No binary/signature/boundary edits.

A native relocation/adjacent-sort history now runs through the frame-tick entry,
entity-picking candidate order and actual Ground atlas lowering. The GPU regression
uses that original history after Display save/restore, with overlapping atlas
sprites where a full sort would change a visible pixel. These are bounded ordering
checks, not complete rendered gamemd parity.

### Evidence and validation

Previous FlightLevel source `bae495bf`: full `cargo test -p vera20k --lib` **9,113 passed,
0 failed,135 ignored**,30.45s (`.local/flight-level-all-tests.log`). The three
new regressions cover all30 native reader/getter cases, locomotor type selection,
and production paradrop height/Foot coordinates/cargo across bincode restore.
Replay pins are unchanged. `cargo clippy -p vera20k --lib` passed with1,033
warnings in53s (`.local/flight-level-clippy.log`). Fresh original-executable
`flight_level --check` and `fly_height --check` both passed. Two initial compile
attempts used nonexistent/private fixture mutation APIs; the final fixture now
supplies FlightLevel through its actual INI text. All owned Cargo runs are terminal.
The new rules reader still needs a fresh release retail map load before merge.

Previous Display source `c3e4876f`: full `cargo test -p vera20k --lib` **9,110 passed,
0 failed, 135 ignored**,18.38s; `.local/display-consumers-tests-4.log`. Existing
replay pins unchanged. Earlier attempts corrected the new integration test's
layer placement (sim must not name app/render, including tests) and fixture
interner; the final test lives in the app boundary and drives `advance_tick`.
Building key/INI coverage now exercises Display submission at its sim authority.
Four `terrain_ground_gpu_tests::` passed with `--ignored --test-threads=1`,1.21s,
including retained-history color overlap, vehicle shadow ordering, mixed
TMP/SHP/VXL destination edits and20k batching. Log `.local/display-consumers-gpu.log`.
`cargo clippy -p vera20k --lib` passed with1,033 warnings,35.40s;
`.local/display-consumers-clippy.log`. Fresh original-executable checks passed:
`crate_ground_membership`6 histories, `display_anim_owner`24 histories and
`display_non_entity`9 cases, with `VERA20K_GAMEMD_EXE` explicitly bound to the
verified retail binary. No corpus files changed. All owned validation processes
are terminal. This source increment has GPU ordering proof, not a new successful
whole-game capture; the prior release-load result and capture limitation follow.


Previous animation source: full `cargo test -p vera20k --lib`: **9,113 passed, 0 failed, 134 ignored**.
Log `.local/anim-display-tests-3.log`; existing replay pins are unchanged.
Earlier attempt exposed three stale/synthetic fixture assumptions, corrected
against original expiry behavior and the restored reverse-link invariant.
Previous animation source: `cargo clippy -p vera20k --lib` passed with1,033 warnings in1m27s.
Log `.local/anim-display-clippy.log`. All owned validation processes are terminal.
User amendment: the dependency map has been removed from GitHub main and is no
longer used. Do not consult or refresh it, even if this branch's older contract
still requests it. No dependency-map refresh was run for this increment.
Release `cargo build -p vera20k --release --bin vera20k` passed (27 warnings,
3m10s; `.local/anim-display-release-build.log`). The built executable SHA256 is
`30cde1ce3aefd13f1e03cbe2ffacb2d833bef6da841b85d7e38268545112becc`.
The sealed Soviet `radar-online-v2` capture loaded retail `Fight.MAP` through
the app, built its render assets and reached production tick1. This exercises
the changed Layer/load-context, Flat and CruiseHeight readers in a release
retail load; it does not compare their resulting values against gamemd.

The full capture is **INVALID**, with no final frame: its first-deploy assertion
expects facing128 immediately but sees the original facing64. The script also
requires a second deploy command. These assumptions predate native MCV mission
continuation (`ff38eb7c`, already in the branch base): current `issue_order`
queues Unload, `start_turn` uses FacingClass and one command completes later.
The full lib suite passed `one_command_turns_and_converts_all_stock_mcv_types`
and `turn_completion_converts_before_the_next_mission_retry`; a fresh
`python -m tools.mcv_deploy_oracle --check` passed. This is a stale capture
contract, not evidence to restore instant turning. Modernizing its script,
sealed timing expectations and validator remains a tooling follow-up; do not
silently weaken v2. Retained run, failure manifest and stderr are under
`.local/anim-display-retail-soviet/`; the owned child exited1 without timeout.
All owned build/validation processes are terminal.

Original executable comparisons for the preceding animation increment:
- `display_anim_owner --check`:24 six-step histories, all five vectors, signed/
  wrapping sort, marked/unmarked detach, moving owner and expiry. Rust consumes
  every step and checks production save/restore. Building coordinates, multiple
  attached Anims and full constructor/AI execution are outside this native corpus.
- `anim_layer_rules --check`:19 original reads; all names, absent/empty, case,
  unknown/numeric values and supplied lexical trimming. Rust consumes all outputs.
- `anim_damage_fire_expiry --check`:8 Building slots, original full Anim expiry
  and real owner callback, then the Building Anim-reference tail44EA07. This
  excludes the inherited listener prefix, full destructors and sound IO.

Rust production regressions additionally cover building damage-fire UnInit,
retained slots/relative coordinates, save/restore before destructor, subsequent
Destroy/deletion and sound-event coordinates; muzzle-firer expiry; presentation
sort adjustment and no-layer routing; malformed slot-graph rejection. Native
corpora and these regressions do not establish complete Anim AI/render parity.
Previously passing native dependencies include Flat ART38, non-entity Display9,
entity layers88 and crate Ground histories6 (see prior source and saved corpora).

Ghidra annotations are saved/read back at4276D4,427DF2,422961,428147,425180,
43BDE8,44EA45 and424801. Prior corrections include Anim Mark4238B0 (formerly
ProcessCloakMode), non-entity GetLayer468B90/62FE80/75F890/74A960 and Fly
ILoco_Process4CCB40 (real GetLayer4CFCF0). Further original body/caller/table
inspection corrected `JumpjetLocomotionClass__State5_Touchdown` at54CA90 to
`JumpjetLocomotionClass__State5_Crash`: table54B19C index5 calls54CA90, while
index4 calls normal descent54C550. Comments54CC0D,4CD4E7 and41CC67 are saved
and read back. No binary, signature or boundary edits.

### Fly height migration and required continuation

Current implementation moves Fly's integer target+38 and takeoff/landing+50/+51
into private `FlyRuntime` fields in its locomotor payload. Constructor target is0;
accepted takeoff resolves the type's FlightLevel. Common `air_phase`,
`target_altitude`, `climb_rate`, the 300-lepton rate constant and `tick_altitude`
are removed. Jumpjet continues using its own parameter/runtime owner; Rocket
and Parachute remain separate. Snapshot185 saves the new Fly fields, including
stashed payloads, and hashes all three. Fixed separators preserve old non-air
hash slots; no replay golden has been changed.

`air_movement` now calls original-range integer stepping after committed XY.
Object Z is authoritative and the old SimFixed altitude is only a saturated cache.
Rules supply IsDropship and effective FlightLevel; Aircraft cargo supplies the
FirstPassenger predicate. Missions derive their compatibility phase from exact
physical height. Their repeated current-target/3 attack update is removed:
4CF3D4..4CF4CF instead selects destination-relative height, conditional
IsDropship approach height or Type FlightLevel. The full native horizontal target
selector is STILL REQUIRED; removal of that invented writer does not implement it.
All aircraft/docking/paradrop/spawn-manager destination calls use the existing
Simulation destination owner. No new independent mission altitude field exists.

`tools.spatial_oracle.fly_height` now executes144 original4CDD0D..4CDFBC/4CE145
steps with real Aircraft/Unit vtables, QueryInterface and GetHeight/SetHeight.
IMPORTANT evidence correction: the previously committed136-case fixture supplied
the cell-table pointer but omitted its length, so ground and bridge lookups used
Dummy. Seven old outputs changed after initializing Map+140; bridge/slope claims
from the earlier corpus are invalid. Read-only observers now require every ground
and bridge query to select the real fixture cell. Eight added cases cover signed
heights and40000/65536 targets beyond I16F16. No instruction/call is substituted.
The comparison excludes full Process admission, preceding XY/crash relocation,
following drift/speed, phase, sounds/animation and Display transactions.

Climb is min(delta,IsDropship?16:hasPassenger?10:20). Ordinary descent clamps its
step to20..50 and can undershoot nonzero targets. Bridge normalization and the
SetHeight-before-OnBridge-clear ordering are preserved. Rust's kernel consumes
all144 outputs. The production cell transaction consumes132 healthy outputs;
health<=0 belongs to the earlier unported crash/fall controller. Additional tests
cover saved target/flags/cargo, eight-frame continuation, hash distinctions and
repeated attack mission visits. Full `cargo test -p vera20k --lib` passed:
**9,116 passed,0 failed,135 ignored**,34.31s, `.local/fly-height-validated-tests.log`.
`fly_height --check`144 and `flight_level --check`30 passed. Earlier failures
were the omitted native table length, a stale snapshot-version assertion,
mapless-ground fallback and an incompletely populated terrain fixture; those
are fixed. `cargo clippy -p vera20k --lib` passed with1,027 warnings in1m23s
(`.local/fly-height-clippy.log`). All owned validation processes are terminal.
This is not complete Fly or combat parity.

Type+C95 IsDropship: ctor7113B9=false, reader712350..712373 key84447C; no stock
rulesmd type sets it. Aircraft auxiliary interface7E2250 at owner+6C0 comes from
QueryInterface414290 for IID820F501C-4F39-11D2-9B70-00104B972FE8. Its+14 function
41B7D0 checks owner+118 FirstPassenger. Its+0C function41B6A0 supplies a0/100
landing base, now implemented by `aircraft/landing_base.rs` and consumed by the
production air destination owner. Carryall/type+DFC has ctorfalse41C8D0 and
reader41CC9B..41CCC7, literal818028. An empty Carryall in effective mission7
**Enter** uses base0 when raw radio slot0 is a Building with UnitRepair+16A9 or
Helipad+16CB. Otherwise a Carryall with any contact or cargo head uses100;
other cases use0. Mission5B3040 chooses current unless exactly-1, then queued.
65AE30 scans every contact;65AD40 reads raw slot0 and never compacts a hole.
Existing Mission, Contacts, PassengerCargo and ObjectType owners supply these
inputs. No cached landing-base state or snapshot version was added.

The building identities are now established by literal81AAF0/reader460915/
store460929 (UnitRepair) and literal81AC48/reader4604DB/store4604E0 (Helipad).
Ctor45DD9A clears EBX;45E0C0/45E18D seed both bytes false. Ghidra comments at
41C8D0,41B6A0,460929,4604E0 were saved and read back. No boundary/type/binary edits.
`fly_landing_base` records nine original Carryall reads and249 original MoveTo
query/decision slices4CCE71..4CCED1/4CCED9, with real QueryInterface, radio,
Mission, RTTI and GetHeight. Covers cargo, contact holes, queued/current mission,
building gates, phases, health, signed/wide height, ramps, OnBridge and six
missing-cell query-order cases. Stops BEFORE BeginTakeoff; full MoveTo and phase
callbacks are excluded. All three updated native harness checks pass.

Move orders now use exact physical height through the current terrain surface,
not the saturated altitude cache. GetHeight remains lazy behind the native
health/takeoff/landing short circuits: it may stamp the shared Dummy coordinate.
The new production comparison checks all249 decisions, and four saved cargo/
contact/queued-mission continuations check recomputation through the same owner.
Final `cargo test -p vera20k --lib` passed: **9,119 passed,0 failed,135 ignored**,
17.72s (`.local/fly-landing-base-final-tests.log`). This includes the six new
Dummy-cadence vectors and four save/restore cases. `cargo clippy -p vera20k --lib`
passed with1,027 warnings in24.40s (`.local/fly-landing-base-clippy.log`). All
owned native/Rust validation processes are terminal. An earlier
full run passed9,119 before the lazy-query correction; do not substitute that
run for the final source. Initial compilation exposed two missing test-fixture
field initializers and an invalid MissionDispatchTimer constructor; corrected.

Display evidence correction: `display_entity_layer` and `display_non_entity`
also omitted Map+140 table length. Both now initialize it; entity queries assert
the real cell for every ground/Jumpjet bridge read. The matching Rust fixture
now populates dense grid slot10,10 rather than putting that cell at slot0.
Four original entity answers changed: Jumpjet marked=true/onBridge=false/
falling=false at bridge Z416 ->Ground, Z624/915 ->Air; Fly slope Z260 ->Ground.
The nine mixed-object histories did not change. All88 corrected entity queries
pass Rust's focused display tests;
the former ramp/bridge comparison claims were invalid. Production layer code
already agrees with the corrected evidence and did not require a behavior edit.
`ObjectType::flight_level(general)` models717800 with exact-1 fallback;30 native
reader/getter cases remain checked. A new release retail load for FlightLevel and
IsDropship and Carryall is required before merge; the prior release load predates
these changes.

The newly stored takeoff flag still lacks its native clearing callback:4CE680
clears BOTH flags unconditionally at4CE756/4CE763, then thresholds select facing/
speed effects. It must run inside4CD2A0's explicit Display/Mark transaction, not
be approximated by reaching the target height. BeginLanding still lacks+52,
admission, sound/animation, air-slot and touchdown effects. Legacy docking adapters
reissue the start mutations each mission visit; port their proper native callers.
BeginTakeoff refusals are70EFD0 (+504 EMP),4DE770 (Foot timer+6A0/+6A8),70C5B0
(+270) and70C5C0 (+271). Existing teleport/power predicates are used; missing EMP
and timer producers are not equivalent to deploy_state or assumed unreachable.
The failed Ground landing transition4CD3C3 also sets+50, clears OnBridge and
SetHeight(GetHeight()+10) inside Mark0/1 when owner virtual+550 refuses.

Ghidra comments4CF4B6 and4CE756 are saved/read back: conditional type-based dive
versus repeated mutable-target division, and unconditional takeoff flag clears.
Prior QueryInterface rename414290 and comments4142C1/4CDE64/712336 remain saved.
No signature, function-boundary or binary edits.

### Facing prerequisite and takeoff evidence

Source `be8e2d62` preserves signed SetROT4C9680/constructor4C91E0 semantics in
the existing FacingClass owner: clamp only >=127, then shift the low byte;
Current/Set/Snap/IsRotating interpret the word as signed. Thus -1 is instant,
but -255 turns at256. Timer epochFFFFFFFF retains its full duration.
Existing rules/locomotor consumers preserve the input: spawn, Unit combat,
turret sweep, movement admission/steering, MCV, transport unload, miner pivot
and Jumpjet linkage. No added facing state, snapshot field or version.
Infantry lazy body owners use ctor517BBD's constant127; Unit/Aircraft use Type
ROT. Full facing lifecycle migration and aircraft steering remain open.

Voxel-building fire retry44B068 intentionally uses absolute signed16 of RAW
ROT's low byte shifted8, without SetROT's upper clamp. The fire receiver now
preserves that distinction. Native `facing_class --check` passes61 retained
histories and `building_fire_turn --check` passes140 decisions; Rust consumes
each. Nineteen signed rules values additionally pass production spawn, Unit
facing/latch and full snapshot restore/continued advance_tick hash checks.

`walk_first_step` retains its five supplied-rate0 rows unchanged, and adds five
rows executing Facing ctor plus Infantry517BBD..517BCA. Only rate32512 differs;
coordinates, RNG, occupancy, head, queue and speed match. Rust supplies rate0
explicitly for old rows and exercises lazy native defaults for new rows.
Slice6's new current hash is3AB40B61DE3B5224. Changing ONLY E1's retained
rate back to0 reproduces the preceding current and pre174/181/182 pins;
the test restores the untouched facing and asserts the new pin afterward.

Final `cargo test -p vera20k --lib`: **9,121 passed,0 failed,135 ignored**,
20.09s (`.local/facing-validated-tests.log`). Clippy passed with1,027 warnings,
27.77s (`.local/facing-clippy.log`). The preceding full run failed only the
supplied-rate0 fixture and Slice6 pin; both now retain causal checks. An earlier
focused run passed150 tests before final voxel/Walk fixture changes. All owned
native/Rust validation processes are terminal. No new release load or critic.

Evidence commit `ae5b3042`: `fly_takeoff --check` passes80 original4CE680
callbacks, using real Aircraft methods and original413FD2..41401A facing setup:
19 no-setter,37 Primary.Set and24 Secondary.Set cases. All clear both flags,
return1 and preserve coordinates. Coverage includes thresholds, Carryall base,
bridge normalization, slopes, signed ROT, active turns and retained destination
directions including zero. This does NOT demonstrate Rust takeoff parity or
cover outer Process admission, BeginTakeoff, landing, or Mark/Display.

Ghidra comments4C93DB/4C93EC,517BBD,44B08E,4CE756,41514C and416041 saved
and read back. Corrected turret.rs's false writer claim: Aircraft AI41514C
READS SecondaryFacing and copies it into Carryall passenger facings; FireAt
416041 READS it for launch math. Self-writers belong to Mission_Attack4181BB..
4185DF and Fly steering/takeoff. Fly rendering4CFB77/4CFBCE samples Secondary;
movement4CDA62 and DropPayload415CB8/415CDB sample Primary. Migrate together.

Landing prerequisite: FootUnlimbo4D7248..4D72A6 increments eight neighbor
Cell+122 counters BEFORE the high-flying test; only the low branch writes
Foot+55C at4D72B9. The existing overlay neighbor-count authority must cover
all Foot lifecycle producers, not just Fly touchdown. Trace Limbo/destruction
before generalizing it; do not add a parallel count plane.

### Aircraft approach range and conflicting attack-state evidence

The current range increment replaces the state3 cell-Chebyshev/`<=` test with
Object5F6440's planar GetCoords distance and strict signed comparison against
the selected slot0 weapon's raw Range leptons (4180F4..418117). Building targets
use their foundation centre, then subtract `(width+height)*64`, clamping at0;
Bib is excluded. Existing `native_x87::distance_3d_leptons` with both Z=0 owns
the deterministic native sqrt ties; no new math emulator or retained state.
Combat's existing target-coordinate projection now also serves `in_range.rs`;
the duplicate implementation is deleted. Missing weapons no longer invent a
five-cell range. The original comparison also exposed the shared coordinate
projection's saturating subtraction for the native `0x0` foundation; GetCoords
must shift its anchor by(-128,-128), not(0,0). That owner is corrected, including
ordinary fire-range consumers. Snapshot schema and state hash layout are unchanged.

Acceptance is bounded to the selected range branch: match original distances,
weapon tier/fallback and strict decisions, reach production mission/move writes,
and preserve those decisions across save/restore. The original range arm is
conditional on auxiliary+18; the legacy handler's other state3 branches are not
ported by correcting this distance. Do not claim full aircraft attack parity.
`aircraft_approach_range.{py,json,meta.json}` preserves235 original full distance
calls followed by bounded caller branches with real virtual receivers and no
substitutions. Ghidra41810F,418037 and41B849 comments saved/read back.

Final validation for d7c15550: `cargo test -p vera20k --lib` passed **9,123 tests,
0 failed,135 ignored** in18.52s (`.local/aircraft-range-validated-tests.log`).
`cargo clippy -p vera20k --lib` passed with1,026 warnings in23.22s
(`.local/aircraft-range-clippy.log`). `aircraft_approach_range --check` passed.
The native rows reach production `tick_aircraft_missions` and move dispatch;
11 representative pending decisions also round-trip through full GameSnapshot.
An initial fixture misspelled the Fly GUID; after correcting it, the full native
comparison exposed the real0x0 offset defect above. Native expected values were
retained. No replay pins changed. All owned Cargo processes are terminal.
No release retail load or PR/critic was run for this increment. The earlier
FlightLevel/IsDropship/Carryall rules changes still require a fresh release retail
load before merge. The dependency map remains retired and was not refreshed.

Tracing Fly's required mode+5C input uncovered a separate required migration:

- Aircraft auxiliary+20 at41B860 reads the actual owner+6D2 byte. The current
  `AircraftReleaseTail.completion_latch` is NOT this authority:418037 clears
  +6D2 at state1 entry while the Rust completion latch persists. The five-field
  tail duplicates Ammo+2FC and pending+6C8, hardcodes every release as final and
  forces1->10->target-clear without consulting current ammo. Its originating
  commit7e181246 cites a three-entry capture, not a general state machine.
- State1 clears+6D2, consumes pending+6C8 with wrapping DEC Ammo, then checks
  target and Ammo!=0; if present it calls4197C0 FindFireLocation, assigns NavCom
  via+480 and selects3 or10 from the resulting NavCom. State3 also consumes
  pending. State10 clears+6D2 and pending but decrements only positive Ammo;
  nonzero Ammo and a retained target return to1 at418CD1..418CE2, not Rust's0.
  Its zero-ammo target-clear is conditional at418C21..418C3D and continues
  through return-location/RNG/NavCom/mission work. Do not replace it with a
  blanket target clear or a new competing latch.
- Auxiliary+18 at41B7F0 resolves slot0 weapon and tests Projectile+2DC<=1 and
  +29E==0; resolve both field identities before naming the classifier. Auxiliary
  +1C at41B840 is **Fighter**, AircraftType+E0E, proven by reader41CC84 literal
  818034 and store41CC95. It is not `FlyBy`. Existing Rules already has Fighter.
- FindFireLocation4197C0 returns the supplied target directly for auxiliary+18;
  otherwise it scans16 angles on a ring around the TARGET, using its NavCom as
  ranking reference when the target is Foot. The retained alternate is the
  previous record minimum, not an independently maintained second-best. The
  current cfg(test)-only `runtime_contract::find_fire_location` gets these facts
  wrong and is not a production prerequisite. Port playfield/shroud/admission
  419B00 and RNG order through their owners before connecting state1.
- `tick_aircraft_missions` currently signals fire by resetting AttackTarget,
  while the generic fire gate blocks every active Attack mission. Trace and
  migrate that actual emission route with its pending-ammo owner; the separate
  generic fire receiver currently deducts ammo on burst completion as well.
  This remains an uncompleted production chain, not a passing combat claim.

Native name array816CAC, dispatcher5B34E8 and Aircraft vtable7E22A4 were read
together to correct five Ghidra labels (saved/read back):25 Patrol417300,
26 ParadropApproach4158E0,27 ParadropOverfly415960,30 SpyplaneApproach4155F0,
31 SpyplaneOverfly4157C0. The old labels respectively said SpyPlane, Open,
Rescue, ParaDropApproach and ParaDropOverfly. Current Rust paradrop comments
still call26/27 Open/Rescue; that is stale identity, not a distinct native chain.
Mission24 Open dispatches to base5B2F50; Mission15 Hunt reaches414A80, so do not
rename415A50 to Hunt from an incorrectly counted vtable entry.

### Aircraft admission/emission boundary and release witnesses

The world receiver now hands its existing admitted shot to one private
`emit_admitted_fire` function. The borrowed `AdmittedFire` carries the existing
selected weapon, source and target facts; it adds no retained state or copies of
emission logic. Every existing fire consumer still reaches this production body.
Admission, infantry fire-action writes, shot order and inline receiver commits
retain their previous caller. Acceptance for this preparatory refactor is unchanged
production results/RNG/save behavior through the full lib suite, plus Clippy.
It does **not** enable aircraft mission shots or complete the release migration.

`aircraft_attack_release.{py,json,meta.json}` preserves316 original state4
successful-release loops and post-reveal continuations, plus72 original
state1/3/10 entry-housekeeping prefixes. SelectWeapon and FireAt are explicit
scratch callbacks; original GetWeapon, auxiliary+18 and Fighter reads execute.
The intervening map reveal is excluded. No emitted-shot, damage, scheduling or
whole-burst parity claim. Important native facts for the implementation:

- Set pending+6C8 BEFORE the first SelectWeapon, even for Burst<=0. Do not use
  a projectile count or returned Bullet pointer as proof of mission success.
- Loop body reselects before every FireAt and again before its next Burst test;
  it does not repeat GetFireError. A cached selected weapon across the entire
  burst would miss target/selection changes from synchronous effects.
- The success arm does not debit Ammo+2FC. Auxiliary+18 selects state6 and
  latch+6D2=true; otherwise Fighter chooses1 for Ammo>0 or10, also latching.
  Both return slot0 raw ROF. The remaining arm selects5, returns1, and retains
  the incoming6D2 value. Witness fixtures supply6D2=false.
- State1/3 consume pending with wrapping signed DEC; state10 clears pending
  but decrements only positive Ammo. All three clear6D2. Prefixes stop before
  the target/ammo guards and navigation, so these are not state1 completion.
- Common FireAt emission must be callable after a single admission, with live
  target/weapon reads and synchronous receiver commit between burst shots.
  Existing batch ammo deduction must migrate with the mission pending owner;
  merely bypassing the active-Attack gate would double-charge ammo.

Ghidra41A9E0 was falsely named Aircraft What_Weapon_Should_I_Use. Actual vtable
+3C0 and Mission_Attack41832E establish GetFireError; renamed accordingly.
It first propagates common6FC0B0's errors, then tests6C9/118 and SecondaryFacing
against DirectionToTarget unless Fighter. Its0/1/2 returns are admission results,
not weapon indexes. GetROF6FCFA0 is also now named and annotated. Labels and
comments were saved and read back; no prototypes, boundaries or binary edits.

Original WhatAmI leaves establish **Unit1, Aircraft2, Building6, Infantry15**.
The initial GetROF class6/Ammo>1 shortcut belongs to Building; class1 Type+6C4
per-burst delays belong to Unit. The old techno_rearm oracle called class1
Aircraft, and Rust's comment called it Infantry. Both labels are corrected.
All267 existing numeric outputs/events/RNG states remain identical after input
key renames. Its metadata now records execution of the four original RTTI leaves.
This is evidence correction, not a new aircraft one-frame rearm implementation.

Final validation for4a32f4c3: `cargo test -p vera20k --lib` passed **9,123 tests,
0 failed,135 ignored**,17.41s after a7m23s fresh build
(`.local/aircraft-emission-split-tests.log`). `cargo clippy -p vera20k --lib`
passed with1,026 warnings in1m45s (`.local/aircraft-emission-split-clippy.log`).
Both native `--check` commands passed; the release fixture's final shape contains
316 release rows and72 entry rows. A mechanical comparison confirmed every
emission statement is unchanged by extraction. No new snapshot schema or replay
pin; no release retail load or PR/critic in this increment. All owned processes
are terminal. This run used the owned worktree's `target` cache (CARGO_TARGET_DIR
was unset), rather than the earlier main-checkout shared cache; preserve it and
check actual Cargo/config state before continuing.

### Next safe implementation

Continue the semantic Aircraft pending-ammo/+6D2 and emission migration above,
using the now-shared production emitter and saved release witnesses. Fly MoveTo
reads that retained byte; it cannot borrow the legacy completion latch. Preserve
the validated range correction while replacing the false final-release path and
porting its required FindFireLocation/NavCom/state1/state10 effects.

Migrate retained Fly destination state and both Aircraft facing writers/readers,
then port native phase callbacks with their Mark/Display transaction. The legacy
turret sweep would overwrite newly enabled stock-aircraft SecondaryFacing;
adding its initialization alone cannot complete the prerequisite.
Full MoveTo
destination XYZ, mode+5C and null-stop
behavior, EMP/Foot timer producers, continuous horizontal slowdown and target
selection, descent drift and crash relocation remain required too.

1. Complete explicit Fly resubmissions:4CD2A0 enters Mark(REMOVE)4CD324 and
   RemoveDisplay4CD333 only for health>0 with loco+50/+51, runs their height
   helpers, then unconditionally submits4CD4E7 before Mark(PUT), even when the
   live layer stayed equal. Aircraft Landable=false takes an earlier branch;
   type+E0A is proven by reader41CC54/41CC67, literal81804C, ctorfalse41C8E2.
   Existing Rules has `landable`; do not add another authority. Helper4CE840
   behind+51 handles landing (dock/bridge base height, refusal/alternate cells,
   touchdown+slot cleanup);4CE680 behind+50 clears both flags and stages takeoff
   facing/speed. The new vertical owner does not implement these transactions.
   AirMovePhase is now a derived mission compatibility view, not native flags. Proven full-object writers:4CF9A5 clears+51 and
   sets+50, then type virtual+BC supplies+38;4CFADF clears+50, sets+51, clears+52
   and zeroes+38 after its refusal/docking gates. ILoco offsets+4C/+4D address
   these same full-object+50/+51 bytes. Comments4CF9A5/4CFADF saved/read back.
   Another +50=1 store exists at4CD3C3; retain that transition too.4CE5A0 is
   separate +53/+54 facing damping, not the missing vertical-motion owner.
   4CD75A/4CD792
   are another explicit relocation pair. DropIn5F400E/5F4196 and Jumpjet
   **crash** relocation54CBD4..54CC0D remain open. Normal State4 touchdown
   54C81A..54C9FB has Mark/SetCoords/pickup and no direct Display submit;
   Process54B17F/54B18E separately compares live entry/exit queries. Preserve
   those distinctions; generic per-frame cache refresh is not equivalent.
   Takeoff4CE680 itself never writes Z: a pure takeoff arm therefore has equal
   entry/exit live layers and still performs Remove/Submit/Mark. After clearing
   both flags it uses bridge-normalized height minus the live landing base:
   above target-target/3, SecondaryFacing+3A0.Set(PrimaryFacing+388.destination);
   otherwise above target/2, PrimaryFacing.Set(direction to DestXY) and target
   speed=1. Reuse the existing two FacingClass owners; establish initialization
   and migrate remaining 8-bit steering rather than adding another facing field.
   Techno ctor6F2EEB..6F2EFC constructs BOTH FacingClasses with4C91C0 (zero
   direction/rate, timer anchored to the current frame). Aircraft InitFromType
   413F80, called by ctor413E2E, sets BOTH rates from Type+71C ROT at413FE7/
   414001, then Secondary.SetCurrent(Primary.Current)414006..414015. Reader
   714B14..714B2F uses literal81B164 ROT. No Turret=yes or TurretROT gate.
   Aircraft Unlimbo414403..414417 snaps SecondaryFacing to authored facing<<8.
   FacingClass now preserves signed ROT and timer semantics; both native
   constructors and61 histories are compared. Aircraft still requires complete
   primary/secondary initialization, retained-destination steering and migration
   of its heading consumers. Comments413FDE/4C9680 saved and read back.

   Landing4CE840 has589 instructions and is not a flag-only counterpart. It
   includes AirportBound/radio or Aircraft4196B0 admission, refusal ->BeginTakeoff
   and FNPC56DC20 (or ReceiveDamage), below300 animation/sound, touchdown height/
   flags/speed, AirSpatial removal, Foot+55C retained source-cell replacement,
   eight old-neighbor decrements (only for a nonzero old packed source) and
   eight new-neighbor increments at Cell+122,
   and destination/NavCom cleanup. Cell+122 ALREADY belongs to overlay_grid's
   retained_wall_neighbor_counts plane; generalize that authority and its
   save/hash/consumers, never create a competing aircraft count plane. Trace
   Foot ctor4D31EF/4D3243..4D324A initializes+55C/+55E=(0,0). BOTH landing arms
   install the current cell (4CEDCF/4CEE42) and increment new neighbors
   (4CEE24/4CEE97); a zero old source only skips decrements. Constructor comment
   saved/read back. Trace other writers and teardown too. Aircraft4196B0 is a real admission
   wrapper with team/map, occupants/alliance and shroud gates; the existing
   always-clear Winged cell leaf does not implement that wrapper. Phase changed-
   Ground admission uses vtable+550 ->4DDC60. Its existing listing was read
   without defining a new Ghidra function:4DDC60..4DDDDB checks nonnull cell,
   Map578460 mode1 playfield, Cell47C3D0 occupant/self/raw radio slot0, unresolved
   Type+D54 and occupant+2D0 branches, then Cell4834A0 and an Aircraft-array
   A8E394/A8E3A0 scan for another+90 true/+81 false owner with NavCom+5A4 equal
   to the supplied cell. Do not reduce this to the Winged cell leaf. Resolve
   the raw type/occupant fields and arguments before implementing it.
2. Finish Display consumers beyond the migrated Ground parent path. Legacy
   `entity_draw_band` still queries altitude; Air/Top VXL, SHP, projectile,
   particle and wave draws remain separate or fully depth-sorted buckets.
   Parachute canopies are still folded into the body parent instead of their
   own Anim Display slot. Preserve actual layer order and remove the remaining
   Top absent-members fallback when its required Fly writers are ported. Do not
   mistake the Ground/picking migration for complete Display/render parity.
3. Finish movement pickup: passive guards, Tag49/Scenario+34BE, free-MCV
   preemption, multiplayer eligibility, water fallback, removal/replacement
   before effects, returns and every selected effect's downstream owner.
   Reuse `combat_weapon::is_armed` for701120. Trace Unit+2E8/Infantry+2F4 count
   producers before using current house totals (categories/limbo windows differ).
4. Anim AI remains partial: `runtime.inactive` conflates +19B with pending-delete
   readiness, and the expiry gate precedes some native looping-sound/bounce/
   visibility work. Occupied-cell424358 and animated-tiberium424427 marker
   writers, bounce landing and per-frame damage remain required combat work.
   Owner shared-Anim flag+84 is not represented; revisit its actual consumers
   while completing Display/animation paths rather than assuming no effect.
5. Finish coherent implementation and validation, then one fresh critic before
   PR. The release retail load ran; the separate full tactical capture failed
   on the stale MCV contract above. PR439/440 critics are finished; do not repeat.

HouseType speed factors+128/+12C/+130 remain absent. Unit+6CC CTF starts-1;
740DF0/740E20 attach/detach, Limbo/destruction return it. Creation4FC060 caller
688C02 is gated by Scenario[0]&0x10; trace the producer before calling it unreachable.
Normal SimFixed speed ranges do not prove arbitrary non-retail crate multipliers.
Broader combat remains open: Active_Click_With+6E0, shared cursor/click, NavCom/
action/fire legality, FLH slope/scatter/homing, special warheads, visibility and
whole-combat acceptance. Recording a required omission does not resolve it.
