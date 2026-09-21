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
The paid step still reaches `movement_tick`'s `advance_lepton_position` adapter,
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
selects the existing same-cell or boundary transaction. Rust currently reaches
`advance_lepton_position` and a direct normalized vector instead; the source
comment claiming a live native-polar WalkHost dispatch is incorrect.

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

`tools/spatial_oracle/walk_paid_step.py --check` passes 35 original-code vectors.
The actual constructor, speed setter, coordinate/cell getters, Walk facing setter
and trigonometric routines execute; only the movement-speed integer is supplied.
Examples at supplied speed 6: native south advances (-1,+6), west (-6,-1), and
northeast (+4,-5), showing why a direct normalized vector is insufficient.
The live-turn equality witness retains body destination `3FFF` but moves north:
the displacement uses the freshly computed SI direction, not the retained body.
Ghidra comments at `75BFA9` and `75C067` are saved and read back. Corrected the
false production-dispatch comment in `movement_step`; runtime port is unfinished.

Speed prerequisite audit: `+538 -> Infantry521D80` calls Foot4DB1A0 and then
applies prone adjustment (Crawls: speed minus truncated speed/3; otherwise
speed plus truncated speed/2). Rust's `infantry::apply_prone_speed` already owns
that adjustment. `track_speed::advance` and
`drive_locomotion::owner_current_speed_from_fraction` own current track production
speed, but their documented house/crate/CTF gaps remain; `track_speed_native` is
an isolated numeric oracle comparison, not a production replacement. Preserve
SimFixed policy when completing the common getter's inputs and consumers.

Next safe action: port/integrate the paid numeric Walk chain using the saved
witnesses and proper Foot speed/facing owners; remove its generic competitor.
No Rust runtime change, Cargo process, new critic or new PR is pending here.
