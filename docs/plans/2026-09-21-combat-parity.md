# Combat parity acceptance and continuation

## Objective and acceptance

User direction: the dependency map is retired. Do not consult or refresh it;
this overrides stale map instructions on this owned branch.

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
Latest production source is published `9de592eacb65da849383bb05cf31e80eee7e48a9`:
retained Techno+3D4 deployment history through Aircraft Unlimbo and paradrop,
empty-selection click admission, save/restore and hashing (snapshot189). The
preceding paid Fly motion uses full Primary facing and live type speed;
`16b609e1` connects pure takeoff callback and Mark/Display. The native comparisons and all9,148 lib tests pass; see the flag section
below for validation and exact coverage limits.
Published evidence `31cf7819` adds78 original Fly map-edge cases, correcting three
earlier inferences: +E0B is FlyBy; scatter runs at most once;6EA089 SETS Team+7F.
Current increment adds39 original Team creation cases, including complete
Team/Script/Tag/Trigger constructors and trigger-action4 dispatch. It changes no
Rust behavior. Production Team creation is still absent (both creation helpers
are cfg(test)); adding an unproduced activation flag would not connect this
prerequisite. Creation requires per-instance Tag/Trigger state and constructor
timer RNG, which current TriggerRuntime's definition-wide latches cannot express.
Next migrate those instances through the existing TriggerRuntime owner, connect
trigger4/TeamType creation to TeamScriptVm with the saved construction corpus,
then membership, activation and action3. Preserve the native Fly map-edge target.
No critic/PR yet. The dependency map is retired and must not be used or refreshed.
Snapshot188 retained Fly destination XYZ in active and stashed runtime. `f241211c` preserves
the native FindFireLocation corpus, deterministic geometry comparisons and removal
of the incorrect unused search model. Production source
`9d3be1105c1ed7beaa73ebf11a40c594d5be5fec`: admitted
Aircraft state4 releases now run through shared production emission, synchronous
burst control, pending ammo and native success state/readiness/raw-delay writes.
The call-local request preserves AttackTarget and rearm. Retained `WeaponBurst`
replaces the obsolete target-owned remaining-shot count (introduced in snapshot187).
The previous pending-state/initialization source `07c48b07` and MissionLeaf+6D2
owner remain intact. **State1 navigation and the full Aircraft attack cycle
remain unfinished.** See the release integration section for exact coverage and
required residuals; this is not whole-combat parity.
Previous source `4a32f4c3` extracted the shared emitter; preceding checkpoint
`1e069d75`. Range source `d7c15550` remains intact, including native
0x0 foundations. The paid-motion increment passed native, full lib and Clippy checks;
`.local/` remains untracked and must not be staged.
The dependency map is retired by user instruction: do not use or refresh it,
regardless of the older branch contract text. Facing source `be8e2d62` and
`ae5b3042` takeoff evidence remain intact, including80 original callbacks.
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

The original vertical increment lacked the clearing callback:4CE680
clears BOTH flags unconditionally at4CE756/4CE763, then thresholds select facing/
speed effects. It must run inside4CD2A0's explicit Display/Mark transaction, not
be approximated by reaching the target height. This is now connected for pure
takeoff; see the current takeoff increment below. BeginLanding still lacks+52,
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
directions including zero. That evidence-only commit did not establish Rust
takeoff parity. The current takeoff increment below connects the Rust callback
and adds a separate full Mark/Display witness; BeginTakeoff and landing remain open.

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

### Pending-ammo authority and the open emission handoff

Acceptance for source07c48b07: reproduce original signed ammo initialization and
pending-consumption prefixes through the production mission/AI owners, including
mission changes, signed wrap, save/restore and hashing. Remove the fabricated
final-shot countdown and duplicate flags. This does not certify admission,
emitted shots, navigation, mission cadence or the whole attack cycle.

`AircraftAmmo` now retains the private+6C8 pending byte beside the actual signed
count. All Aircraft, including map-authored and negative-ammo objects, receive
this owner. `InitialAmmo` (Type+680, reader71474C/key843AEC, default-1) selects
`Ammo` only for exactly-1 at41403A..41404B; other signed values are not clamped.
The legacy docking FSM checks max>=0 instead of treating component presence as
finite ammo. Common GetFireError6FCA0D rejects exactly zero, not negative counts.

`enter_attack_state` clears the existing MissionLeaf action latch+6D2 for
states0/1/3/10; state0 leaves pending alone. State1/3 consume pending with wrapping
signed DEC; state10 clears pending and decrements only positive ammo. Aircraft
AI41505E consumes pending after Ready/Commence when canonical Mission+AC!=1,
independent of the legacy AircraftMission mirror. The existing live Object turn
calls this post-movement host. Removed `AircraftReleaseTail`, its five fields,
and Attack's `has_fired`/`is_strafe` flags and fabricated countdown tests.
Snapshot186 saves/hashes the real pending byte; it cannot recover arbitrary old
fake-tail state. Historical hash projections only reconstruct the bounded absent
tail and false/false Attack fixtures, explicitly not arbitrary old saves.

Historical07c48b07 boundary, superseded by the release integration below:
`AircraftAmmo::begin_release` had test callers only. `tick_aircraft_missions`
reset AttackTarget in its legacy request adapter, losing cooldown/burst
bookkeeping; `combat_fire_gate` still excluded active Attack. State4 waited for
real release admission, and state1 with a retained target/nonzero ammo waited for
its unported FindFireLocation
suffix (missing target/zero ammo goes10). State10's nonzero re-engagement returns1,
while its zero-ammo/targetless return-location/conditional-clear/RNG suffix remains
legacy Guard. Do not mistake these honest open boundaries for completed behavior.
Do not simply unblock Attack or restore request-time fake ammo bookkeeping.

Use the existing `world_receiver::emit_admitted_fire` extracted by4a32f4c3; it
still owns all current shot/Spawner/Drain/damage/rearm bookkeeping. Native418403
sets pending BEFORE the loop even for Burst<=0 or FireAt returning no Bullet.
It admits once, then reselects before each FireAt and again for each loop bound.
Commit synchronous receiver effects between shots. Move the generic burst-end
ammo deduction to the proper caller as part of that migration. Auxiliary+18
(41B7F0, slot0 ProjectileROT<=1 && !Inviso) selects6 and latches+6D2; otherwise
Fighter selects1 for Ammo>0 or10 and latches. Both use slot0 raw ROF. The remaining
arm selects5/delay1 without writing+6D2. Mission cadence and later states stay open.

