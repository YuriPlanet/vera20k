# Extra threat bonuses

Phobos can add per-type bonuses to the AI's threat score for a candidate. The
hook `TechnoClass_ThreatCoefficient_CanAttackMeThreatBonus` at `0x70CF87`
(`Ext/Techno/Hooks.TargetEvaluation.cpp:429-511`) runs inside
`TechnoClass::Calculate_Threat_Score @ 0x0070CD10` after the B, C, enemy-house,
A and D terms, at the E term's range load (`0x0070CF81`) and before its
distance subtraction (`0x0070D0A9`); every bonus is a plain double added to the
stack `totalThreat`.

```ini
[TECHNO]                                     ; TechnoType
ExtraThreat.IsThreat=                        ; Nullable<double>, unset -> [General]
ExtraThreat.InRange=                         ; Nullable<double>, unset -> [General]
ExtraThreatCoefficient.InRangeDistance=      ; Nullable<double>, unset -> [General]
ExtraThreatCoefficient.Facing=               ; Nullable<double>, unset -> [General]
ExtraThreatCoefficient.DistanceToLastTarget= ; Nullable<double>, unset -> [General]
AlwaysConsideredThreat=no                    ; bool (prerequisite for IsThreat)

[General]
ExtraThreat.IsThreat=0.0
ExtraThreat.InRange=0.0
ExtraThreatCoefficient.InRangeDistance=0.0
ExtraThreatCoefficient.Facing=0.0
ExtraThreatCoefficient.DistanceToLastTarget=0.0
```

Readers `Ext/TechnoType/Body.cpp:1181-1186`, declarations
`Ext/TechnoType/Body.h:423-429`, constructor `:843-849`; the `[General]` reads
are `Ext/Rules/Body.cpp:535-539`, defaults `Ext/Rules/Body.h:952-956`. There is
no enable key: `ExtraThreat_Enabled` is "any resolved value is not `0.0`"
(`Body.cpp:1187-1191`).

## Behavior

The bonuses are applied to the scanning techno's score, in this order:

1. `ExtraThreat.IsThreat` when `IsAThreatToMe` (`:381-404`) holds: the target
   is a Techno, has `AlwaysConsideredThreat=yes`, or its selected weapon
   against the scanner exists and `GetFireError(scanner, slot,
   ignoreRange=true)` is not `ILLEGAL` (a Building also rejects `RANGE`).
   `ignoreRange=true` keeps an armed target that is merely out of range.
2. `ExtraThreat.InRange` + `distanceCells * InRangeDistance` when
   `IsCloseEnoughToAttack` holds; `distanceCells = DistanceFrom / 256`.
3. `deltaFacing * Facing`, where
   `deltaFacing = 32768 - | |targetDir.Raw - fireFacing.Raw| - 32768 |`, so it
   is 0 when the fire facing points at the target and 32768 when it points
   away. The fire facing is PrimaryFacing for a Building, SecondaryFacing for
   an Aircraft, SecondaryFacing (`Turret=yes`) or PrimaryFacing for a Unit, and
   none for an Infantry.
4. `targetDistanceFromLastTarget * DistanceToLastTarget` unless the scanner's
   `LastTargetCrd` is `(0, 0, 0)`. The coordinate is maintained by
   `TechnoExt::UpdateLastTargetCrd` (`Ext/Techno/Body.Update.cpp:1286-1309`,
   from `OnEarlyUpdate` at `:24`): with a target it follows
   `Target->GetCoords()`; without one a 45-frame timer clears it.

Each bonus is skipped when its resolved value is `0.0`.

## Where VERA20K owns it

The keys live in `rules::phobos_extra_threat` on
`ObjectType::phobos_extra_threat`, with the `[General]` fallbacks in
`PhobosGlobals`. The score body `sim::combat::greatest_threat::threat_score`
calls `sim::combat::extra_threat::apply_extra_threat_bonus` after its D term and
before its E term, so both the target scan and `ShouldRetaliate` (through
`combat_targeting::calculate_ai_threat_score`) see the same bonuses. That
function reuses the existing owners: the candidate's `SelectWeapon` result from
the B term, `FireSubject::fire_error` for step 1, `in_range::can_fire_at_target`
for step 2, `turret::facing_toward_lepton` plus the entity facings for step 3,
and `object_distance_to`/`object_get_coords` for the two distances.

`GameEntity` stores `TechnoExt::LastTargetCrd` and its 45-frame clear timer;
`update_phobos_extra_threat_last_target` runs in the shared techno AI body where
`OnEarlyUpdate` calls it, after `EatPassengers` and before
`CheckDeathConditions`.

## Precision

Both distances follow the native arithmetic:

- Step 2 is `AbstractClass::DistanceFrom @ 0x005F6440`, which returns an
  **int**: it sums `dx² + dy²`, runs `Sqrt_Approx @ 0x004CAC40` and `ftol`,
  then subtracts `(Width + Height) * 64` for a building and clamps at 0. VERA's
  `object_distance_to` owns that body, so `f64::from(int) / 256.0` is exactly
  the native `DistanceFrom / 256.0`.
- Step 4 is YRpp's `CoordStruct::DistanceFrom` (`Vector3D<int>`):
  `sqrt((X*X + Y*Y) + Z*Z)` in double (`YRMathVector.h`). Phobos is compiled
  with `/arch:SSE2` (`Phobos.props:45`), so its `sqrt` is the IEEE `sqrtsd`
  that Rust's `f64::sqrt` emits.

One residual remains in the shared step-2 kernel: `object_distance_to` (the
`ObjectClass::Distance @ 0x005F6360` body) narrows the squared sum to f32
before `Sqrt_Approx`, while `0x005F6440` stores the sum to a double first; a
double-rounding boundary could move the truncated int by one lepton.

## Residuals

- A synthetic entity with no `barrel_facing` falls back to its body facing;
  every production Aircraft and `Turret=yes` Unit gets a SecondaryFacing at
  construction (`world_spawn/construction.rs:210`).
- `LastTargetCrd` advances only from `techno_common_steps`, the port of
  `TechnoClass::AI` where Phobos hooks `OnEarlyUpdate`. A limboed object runs no
  techno AI in either game, so its coordinate and timer carry over unchanged.

Both the target scan and the retaliation path pass their live overlay registry
into the bonus, so the InRange walk sees the same walls in both.
