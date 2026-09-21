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

## Current dependency: live Foot speed inputs and crate pickup

Task-owned worktree: `C:/Users/enok/.codex/worktrees/engine-ownership-boundaries/ra2-rust-game`.
Branch `feature/combat-foot-speed`, based on merged PR440 (`a37e8118`).
Selection/removal prerequisites: `8c8e0f8e`; live speed: `d2a45ba4`;
display ownership source HEAD: `e8e12fb2` (snapshot182).
Module map refreshed from that commit:842 modules,5,311 dependency edges.
No PR or critic pass for this branch. Production pickup remains unfinished.
The primary checkout and `.local/` data are excluded from publication.

Acceptance remains movement pickup through native selection, eligibility,
trigger, removal/replacement, effect and subsequent movement/combat, with matching
RNG, returns and save/restore. The display prerequisite must preserve registration,
resubmission, removal and single-pass Ground ordering through all actual consumers.
Neither the isolated effects nor an entity-only list complete pickup.

### Retained speed work

`FootSpeedState` owns binary64 +580, initialized exactly1.0. The effect writes
this owner; the entity speed resolver consumes it before FASTER, retaining the
separate native truncations. Drive/Ship and paid Walk regressions cover live
changes without another move order; Walk attack survives production save/load.
Hover and tube exit use the live getter. These tests do not walk through a crate.
Snapshot181 introduced the factor; current work advances the schema to182.

`crate_speed_effect` has23 original recipient loops plus subsequent Foot queries;
`crate_pickup` has28 entry-to-return/dispatch cases; `track_speed_native` has75 full
getters and116 Drive/Ship prefixes. Exact input bit strings prevent the JSON
decoder from rounding a neighbor of1.0 into an eligible factor. Stock
VeteranSpeed1.2 produces10/15/17 ->11/17/20, or13/20/23 after crate1.2.
The previous committed candidate passed9,099 library tests,134 ignored, and
library Clippy (1,032 warnings). Logs remain under `.local/crate-speed-live-*`.

### Display owner under implementation

`world/display_layers.rs` owns five private ordered vectors. The ID-to-layer
index is derived, never iterated for gameplay, and rebuilt on load. Submit4A9720
removes old membership, queries the layer and inserts Ground before the first
strictly greater GetYSort; equal keys remain ahead. Other layers append. Remove
4A9770 preserves order and has a fallback scan for a wrong cached layer.
MainTick55DBC8 runs one adjacent Ground sort551A30 before Logic55DC9E, including
its trigger work. Snapshot182 preserves the vectors and rejects null/duplicate
or missing identities. Hashing folds their order independently of Logic.

Entity Reveal submits at its existing display boundary before the separate
Logic gate; Conceal removes display before Logic. Ground sorting runs at the
pre-Logic frame rung. Jumpjet Process captures its live layer query on entry,
then the alive tail resubmits only if its new query differs (54AECB/54AED1 and
54B16F..54B18E). It does not compare with cached Object+94. Speed effects now
read the owner's Ground slice rather than accepting an arbitrary supplied list.
The building render-coordinate/Y-sort adjustment is shared with presentation
through `ground_pose`, replacing that duplicated math.

`display_entity_layer` executes88 original query cases with real Unit/Building
vtables and constructed Walk/Drive/Fly/Jumpjet instances. Physical GetHeight,
marked-on-map, structural bridge, falling, slope and distinct linked/global
heights are covered. No calls are substituted; supplied lifecycle/type/map state
and excluded Process cadence are declared in the sidecar. All88 Rust comparisons
passed the first run. `crate_ground_membership` preserves six actual registration,
removal and single-pass-sort sequences; its Rust comparisons also passed.
Both corpora and `crate_speed_effect --check` reproduce their saved native outputs.

The independent [JumpjetControls] CruiseHeight reader supplies Rules+420:
constructor665C3A defaults400; ReadJumpjet67446D/67447E reads the signed key.
Object5F4260 uses it, while Jumpjet54B8D0 uses linked locomotor+2C.
Retail RULESMD sets500. The Rust reader also handles an absent General section.
Rules changed, so a release retail map-load check is still required before merge.