`aircraft_attack_release.{py,json,meta.json}` now contains316 successful-release
rows (SelectWeapon/FireAt are scratch callbacks; reveal omitted),72 state1/3/10
housekeeping rows,144 post-Commence AI rows,21 ammo-initialization rows and7
common-fire-error ammo checks. Prior316+72 outputs are unchanged. Rust consumes
the new/prefix witnesses through actual constructor, mission and AI entry points;
all72 mission-entry cases also round-trip through GameSnapshot. That test uses
Scenario seed0 to respect the independent loader reset policy, not a changed RNG
contract. The corpus excludes full admission, actual damage, scheduling and
navigation. Do not promote this bounded evidence to whole-burst parity.

Ghidra annotations41505E,41403A,418037,4143FC were saved/read back. The existing
readiness leaf41B5E0 and Commence41B870 confirm+6D2, with+6D4 separate. Additional
fire-admission prerequisite: Aircraft Unlimbo4143FC sets+6C9 when PassengerCount
+118!=0, after successful Foot Unlimbo; constructor413D47 clears it. GetFireError
41A9FF refuses when the retained byte is set and current cargo is empty. Do not
derive it from current cargo. Other SET writers65DCE9/65E7B8/65EA0B need their
reinforcement caller/admission traces. No such new retained Rust byte is added
yet. The preceding Unlimbo+3D4 writer is independent (Selectable/Landable/Camera).

Retain the earlier evidence corrections:41A9E0 is Aircraft GetFireError, not
weapon selection. Original WhatAmI leaves are Unit1/Aircraft2/Building6/Infantry15.
GetROF6FCFA0's class6 Ammo>1 shortcut is Building; class1 authored burst delays are
Unit. All267 techno_rearm outputs/events/RNG are unchanged after correcting names;
there is no demonstrated special one-frame Aircraft rearm branch.

Validation07c48b07: full `cargo test -p vera20k --lib` passed **9,128 tests,
0 failed,135 ignored**,17.07s after2m30s compilation
(`.local/aircraft-release-validated-tests.log`). Clippy passed with1,029 warnings,
41.00s (`.local/aircraft-release-clippy.log`). Native `aircraft_attack_release
--check` passed. Three current replay hashes changed only by removing the absent
release-tail fold: pre186 projections reproduce their prior full pins, preserving
all replay/position/native Walk/RNG checks and the earlier Infantry-rate probe.
The initial focused run found two literal ObjectType fixtures needing InitialAmmo
and a nonzero-Scenario-seed snapshot fixture; fixed without changing load policy.
No release retail load, PR or critic in this increment; new InitialAmmo and prior
FlightLevel/IsDropship/Carryall rule changes still need a fresh release retail load
before merge. All owned processes are terminal. Keep using the owned worktree's
`target` cache (CARGO_TARGET_DIR unset); do not switch to the main checkout cache.

### Admitted Aircraft release integration (source9d3be110)

Acceptance: a state4 mission visit reaches shared admission once, then runs
native signed Burst control with live selection and synchronous effects between
shots. Pending ammo must precede even a zero-shot/NULL-return release; the native
success suffix must own state, readiness and raw mission delay. Preserve existing
rearm when requesting fire. Save/hash the retained object burst index and remove
the superseded target-owned remaining-shot count. This is bounded release
integration, not complete Aircraft Mission_Attack or projectile parity.

`tick_aircraft_missions` now returns call-local firing receipts through
`advance_tick` into the combat host. It no longer replaces AttackTarget. The
generic gate no longer blocks every Attack; the combat snapshot requires that
visit's receipt, and the release caller rechecks the live state4 before admission.
State3 reaching4 cannot fire on that same visit. The shared admission result is
owned call-local data, never saved permission. Shared weapon resolution separates
GetFireError's targeting checks from selection for an already admitted FireAt.

`combat/aircraft_release.rs` sets pending before the signed Burst test, reselects
from current target/type/tier before each FireAt and subsequent bound, and runs
the shared emitter. NULL/detached targets still take part in the bound without
emission. `FireCommitBoundary` commits damage/deaths, weapon writes, rearm,
drain/spawner links and wave callbacks before another shot. Active mission
releases bypass the old generic burst-end ammo deduction. Suffix4184C2 selects
6 for auxiliary+18, otherwise Fighter1/10 by Ammo>0, otherwise5; only the first
two write readiness and raw slot0 ROF, the last returns1 without clearing the
latch. The existing Mission dispatch timer gates subsequent Attack visits.

State4's non-strafe body/secondary Set writers4182D3..41830C precede admission.
Aircraft secondary facing is initialized even without Turret; the generic
target-or-body turret sweep no longer overwrites it. Aircraft admission reads
secondary current with inclusive0x800 tolerance and Fighter bypass; OmniFire and
homing do not bypass/widen that class leaf. Other Aircraft/Fly facing writers
remain open below; initializing this controller does not complete them.

`WeaponBurst` privately owns Techno+3B8. Shared FireAt uses its pre-shot parity
for FLH and incremented signed index for the mid-burst rearm branch, then stores
the signed remainder. Removed AttackTarget/AttackerSnapshot `burst_remaining`
and migrated all consumers/fixtures. Assign_Target6FCF5B resets the index only on
a changed assignment whose final target is null; same-target and non-null
replacement preserve it. Direct Detach is not that setter. Snapshot187 replaces
the obsolete count with the retained index; pre187 hash projections only restore
the independently established zero-count replay fixtures, not arbitrary old state.

Evidence: existing316 original release rows now exercise actual mission-to-combat
dispatch and shared emission for loop count/pending/suffix assertions. Their
native FireAt remains a NULL callback, so actual projectile/damage parity is not
claimed. New `techno_burst_index` executes54 original signed increment/remainder
tails, including wrapping index and Burst257; supplied GetROF only records the
incremented index and returns20. Both generators pass `--check`. Ghidra comments
41840E,6FF27F,6FCF5B saved/read back. Rust also covers raw Burst257, one pending
charge, request/rearm preservation, state3 handoff timing, snapshot continuation,
secondary-facing boundaries and an actual `advance_tick` release.

Required residuals: the common admission is still an incomplete native
GetFireError, including unported Aircraft+6C9 retained cargo history. Loaded
Aircraft's DropPayload arm is explicitly still blocked rather than substituted
with gun fire. Native481670 release reveal, error-specific state4 transitions,
FindFireLocation/state1, state3 auxiliary/navigation branches, states5..9 and
state10 return/navigation/RNG remain required. Air FireAt's launch velocity,
ROT0/1/homing math and6CA suffix remain unported. The host remains phased rather
than the native per-object schedule. Shared GetROF still has bounded/clamped
legacy cooldown arithmetic and lacks House/bunker/full authored-delay behavior;
its timer remains split between AttackTarget and cloak. SpawnManager's temporary
parent burst-index writes6B73F6/6B7585 around launch were found but not migrated
here. Two more writes746493/74656C require verified surrounding caller context;
current Ghidra save/load labels alone are insufficient evidence. These are not
resolved by the new index owner. Snapshot continuation isolates the existing
loader's Scenario-seed reset policy; it does not certify that separate policy.

