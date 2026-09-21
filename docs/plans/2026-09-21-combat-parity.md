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

Current task-owned branch: `feature/combat-foot-speed`, created from fetched
`origin/main` at `a37e8118`. Walk work is integrated. Pickup selection/removal
prerequisites are committed at `8c8e0f8e`; no PR or critic pass has been requested
for this unfinished mechanism. The primary checkout remains untouched.

Native4DB1A0 applies type speed, House50C050's category factor and Foot+580,
truncates, applies FASTER/VeteranSpeed with another truncation, then applies
Foot+578 and truncates before the Unit+6CC conditional halving. Production
`foot_speed` lacks the house/crate/flag inputs; `CountryRules` does not parse the
speed multipliers, and `crates` explicitly leaves pickup effects unported.

Walk's accepted-head selection calls481A00 at75C56C, passing the Foot owner and
the Cell returned by Map565730. This proves a production pickup route; the prior
Ghidra name `CrateClass__PickupDispatch` assigned the wrong receiver class.
Renamed to `CellClass__PickupCrate`, with the receiver/caller evidence saved and
read back. Entry reads Cell+44 overlay and+11E selection. Its speed continuation
48302E..48306C checks a strict distance, a currently-one+580 multiplier and object
kind before storing the multiplied factor. The continuation lies beyond Ghidra's
current function boundary; that boundary was not changed.

Before implementation, establish pickup selection, eligibility, effect iteration,
RNG, removal and return behavior from original instructions, with executable
witnesses. Acceptance requires production movement to reach the same pickup
effects and subsequent Foot speed, including affected consumers and save/restore.
Use the existing country, crate, Foot and lifecycle owners; adding a supplied
factor to an isolated speed helper does not complete this dependency. Trace the
Unit+6CC active-scenario reachability before adding a producer to normal play.

Native evidence checkpoint: `crate_speed_effect --check` passes 23 original
recipient loops followed by original Foot4DB1A0 queries on the resulting state.
The fixture supplies selection/frame/vector state; it replaces no gameplay call.
Infantry/Unit/Aircraft actual vtables return15/1/2, establishing that the kind2
gate excludes aircraft. Distinct-house recipients both receive the factor; the
loop has no house-equality filter. Existing factors, even the binary64 neighbors
of1.0, refuse stacking. The native distance approximation truncates a767-lepton
axial offset to766; radius768 still refuses distance768. Flat/elevated/slope,
null-vector and control-byte contrasts are preserved. The applied stock1.2 factor
then makes raw speed10 return11, not12, in original Foot speed math. The483054
annotation is saved/read back. None of these cases executes weighted selection,
removal or the UI suffix; pickup production code remains to be implemented.

`crate_pickup --check` now preserves 28 original entry-to-return/dispatch cases.
Four guards stop before selection; 24 cases expose the prepared outcome, full
Scenario RNG before/after, removal return, overlay fields and both first slots.
Only screen rectangle/dirty sinks are supplied; gameplay callees execute original
bytes. Free-MCV preemption, attached triggers, regeneration and non-speed effects
remain outside this corpus. The unchanged speed-effect corpus still passes.

`crates::pickup::select_pickup_outcome` ports stored/weighted selection and solo
overrides using existing rule/RNG owners. Silver, Wood and Water image matches
run independently in that order: aliased images allow later mappings to win.
Only stored selection zero installs SoloCrateMoney and those overrides. The
multiplayer eligibility fallbacks still follow, so e.g. a drawn Firepower may
become Money without refunding RNG. A one-weight table consumes no draw.

`crates::runtime::remove_pickup_crate` ports Map56C020. Multiplayer clears the
first occupied matching slot using the existing clear/timer owner; no matching
slot leaves a visible crate intact. Solo accepts any Crate-flagged overlay,
without the diamond precheck, and does not touch slots. The existing regeneration
path retains its Rules-image identity gate; both paths share only final field
erasure and its presentation receipt. Rust corpus tests compare 24 removal
transitions and 24 prepared selections/full RNG states. These owners are not yet
called from movement: do not describe this as production pickup parity or merge
it as a finished pickup mechanism. The final expanded crate suite passes:
63 passed, zero failed. Log: `.local/crate-pickup-tests.log`. Both native corpus
checks, Python compilation/help and `git diff --check` pass. No Cargo operation
remains pending. Full-library/Clippy and one fresh critic remain pre-PR work once
the production mechanism is integrated.

Native identities/comments corrected in source and saved/read back in Ghidra:
Cell receiver at481A00 (not CrateClass), removal56C020, solo ordering481B58,
and Foot speed's final truncation4DB213 (third when FASTER runs, second otherwise).
Unit+6CC identity now has writer/lifecycle evidence: constructor7353F2 seeds-1;
740DF0 attaches the flag index and re-Marks;740E20 detaches;7440BD and735884
return/clear it on Limbo/destruction. Render73D395 indexes Houses with it.
The sole observed House4FC060 creation caller688C02 is gated by Scenario[0]&0x10
at688BF8. Retail `[MultiplayerDefaults] CaptureTheFlag=no` is explicitly marked
DESUPPORTED, but this does not prove all-scenario unreachability. The4DB226 note
preserves that distinction; normal-play flag creation is not implemented.

Next safe action: finish the pickup eligibility/effect host, including free-MCV
selection, the synchronous trigger, replacement-before-effect order and return
behavior; integrate movement callers and live Foot inputs/save-restore. Reuse
`combat_weapon::is_armed` for the Firepower vt+2AC predicate (original701120),
not a new primary-weapon check. `HouseState::owned_unit_count` counts all
non-buildings, so do not substitute it for native House+2E8 Unit/House+2F4 Infantry counts
without tracing their lifecycle. EntityStore's per-type count also documents a
limbo-window difference. Preserve effect vector order and exact predicates.
The full speed dependency and whole-combat goal remain open.
