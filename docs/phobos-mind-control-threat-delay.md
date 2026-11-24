# MindControl.ThreatDelay

A fresh mind-control capture normally makes the new pawn shoot back at the
allied units it was just fighting beside. Phobos adds a grace period: for its
configured frames, the houses allied with the pawn's original owner neither
retaliate against it nor pick it as an auto-target.

```ini
[General]
MindControl.ThreatDelay=0          ; Valueable<int>, default 0

[CONTROLLERWARHEAD]                ; Warhead section
MindControl.ThreatDelay=30         ; Nullable<int>, unset -> the general key
```

Readers: the general key at `Ext/Rules/Body.cpp:380` (constructor
`Ext/Rules/Body.h:802`); the warhead key at `Ext/WarheadType/Body.cpp:244`
(`Nullable<int>`, `Body.h:90,369`).

## Native body

- `BulletClass_Detonate` (`0x4692BD`,
  `Ext/Bullet/Hooks.DetonateLogics.cpp:41-56`) resolves
  `WarheadTypeExt::MindControl_ThreatDelay.Get(RulesExt::Global()->MindControl_ThreatDelay)`
  and passes it to `CaptureManagerExt::CaptureUnit`.
- `CaptureManagerExt::CaptureUnit` (`Ext/CaptureManager/Body.cpp:200-201`)
  sets `TechnoExt::BeControlledThreatFrame = CurrentFrame + threatDelay` only
  for a positive delay; zero leaves the field alone.
- `CaptureManagerExt::FreeUnit` (hook `0x471FF0`,
  `Ext/CaptureManager/Hooks.cpp:15`; `Ext/CaptureManager/Body.cpp:142`)
  clears the field before the original owner returns.
- `CanAttackMindControlled` (`Ext/Techno/Hooks.Misc.cpp:606-620`) admits the
  attack when the object is not mind-controlled, the asker is berserk, the
  controller has no capture manager, or the asker's house is not allied with
  the manager's `GetOriginalOwner` of the pawn. Otherwise only
  `BeControlledThreatFrame <= CurrentFrame` admits it.
- The retaliation hook `TechnoClass_AllowedToRetaliate_AttackMindControlledDelay`
  (`0x7089E8`, `Hooks.Misc.cpp:621-629`) jumps to `CannotRetaliate`
  (`0x708B17`, returns false).
- The auto-target hook `TechnoClass_CanAutoTargetObject_AttackMindControlledDelay`
  (`0x6F88BF`, `Hooks.Misc.cpp:631-641`) jumps to `CannotSelect`
  (`0x6F894F`, returns no score) after
  `Evaluate_Candidate` has computed the score and before
  `ThreatAvoidance_Modifier`.

## VERA

`PhobosGlobals::mind_control_threat_delay` reads the general key;
`WarheadType::mind_control_threat_delay` reads the nullable warhead key.
`GameEntity::phobos_be_controlled_threat_frame` holds the per-object frame
and serializes with the entity (Phobos serializes it in `TechnoExt`,
`Ext/Techno/Body.cpp:1178`). `mind_control_detonation` resolves the key pair
and arms the field after a successful capture;
`Simulation::free_unit_with_sound` clears it. `Simulation::can_attack_mind_controlled`
is the shared gate, called from `combat_targeting::should_retaliate` (between
the human C4/artillery arm and the `PlayerReturnFire` arm, where the hook
replaces the repeated `IsHuman` test on the common path) and from
`greatest_threat::evaluate_candidate` (after the score, before
`threat_avoidance`).

## Tests

- `src/sim/capture_manager_tests.rs` — `mind_control_threat_delay_arms_the_shield_and_free_unit_clears_it`:
  a warhead key of 5 beats a general 9; an unset warhead key takes the
  general 9; an explicit warhead 0 disables the shield; `FreeUnit` clears it;
  a plain `capture_unit` (no bullet detonation) never arms it.
- `src/sim/combat/combat_tests.rs` — `a_freshly_mind_controlled_source_is_shielded_from_its_former_allies_retaliation`:
  refused through the boundary frame minus one, admitted on the boundary,
  and admitted for a berserk retaliator, a manager-less controller and a
  non-allied house.
- `src/sim/combat/greatest_threat_mind_control_tests.rs` —
  `a_fresh_capture_shield_keeps_the_candidate_out_of_the_scan_until_it_expires`:
  the same boundary behavior through the model's real target scan, plus the
  berserk and no-manager arms.

The capture-side write, the `FreeUnit` clear and the shared gate were each
mutated red and restored.

## Residuals

- Only the bullet mind-control detonation passes a delay, as in native; the
  Ares survivor and vehicle-theft captures call `CaptureUnit` with the
  default 0 and leave the field untouched.
- `GetOriginalOwner` answers null without a node; Phobos would pass that null
  into `IsAlliedWith`, while VERA treats it as "no shield". A live manager
  holding the pawn always has the node, so the arm only covers detached
  states.
- The general key is read from `[General]` in the single Phobos-globals
  pass; it is not a warhead fallback for `MindControl.Anim`.
- The frame is written after `capture_unit_with_ring` returns, where Phobos
  arms it before `SetOwningHouse`/`DecideUnitFate`. No reader of the field
  runs inside that window, so the same-tick state is indistinguishable;
  a native capture whose owner change fails after arming would keep the
  shield, which VERA does not reproduce.
