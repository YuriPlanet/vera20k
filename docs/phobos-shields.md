# Optional Phobos Shields

Build with `--features phobos` to register `[ShieldTypes]`, bind a techno with
`ShieldType=`, and read the warhead-side `Shield.*` overrides. Default builds
have no shield state and keep gamemd's receiver untouched.

```ini
[ShieldTypes]
0=SOMESHIELDTYPE

[SOMESHIELDTYPE]                    ; ShieldType
Strength=0                          ; integer
InitialStrength=0                   ; integer, defaults to Strength
Armor=none                          ; ArmorType
InheritArmorFromTechno=false        ; boolean
InheritArmor.Allowed=               ; List of TechnoTypes
InheritArmor.Disallowed=            ; List of TechnoTypes
ApplyArmorMult=                     ; boolean, defaults to [CombatDamage] ShieldApplyArmorMult
Powered=false                       ; boolean
AbsorbPercent=1.0                   ; floating point value
PassPercent=0.0                     ; floating point value
AbsorbOverDamage=false              ; boolean
Respawn=0.0                         ; floating point value, percents or absolute
Respawn.Rate=0.0                    ; floating point value, ingame minutes
Respawn.RestartInCombat=true        ; boolean
Respawn.RestartInCombatDelay=0      ; integer, game frames
SelfHealing=0.0                     ; floating point value, percents or absolute
SelfHealing.Rate=0.0                ; floating point value, ingame minutes
SelfHealing.RestartInCombat=true    ; boolean
SelfHealing.RestartInCombatDelay=0  ; integer, game frames
ReceivedDamage.Minimum=-2147483648  ; integer
ReceivedDamage.Maximum=2147483647   ; integer
ImmuneToBerserk=false               ; boolean
Pips=-1,-1,-1                       ; integer, PIPS.SHP frames Green,Yellow,Red
Pips.Building=-1,-1,-1              ; integer, PIPS.SHP frames Green,Yellow,Red
Pips.Background=                    ; filename
Pips.Building.Empty=0               ; integer, PIPS.SHP frame
Pips.HideIfNoStrength=false         ; boolean

[SOMETECHNO]                        ; TechnoType
ShieldType=SOMESHIELDTYPE           ; ShieldType; none by default

[CombatDamage]
ShieldApplyArmorMult=false          ; boolean

[AudioVisual]
Shield.ConditionYellow=             ; floating point value, percents or absolute
Shield.ConditionRed=                ; floating point value, percents or absolute
Pips.Shield=-1,-1,-1                ; integer, PIPS.SHP frames Green,Yellow,Red
Pips.Shield.Building=-1,-1,-1       ; integer, PIPS.SHP frames Green,Yellow,Red
Pips.Shield.Background=PIPBRD.SHP   ; filename
Pips.Shield.Building.Empty=0        ; integer, PIPS.SHP frame

[SOMEWARHEAD]                       ; WarheadType
Shield.Penetrate=false              ; boolean
Shield.AffectTypes=                 ; List of ShieldTypes
Shield.Penetrate.Types=             ; List of ShieldTypes
Shield.AbsorbPercent=               ; floating point value
Shield.PassPercent=                 ; floating point value
Shield.ReceivedDamage.Minimum=      ; integer
Shield.ReceivedDamage.Maximum=      ; integer
Shield.ReceivedDamage.MinMultiplier=1.0 ; floating point value
Shield.ReceivedDamage.MaxMultiplier=1.0 ; floating point value
```

A techno creates a shield only when the bound entry resolves and `Strength` is
above zero; the points start at `min(InitialStrength, Strength)`.

## Damage

Phobos's installed hook `0x701900+0x6` runs at the entry of
`TechnoClass::ReceiveDamage` and runs before gamemd's armour divides
(`src/Ext/Techno/Hooks.ReceiveDamage.cpp`). VERA's stage sits at the same
place: `sim::shield::pre_receive_damage` runs before
`combat::damage::receive_damage` and substitutes its surviving damage for the
receiver input.

- A shield only absorbs while it has points and is online; a warped-out
  object, a zero packet, an `Immune`/`TypeImmune` target and an active
  Iron Curtain/Force Shield keep the hit unchanged. VERA has no Ares
  `PenetratesIronCurtain` reader, so any active curtain blocks absorption.
- `Shield.Penetrate=yes` (subject to `Shield.Penetrate.Types`) passes the
  whole hit through, as does a Psychedelic warhead unless the shield type sets
  `ImmuneToBerserk=yes`.
- The shield's damage uses its own armour: `InheritArmorFromTechno` with the
  allowed/disallowed name gates, otherwise the type's `Armor=`.
  `ApplyArmorMult` divides by the techno's armour multiplier first, as the
  native `GetCurrentArmorMultiplier` path does, then the shared warhead kernel
  applies falloff and the shield armour's Verses.
- `AbsorbPercent` takes damage from the shield; `PassPercent` takes it from
  health, measured on the raw receiver value (not the shield-armour kernel).
  Both signs use these coefficients and matching warhead overrides.
- `ReceivedDamage.Minimum/Maximum` and the warhead multipliers clamp the
  shield packet.
- A packet that exceeds the remaining points breaks the shield. With
  `AbsorbOverDamage` the surplus is dropped; otherwise the share of the
  original packet beyond the points is converted back to techno damage.
- A negative packet heals the shield, and stops at full points without leaking
  into health.
- Real damage restarts the recovery timers, whether or not the shield absorbed
  it, through `Respawn.RestartInCombat*` and `SelfHealing.RestartInCombat*`.

