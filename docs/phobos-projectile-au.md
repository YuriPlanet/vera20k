# Projectile `AU`

`[Projectile] AU=` lets a weapon whose projectile carries the flag attack a
techno that is underground. It is a plain `Valueable<bool>` read by
`Ext/BulletType/Body.cpp:83` and defaults to `no`.

```ini
[PROJECTILE]
AU=yes
```

## Native evidence

| Address | Site | Behavior |
| --- | --- | --- |
| `0x772AB3` | `WeaponTypeClass_AllowedThreats_AU` (`Ext/Techno/Hooks.cpp:1537-1545`) | When the weapon's projectile has `AU`, ORs `0x20000` into the allowed-threat flags. |
| `0x6F8DF0` | `TechnoClass_SelectAutoTarget_Start_AU` (`:1553-1558`) | Keeps that flag in the auto-target scan context. |
| `0x6F8FA8` | `SelectAutoTarget_SetCanTargetWhatAmI_AU` (`:1560-1570`) | With the flag, adds Infantry/Unit/Aircraft to `CanTargetWhatAmI`. |
| `0x6F37EB` | `TechnoClass_WhatWeaponShouldIUse_AntiAir` (`Ext/Techno/Hooks.Firing.cpp:167-194`) | Secondary projectile `AU` with no `AU` on the primary, and the target in `Layer::Underground`, answers Secondary. |
| `0x6FC749` | `TechnoClass_CanFire_AntiUnderground` (`:535-558`) | `Layer::Underground` answers ILLEGAL unless the projectile has `AU`; Air/Top repeat the `AA` requirement. |
| `0x6F36DB` | `TechnoClass_WhatWeaponShouldIUse` shield arm (`:121-165`, `:151`) | Inside the active-shield branch, the secondary is kept when its projectile has `AU` and the target is under ground. |
| `0x6F3432` | `TechnoClass_WhatWeaponShouldIUse_Gattling` (`:196-272`, `:229`) | `isWeaponValid` rejects a weapon for a burrowed target without `AU`, before AA, the shield and the Verses-0 test. |
| `0x70023B` | `TechnoClass_MouseOverObject_AttackUnderGround` (`Ext/Techno/Hooks.cpp:1497-1511`) | The manual cursor admission accepts the object when it is surfaced or the selected weapon's projectile has `AU`. |

The auto-target side reads the object's `GetHeight` (vt+0x1C8,
`0x005F5F40`): a candidate below -20 leptons is refused at
`0x006F7E11..0x006F7E1E` unless `AU` admits underground targets.

## Where VERA20K owns it

- `rules::projectile_type::ProjectileType::au` reads the key
  (`rules/projectile_type.rs:582`, `#[cfg(feature = "phobos")]`).
- Auto-target: `sim::combat::greatest_threat::evaluate_candidate` resolves the
  selected weapon's projectile and skips the `< -20` height rejection when `au`
  is set (`sim/combat/greatest_threat.rs:2506-2533`).
- The layer answer: `sim::world::entity_is_underground` reads the cached
  `MovementLayer` on the active locomotor; `entity_layer` answers
  `DisplayLayer::UNDERGROUND` (0) from it before any height query. This is the
  one owner both fire-time consumers read.
- `CanFire`: `sim/combat/fire_error.rs` carries the `0x6FC749` arm in its native
  slot between T39 (`0x006FC73C`) and T40 (`0x006FC76A`), over the projectile
  facts `sim/combat/fire_error_world.rs` (`ProjectileFacts::au`).
- Selection: `sim/combat/combat_weapon.rs` carries the `0x6F37EB` AU arm after
  the stock `0x6F37E7` AA answer, via `projectile_au`.
- Both consumer families are `#[cfg(feature = "phobos")]`, so a build without
  the feature keeps the vanilla paths.
- The cursor half is the app crate's `object_attack_is_admitted`
  (`src/app/input/context_order.rs`), which reads the same
  `sim::world::entity_is_underground` answer and the same
  `ProjectileType::au` through `RuleSet::weapon_projectile`.

## Tests

- `sim::combat::phobos_au_underground_tests::a_weapon_without_au_refuses_an_underground_target`
- `sim::combat::phobos_au_underground_tests::au_yes_makes_the_selection_prefer_the_underground_weapon`
- `sim::combat::greatest_threat_scenario_gate_tests::guard_scan_an_au_projectile_admits_an_underground_candidate`
  (the auto-target half, already in the base)

## Residuals

- The `What_Action` cursor admission (`0x70023B`) is now ported in
  `src/app/input/context_order.rs`: the unforced object rung
  (`object_attack_is_admitted`, reached by [`attack_action`] and the hover
  cursor) refuses a target that is not surfaced when the selected weapon's
  projectile lacks `AU`, and falls to the native tail. It reads
  `sim::world::entity_is_underground` (the one-owner layer answer) and
  `RuleSet::weapon_projectile(...).au`. Tests:
  `app::input::context_order::phobos_attack_underground_tests` (three cases:
  refused burrowed cursor plus its surfaced twin, `AU=yes` admitted, and the
  selected secondary's `AU` admitted over a `AU=no` primary). Dormant in retail
  (no Tunnel locomotor, and no retail projectile sets `AU`).

[`attack_action`]: ../src/app/input/context_order.rs
- The gattling selection hook `0x6F3432` is now ported
  (`phobos_gattling_weapon_index` + `phobos_weapon_valid_against`): its
  `isWeaponValid` walks AA, underground `AU`, the target shield's
  `CanBeTargeted` and the raw-armor Verses-0 row in native order over the
  `2 * CurrentGattlingStage` pair, after a `PickWeaponIndex` with both fallback
  gates on. Tests:
  `sim::combat::phobos_gattling_select_tests`. The stock golden
  (`original_weapon_selection_rows`) still drives the closure-less entry, so it
  keeps describing the stock pair.
- The shield arm at `0x6F36DB:151` is now ported
  (`phobos_shield_weapon_index` + `sim::shield::can_be_targeted`): its
  underground-secondary row is one of the four gates that admit the
  secondary.
- The Tunnel locomotor is unported: `rules/locomotor_type.rs` keeps the
  Tiberian Sun Tunnel/Mech/DropPod classes out of the installed table, so a
  unit whose `Locomotor=` names that CLSID never caches
  `MovementLayer::Underground` in VERA and the gate and arm are dormant there.
  Stock `ini/rulesmd.ini` never names the CLSID (the dormancy golden at
  `rules/locomotor_type.rs:471`), but a mod's rules may — the installed mod's
  `rulesmd.ini` does, on units such as `ZDWZ`, `SAPC2`, `ZDDTRUCK` and
  `TANKCHONGT`. Trigger in that ruleset: those units should burrow and only
  `AU=yes` weapons should shoot them. Frequency: only where a type names the
  dormant CLSID. Risk: without the locomotor the engine falls back to the
  type's INI seed and those units never go underground, so `AU` has nothing to
  admit — a locomotion-subsystem gap, not this key's.
- The fixture path: the tests set the cached layer by hand
  (`LocomotorState::set_layer_for_test`), which is the only way to reach the
  arm until that locomotor lands.
- Retail/visual parity is untested here (no retail `ini/` in this checkout).
