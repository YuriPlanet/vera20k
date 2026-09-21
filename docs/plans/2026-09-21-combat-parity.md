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

## Current dependency: Display membership required by movement pickup

Task-owned worktree: `C:/Users/enok/.codex/worktrees/engine-ownership-boundaries/ra2-rust-game`.
Branch `feature/combat-foot-speed`, based on merged PR440 (`a37e8118`).
Source increment: `17520993c5e79ec1752736a0a9f2a5774bde887d`; preceding HEAD `4aae0c2b`.
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
type layer and registered layer can differ: the renderer still needs migration.

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

### Evidence and validation

Full `cargo test -p vera20k --lib`: **9,113 passed, 0 failed, 134 ignored**.
Log `.local/anim-display-tests-3.log`; existing replay pins are unchanged.
Earlier attempt exposed three stale/synthetic fixture assumptions, corrected
against original expiry behavior and the restored reverse-link invariant.
`cargo clippy -p vera20k --lib` passed with1,033 warnings in1m27s.
Log `.local/anim-display-clippy.log`. All owned validation processes are terminal.
User amendment: the dependency map has been removed from GitHub main and is no
longer used. Do not consult or refresh it, even if this branch's older contract
still requests it. No dependency-map refresh was run for this increment.
Retail release load is still required for Layer/load-context, Flat and CruiseHeight.

Original executable comparisons passed this increment:
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
ILoco_Process4CCB40 (real GetLayer4CFCF0). No binary, signature or boundary edits.

### Required continuation

1. Complete explicit Fly resubmissions:4CD4E7 before Mark(PUT),4CD75A/4CD792
   around relocation. DropIn5F400E/5F4196 and Jumpjet touchdown54CC0D remain open.
   Generic per-frame cache refresh is not equivalent. Resolve Aircraft overrides
   and reachable receivers from their native objects.
2. Migrate presentation/input from Logic-based `tactical_registration_order()`
   and full sorting to Display. Current fallback appends store objects; Anim
   destination still derives live Layer rather than historical membership.
   `overlays.rs`, `render/build_instances.rs` and `instances/helpers.rs` are
   known consumers. Preserve layer-specific order and deletion visibility.
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
5. Release retail load, then one fresh critic only after coherent implementation
   and validation before PR. PR439/440 critics are finished; do not repeat them.

HouseType speed factors+128/+12C/+130 remain absent. Unit+6CC CTF starts-1;
740DF0/740E20 attach/detach, Limbo/destruction return it. Creation4FC060 caller
688C02 is gated by Scenario[0]&0x10; trace the producer before calling it unreachable.
Normal SimFixed speed ranges do not prove arbitrary non-retail crate multipliers.
Broader combat remains open: Active_Click_With+6E0, shared cursor/click, NavCom/
action/fire legality, FLH slope/scatter/homing, special warheads, visibility and
whole-combat acceptance. Recording a required omission does not resolve it.
