# Warhead `Shield.*`: warheads that reconfigure and break shields

A Phobos warhead can attach, remove, replace, break, respawn-enable and
self-heal shields, tune how shield damage splits from health damage, and
penetrate shields (`Ext/WarheadType/Body.cpp:246-287` reads the keys;
`Ext/WarheadType/Detonate.cpp:368-465` `ApplyShieldModifiers` applies them per
target from `WarheadTypeExt::Detonate`).

```ini
[HIT]                            ; WarheadType
; Damage split (ShieldClass::ReceiveDamage, ShieldClass.cpp:225-245)
Shield.AbsorbPercent=            ; double, warhead override, default the type's
Shield.PassPercent=              ; double, warhead override, default the type's
Shield.ReceivedDamage.Minimum=   ; int, warhead override, default the type's
Shield.ReceivedDamage.Maximum=   ; int, warhead override, default the type's
Shield.ReceivedDamage.MinMultiplier=1.0   ; double, scales the minimum
Shield.ReceivedDamage.MaxMultiplier=1.0   ; double, scales the maximum
Shield.AffectTypes=              ; shield-type list; empty affects every type

; Penetration (ShieldClass::CanBePenetrated, ShieldClass.cpp:379-395)
Shield.Penetrate=false           ; bool, default false
Shield.Penetrate.Types=          ; nullable shield-type list, default AffectTypes

; Break (BreakShield, ShieldClass.cpp:836-860)
Shield.Break=false               ; bool, default false
Shield.Break.Types=              ; nullable shield-type list, default AffectTypes
Shield.BreakAnim=                ; anim list, default the type's BreakAnim
Shield.BreakWeapon=              ; weapon (Read<true>), default the type's BreakWeapon
Shield.HitAnim=                  ; anim list on an absorbed hit, default the type's HitAnim
Shield.SkipHitAnim=false         ; bool, default false
Shield.HitFlash=true             ; bool, default true

; Respawn (SetRespawn, ShieldClass.cpp:930-963)
Shield.Respawn.Duration=0        ; int frames the modifier stays installed
Shield.Respawn.Amount=           ; double, default the type's Respawn
Shield.Respawn.Rate=-1           ; double minutes, (int)(minutes * 900) frames
Shield.Respawn.RestartInCombat=  ; nullable bool, default the type's
Shield.Respawn.RestartInCombatDelay=-1    ; int frames
Shield.Respawn.RestartTimer=false  ; bool, restart the broken shield's timer
Shield.Respawn.Anim=             ; anim list on respawn, default the type's
Shield.Respawn.Weapon=           ; weapon, default the type's RespawnWeapon
Shield.Respawn.Types=            ; nullable shield-type list, default AffectTypes

; Self-healing (SetSelfHealing, ShieldClass.cpp:987-1017), no anim/weapon
Shield.SelfHealing.Duration=0
Shield.SelfHealing.Amount=
Shield.SelfHealing.Rate=-1
Shield.SelfHealing.RestartInCombat=
Shield.SelfHealing.RestartInCombatDelay=-1
Shield.SelfHealing.RestartTimer=false
Shield.SelfHealing.Types=

; Attach / remove / replace (ApplyShieldModifiers, Detonate.cpp:368-465)
Shield.AttachTypes=              ; shield-type list
Shield.RemoveTypes=              ; shield-type list
Shield.RemoveAll=false           ; bool, default false
Shield.ReplaceOnly=false         ; bool, default false
Shield.ReplaceNonRespawning=false  ; bool, default false
Shield.InheritStateOnReplace=false ; bool, default false
Shield.MinimumReplaceDelay=0     ; int frames
```

## Behavior

`ApplyShieldModifiers` runs once per target from the per-target detonation
pass, after the PenetratesTransport arm and before the attach-effect and crit
arms, so the receiver's damage hits the reconfigured shield. The remove arm
captures the dying shield's health ratio; the attach arm picks
`AttachTypes[0]`, or with `ReplaceOnly` the entry at the removed shield's
`RemoveTypes` index (clamped to the last entry) or, after a `RemoveAll`, the
first entry. An attach over a live shield needs `ReplaceNonRespawning` plus a
shield that is broken with no respawn pending and has been down at least
`MinimumReplaceDelay` frames. `InheritStateOnReplace` sets the new shield to
`(int)(strength * ratio)` and restarts both recovery timers.

