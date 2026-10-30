# Optional Phobos AttachEffect

Build with `--features phobos` to register `[AttachEffectTypes]`, attach those
effects from warheads and TechnoTypes, and read the stat, weapon, tint,
discard and critical-hit blocks each effect carries. Default builds have no
attach vector, instance lifecycle or weapon trigger anywhere in the game.

```ini
[AttachEffectTypes]
1=EXAMPLEBUFF

[EXAMPLEBUFF]                       ; AttachEffectType
Duration=0                          ; game frames; negative = indefinite
Duration.ApplyFirepowerMult=no      ; fold FirepowerMultiplier into Duration
Cumulative=no                       ; same-type instances stack
Cumulative.MaxCount=0               ; 0 = uncapped
Powered=no                          ; an unpowered carrier pauses the effect
Cloakable=no                        ; the effect hides while the carrier is cloaked
ForceDecloak=no                     ; attach decloaks the carrier
Unkillable=no                       ; clamps lethal damage; the carrier survives
PenetratesIronCurtain=no
AllowTransfer=                      ; move to a passenger/convert recipient
AllowTransfer.Convert=              ; move across a type conversion
Animation=                          ; AnimType shown on the carrier
Animation.ResetOnReapply=no
Animation.UseInvokerAsOwner=no
Animation.HideIfAttachedWith=       ; hide while these AttachEffectTypes are held
Tint.Color=255,0,0                  ; 0..255 per channel
Tint.Intensity=0.0
Tint.Cumulative=no
FirepowerMultiplier=1.0
ArmorMultiplier=1.0                 ; conditional block below
ArmorMultiplier.Chance=0.0
ArmorMultiplier.Delay=0
ArmorMultiplier.AffectsHouse=       ; owner/allies/enemies/neutral flags
ArmorMultiplier.HitAnim=            ; one plays when the multiplier applies
SpeedMultiplier=1.0
ROFMultiplier=1.0
WeaponRange.Multiplier=1.0
WeaponRange.ExtraRange=0.0          ; cells
WeaponRange.AllowWeapons=           ; empty list = every weapon
WeaponRange.DisallowWeapons=
Crit.Multiplier=1.0                 ; critical-hit block, see the crit page
Crit.ExtraChance=0.0
Crit.AllowWarheads=
Crit.DisallowWarheads=
DisableWeapons=no
AffectsTarget=All                   ; None/Land/Water/NoContent/Infantry/Unit/
                                    ; Building/Aircraft/All/AllCells/AllTechnos
AffectTypes=                        ; only these TechnoTypes carry the effect
IgnoreTypes=                        ; never these TechnoTypes
Groups=                             ; ownership groups for Count gates
DiscardOn=                          ; comma-separated discard conditions
DiscardOn.Health.BelowPercent=0.0
DiscardOn.Health.AbovePercent=0.0
DiscardOn.Ammo.MinimumAmount=0
DiscardOn.Ammo.MaximumAmount=0
DiscardOn.Firing.Count=0
DiscardOn.Missions=                 ; carrier enters one of these Missions
DiscardOn.AIMissions=
DiscardOn.LandTypes=
DiscardOn.Sequences=
LaserTrail.Type=                    ; reserved; no LaserTrail owner yet
ExpireWeapon=                       ; WeaponType fired when the effect ends;
                                    ; ExpireWeapon.TriggerOn=expire/remove/
                                    ; death/discard (comma-separated flags)
RevengeWeapon=                      ; WeaponType fired back at a damaging firer
ReflectDamage=yes/no                ; reflect a hit back at its firer
ReflectDamage.Warhead=              ; the reflected warhead
ReflectDamage.Override=             ; fixed damage instead of the mirror
ReflectDamage.Multiplier=1.0
ReflectDamage.Chance=1.0            ; 0..1 probability
ReflectDamage.Delay=0
ReflectDamage.AffectsHouse=         ; owner/allies/enemies/neutral flags
                                    ; (the .AffectsHouses spelling is honoured)
```

TechnoTypes attach effects to themselves when created:

```ini
[TANK]
AttachEffect.AttachTypes=EXAMPLEBUFF
AttachEffect.Delays=                ; per-list-position attach delay, frames
AttachEffect.InitialDelays=         ; per-list-position initial delay, frames
AttachEffect.DurationOverrides=     ; per-list-position Duration override
AttachEffect.RecreationDelays=      ; per-list-position re-create delay, frames
```

