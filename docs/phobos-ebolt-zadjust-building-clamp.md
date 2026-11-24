# EBoltZAdjust.ClampInitialDepthForBuilding

A building's electric bolt subtracts the building origin from its source depth
and stock clamps a positive result to zero. Phobos makes that clamp
switchable, per weapon over an `[AudioVisual]` default.

```ini
[AudioVisual]
EBoltZAdjust.ClampInitialDepthForBuilding=yes  ; boolean, default yes

[COIL]                              ; Weapon section
EBoltZAdjust=0
EBoltZAdjust.ClampInitialDepthForBuilding=no   ; nullable, unset -> global
```

Readers: the global at `Ext/Rules/Body.cpp:279` (`Valueable<bool>`,
constructor yes at `Ext/Rules/Body.h:197,720`); the weapon at
`Ext/WeaponType/Body.cpp:132` (`Nullable<bool>` unset, `Body.h:62,157`).

## Native body

- `TechnoClass_FireEBolt` (`0x6FD4E4..0x6FD4ED`) computes
  `zAdjust = source depth - building origin depth` and clamps a positive
  result to zero with `test esi,esi; jle; xor esi,esi`.
- `TechnoClass_FireEBolt_Building_ClampPositive` (hook `0x6FD4E4`,
  `Ext/EBolt/Hooks.cpp:189-205`) reproduces the subtraction and clamps only
  when `EBoltZAdjust_ClampInitialDepthForBuilding` is set; with the key off
  the positive delta reaches `EBolt_Draw` (`0x6FD4ED`) unchanged, which then
  adds `EBoltZAdjust=` (`Hooks.cpp:207-220`).

## VERA

`BeamZAdjustGlobals::ebolt_clamp_initial_depth_for_building` reads the global
(default true) and `WeaponType::phobos_ebolt_clamp_initial_depth_for_building`
the nullable override. `electric_bolt::create` resolves the pair and passes it
to `fire_coord::effect_z_adjust`, whose `ClampedBuilding` arm keeps the
positive delta when the clamp is off. Lasers and the radiation beam keep the
stock clamp.

## Tests

- `src/sim/combat/electric_bolt_tests.rs` —
  `the_building_initial_depth_clamp_is_keyed`: a synthetic tower fires a bolt
  from 4000 leptons below its projected base; the default and weapon `yes`
  clamp the positive delta to 0, the weapon or global `no` keeps it positive,
  and the weapon key overrides the global.
- `src/rules/phobos_beam_z_adjust_tests.rs` — the global clamp key defaults to
  yes and reads `no`.

The resolved clamp was mutated red and restored.

## Residuals

- `BulletExt::SimulatedFiringElectricBolt` (`Ext/Bullet/Body.cpp:268-280`)
  clamps a positive `EBoltZAdjust` for a building-owned simulated bolt; VERA's
  simulated path has no owner at that point (airburst/split after the firer
  expired), so the clamp is not ported there. Retail leaves the key and
  `EBoltZAdjust` unset.
