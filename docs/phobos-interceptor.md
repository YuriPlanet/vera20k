# Projectile interception (`Interceptor.*`)

Phobos lets an armed TechnoType intercept enemy projectiles: it scans the
bullet store, targets a bullet with a dedicated weapon slot, fires through the
ordinary `FireAt` path, and the interceptor bullet's warhead entry damages or
destroys the intercepted bullet before its own area damage. Twelve TechnoType
keys, four BulletType keys and three `[CombatDamage]` globals.

```ini
[CombatDamage]
ProjectileInterceptable=false               ; Valueable<bool>
Interceptor.GuardRange.IsCylindrical=false  ; Valueable<bool>
Interceptor.ApplyFirepowerMult=true         ; Valueable<bool>

[SOMETECHNO]                                ; TechnoType
Interceptor=no                              ; Nullable<bool>, only yes creates the type
Interceptor.TargetingDelay=1                ; Valueable<int>, 0 clamps to 1
Interceptor.CanTargetHouses=enemies         ; Valueable<AffectedHouse>
Interceptor.GuardRange=0.0                  ; Promotable<Leptons>, cells
Interceptor.RookieGuardRange=
Interceptor.VeteranGuardRange=
Interceptor.EliteGuardRange=
Interceptor.MinimumGuardRange=0.0           ; Promotable<Leptons>, cells
Interceptor.RookieMinimumGuardRange=
Interceptor.VeteranMinimumGuardRange=
Interceptor.EliteMinimumGuardRange=
Interceptor.GuardRange.IsCylindrical=       ; Nullable<bool>, [CombatDamage] fallback
Interceptor.Weapon=0                        ; Valueable<int>, GetWeapon slot index
Interceptor.WeaponOverride=                 ; Nullable<WeaponTypeClass*>
Interceptor.WeaponReplaceProjectile=no
Interceptor.WeaponCumulativeDamage=no
Interceptor.KeepIntact=no
Interceptor.ApplyFirepowerMult=             ; Nullable<bool>, [CombatDamage] fallback
Interceptor.DeleteOnIntercept=              ; Nullable<bool>, bullet-type fallback

[SOMEPROJECTILE]                            ; BulletType
Interceptable=                              ; Nullable<bool>, [CombatDamage] fallback
Interceptable.DeleteOnIntercept=no
Interceptable.WeaponOverride=               ; Valueable<WeaponTypeClass*>
Strength=0                                  ; int, interception hit points
Armor=                                      ; Nullable<ArmorType>, unknown retains
```

## Native evidence and hooks

| Address | Hook | Behavior |
| --- | --- | --- |
| `0x6F9E50` | `TechnoClass_AI` → `TechnoExt::OnEarlyUpdate` → `ApplyInterceptor` | Gate `(CurrentFrame + RandomFactor) % TargetingDelay == 0`; skip without bullets, burrowed or unarmed; an existing target must be a Locked Bullet; a building in `Selling`/`Construction` returns. Scans `BulletClass::Array` for interceptable, non-`SpawnNextAnim` bullets in `[minRange, GuardRange]` (Z overwritten when cylindrical), with a nonzero versus against the interceptor warhead and a `CanTargetHouses` match; keeps the first Targeted/Locked bullet as an optional target and prefers the first clean one. `SetTarget`; a building then queues and commences Attack. `Ext/Techno/Body.Update.cpp:33-137`. |
| `0x6F3339` | `TechnoClass_WhatWeaponShouldIUse_Interceptor` | A Bullet target answers the interceptor type's `Weapon` slot before the stock ladder (`Ext/Techno/Hooks.Firing.cpp:19-40`). |
| `0x6FF660` | `TechnoClass_FireAt_LateLogic` | An armorless target becomes Locked; the fired bullet records `InterceptorTechnoType` and Targeted; `ApplyFirepowerMult=no` stores the weapon's raw Damage as the bullet Health (`Hooks.Firing.cpp:868-900`). |
| `0x466556` | `BulletClass_Init` → `BulletExt::InitializeState` | Stores `FirerHouse`, the firepower multiplier and `CurrentStrength = BulletType::Strength` (`Ext/Bullet/Hooks.cpp:8-30`). |
| `0x4666F7` | `BulletClass_AI` interceptor block | Targeted locks an armorless target; Intercepted clears the target's Locked, detonates at its own coordinates when `DetonateOnInterception`, limbos/uninits, then runs the `LimboLaunch` self-damage tail (`:53-100`). |
| `0x4668BD` | `BulletClass_AI_Interceptor_InvisoSkip` | An `Inviso` bullet carrying `InterceptorTechnoType` jumps to the detonate path (`:163-178`). |
| `Ext/WarheadType/Detonate.cpp:38-48` | `WarheadTypeExt::Detonate` | With an interceptor bullet, `InterceptBullets` runs before create-gap and all per-target effects. |
| `Detonate.cpp:633-672` | `WarheadTypeExt::InterceptBullets` | `CellSpread == 0`: the direct target only, within 32 leptons unless the interceptor type is Inviso. Otherwise every interceptable bullet within `(CellSpread * 256)²`. |
| `Ext/Bullet/Body.cpp:17-90` | `BulletExt::InterceptBullet` | Armorless: Intercepted unless `KeepIntact`. Armored: `CurrentStrength -= int(interceptor bullet Health * versus)`, at zero Intercepted unless `KeepIntact`. Sets `DetonateOnInterception = !DeleteOnIntercept.Get(bullet-type value)`. `WeaponOverride` re-arms damage/warhead/brightness; `WeaponReplaceProjectile` also swaps the projectile type and Speed and drops the source's target when the new type no longer qualifies. |
| `Detonate.cpp:171,247` | crit pass | `bulletWasIntercepted` suppresses a crit with `Crit.SuppressWhenIntercepted`. |