First full Rust run:9,099 passed,4 failed,134 ignored. New owner/query/save tests
passed. Three failures were current replay pins; old pre181 projections and RNG
checks still passed. New pre182 projections now require the exact previous whole
fixture hashes before accepting the new pins. The fourth failure exposed a
cloaking fixture that erased a displayed entity directly from storage; it now
calls Conceal before erase. Jumpjet's initial cached-layer comparison was also
replaced with the native before/after query comparison, with a regression for
stale cached membership, missing registration and the alive gate.
Final full library validation passes:9,105 passed,0failed,134 ignored, logged
in `.local/display-owner-tests-final.log`. The pre182 projections reproduce all
three former full-state pins; only the added display fold changes their current
hashes. The production lifecycle/effect test changes the displayed non-Logic
recipient and leaves concealed/unrevealed peers unchanged. A display-only sort
changes the current hash while preserving its pre182 projection.
No critic, Clippy or release loader run for this unfinished candidate.

### Required continuation

1. Complete non-entity display dispatch and lifecycle (anims including owner
   attach/expiry, terrain, voxel debris, projectiles, particles and waves).
   The current sort dispatcher deliberately expects entity identities; extend
   it before admitting another registry. Do not substitute Logic membership.
2. Complete Fly, falling/DropIn and other explicit resubmission writers. Fly
   cannot use a generic layer/cache refresh:4CD4E7 submits in its ascent/descent
   wrapper before Mark(PUT), and4CD75A/4CD792 surround relocation. The current
   integration does not claim these producers. Aircraft special-type +54
   overrides and invalid/headless-only locomotor combinations also need their
   actual receiver when they become reachable. No-terrain query fallback remains
   a headless adapter, not native parity evidence.
3. Migrate presentation/input from `tactical_registration_order()` (still Logic)
   and full sorting to the actual display owner. Its entity fallback currently
   appends store objects. Do not expose the partial display migration as complete.
4. Finish movement pickup. Selection/removal helpers remain uncalled. Preserve
   passive guards, Tag49/Scenario+34BE, free-MCV preemption, multiplayer eligibility,
   water fallback, removal and replacement-before-effect order, and returns.
   Reuse `combat_weapon::is_armed` for701120. House `owned_unit_count` combines
   non-building families; per-type counts also have a limbo-window mismatch.
   Trace native Unit+2E8/Infantry+2F4 producers before using those counts.
   Port all selected effect arms through their downstream owners.

HouseType speed multipliers+128/+12C/+130 remain absent. Unit+6CC carries a CTF
flag, initialized-1;740DF0/740E20 attach/detach, Limbo/destruction return it.
Creation4FC060 has caller688C02 gated by Scenario[0]&0x10. Retail disabled flags
alone do not prove unreachability; trace the scenario producer. SimFixed's public
speed range also does not certify arbitrary large non-retail crate multipliers.

The broader goal remains open: vehicle Active_Click_With+6E0, shared cursor/click
resolution, NavCom/action/fire legality, launch FLH slope/scatter/homing, special
warheads, visibility and the final whole-combat audit. Prior accepted PR439/440
critic passes are complete; do not repeat them. This branch needs its one fresh
pre-PR critic only after coherent production integration and validation.

Ghidra annotations are saved/read back at55DBC8 (pre-Logic adjacent sort),67447E
(global CruiseHeight),54B17F (live before/after query). Corrected the false
FlyLocomotionClass__Layer name at4CCB40 to FlyLocomotionClass__ILoco_Process:
constructor4CCA12 installs table7E89F4, slot+40 ->4CCB40; active FootAI4DA877
calls it through Foot+674. Slot+74 ->4CFCF0 is the real layer getter. This is a
label/comment correction only, with no signature, boundary or byte changes.
Earlier crate/getter annotations remain saved. Preserve these unfinished
production migrations; a passed owner test or committed dependency does not
complete the mechanism or the combat goal.