Validation9d3be110: full `cargo test -p vera20k --lib` passed **9,136 tests,
0 failed,135 ignored**,17.30s after2m56s compilation
(`.local/aircraft-release-validated-tests.log`). Clippy passed with1,032 warnings,
21.38s (`.local/aircraft-release-clippy.log`); two unused copied coordinates were
removed after the test run without changing behavior. Both native generators
passed `--check`. The first full candidate's only failures were three expected
current-hash pins; pre187 projections reproduce each preceding full pin, and all
historical/native Walk/replay/position/RNG checks remain intact. Current pins:
globalC373742E090E5AAC, bridge17815022180346188402, slice6=8A7D78556AAF1E46.
The initial fixture failure copied its interner before constructing the actor;
fixed the fixture, with no registry/load-policy change.
No critic, PR or release retail load has run for this branch. Required rules-load
and review checks still precede merge. Owned compile/native processes are terminal.

### FindFireLocation evidence and obsolete search removal (sourcef241211c)

Acceptance for the next production chain: state1 must use native search inputs,
preserve the returned Target-versus-Cell identity through AssignDestination,
publish the actual NavCom and Fly destination, and schedule the native mission
delay. Validate candidate generation, ranking, visibility/reservations and RNG
against original calls, then exercise state1 through production and save/restore.
This section establishes prerequisites; it does not claim that acceptance passes.

`tools/spatial_oracle/aircraft_fire_location.py` now executes51 full original
4197C0 calls with no substituted instructions/calls. Original7012C0/70E140,
41B7F0,578460/5657A0,419B00/47C3D0, trig/sqrt/ftol and Scenario RNG run against
supplied flat-map/object state. The corpus records896 candidate coordinates,
565 ranked distances, returned identity, conditional RNG and its next draw.
Cases include native tier fallback, short/odd ranges, direct strafe Target,
target NavCom ranking, visibility+12C bit10 versus bit8, Techno+3D4/game-mode
bypasses, self/physical/NavCom reservations, marked/limbo gates, Carryall,
AirportBound and Spawned beside a ground-list Techno with SpawnManager/Spawned.
The concrete target/destination receivers here are Units; Building/Cell receiver
variants, OpenTopped passenger range reduction and lifecycle producers are not
covered by these supplied-state calls. No full Mission_Attack claim.

The unused `runtime_contract::find_fire_location` and `FireLocationSearch` are
removed: they searched around the aircraft, used swapped host sin/cos axes,
ranked a true second-nearest candidate and ignored native admission. Only their
own tests called them. The retained production math already matches native
candidate coordinates and ranked distances in the corpus; the new
`util::native_trig::tests::aircraft_fire_location_candidates_and_distances_match_native`
compares that geometry directly. The search must reuse those owners, not add
another float-based implementation. Its mission caller is still absent.

Ghidra4197C0 renamed from misleading `Find_Approach_Cell` to `FindFireLocation`:
auxiliary+18 may return the original non-Cell Target pointer. Comments at
4197EF,41988E,419A40 and419C13 saved/read back. The opposite-side native case
selects previous record minimum[69,62] for RNG83; the true second-nearest is
[69,66], demonstrating why the old ranking was wrong.

Destination trace: Aircraft vtable7E22A4+480 ->41AA80, then Foot4D94B0 and
Fly4CCC80. Aircraft handles invalid Target+54, Enter/building radio and current
cell docking interactions before Foot. Foot clears NavComAux before refusal
gates; stores the live NavCom at4D9510; gets requester-dependent target+4C
coordinates and calls locomotor MoveTo. A null Aircraft destination while
current/queued Attack and Target non-null skips Stop at4D9672..4D969C, but still
writes Foot timer epilogue. The Fly adapter now retains non-null XYZ (see below),
but `issue_air_cell_destination` still lacks these class-specific NavCom effects.
FindFireLocation itself uses target+48, not navigation+4C: Unit/Aircraft5F65A0;
Building vtable7E3EBC+48 ->447AC0 adds foundation center offsets. Type+5E4 is
OpenTopped (7143BD/7143CA, literal843CCC read back), so7012C0's cargo-minimum
range arm must use that existing rule and passenger owner.

Validationf241211c: `python -m tools.spatial_oracle.aircraft_fire_location --check`
passes; `cargo test -p vera20k --lib aircraft` passes **93 tests,0 failures**,
0.09s after3m32s compilation (`.local/aircraft-fire-location-final-tests.log`).
The first48-case geometry probe also passed before adding Spawned cases and
explicit candidate indexes. Existing math bodies and production behavior were
not changed; no snapshot/hash version changed. No full-suite/Clippy repetition
was needed for this fixture and dead-code cleanup. The previous full runtime
validation is9d3be110 above. No critic, PR or retail release load has run yet.
All owned Cargo/native sessions are terminal; keep the existing target cache.

### Retained Fly destination increment (2026-09-22)

Bounded acceptance: retain the original non-null destination XYZ and refusal
rules in the existing Fly owner; make production steering read it; preserve
active/stashed runtime through save/restore and hash it. No full MoveTo, Stop,
FindFireLocation/state1 or Fly Process acceptance is claimed by this increment.

`fly_destination.{py,json,meta.json}` executes26 full original4CCC80 calls with
the real Fly constructor, Aircraft vtables, type getter and ground/auxiliary
calls. No instructions or callees are substituted. Cases deliberately avoid
BeginTakeoff/BeginLanding callbacks, which throw if unexpectedly entered.
Draft fixture correction before acceptance: FlightLevel is Type+618, not+D00;
original717800 uses exactly -1 to select Rules+7B4. The corrected corpus includes
negative/zero/large FlightLevel, signed Ammo -1/0/positive, slope/level, subcell,
power refusal and signed-truncation/i16-alias same-cell landing refusal.

`FlyRuntime` privately owns full+1C/+20/+24. Non-null requests preserve supplied
XYZ except when Target exists and retained Aircraft Ammo!=0: then native
4CCE25..4CCE6E reads destination ground and adds resolved FlightLevel. This
happens before landing-base/current-height queries. Same-cell landing refusal
precedes power/warp admission. The cell order adapter resolves Cell+4C through
the shared coordinate owner. Horizontal steering/distance and legacy arrival
now read Fly's exact XY; they no longer reconstruct cell-center coordinates
from `MovementTarget.final_goal`. That cache remains a cell projection for
legacy mission consumers and owns the still-unported execution lifetime/speed.
Snapshot188 stores the retained destination, including the piggyback stash;
pre188 hash projections omit only the new XYZ fold.

