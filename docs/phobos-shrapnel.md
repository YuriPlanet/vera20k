# Phobos shrapnel admission

`BulletClass_Shrapnel_Forced @ 0x46A3D6` (`Ext/Bullet/Hooks.cpp:252-282`)
replaces the stock admission test that decides whether a detonating bullet
spawns its `ShrapnelWeapon=` children at all; the same hook's targeting pass
(`0x46A4FB`) then filters the ring's candidate heads. This page covers both;
terrain heads and Ares `SkipWeaponPicking` remain residual.

## Keys

| key | section | default | source |
|---|---|---|---|
| `Shrapnel.AffectsGround` | BulletType | `no` | `Ext/BulletType/Body.cpp:41` |
| `Shrapnel.AffectsBuildings` | BulletType | `no` | `Ext/BulletType/Body.cpp:42` |
| `Shrapnel.IgnoreHitBuildings` | BulletType | `no` | `Ext/BulletType/Body.cpp:44` |
| `Shrapnel.UseWeaponTargeting` | BulletType | `no` | `Ext/BulletType/Body.cpp:43` |

The four type-level keys are `Nullable<bool>` and fall back to the same-named
`[CombatDamage]` globals (`Ext/Rules/Body.cpp:554-558`). That global is a `Valueable<bool>` with constructor
default `false` (`Ext/Rules/Body.h:464-465`, `:968-969`), and Phobos passes it to
`Get(T)` through `Valueable<T>::operator const T&`, so an unset key and an unset
global both read `no`. `no` is also the stock behaviour, so a rules set that
never names these keys is unchanged. The global has no unset state and no
`none`/`<default>` clearing token — do not give these reads `Nullable`
semantics.

## Behavior

Stock refuses shrapnel when the impact cell holds no ground object and when its
first ground object is a building; it admits a terrain object or any other
entity. Phobos keeps that and adds:

- `Shrapnel.AffectsGround=yes` admits an empty impact cell.
- `Shrapnel.AffectsBuildings=yes` admits a building head. That building becomes
  the emission's `InitialTargetBuilding`, which the ring skips **by identity**,
  so it is never a child's target.
- `Shrapnel.IgnoreHitBuildings=yes` keeps every other building to one child per
  emission: a building that passed the ring's tests is recorded, and later cells
  headed by it are skipped. The record is per emission (`Hooks.cpp:260`).
- `Shrapnel.UseWeaponTargeting=yes` replaces the stock alliance test in the ring
  with the child weapon's own target eligibility (`Hooks.cpp:309-345`):
  `LegalTarget`, the warhead's Verses against the target's (or its shield's)
  armor, the weapon's `CanTargetHouses`, then `CanTarget`'s cell and techno
  bits, the health and veterancy thresholds and the AttachEffect gates. The
  warhead's `Affects` gate is skipped, so an allied unit the weapon can target
  becomes a legal child target. VERA shares this admission with
  `Splits.UseWeaponTargeting` (`sim::combat::phobos_airburst`).

The firing loop, the child construction and the RNG draws are unchanged; only
the early refusal differs.

## Residuals

- `Shrapnel.UseWeaponTargeting` applies to entity heads only. A terrain head
  still passes the ring unconditionally; native also tests its `LegalTarget`,
  the weapon's `CanTarget` cell bits and Verses (terrain armor inherits Wood,
  so most warheads pass). Trigger: the key is on and a tree/tiberium is the
  cell's ground head with a land-restricted `CanTarget` or a zero-versus-armor
  warhead; effect: VERA fires a child native would skip; frequency: rare;
  risk: cosmetic extra damage to terrain.
- `SkipWeaponPicking` (Ares) is not modelled; the shared admission evaluates the
  all-default case, matching `sim::combat::phobos_airburst`'s recorded choice.
- `Shrapnel.ObeyWarheadTriggerConditions`
  (`Ext/Bullet/Hooks.DetonateLogics.cpp:22`) is consumed at the detonation
  entry (`0x4690D4`); see the [warhead trigger conditions]
  (phobos-warhead-triggers.md#bullet-detonation-gate) page.
- The `[CombatDamage]` globals are read once from the merged layer state
  (`rules::phobos_globals`), which equals Phobos re-reading them on every
  `Process` for scalar keys. A later layer naming only one key keeps the other.
- Retention across scenarios in one process is unverified: Phobos reads these
  into the `RulesClass`-lifetime `RulesExt::Data` and never resets them, while
  VERA builds a fresh `PhobosGlobals` from the current layer stack. Within one
  scenario the fold is equivalent; a second scenario that does not author a key
  the first map authored would read `no` in VERA and keep the earlier `yes` in
  Phobos. Settle it with a two-map load.
