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

## Tests

- `sim::combat::phobos_au_underground_tests::a_weapon_without_au_refuses_an_underground_target`
- `sim::combat::phobos_au_underground_tests::au_yes_makes_the_selection_prefer_the_underground_weapon`
- `sim::combat::greatest_threat_scenario_gate_tests::guard_scan_an_au_projectile_admits_an_underground_candidate`
  (the auto-target half, already in the base)

## Residuals

- The `What_Action` cursor admission (`0x70023B`) has no VERA20K owner in this
  package: the port lives in `src/app/input` (`What_Action`/`what_action_*`),
  which this package's boundaries exclude. Trigger: a player moving the cursor
  over a burrowed techno with a weapon whose projectile lacks `AU`. Frequency:
  none in retail (no Tunnel locomotor). Risk: the vanilla cursor answer stands.
- The gattling selection hook `0x6F3432` is unported as a whole: VERA's
  gattling arm returns the stage pair without asking `isWeaponValid`, so its
  AA, `CanBeTargeted`, Verses-0 and AU rows are all missing. Porting only the
  AU row would answer the stage index while skipping the other three gates, so
  that belongs to a gattling-eligibility chain.
- The shield arm at `0x6F36DB:151` is unreachable here: VERA has no
  `Shield::CanBeTargeted` owner at all, so the enclosing `if (pShield)` block
  has no equivalent state.
- No Tunnel locomotor is ported and no retail type uses one, so the
  Underground layer is dormant in play: the gate and the arm are exercised by
  fixtures that set the cached layer, not by retail data.
- Retail/visual parity is untested here (no retail `ini/` in this checkout).