## Shield bar

`ShieldClass::DrawShieldBar_Building`/`DrawShieldBar_Other` are installed at
`TechnoClass::DrawHealthBar` (`0x6F65D1` buildings, `0x6F683C` mobiles) and run
before the stock health fill. VERA's `ui_overlays` emits the same commands:

- A building draws `num_pips` shield pips at the Top bracket anchor minus
  `(6,3)`, stepping `(-4,+2)`; filled pips use the shield frame, the rest the
  empty frame (`Pips.Building.Empty`, default `0`).
- A mobile draws up to its 17 (8 for Infantry) pips at the foot Left/Top
  bracket, stepping `+2` X. A selected object also draws the shield
  `PIPBRD.SHP` background at `length+1` (`+length+1` more for Infantry).
- `Pips`/`Pips.Building` select the green/yellow/red PIPS.SHP frame through the
  shield condition thresholds; an entry whose X is `-1` falls back to
  `[AudioVisual] Pips.Shield`/`Pips.Shield.Building`, then the native 16/5.
- `Pips.HideIfNoStrength` suppresses a broken shield's bar, and the filled
  count is `round(hp / Strength * length)` clamped to `[1,length]` while active
  and `0` while offline.

Custom `Pips.Background` names and the per-type background are parsed but the
draw still uses the stock PIPBRD strip; custom background loading remains
open.

## Recovery

`TechnoExt::OnEarlyUpdate -> ShieldClass::AI` runs from
`TechnoClass_AI @ 0x6F9E50+0x5`. VERA's `sim::shield::tick` runs at the same
boundary after the category's Temporal prologue in `sim::world::techno_ai`.
Temporal-beam visits pause recovery through `ShieldClass::AI_Temporal` semantics;
normal recovery runs only after the object can resume its Techno AI.

For a broken shield the respawn timer counts down and restores
`Respawn` (percent of `Strength` when in -1..=1, otherwise the literal amount);
for a damaged shield the self-healing timer does the same with `SelfHealing`.
Rates are `* 900` frames. `Powered` shields go offline while their building is
without power. Offlining or a temporal beam pauses the active recovery timer,
resuming where it stopped. A Chronosphere warp suppresses recovery while the
object is immobilized, but its countdown continues until warp-in resumes AI.

## Where VERA20K owns it

- `rules/phobos_shields.rs` — `[ShieldTypes]` catalog, globals and warhead
  override reads.
- `rules/native_processing.rs` — the ordered per-pass read, retained with the
  other extension catalogs.
- `rules/ruleset.rs` — immutable projection, config hash and the `ShieldType=`
  bindings.
- `sim/shield.rs` — the single owner of shield points and recovery timers.
- `sim/combat/world_receiver.rs` / `receiver_call.rs` — the receiver pre-stage.
- `sim/world/world_spawn/construction.rs` — creation at the techno constructor.
- `app/presentation/ui_overlays.rs` — the shield bar commands; `render/selection_overlay.rs`
  supplies the PIPS/PIPBRD frames.
- `app/presentation/lighting.rs` — the shield `Tint.Color`/`Tint.Intensity`
  word and intensity on SHP bodies, building draws/animations and voxel
  bodies, bucketed for the viewer as `ApplyCustomTintValues` does and gated by
  `Tint.VisibleToHouses`.

The shield state is saved, hashed under `phobos-shield-v1`, and a save records
the build capability. A catalog without a bound entry leaves the object
shield-less.

## Residuals

This chain owns damage, recovery, break/respawn state, the shield bar and the
shield tint. The following Phobos shield behavior is not implemented yet and
stays open:

- Custom `Pips.Background`/per-type background names: only the stock PIPBRD
  strip is drawn.
- A building slot anim whose type lacks `ShouldUseCellDrawer=` keeps its cell
  light without the shield `Tint.Intensity`; its colour word still carries
  `Tint.Color`.
- `IsBrokenAndNonRespawning` gates the bar from the shield type's `Respawn=`
  only; the warhead `Respawn` timer modifier that can override it is not read.
- A negative hit on a shield does not remove the attacker's parasite
  (`ShieldClass.cpp` `ParasiteEatingMe` arm).
- `BreakAnim`, `HitAnim`, `HitFlash*`, `IdleAnim*`, `Respawn.Anim` and the
  `BreakWeapon`/`Respawn.Weapon` detonations: no animation or self-fire is
  spawned.
- `SelfHealing.EnabledBy` and `AllowTransfer`/`AllowTransfer.Convert` are read
  but not consumed.
- Type conversion/deploy transfer (`SyncShieldToAnother`, `ConvertCheck`) and
  the warhead `Shield.Break`, `Shield.BreakAnim`, `Shield.BreakWeapon`,
  `Shield.HitAnim`, `Shield.SkipHitAnim`, `Shield.HitFlash`,
  `Shield.AttachTypes`/`RemoveTypes` effects are not ported.
- Ares EMP does not yet offline a shield (`IsUnderEMP` in native); only the
  building power and warp paths gate it.
- The shield-armour substitution in targeting/threat (`CanBeTargeted`,
  `TechnoClass_ReplaceArmorWithShields` at `0x70CF39`/`0x6F7D31`/`0x6FCB64`/
  `0x708AEB`) is not ported; acquisitions and threat still read the techno's
  own armour.

These do not change the absorbed/recovered points; they change how the shield
looks, sounds and reacts to type changes.