Required residuals: native moving+34 and mode+5C lifecycle/consumers; mode's
second ground query and readiness/Landable suffix; native horizontal/facing,
phase/Mark/Display callbacks, null4CCC80 and Stop4CCFD0; actual Aircraft/Foot
AssignDestination and its NavComAux/refusal/teardown/timer ordering. The current
adapter's bool reports its execution update, whereas native MoveTo returns void;
Foot can retain a new NavCom and execute its timer epilogue even if Fly ignores
the coordinate request. Do not use this bool as state1's NavCom result. These
residuals affect normal attack approach/landing and keep the combat loop open.
Fly on a non-Aircraft owner also needs the common native Ammo authority instead
of assuming an AircraftAmmo component. Arrival still uses the legacy adapter;
its cache clear must not fabricate a native retained-destination clear.

Ghidra comments4CCC80/4CCE1C/4CCED9 are saved/read back, distinguishing the
interface+4 receiver, exact destination writes and unported mode suffix.
The dependency map is retired by explicit user instruction; do not use/refresh it.

Validation: `python -m tools.spatial_oracle.fly_destination --check` passes26
native calls. `cargo test -p vera20k --lib` passes **9,139 tests,0 failures,
135 ignored** (3m17s compilation,16.86s execution), including the26 destination
comparisons, production cell order, deliberately stale-cell-cache subcell arrival,
active/stashed save/restore, hash exclusion, existing144 vertical vectors and249
landing-base cases. Log: `.local/fly-destination-full-tests.log`. All established
whole-fixture hash pins remain unchanged; none was rebaselined. An initial
test-only AttackTarget import error was fixed before this passing run. No critic
or PR was opened for this increment; retain the single final pre-PR review.
`cargo clippy -p vera20k --lib` also passes (37.04s,1032 existing warnings;
`.local/fly-destination-clippy.log`). Both owned Cargo sessions are terminal.

### Pure Fly takeoff production increment (2026-09-22)

Acceptance is the previously traced pure-takeoff branch: preserve original flag
clears, strict thresholds, live Carryall/bridge height inputs, facing histories,
speed write and Mark/Display order, then retain behavior across save/restore.
The landing and non-Landable branches remain separate required work.

`FlyRuntime::complete_takeoff` owns both flag clears and threshold selection.
`Simulation::apply_fly_takeoff_callback` queries current coordinates/landing base
and updates the existing Primary/Secondary FacingClass owners. Above target-target/3,
Secondary receives Primary's **destination**, not its current animated value;
above target/2, Primary receives the direction to retained Fly destination and
target speed becomes1. The callback changes no coordinates and draws no RNG.
Facing setters retain native timer writes; no new facing state was added.

The production air wrapper invokes `complete_fly_takeoff_phase` after motion:
Mark(REMOVE), RemoveDisplay, callback, SubmitDisplay, Mark(PUT). Even equal live
layers reorder the owner behind a peer. The new75-call original dispatcher
corpus executes real Mark/Display callees, without substitutions, and compares
this ordering and health/flag admission. The existing80 callback histories now
compare Rust flags, speed, coordinates and every retained facing field.

Aircraft construction initializes both facings from ROT even without Turret=yes.
The traced Unlimbo chain414310 ->4D7170 ->6F6CA0 snaps Primary at6F6DAA;
Aircraft414417 snaps Secondary. Both retain the binary spawn frame. Production
authored/runtime tests cover signed ROT and multiple directions. Fly movement
samples Primary.Current at the binary frame and its legacy navigation adapter
uses Primary.Set after motion. The competing 8-bit ROT controller is deleted.
The legacy XY integrator still quantizes this heading and retains its existing
speed/arrival policy; this is not native horizontal or full4CEFB0 parity.

Snapshot layout remains188: this increment changes writers of already-saved
state. A production tick regression poisons the byte heading, verifies the
retained controller wins and compares restored/uninterrupted full state hashes.
Required residuals include landing/both-flags, non-Landable mode/height prefix,
BeginTakeoff admission/effects, native horizontal motion, complete navigation
and its destination/secondary-facing branches, retained moving/mode lifecycle,
null/Stop, and all state1/Aircraft attack dependencies above. Clearing takeoff's
latch does not complete the flight or attack loop.

Ghidra labels/comments are saved and read back:4CE680's misleading Ascent_Step
name is corrected to Takeoff_Facing_Callback (the callback never ascends), and
4CD2A0 is identified as Process_Phase_Transitions. Comments4CE756/4CD4E7 record
the native witnesses, production connection and bounded coverage.

Validation: final `cargo test -p vera20k --lib` passes **9,142 tests,0 failures,
135 ignored**, including both native corpora, production tick continuation and
authored/runtime facing initialization (14.08s execution; log
`.local/fly-takeoff-full-tests.log`). All whole-fixture hash pins remain unchanged;
none was rebaselined. Both native `fly_takeoff --check` (80) and
`fly_takeoff_phase --check` (75) pass. `cargo clippy -p vera20k --lib` passes
(23.27s,1032 existing warnings; `.local/fly-takeoff-clippy.log`). All owned Cargo
processes are terminal. No critic/PR; retain one final review before the coherent PR.

### Paid Fly motion increment (2026-09-22)

Horizontal-step acceptance: compare the original paid Fly
range4CDA3C..4CDB4C, including actual Primary.Current, interface speed4CFE20
and original trig. Use full direction words and whole-lepton final-coordinate
truncation. Production must read live type Speed rather than an adjusted/stale
MovementTarget cache, preserve XYZ/height order and continue identically after
restore. Compare signed/fractional speed, active facing histories and cell
boundaries; explicitly bound earlier IsMoving admission and later map-edge,
descent/landing/navigation policies. Reuse the existing deterministic trig owner
if original execution demonstrates equivalence; remove the superseded byte
heading displacement path. Fly's retained SimFixed speed-fraction precision
policy remains unchanged by this numeric-step migration.

The original199-case `fly_paid_step` corpus executes4CDA3C..4CDB4C with real
Fly/Facing constructors, live turning histories, type Speed conversion block,
Fly interface+84 getter and sine/cosine calls. It stops before candidate
validation/placement and substitutes no instructions/callees. An initial draft
read the getter result from an already-reused stack slot; corrected before
acceptance to observe EAX directly at4CDA78. Regenerated output was reviewed and
the read-only native check passes. Original Type Speed is clamped0..100, then
converted to min(raw*256/100,255). Fly multiplies that integer by its retained
current fraction and truncates once; Foot speed factors are not on this chain.

