# ApproachTarget

Two Phobos keys decide how a foot's Approach (MegaMission) terminates when it
is close enough to fire.

```ini
[TECHNO]                       ; TechnoType
AttackMove.StopWhenTargetAcquired=  ; deprecated alias of the next key
ApproachTarget.StopWhenInRange=     ; Nullable<bool>, [General] fallback
AttackMove.PursuitTarget=           ; deprecated alias of the next key
ApproachTarget.PursuitTarget=       ; Valueable<bool>, default no

[General]
ApproachTarget.StopWhenInRange=no   ; Valueable<bool>
```

Readers: `Ext/TechnoType/Body.cpp:1093-1101`, declarations
`Ext/TechnoType/Body.h:316-317`, constructors `:750-751`; the global is
`Ext/Rules/Body.cpp:642-643` (`Body.h:537,1036`, default no). Both type keys
read the deprecated `AttackMove.*` name first and the new key second, so the
new key wins when both are present and the alias survives otherwise; the
`Nullable` stop key falls back to `[General]` only when neither is set.

## StopWhenInRange

`FootClass_ApproachTarget` (`0x004D5690`) computes `closeEnough` from
`TechnoClass::IsInRange` (`0x4D570E`, stored at stack `+0x12`). At
`0x4D5A1A` the in-range return is `closeEnough && IsInPlayfield`
(`+0x3D5`); the Phobos hook `0x4D5A34`
(`Ext/Techno/Hooks.Firing.cpp:1303-1340`) runs inside that return: an
in-limbo or not-really-moving foot is skipped, otherwise the type key
(else the global) calls `StopMoving()` and `AbortMotion()`. VERA performs the
same check in the in-range arm of
`sim/movement/foot_approach.rs::approach_foot_target`, using
`motion_query::is_really_moving_now` and the existing `locomotor_stop_moving`
(`StopMoving`) plus `navigation.stop_moving` (`AbortMotion`) owners.

The in-range arm is evaluated before the existing-path return: native reaches
the `0x4D5A34` tail even while the foot holds a destination
(`0x4D5A07`'s `Is_Moving` arm), and `StopWhenInRange` needs to run there.

## PursuitTarget

Hook `0x4D57EA` (`Ext/Techno/Hooks.Firing.cpp:1343-1362`, 9 bytes replaced)
sits before the nav recalculation. When the type key is on and the foot is
not in limbo and not (`closeEnough && StopWhenInRange`), it calls
`SetDestination(Target, true)` (`vt+0x480` mode 1), clears EDI and stack
`-0x130`, and jumps to `0x4D5A34`. The zeroed pointer makes the original
`mov eax, [esp+0x28]; cmp eax, edi; je` take its return branch, so native
performs no nav recalculation, no queued-destination hand-off and no search:
the Target stays the destination.

VERA's single destination owner is `assign_destination_represented`; the
pursuit arm calls it with the target reference and returns `Ok(None)`.

## Where VERA20K owns it

- `rules::phobos_attack_move::PhobosAttackMove` gains the two keys and
  `stop_when_in_range(&PhobosGlobals)`; `PhobosGlobals` gains the
  `[General]` fallback.
- `sim/movement/foot_approach.rs::approach_foot_target` places both hooks at
  their native points; no second approach owner is introduced.

## Residuals

- Native hook A also runs from the not-moving tail at `0x4D5A18`. There
  `closeEnough` is stale/zero for the interesting case, so its stop arm
  cannot fire; VERA evaluates the stop only in the in-range arm. No trigger.
- `SetDestination` cannot fail natively; VERA's preflight can reject a
  destination whose object vanished between reads. The arm returns the
  approach error instead of a silent no-op. Trigger: a target destroyed in
  the same read window; no retail scenario observed.
- The `DeployToFire=` range override (`0x4D57EA..0x4D5863`) remains the
  existing residual in `foot_approach.rs`; the pursuit jump skips it exactly
  as native does when it fires.
- No retail type authors either key; the assertions use synthetic rules.
