# Optional Phobos warhead self-damage

Build with `--features phobos` to let a warhead damage the object that fired
it even when that object's type keeps `DamageSelf=no`.

```ini
[General]                     ; rulesmd.ini
AllowDamageOnSelf=false       ; boolean, default no

[SOMEWARHEAD]                 ; a [Warheads] entry
AllowDamageOnSelf=            ; boolean, defaults to [General]
```

The warhead key is a Phobos `Nullable<bool>`: `yes` and `no` override the
global, while an empty, `none` or invalid value leaves it to `[General]`.

## Native sites

- `MapClass::DamageArea @ 0x00489280` leaves the firer out of its ground and
  bridge-deck list unless the firer's type has `DamageSelf=yes` or the warhead
  is `[CombatDamage] CrushWarhead=` (`0x004896DD..0x004896FC`). Phobos
  `Explosion_Damage_DamageSelf` (`0x004896EC`) jumps past that test to
  `0x00489702` when the warhead allows self-damage.
- `BuildingClass::ReceiveDamage` returns zero for a building's own shot at
  `0x00442262`. Phobos `BuildingClass_ReceiveDamage_DamageSelf` (`0x0044224F`)
  skips that return to `0x00442268` for an allowing warhead.
- Retail's airborne phase of `DamageArea` takes the firer unconditionally.
  Phobos `MapClass_DamageArea_AirDamageSelfFix` (`0x00489416`) is an
  unconditional bug fix: an aircraft is hit by its own blast only when its type
  has `DamageSelf=yes` or the warhead allows self-damage. `CrushWarhead=` plays
  no part there. Every `phobos` build applies it, so an aircraft with
  `DamageSelf=no` no longer takes its own splash damage.

Source: `src/Ext/WarheadType/Hooks.cpp`, `src/Ext/WarheadType/Body.cpp`,
`src/Ext/Rules/Body.cpp` (the toggle, by Starkku) and
`src/Misc/Hooks.BugFixes.cpp` (the airborne fix, by NetsuNegi).

## Where VERA20K owns it

- `rules/warhead_type.rs`: the per-warhead `AllowDamageOnSelf`.
- `rules/ruleset.rs`: `[General] AllowDamageOnSelf` and
  `RuleSet::allow_damage_on_self_for`, which falls back from the warhead to the
  global. A non-Phobos build answers no.
- `sim/combat/combat_aoe.rs`: the ground/deck and airborne source admissions.
- `sim/combat/receiver_call.rs`: the building receive prelude's self return.