`fixed_math::ra2_speed_to_leptons_per_frame` is now the shared conversion used
by existing per-second adapters too. `air_movement::current_fly_speed` uses a
wide integer product of type speed and the existing Q16 fraction. Fly and Walk
share `native_trig::facing_step_world_xy`; no new trig table or x87 emulator.
The legacy byte-heading/per-second displacement and checked saturation helper
are removed. Production uses the full Primary.Current and live object type
speed, bypassing a stale order-speed cache. The cache is only a fallback when
no rules/type is available. Movement consumes the entry current fraction before
legacy approach slowdown changes it. No saved layout or hash schema changed.

Rust validation passes: all199 numeric candidates,196 in-grid production
steps with a poisoned speed/heading cache, and restored/uninterrupted continuation
are covered by `fly_paid_step_matches_native_math_and_production_type_speed`.
The other three candidates cross the legacy storage boundary; their provisional
native coordinates are compared, but placement still requires4CDB4C..4CDD07's
map-shape/owner admission and edge correction. Do not claim their production
placement is ported. Earlier movement admission, continuous slowdown, current
fraction precision/ramp differences, descent drift, landing and complete
navigation remain required. Neither this numeric increment nor its samples
establish full flight parity. Ghidra4CFE20 is labeled Get_Current_Speed;
comments4CFE20/4CDA68 are saved/read back with original input ownership and scope.

Validation: `python -m tools.spatial_oracle.fly_paid_step --check` passes199
original samples. `cargo test -p vera20k --lib` passes **9,143 tests,0 failures,
135 ignored** (12.37s execution; `.local/fly-paid-step-full-tests.log`); the
focused compile took3m06s. Whole-fixture hash pins are unchanged and none was
rebaselined. `cargo clippy -p vera20k --lib` passes (25.57s,1030 warnings;
`.local/fly-paid-step-clippy.log`). All owned Cargo/native processes are terminal.
No critic/PR; retain one final review for the coherent branch. The dependency map
remains retired.

Next map-edge dependency is concrete:4CDB88 calls original568300 (the existing
`NativeOverlayMapShape::admits` geometry, not allocated-cell membership). An
out-of-shape candidate calls owner+4DC: Aircraft table7E22A4 ->41B890. That
predicate reads Target+2B4, raw mission+AC, retained+3D5, mission getter exclusions,
Team+5D4/6EC300 and retained Techno+3D4 (now owned by GameEntity). True enables
correction. AircraftType FlyBy+E0B bypasses it (ReadINI41CD54..41CD6E,
literal817FF8); Spawned is separately+D54. Otherwise565660 converts the packed
cell to local coords, compares signed local.x to trunc(Map+12C/2), and adds128
to world X when less, subtracts128 otherwise. Resize566332 writes+12C as
Size.width+Size.height-1; this is not a LocalSize-width or generic center clamp.
If still outside,49F420(distance64,flag0) runs ONCE. A final failed568300 skips
SetCoords at4CDCFB->4CDD0D, while following height/phase work continues. There is
no retry loop. The scatter consumes one Scenario RNG draw via65C780; reuse
`combat::inviso_scatter::random_direction_coord` after checking its bounds/math.
Do not create a second geometry/scatter authority or replace retained+3D4 with a
type inference. The previous Spawned/repeated-scatter descriptions were wrong.

### Next safe implementation

Finish FindFireLocation/NavCom/state1/state10 and native cadence dependencies;
preserve the validated range correction and now-connected release caller. Trace
the class-specific error transitions and payload/launch/reveal arms listed above.
The pending consumers, burst position and+6D2 owner are in place; do not recreate
the retired final-release tail or target-owned remaining-shot count.
Use the new native search corpus. Complete the destination owner and retained
Techno+3D4 producers before wiring state1; do not flatten returned entity targets
to cells or install the removed test-only search as a production shortcut.

Finish Fly destination lifecycle and the remaining Aircraft facing writers/
readers, then the landing/non-Landable phase branches and their effects. The
generic turret sweep excludes Aircraft, state4 owns its two Set calls and pure
takeoff now has its own Mark/Display transaction; full navigation and other
mission-state setters remain required.
The remaining MoveTo
moving/mode state and null-stop
behavior, EMP/Foot timer producers, continuous horizontal slowdown and target
selection, descent drift and crash relocation remain required too.

1. Complete the remaining explicit Fly resubmissions:4CD2A0 enters Mark(REMOVE)4CD324 and
   RemoveDisplay4CD333 only for health>0 with loco+50/+51, runs their height
   helpers, then unconditionally submits4CD4E7 before Mark(PUT), even when the
   live layer stayed equal. Aircraft Landable=false takes an earlier branch;
   type+E0A is proven by reader41CC54/41CC67, literal81804C, ctorfalse41C8E2.
   Existing Rules has `landable`; do not add another authority. Helper4CE840
   behind+51 handles landing (dock/bridge base height, refusal/alternate cells,
   touchdown+slot cleanup);4CE680 behind+50 clears both flags and stages takeoff
   facing/speed. The pure-takeoff transaction is now connected; landing and
   non-Landable branches remain required.
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
   constructors and61 histories are compared. Both Aircraft facings now initialize
   through construction/Unlimbo semantics and pure takeoff uses their existing
   owners; complete native navigation and remaining heading consumers still need
   migration. Comments413FDE/4C9680 saved and read back.

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

## Retained Techno+3D4 deployment prerequisite

Before implementation: retain one private GameEntity byte (VERA name
`mission_only`, historical native field name unconfirmed). Constructor6F2F55
clears it. AircraftUnlimbo4143A0..4143F2 promotes it only after successful Foot
Unlimbo when !Selectable, !Landable, or GetWeapon(0).Camera; the normal arm
preserves prior history. Use the existing tier-aware weapon owner. Paradrop
65E6BE sets it immediately after construction, before edge selection/Unlimbo.
Do not derive it from live cargo, type, playfield membership or mission.

Acceptance: original instruction-region comparisons cover failure, preservation,
type gates, null/base/elite Camera weapons; production construct/reveal and
paradrop preserve ordering; empty-selection click admission reads retained history via
DisplayDetermineAction69261B->692762..692778; save/restore and entity hashing retain it.
Do not globally reject forced selection: ObjectSelect5F4578 consults this byte
only after virtual+A0 succeeds, and that is not the ordinary click-action gate.
No full ObjectSelect or complete AircraftUnlimbo parity claim (height, cargo
history and other suffix effects remain open). The other native SET producers
65DBF2/65DFA1/65E8FF/65EB10 belong to still-unported reinforcement/airstrike
chains. Native constructor/SET instruction census found no ordinary clear writer.
Fly map-edge and FindFireLocation production readers remain the next dependency.
No RNG, timer or detach operation occurs in this bounded flag writer/reader;
parent Unlimbo, reinforcements and following mission-timer writes remain distinct.