## Where VERA20K owns it

- `rules::phobos_interceptor::InterceptorSettings` (all twelve keys; the Ares
  `Promotable` reader moved to `rules::promotable`, `read_affected_house` and
  the Phobos Leptons read reused). `rules::art_data`/`ObjectType` carry it as
  `Option` only for `Interceptor=yes`.
- `rules::projectile_type::ProjectileType` gains `Strength`, `Armor`,
  `Interceptable`, `Interceptable.DeleteOnIntercept` and
  `Interceptable.WeaponOverride`; `rules::armor::try_armor_index` is the
  unknown-retaining lookup.
- `rules::phobos_globals` holds the three `[CombatDamage]` fallbacks.
- `sim::projectile::Projectile` gains `current_strength`,
  `intercepted_status` (`Targeted 0x1`, `Intercepted 0x2`, `Locked 0x4`),
  `detonate_on_interception`, `interceptor_type` and `interceptor_target`;
  `construct_bullet_scheme` initializes `CurrentStrength` from the bullet
  type; the hash, save validation and `pointer_expired` cover the new ids.
- `TargetKind::Bullet(id)` enters the shared target model: coordinate/range
  resolution, presence and the ground-fire error path read the projectile
  store; the expiry broadcast clears a techno's bullet target. Wire commands
  and MEGAMISSION never encode it.
- `sim::world::techno_ai::interceptor` is `ApplyInterceptor` (scan) and the
  bullet-AI pre-tick (`0x4666F7`/`0x4668BD`).
- `sim::combat::combat_weapon` answers the interceptor slot for a Bullet
  target; `sim::combat::world_fire::emit_admitted_fire` runs the FireAt late
  logic.
- `sim::combat::interceptor` is `InterceptBullets` + `InterceptBullet`, called
  at the head of `emit_detonation_receivers`; `sim::combat::crit` takes the
  intercepted flag for `Crit.SuppressWhenIntercepted`.

## Phobos quirks kept

- `Interceptor=` is a `Nullable<bool>`: only `yes` creates the type; `no`
  removes it. In VERA's single merged read an absent/blank/`none` key also
  leaves no type (Phobos keeps a blank key's previous pass).
- `TargetingDelay=0` becomes 1; other values (including negatives) are kept.
- Guard ranges are cells read with the native `F2I(cells * 256)` conversion;
  the three ranks fall back to the common key.
- The scan's distance is 3D and squared; `GuardRange.IsCylindrical` replaces
  the bullet's Z with the techno's.
- An armorless target is Locked by FireAt and by the interceptor bullet's AI;
  an armored one is never Locked.
- `DeleteOnIntercept` is the interceptor's nullable, else the intercepted
  bullet type's value; the detonation flag is its negation.
- `WeaponCumulativeDamage` adds the override Damage to the bullet's carried
  Health (not to `CurrentStrength`).
- `Strength` defaults to 0, so an Armor-bearing bullet without an authored
  `Strength` dies to the first nonzero damage.
- Interception runs before the interceptor warhead's own area damage.
- No step draws RNG; a regression test asserts the Scenario stream and
  unrelated bullets are unchanged.

## RNG draws

None in the scan. TargetingDelay's phase is the frame counter alone (see
residuals); range, armor, house and crit checks read state only.