Warheads attach effects on detonation:

```ini
EXAMPLEWH
AttachEffect.AttachTypes=EXAMPLEBUFF
AttachEffect.DurationOverrides=
AttachEffect.CumulativeSourceMaxCount=-1   ; negative = no cap
AttachEffect.CumulativeRefreshAll=no
AttachEffect.CumulativeRefreshAll.OnAttach=no
AttachEffect.CumulativeRefreshSameSourceOnly=no
AttachEffect.ReplaceLongerDuration=
AttachEffect.RemoveTypes=
AttachEffect.RemoveGroups=
AttachEffect.CumulativeRemoveMinCounts=
AttachEffect.CumulativeRemoveMaxCounts=
```

Weapons gate their shots on the carrier's held effects
(`TechnoClass_CanFire 0x6FC339`):

```ini
[SOMEWEAPON]
AttachEffect.RequiredTypes=               ; carrier must hold these
AttachEffect.RequiredGroups=
AttachEffect.RequiredMinCounts=           ; per list position
AttachEffect.RequiredMaxCounts=
AttachEffect.Required.Any=no              ; any one suffices instead of all
AttachEffect.Required.Houses=             ; carrier's owner among these
AttachEffect.DisallowedTypes=
AttachEffect.DisallowedGroups=
AttachEffect.DisallowedMinCounts=
AttachEffect.DisallowedMaxCounts=
AttachEffect.Disallowed.Any=no
AttachEffect.Disallowed.Houses=
AttachEffect.CheckOnFirer=no              ; test the firer instead of the target
AttachEffect.IgnoreFromSameSource=no
AttachEffect.SameSourceOnly=no            ; match only same-source instances
```

## Where VERA20K owns it

- `rules/attach_effect.rs`: the `[AttachEffectTypes]` catalog
  (`AttachEffectTypeClass`, an `Enumerable` whose entries re-read their own
  section on every reached rules pass, keeping the previous value for an
  absent key).
- `rules/phobos_crit.rs` and `sim/combat/crit.rs`: the `Crit.*` block and the
  per-target warhead pass; see [critical hits](phobos-critical-hit.md).
- `rules/warhead_type.rs` and `rules/object_type.rs`: the warhead-side and
  TechnoType-side `AttachEffect.*` lists above.
- `sim/attach_effect.rs`: one instance's timing state and the per-frame AI
  walk — initial delay, re-creation delay, duration refresh, duration tick,
  and the animation each effect shows (`AttachEffectClass::AI`).
- `sim/game_entity/attach_effect.rs`: the carrier-side attach sites
  (`CreateAndAttach` gate, stacking counts, source House).
- `sim/combat/world_receiver.rs`: damage-time arms — expire/revenge weapons
  on death (`0x702050`), reflect damage, and the per-target pass that runs
  AttachEffect lists and crit before the native area damage
  (`BulletClass_Detonate 0x46920B`, `MapClass_DamageArea 0x489286`).
- `sim/combat/fire_error.rs` and the weapon-side gate: `Required`/
  `Disallowed` types and groups, counts, `CheckOnFirer`,
  `IgnoreFromSameSource`, `SameSourceOnly`, `Required.Any`/
  `Disallowed.Any`, `Required.Houses`/`Disallowed.Houses`
  (`TechnoClass_CanFire 0x6FC339`).
- Presentation: the tint block feeds the existing damage-tint lane; a
  carrier's `ForceDecloak` runs through the stock cloak transitions.

## Residuals

- `LaserTrail.Type` is retained as the raw key text: VERA20K has no
  LaserTrail catalog or trail owner yet, so no trail is drawn.
- `AllowTransfer` is retained (`Nullable<bool>`, `AllowTransfer.Convert`
  alongside) but no transfer site consumes it yet.
- `Duration.ApplyFirepowerMult` folds a firepower multiplier into the
  duration the way the reader stores it; `ArmorMultiplier` folds onto the
  target's armor the shared combat-stats owner already folds Ares
  attachments through.
- The per-target pass runs only in the ordinary-damage arm of bullet
  detonation and `DamageArea`; a warhead on a special arm (Ivan bomb, mind
  control, temporal, locomotor, ...) skips its per-target effects and
  prefix draws.