The Display flag gate is reachable only with CurrentObjects.Count=0. Nonempty
selection delegates692640/692666 to WhatActionOnObject/Cell. Preserve this
boundary: those source/target/modifier branches and forced ObjectSelect remain
required reader migrations. Cursor feedback currently returns early for an empty
selection; completing its ordinary selection feedback remains part of the shared
cursor/click action resolver work, not proof supplied by this flag leaf.

Next map-edge Team dependency:6EC300 requires Team+7F, Script6915D0
(unsigned cursor+2C < ScriptType+24.Count+A0), current action3, and its
Scenario68BCC0 waypoint outside mode-one Map578460. Existing TeamScriptVm
owns membership, cursor and actions, but not +7F. Direct writers found:
constructor6E8B11 clears; AI6E91BE SET after +79/+77 gates;6EA089 also SETS
(BL=1 at6EA084); regroup6EA0E2 clears. AI SET also writes +78=1,
+7A=0, conditional Foot member+689, calls Script691590, then sets+80.
Do not derive+7F from the VM completed flag or assume teamless behavior for a
member. Reuse this owner and trace the linked prerequisite flags before adding
state. No Team implementation was changed in the deployment increment.

Current Team prerequisite evidence:6EA3E0 recomputes+79 (member count equals
signed TaskForce count sum), retains+78 once reached, and updates+7A. With
TeamType.Reinforce+AB, +7A is count<=trunc(required/3) for required>2, otherwise
count<required; without Reinforce it is !+78. GuardSlower+A7 makes+76=!+7A.
Positive membership clears+7D/+7E; a changed+7A sets+7B. Empty teams clear+76,
set+7A, clear+79/target+34; previously reached+78 takes Tag23/deletion instead.
6EA0D0 also chooses regroup destinations and submits member missions: porting
only its clear would omit required effects. Native Script ctor6913C0 and reset
691590 start cursor=-1, unlike the current test constructor's0. The live AI
advance6915B0 occurs after leader/recruitment/gather gates and submits a null
argument to70C610 for members; preserve those transitions and action3's downstream path.

The TeamType policy identities are proven by ReadINI6F119E..6F1206: key8430AC
GuardSlower stores+A7, key843088 Reinforce stores+AB, both defaultfalse in ctor.
Corrected Ghidra's false `TeamTypeClass__AI` label at6F1090 to
`TeamTypeClass__ReadINI`: ctor installs vtable7F47D0 and slot+64 points here;
the body calls AbstractType.ReadINI410A60 and INI readers, not live Team AI.
Saved/read back that label/plate and focused comments at41CD6E,4CDBE1,4CDC37,
4CDCFB,566332,6EA089 and6EC300, preserving existing annotations.

### Team creation prerequisite: current acceptance and evidence

Acceptance before the production port: trigger action4 resolves its materialized
TeamType reference, brackets creation with the ScenarioInit-depth increment,
and constructs an empty native-equivalent Team through TeamScriptVm. Preserve
owner precedence, Max admission, initial waypoint Cell target, registration order,
base-defense/type counts, initial flags/timers/cursor, per-instance Tag/Trigger
construction and its Scenario RNG effects. Save/restore must retain each instance
independently. Do not reuse the test helper's pre-admitted members/cursor0 or add
a second trigger-state authority. Recruitment/activation must precede executing
the team's script and remain required after the construction rung.

`team_creation` runs39 original full creation paths:22 construct,17 refuse/no-op.
Only operator-new storage is supplied (poisoned arena); all gameplay constructors,
owner/limit branches, registry stores, waypoint and timer/RNG callees execute.
Its `--check` passes, with binary identity and exclusions in the sidecar. No Rust
source/schema/loader changed, so no Cargo rerun is warranted for this evidence.

- Action4 table6DFDEC+(4-1)*4 selects6DEB57. It increments A8E7AC and calls
  6F09C0 with NULL House, then restores depth and returns true even on refusal.
  At normal depth0 it bypasses Max. The triggering House is not the team owner.
- Create6F09C0 chooses explicit House, then Type+C4 House, then special slot+C8.
  **+C8 is not an ordinary country index**:510F60 accepts0x117B..0x1182;
  510ED0->68C030 maps it through Scenario+1180 to a registered House index.
  Without the depth bypass, campaign checks Type+DC; other modes count live
  teams matching both House and Type via5095D0. No RNG occurs on these gates.
- Constructor6E8A90 zeros membership and six TaskForce-entry counts;7A/7D/80
  start1, other74..84 bytes start0. Timers58/64 start at the current frame with
  zero durations60/6C. Native Script6913C0 starts cursor=-1. Waypoint helper
  6F18A0 supplies zero for index-1; ctor's empty-cell comparison is zero/zero,
  established by original static initializers6F0610/6E8A20. Nonzero waypoint
  becomes a Cell target+34, which the VM's entity-only target cannot represent.
- Type+F6 is existing IsBaseDefense (key81A7EC, store6F13CE): ctor increments
  House+566C when true, and Type+DC separately. Definition counts are not live
  Team counts. Membership6EA500 increments the admitted TaskForce **entry slot**,
  not an aggregate by type; retain this distinction for duplicate entry types.
- Type.Tag+D0 creates a new Tag6E4DE0 before Script. Each linked TriggerType is
  constructed in definition-chain order, then prepended to the runtime list:
  runtime traversal reverses construction/RNG order. Trigger725FA0 calls726400
  BEFORE its difficulty/enabled gate. Event13 starts duration15*argument;
  event51 uses15*(trunc(argument/2)+RandomRanged(0,argument)), wrapping i32.
  The disabled random-timer sample still draws. Seed31's two-trigger case uses
  four recurrence draws including rejections;65C7E0 inlines65C837 rather than
  calling65C780. Do not count API calls instead of recurrence updates.

Current TriggerRuntime is owned by Simulation but constructed in app/loading/init
from definitions alone. Its disabled/fired latches are keyed by TriggerType ID;
it has no live Tag/Trigger instance, timer or instance event-completion owner.
Migrate that existing owner and loader/restore/advance consumers before connecting
Team tags; do not create a competing Team-local trigger implementation. Native
Tag.ProcessTriggerEvent6E53A0 and Trigger.Spring7265C0 are the next lifecycle
readers to trace. Spring checks instance+44/+30, walks Type+B0 actions, resolves
the Type's House and ORs successful action results. Ghidra's false voice-only
label at7265C0 is corrected to TriggerClass__Spring. Named6F09C0
TeamTypeClass__Create_Team and saved/read back focused constructor/action/timer
annotations. Native creation evidence excludes loading, Spring, membership,
activation, recruitment and deletion; it does not complete this dependency.