## Residuals (trigger · impact · frequency · risk)

- **RandomFactor phase and draw.** Phobos `TechnoClass_Init` (`0x6F42F7` →
  `TechnoExt::InitializeState`, `Ext/Techno/Hooks.cpp:292`) draws
  `RandomRanged(0, 15)` for every techno and adds it to the TargetingDelay
  phase. VERA stores neither the byte nor the draw, so (a) every interceptor
  with delay > 1 scans on the same frame phase, and (b) with Phobos gameplay
  VERA's Scenario stream is one draw per techno ahead of a Phobos game, and
  every later Phobos RNG consumer diverges. Trigger: any techno construction
  under Phobos. Frequency: always. Risk: RNG-stream divergence; the scan
  itself stays deterministic. A complete fix needs a per-techno field and the
  init draw, which re-bases every native-golden Scenario assertion; tracked as
  its own chain.
- **Bullet target pointer.** Native keeps the prey in `BulletClass::Target`
  (a real Object pointer) and the bullet flies/homes at its live coordinate.
  VERA keeps the prey id in `Projectile::interceptor_target` and leaves
  `ProjectileTarget` entity/cell-only: a non-Inviso interceptor bullet only
  detonates once the prey is within the 32-lepton direct gate (otherwise it
  flies to its fire-time aim point), and an Inviso interceptor detonates at
  its launch placement on its next AI. Trigger: non-Inviso interceptor
  projectiles, or a prey that moves more than 1/8 cell after the shot.
  Frequency: authored mods with non-Inviso interceptors. Risk: missed
  interceptions; retail interceptors use `Inviso=yes`.
- **`SpawnNextAnim`.** VERA has no per-bullet spawn-animation wait, so a
  bullet in that native state is not skipped by the scan or the CellSpread
  arm. Trigger: a bullet type with a spawn-next animation. Frequency: none in
  stock data. Risk: it could be intercepted while native skips it.
- **WeaponOverride closure.** VERA's RuleSet builds only weapons referenced
  by a TechnoType; native's `FindOrAllocate` also creates a weapon named only
  by `WeaponOverride`. Trigger: an override weapon no type references.
  Frequency: authored mods. Risk: the override is ignored. Workaround: list
  the weapon on any type.
- **Laser trails on projectile replacement.** `WeaponReplaceProjectile`
  clears and re-initializes the bullet's laser trails natively; VERA has no
  simulated trail state, so only type and speed change. Trigger: a replaced
  projectile drawing trails. Frequency: visual mods. Risk: presentation only.
- **TargetingDelay negative.** Native only clamps 0; a negative delay keeps
  its C++ remainder semantics. VERA uses an `i64` remainder with the same
  sign rule, but the RandomFactor term is missing as above.
- **Wire and snapshots.** `TargetKind::Bullet` is engine-internal: commands
  and MEGAMISSION refuse to encode it, and it is never a nav destination. A
  Foot approach converts it to the projectile's current cell each visit
  (`FootClass::Approach_Target` reads the target's GetCoords); a non-Foot
  destination built from a bullet id is not reachable, and the target model
  keeps the conversion only as an object-comparison key. A saved projectile's
  `interceptor_target` must resolve in the projectile store; the
  pointer-expiry broadcast clears it on removal.
- **Bullet Health in GetFireError.** Native reads a bullet target's real
  Health (the carried damage); VERA's projectile has no Health field yet, so
  the Bullet `TargetFacts` use 1. Trigger: a health-gated fire-error arm
  against a bullet target (healing, damaged-target gates). Frequency: authored
  mods. Risk: a refused/admitted shot differs only there.
- **Retail/visual parity untested** (no retail `ini/` in this checkout).