The per-shield-type arms (Break, Respawn, SelfHealing) filter through their
`.Types` list, which falls back to `Shield.AffectTypes` when unset; a list
whose every name has no shield type reaches every type, because the native
reader drops unknown names. `Shield.Break` breaks the eligible active shield,
emitting the break animation and firing the break weapon on the owner
(`FireWeaponAtSelf`). The respawn/self-healing setters install a warhead
modifier for `Duration` frames: while it runs, the shield recovers with the
warhead's amount and rate (minutes * 900) and, on respawn, the warhead's
animation and weapon; `RestartTimer` restarts the broken shield's respawn
timer (or the active shield's healing timer) without changing values.

In `ShieldClass::ReceiveDamage` the warhead's absorb/pass and received-damage
values override the shield type's only when the shield type is in
`Shield.AffectTypes` (or that list is empty). `Shield.Penetrate.Types`
restricts which shield types a penetrating warhead bypasses; a Psychedelic
warhead penetrates unless the shield type is `ImmuneToBerserk`.

## Where VERA20K owns it

`rules::phobos_shields::ShieldWarheadRules` reads the warhead keys on the
merged warhead section (`WarheadType::shield`); `Shield.BreakWeapon.Read<true>`
pre-allocates the weapon it names in
`native_processing::allocate_shield_break_weapon_references`, the same pass
shape as the WarpInWeapon chain. `sim::shield` owns the effects:
`apply_shield_modifiers` is the `ApplyShieldModifiers` port, called per target
from `sim::combat::crit::detonate`; the receive-stage arms live in
`shield_stage` (the split, penetration, hit animation and the damage-side
`BreakShield`); `ShieldState` carries the warhead recovery modifiers and their
timers, consumed by the recovery AI (`respawn`/`self_healing`) and the
combat-restart policy (`restart_recovery_in_combat`). Break animations reuse
the shared `spawn_random_owned_anim` owner and break weapons the shared
`weapon_detonation_at` shot builder.

## Hit flash

`ShieldClass.cpp:275..295`: a non-breaking absorbed hit whose shield type sets
`HitFlash=yes` (`ShieldTypeClass.h:40`, default false `:98`) and whose warhead
leaves `Shield.HitFlash` on calls
`MapClass::FlashbangWarheadAt(size, pWH, pTechno->Location, true, flags)`
before `WeaponNullifyAnim`. `size` is `HitFlash.FixedSize` (default twice the
clamped shield damage); the flags start at `SpotlightFlags::NoColor` (1), are
replaced by `NoRed` (2) when `HitFlash.Red=no`, and OR in `NoGreen` (4) /
`NoBlue` (8). `0x48A620` is the function Phobos hooks in
`Misc/Hooks.LightEffects.cpp:69-74`, so the call first spends
`CombatLightChance`'s draw and then takes the detail/fps/`Bright` gates;
`Force=true` bypasses only `Bright`.

VERA's `sim::shield` queues the call in the absorbed arm and the committing
`pre_receive_damage` runs `combat_light_chance_admits` before emitting a
`CombatLightRequest::Impact { force_create: true, damage: size, flags, coord:
owner Location }`. The presentation owner materializes it (detail level, fps,
`CLIsBlack`, the channel blend). `NO_COLOR_SPOTLIGHT_FLAGS` is native
`NoColor` = 1; the light's fps admission still reads `flags & 0xE`.

Residual: `CombatLightChance` keeps the shared no-draw-at-1.0 deviation.
The coordinate test pins the value to `object_location`; on the vehicle
fixture `Location` and `GetCoords` coincide, so a building fixture would be
needed to tell the two sources apart.

## Residuals

- `ShieldClass::WeaponNullifyAnim`'s `AreAnimsHidden` gate has no VERA reader;
  VERA always draws the absorbed-hit animation.
- The break/respawn weapon detonates as a `DetonateAt` self shot. `DamageArea`'s
  self exclusion (`Explosion_Damage_DamageSelf @ 0x004896EC`) keeps the owner
  out of its own weapon's ground list, so an ordinary warhead's damage reaches
  only the neighbors the spread covers; whether native's
  `BulletClass::Explode` opens a direct-target arm for the self bullet is not
  confirmed. Weapons used for their side effects (attach effects and the
  like) behave as native's damage chain does here.
- `WarheadTypeExt::Detonate`'s shield members feed `PossibleCellSpreadDetonate`
  (`Body.cpp:495-516`), so a shield-only warhead reaches the per-target pass;
  as with the other members, the two per-detonation buffer draws VERA makes
  for it are the ones the existing pass documents.