### Trigger loading prerequisite: flag correction

Acceptance: native disabled/difficulty tokens must reach the existing map loader
and Simulation trigger owner with their original polarity, missing-token and
decimal conversion semantics. Parsed map input must produce the expected actions
through a production frame, including continuation after save/restore. This
bounded correction does not complete the required live Tag/Trigger migration.

`trigger_type_flags` preserves64 original reader runs: ReadString512 and original
CRT strtok/atoi followed by7273B9..72749B, with fresh and retained field priors.
No gameplay instruction/callee is replaced; external CRT TLS imports are supplied.
The first three tokens are consumed by original strtok; House/link/name resolution,
Events/Actions and live lifecycle are excluded. All29 applied fresh-definition
rows compare the four modeled Rust flags to native output. Token4 means disabled:
enabled+9F is true only for a present token with atoi==0; difficulty+9C/D/E is
true only for a present nonzero token. Missing tokens writefalse. Empty comma
tokens are skipped, whitespace tokens consume a slot. The existing shared decimal
scanner preserves CRT whitespace, prefixes and wrapping without another parser.

Production `map::triggers` now uses the bounded ReadString owner and separates
raw diagnostic fields from semantic tokens. `MapFile::from_bytes` regression
initializes TriggerRuntime as the app loader does, advances a real master frame,
and compares original/restored action effects. No snapshot layout changes.

Residuals: TriggerType token8 setsA0 only for nonzero input and otherwise retains
its prior value; it is NOT repetition. Current `MapTrigger.repeating` remains an
explicit legacy approximation pending the instance migration. TagType reader
6E6080 uses ReadString128: token1 atoi ->TagType+9C, token2 name, token3 TriggerType.
Tag6E53A0 walks live Trigger instances, gates through7264C0 and springs7265C0.
Mode2 repeats; mode1 springs/expires only withTag+2C==1; mode0 springs/expires.
Source detach5F5B50/485250 and deferred tag/trigger deletion throughB0F698 remain
required. Trigger7264C0 owns completion bits+40 and reset726400 before repeat
Spring;726720 marks+30 and enqueues. Spring resolves the TriggerType's House and
ORs action results. Do not reuse the definition-ID latches as instance state.
Corrected the misleading TriggerActionEntry wording in Ghidra6E53A0 and annotated
ctor726C9C with reader semantics; saved and read back both. No boundary edits.

Validation: native `trigger_type_flags --check` passes all64 saved rows;33 focused
trigger tests pass. Full `cargo test -p vera20k --lib`: **9,150 passed,0 failed,
135 ignored**,13.06s (`.local/trigger-type-flags-full-tests.log`); existing replay
hash pins pass unchanged. Library Clippy passes with1,030 existing warnings,
22.48s (`.local/trigger-type-flags-clippy.log`). The flags corpus compares fields
by native offset; only the existing medium-difficulty runtime selection is wired.
Scenario difficulty selection and live-instance lifecycle still need their full
production mapping. No critic or PR for this branch yet.

Consumer audit for the instance port: `headless_scenario::load_with_launch`
currently binds EMPTY graph/triggers/events/actions in SimResources and never
initializes TriggerRuntime. It cannot validate map-trigger parity. MapEntity
explicitly omits authored object tags; GameEntity has no live tag attachment.
SimResources also has no TagMap/CellTagMap, and TeamTypeIni retains raw fields
without a materialized Tag reference. Extend the shared simulation construction
and resource owners, migrate app/headless/restore consumers together, and preserve
native constructor/RNG ordering. The diagnostic TriggerGraph is currently reused
for definition-wide polling; it is not a substitute for native Tag instances.

Map-edge acceptance for the eventual production port: retained state and raw
versus queued mission gates select the correct branch; reuse map Size/LocalSize,
height-aware waypoint and scatter owners; preserve whole-lepton arithmetic,
zero/one RNG draw, coordinate-write refusal and following height/Mark effects.
Team creation, membership, activation/regroup and script progression must reach
the consumer through production and survive save/restore. Current78-case native
corpus `fly_map_edge` executes original4CDB4C up to4CDCFD or4CDD0D, including
Aircraft/Team/Script/map/RNG callees with no substituted calls. It covers34
commit and44 skip outcomes,33 zero-draw and45 one-draw cases, scatter success
and failure, FlyBy versus Spawned, raw versus queued missions, unsigned cursor
admission, waypoint height/slope, odd/non-square Size and local/span differences.
This is reproducible native evidence only: supplied Team states do not prove
their lifecycle, and the bounded run excludes prior Process admission and the
following SetCoords/Mark/height effects. Rust map-edge integration remains open.
Validation: `python -m tools.spatial_oracle.fly_map_edge --check` passes all78
saved cases; `git diff --check` passes. No Rust source, schema or loader binding
changed, so the deployment increment's lib/Clippy results remain applicable.
All owned native operations are terminal; no Cargo process was launched.

Deployment validation: `python -m tools.spatial_oracle.aircraft_mission_only
--check` passes96 original instruction-region pairs. The five added tests pass
for production construction/reveal, reinforcement on an ordinary landable type,
empty-selection clicks, snapshot/limbo/reveal continuation and isolated hashing.
`cargo test -p vera20k --lib` passes **9,148 passed,0 failed,135 ignored**
(14.26s execution; `.local/aircraft-mission-only-final-tests.log`). The initial
full run found only the three expected replay hash pins. Before(189) reproduces
each preceding pin and all RNG/behavior tripwires pass: current global
6AA2FA03DF444115, bridge2395935886825451856, slice6 3C8EAEF3C7685EC1.
Snapshot189 is an intentional layout/hash change, not a compatibility claim for
older saves. Ghidra comments4143EB/65E6BE/692766 saved and read back, preserving
the independent Aircraft+6C9 annotation. No critic or PR for this branch yet.
`cargo clippy -p vera20k --lib` passes (1,030 existing warnings,28.06s;
`.local/aircraft-mission-only-clippy.log`). All owned Cargo/native processes are
terminal. No release loader binding changed in this increment; the coherent
branch still needs the post-FlightLevel/Carryall retail load before merge.

## Current checkpoint (2026-09-22): native event records and Tag lifecycle

Owned worktree: `C:/Users/enok/.codex/worktrees/engine-ownership-boundaries/ra2-rust-game`;
branch `feature/combat-foot-speed`. Increment based on published `56c70104`;
the commit containing this checkpoint is the next source checkpoint. `.local/`
is intentional untracked evidence. No critic or PR for this branch yet. The
combat goal remains active. The dependency map is retired: never consult or refresh it.

