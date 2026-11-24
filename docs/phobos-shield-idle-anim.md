# Phobos shield `IdleAnim` chain

Keys: `IdleAnim`, `IdleAnim.ConditionYellow`, `IdleAnim.ConditionRed`,
`IdleAnimDamaged` (+ its condition bands), `IdleAnim.OfflineAction`,
`IdleAnim.TemporalAction` on `[ShieldTypes]` entries.

## Native evidence

Phobos `src/New/Type/ShieldTypeClass.{h,cpp}`:

- `:36..37` `Damageable<AnimTypeClass*> IdleAnim, IdleAnimDamaged`; the reads
  are `Damageable::Read` (`Utilities/TemplateDef.h:2227..2245`, base flag with
  the trailing dot stripped, then `.ConditionYellow`/`.ConditionRed` as
  `Nullable<AnimTypeClass*>::Read<true>`), constructor-unset at `:94..97`.
- `:56..57` `Valueable<AttachedAnimFlag> IdleAnim_OfflineAction/TemporalAction`,
  constructor `Hides` at `:92..93`.
- `:132..147` `ShieldTypeClass::GetIdleAnimType`: `isDamaged` picks
  `IdleAnimDamaged.Get(shieldRatio, ConditionYellow, ConditionRed)` first and
  falls back to `IdleAnim.Get`, with the shield type's own thresholds falling
  back to `Shield_ConditionYellow/Red` -> `[AudioVisual] ConditionYellow/Red`.

Phobos `src/New/Entity/ShieldClass.cpp`:

- `:398..437` `AI_Temporal`: on the transition into a temporal freeze, apply
  `IdleAnim_TemporalAction` (Hides kills, Temporal sets `UnderTemporal`,
  Paused pauses, PausedTemporal does both).
- `:493..500` `CloakCheck`: `CloakState == Cloaked || Cloaking` and the live
  anim type's `DetachOnCloak` (default true) kill the anim.
- `:529..591` `OnlineCheck` runs every visit: inactive = kill/pause/freeze by
  `IdleAnim_OfflineAction`, active = clear `UnderTemporal` and unpause; the
  non-Powered type skips the whole check.
- `:593..605` `TemporalCheck` clears `UnderTemporal` and unpauses.
- `:466..482` the `AI` tail: `IdleAnimDamaged.isset()` recomputes the selection
  on a `HasHealthRatioThresholdChanged` band change, then
  `!IdleAnim && Online && HP > 0 && !AreAnimsHidden` creates.
- `:836..860` `BreakShield` kills the anim; `:1076..1125`
  `UpdateIdleAnim`/`GetIdleAnimType` perform the kill-and-recreate on a
  selection change; `:1041..1074` `CreateAnim` builds the anim at the techno
  location, `SetOwnerObject`, owner house, `SetInvoker`, and
  `RemainingIterations = 0xFF`.
- `PointerGotInvalid` (`:52..76`) drops the pointer when the anim dies.

`UnderTemporal` is gamemd `Anim+11A` (yrpp "temporal'd building's active
anims"), the byte the building temporal erase also freezes (`0x004521C0`,
cleared by `0x00452210`); Anim AI stops on it like on `+19E`
(`0x004244A9`). The Phobos action sets only that byte, without the erase's
translucent draw.

## Implementation

- `rules::phobos_shields::ShieldIdleAnimSet` owns the Damageable reads and
  selection; `ShieldType` gains both sets and the two action flags. The base
  reads as `Valueable<AnimTypeClass*>` (`none`/`<none>` clear through
  `INIClass::IsBlank @ 0x40F4D0`, which compares those two exact sentinels;
  `<default>` retains); the two bands read as
  `Nullable<AnimTypeClass*>::Read<false, false>`, where
  `none`/`<none>`/`<default>` reset the band rather than installing a present
  null.
- `sim::shield` owns the live anim id in `ShieldState::idle_anim` and
  creates/kills it through the existing anim owner (`spawn_anim_at_world`
  with constructor defaults, `hold_loops`, `destroy_anim_with_context`).
  `CloakCheck` runs both before `AI_Temporal`'s action switch and before the
  AI create/retype tail, through `anim_type_detach_on_cloak` — the same query
  the cloaking object's `detach_attached_anims_for_cloak` uses.
- The AI visit re-evaluates the selection every tick instead of latching on
  `HasHealthRatioThresholdChanged`. The selection is a pure function of the
  techno and shield ratios, so the chosen animation can only change on the
  same HP transitions native reacts to; the latching only avoids repeat work.
- The heal/respawn tail's staged animations commit before the idle create, in
  the AI's own order. A break queued by `SelfHealing` therefore constructs
  before the idle kill rather than after it; both objects are distinct, no
  RNG separates them, and both states are settled before the frame draws.

## Residuals

- `AreAnimsHidden` (`ShieldClass.h:82..86`, `ShieldClass.cpp:481`/`:1078`) follows
  the tunnel writes now: `FootExt::UpdateOnTunnelEnter/Exit` sets it from
  `TunnelLocomotionClass_Process_KillAnims @ 0x728F74` and `RestoreAnims @
  0x728E5F` (`Ext/Techno/Hooks.Misc.cpp:487-527`), so
  `update_idle_anim` kills the idle anim and skips the create gate while the
  owner's `Tunnel` locomotor is in `DugIn`/`Digging`, and recreates it once the
  state reaches `PreDigOut`
  (`a_tunnel_owner_hides_and_restores_the_shield_idle_anim`). The other
  writers of that flag (the shield's own disarm/`ShowShield` path) stay out of
  scope; the create/update gates treat them false.
- A retype caused by shield damage or self-healing happens on the next shield
  AI visit rather than synchronously inside `ReceiveDamage`/`SelfHealing`; the
  object's AI runs every frame, so the visible state converges within a tick.
- An idle-anim name no anim type answers is retained by the reader and
  treated as null at the use site. Native's read-time lookup instead keeps
  the value a previous layer set; the two differ only for a misspelt band in
  a later INI layer.
- `ConvertCheck`'s idle-anim transfer on DeploysInto/UndeploysInto and type
  conversion follows `ShieldClass::ConvertCheck` (`ShieldClass.cpp:608-760`,
  called from `FootExt::UpdateTypeData`, `Ext/Foot/Body.cpp:186-187`):
  `sim::shield`'s `phobos_shield_convert_check` applies the old type's
  `AllowTransfer.Convert`/`AllowTransfer` (falling back to `Attached`),
  rebinds or deletes the shield, compares both types'
  `GetIdleAnimType` selections and kills the animation when they differ, then
  rescales `HP` by the strength ratio and the recovery timers by the rate
  ratio. Tests `a_conversion_rebinds_the_shield_and_rescales_its_hp`,
  `an_allowed_conversion_keeps_the_shield_type`,
  `a_conversion_without_a_new_shield_type_deletes_the_shield`,
  `allow_transfer_alone_also_keeps_the_shield`.
  Residual: the deploy paths that do not run type conversion (`DeploysInto`
  on a type with no `DeploysInto=` rewrite) have no shield to check.
- The warhead replace path constructs a fresh shield whose idle anim is null;
  the old anim stays attached until something else retires it, exactly as
  Phobos's destructor (which does not `KillAnim`) leaves it.
- Kill-on-break from `SelfHealing` runs at the end of the same AI visit; the
  anim is gone before the frame draws, and the break animation order within
  that visit is otherwise unchanged.