Implemented: Event records now materialize native numeric values, optional type
names and unresolved Team references. TEvent Read71F4E0 consumes three tokens,
or four for parameter type2; empty comma tokens are skipped, names retain case
and whitespace and at most24 bytes, numeric values use CRT decimal-prefix/wrapping
semantics. The original TriggerType7274DC..727516 loop prepends records, so the
shared loader reverses authored conditions. All runtime consumers and fixtures
use these fields instead of mistaking ParamType for the operand. No new mutable
runtime state, snapshot version or replay rebaseline. Invalid global/local indices
return false: native Get leaves an output byte uninitialized outside0..49/0..99,
so stack residue is intentionally not reproduced.

Acceptance/validation: `trigger_event_records.{py,json,meta.json}` preserves54
original full constructor/read/list cases and bounded predicate samples. `--check`
passes. Rust `parsed_event_records_match_native_list_and_production_predicates`
compares record shape/order for every row and elapsed/global/local predicates
through340 production master-frame checks, including signed frame boundaries. Resolved
Team references, timer-instance state and nonempty TechnoType queries are NOT
certified. Existing type-count queries still use an approximate case-insensitive
entity-name scan without native reverse Type registry resolution; missing type
must eventually return false for BOTH60/61. The signed threshold/empty-entity
rule follows71EA03, but those registry cases need executable Rust comparisons.

All25 focused trigger tests pass. Full `cargo test -p vera20k --lib`: **9,152
passed,0 failed,135 ignored**,12.25s; `.local/trigger-event-records-full-tests.log`.
Library Clippy passes with1,030 existing warnings,29.15s;
`.local/trigger-event-records-clippy.log`. Snapshot189/replay pins are unchanged.
Release build passes in2m00s (`.local/trigger-event-records-release.log`). Retail
validation details and limitations are below.

`tag_lifecycle.{py,json,meta.json}` now preserves49 original histories;
`--check` passes and the39 earlier histories are unchanged. New cases execute
real Object construction/attachment, deferred drain725C70, original Tag/Trigger
destructors and the complete Logic tag-poll prefix55AFB0..55B205. Only storage
allocation/free, Windows IsBadReadPtr transport and fixture state are supplied;
no gameplay call or instruction is replaced. Lifecycle cases keep gameplay/
shutdown gates false. Registration classification, countdown expiry/UI callback,
Team/physical Object destruction and gated House-win/cursor updates remain open.
No Rust live-instance parity claim. Ghidra TEvent71F4E0 was named/annotated, and
Logic55AFB0's plate was extended while preserving prior notes; both saved/read back.

Native ownership and ordering to preserve in the migration:
- Find/create6E52A0 returns the first registered same-TagType instance even pending.
  Map object readers and scenario setup use it; Team constructor creates a fresh
  Tag and does NOT increment its reference count. Object/cell attachments do.
- Tag6E4DE0 constructs TriggerTypes forward through+A8 but prepends instances.
  Trigger725FA0 resets timers BEFORE enabled/difficulty checks. Abstract410170
  initializes+10=-1; these constructors do not assign a native unique ID.
- Trigger7264C0 evaluates all events, retains completion bits and resets before
  successful repeat Spring7265C0. Reset726400 walks reversed Event order:13
  writes15*value;51 draws RandomRanged(0,value), adds trunc(value/2), multiplies15
  with wrapping i32. Several events overwrite ONE timer; event bits use index mod32.
- Global689670/local689910 changed writes set Scenario+34AA and reset matching
  events on ALL live Tags, including disabled Triggers. Unchanged writes draw no RNG.
  Action22 springs matching live Triggers in construction order directly;53
  enables difficulty-admitted instances and resets;54 disables. Legacy force/
  enable queues and their tests still encode incorrect behavior and must be replaced.
- TagProcess6E53A0 repeat2 springs repeatedly; mode1 detaches until refs1 before
  spring/expiry; mode0 springs/expires. Trigger726720 marks+30 and queues. Mode0/1
  Tag expiry detaches/queues WITHOUT setting+34; separate6E5230 sets+34.
- Real ObjectAttach5F5B50 changes reference counts, including same-tag reattachment.
  Original deferred drain collapses duplicate pending entries. Trigger726950
  destructor splices expired instances from Tag heads and predecessor links.
  Tag6E4F60 queues remaining Triggers; the same drain processes those appended IDs.
- Scenario684C30 registers TagTypes in source order by ORed event/action category:
  bit4 map8B41A8, bit16 Logic8B40C8, bit8 House+38. Classifier71F680 takes KIND
  in ECX. Poll55AFB0 tries conditional50; changed27/28/36/37; conditional45/46;
  always13/51; expired countdown14. First successful Process skips later kinds.
  Corpus proves changed variables affect later Tags in the SAME poll, and mode0
  expiry compacts the LIVE vector without cursor repair: Tags0/2 run, Tag1 waits
  until the next poll. Tail clears+34AA/+34A9/+34AB/+34BE. Never use diagnostic
  TriggerGraph sorting as production order.

Next safe action: migrate the EXISTING TriggerRuntime to live Tag/Trigger instances,
removing definition-ID fired/disabled latches and MapTrigger.repeating. Keep this
owner reachable during actions: world::advance_triggers currently mem::takes it,
which would lose nested Team creation state. Move dispatch onto Simulation with
short state borrows; use world/lifecycle's existing pending-delete owner. Bind
static Tag/CellTag definitions before map-object construction, retain authored
Object attachments and initialize RNG/timers in native construction order. Wire
app, headless and restore together. App creates a detached TriggerRuntime in
loading/init.rs:3411; headless_scenario binds empty trigger tables; SimResources
lacks Tag/CellTag definitions; MapEntity omits tags. Shared object population is
sim/runtime.rs -> world/world_spawn.rs. Team construction/recruitment/AI are still
test-only prerequisites. These migrations are required, not optional follow-ups.

Current release SHA256
`38b387174a0328ee8fff4f527341b4bec585b1f2e38d5669abcc8ccaa3d349fe`
loaded sealed Soviet Fight.MAP far enough to reach scripted tick1. Capture is
still INVALID: MCV582 facing64 versus expected128, the same failure as the prior
checkpoint. See `.local/trigger-event-records-retail-soviet/{run.json,loader.log,
capture/capture.json}`; all sealed inputs stayed unchanged and the child exited.
Inherited RUST_LOG=1 retained warnings/errors, not INFO frame markers; do not claim
a new successful frame capture. No visual parity claim. All owned Cargo/native/
capture operations are terminal at this checkpoint.
Team activation/recruitment/scripts, Fly map-edge and other flight states,
launch/scatter/homing, vehicle click/shared resolver, fire legality, special
warheads and destruction remain required. No merge is pending.
