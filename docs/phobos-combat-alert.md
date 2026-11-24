# Combat alert (`CombatAlert.*`)

A hit on the current player's object raises a throttled alert: one Combat radar
event at the object's cell, a House-wide cooldown timer, and at most one global
voice or EVA line. The block sits inside the Phobos hook
`TechnoClass_ReceiveDamage_Shield @ 0x701900`
(`Ext/Techno/Hooks.ReceiveDamage.cpp:86-140`): after the damage-multiplier
block, before the shield stage.

```ini
[AudioVisual]
CombatAlert=false                     ; Valueable<bool>, master switch
CombatAlert.Default=                  ; Nullable<bool>, unset -> !Insignificant && !Spawned
CombatAlert.IgnoreBuilding=true       ; Valueable<bool>
CombatAlert.SuppressIfInScreen=true   ; Valueable<bool>
CombatAlert.Interval=150              ; Valueable<int>, frames
CombatAlert.SuppressIfAllyDamage=true ; Valueable<bool>
CombatAlert.MakeAVoice=true           ; Valueable<bool>
CombatAlert.UseFeedbackVoice=true     ; Valueable<bool>
CombatAlert.UseAttackVoice=true       ; Valueable<bool>
CombatAlert.UseEVA=true               ; Valueable<bool>

[SOMETECHNO]                          ; TechnoType
CombatAlert=                          ; Nullable<bool>, unset -> global Default
CombatAlert.NotBuilding=              ; Nullable<bool>
CombatAlert.UseFeedbackVoice=         ; Nullable<bool>, unset -> global
CombatAlert.UseAttackVoice=           ; Nullable<bool>, unset -> global
CombatAlert.UseEVA=                   ; Nullable<bool>, unset -> global
CombatAlert.EVA=                      ; NullableIdx<VoxClass>, unset -> EVA_UnitsInCombat

[SOMEWARHEAD]
Nonprovocative=false                  ; Valueable<bool>
CombatAlert.Suppress=                 ; Nullable<bool>, unset -> !Malicious || Nonprovocative
```

Readers: globals `Ext/Rules/Body.cpp:355-364`, constructors
`Ext/Rules/Body.h:783-792`; type keys `Ext/TechnoType/Body.cpp:846-851`,
fields `Ext/TechnoType/Body.h:112-117`, constructors `:593-598`; warhead
`Nonprovocative=` `Ext/WarheadType/Body.cpp:337` / `Body.h:461` (`false`) and
`CombatAlert.Suppress=` `Ext/WarheadType/Body.cpp:367` / `Body.h:212,491`
(unset); `Malicious=` is Ares' warhead field (`Body.h:280,518`, default
`true`).

Corrected defaults, from those constructors rather than from the INI example
an earlier outline showed:

- `CombatAlert.Default` is `Nullable<bool>` constructed **unset**. Unset means
  the type-level fallback `!Insignificant && !Spawned`; an authored `yes`/`no`
  replaces it. It is not a plain `false`.
- `IgnoreBuilding`, `SuppressIfInScreen`, `SuppressIfAllyDamage`,
  `MakeAVoice`, `UseFeedbackVoice`, `UseAttackVoice` and `UseEVA` construct
  **`true`**; `Interval` constructs `150`.
- `CombatAlert.EVA` is `NullableIdx<VoxClass>`, not an int: unset keeps the
  `-1` "no value" state and the consumer passes
  `VoxClass::FindIndex("EVA_UnitsInCombat")`; `none`/`<none>` stores a present
  `-1` that plays nothing.

## Native behavior

Order of gates in `raiseCombatAlert` (all line numbers in
`Ext/Techno/Hooks.ReceiveDamage.cpp`):

1. `RulesExt::Global()->CombatAlert && damage > 1` (`:86`). `damage` is the
   packet after the same hook's damage-multiplier block (`:49-85`).
2. Victim House controlled by the current player (`:90`); otherwise no alert.
3. With `SuppressIfAllyDamage`, `pTargetHouse->IsAlliedWith(pSourceHouse)`
   skips (`:90`); `pSourceHouse` is the attacker's owner, else
   `args->SourceHouse`.
4. `HouseExt::CombatAlertTimer.HasTimeLeft()` skips, as does the warhead's
   `CombatAlert.Suppress.Get(!Malicious || Nonprovocative)` (`:95`). The
   House timer is shared by every object the House owns.
5. The type's `CombatAlert.Get(Global()->CombatAlert_Default.Get(
   !Insignificant && !Spawned))` must admit, and the object must be in the
   playfield (`:101`).
6. `IgnoreBuilding` skips an ordinary building: with `NotBuilding` set, only
   `CombatAlert.NotBuilding=yes` admits; unset, a building whose type is
   `IsVehicle()` (`1x1` with `UndeploysInto=`) admits (`:106`).
7. `SuppressIfInScreen` skips when `CoordsToScreen(GetCoords()) -
   TacticalPos` lands inside the composite surface rect (`:111-119`).
8. Accepted: `CombatAlertTimer.Start(CombatAlert.Interval)` (`:121`), then
   `RadarEventClass::Create(RadarEventType::Combat,
   CellClass::Coord2Cell(GetCoords()))` (`:122`), then one line (`:125-138`):
   `MakeAVoice` off ends it; else `VoiceFeedback[0]` when its switch resolves
   true and the list is non-empty; else `VoiceAttack[0]` the same way; else the
   `CombatAlert.EVA` index when `UseEVA` resolves true, where index `-1` plays
   nothing. Feedback wins over attack wins over EVA; the radar event and the
   timer happen even when no voice plays.

## Where VERA20K owns it

- `rules::phobos_combat_alert::PhobosCombatAlert` on
  `ObjectType::phobos_combat_alert` holds the six type keys, read per type
  section in native order; `CombatAlertEva` is the `NullableIdx` tri-state
  (`Default`, `None`, `Name`). `rules::phobos_globals::PhobosGlobals` holds
  the ten `[AudioVisual]` globals. `WarheadType::nonprovocative` and
  `WarheadType::combat_alert_suppress` hold the warhead pair; the latter
  falls back to the existing `world_receiver::warhead_is_malicious` owner and
  `Nonprovocative`.
- `HouseState::combat_alert_timer` / `start_combat_alert` is the retained
  `HouseExt::CombatAlertTimer` (`Ext/House/Body.h:46,102`): serde-defaulted,
  in the state hash only once started, and gated by `CdTimer::remaining(frame)
  > 0`, which is `CDTimerClass::HasTimeLeft()`.
- `sim::combat::combat_alert::raise_combat_alert` is the one port, called from
  `world_receiver::commit_entities` at the native point (after the `:26-34`
  warhead-trigger block, before the shield). It reuses `vehicle_like` for the
  `IsVehicle()` building answer and reads the object's coords once for
  `Coord2Cell`'s truncating divide.
- The accepted hit emits `SimSoundEvent::CombatAlert { owner, radar, voice }`
  with `CombatAlertVoice::{Feedback, Attack, Eva, None}`; sim owns no audio
  and no view. `app::match_runtime::sound_dispatch` keeps the local player's
  event only, admits the radar request and maps the voice to `VocAt`/`Eva`
  playback.
- `Ruleset` hashing folds the type keys and warhead flags behind their own
  tags only when they differ from the constructor state, so untagged retail
  rules hash as before.

## RNG/timer/detach ledger

The chain draws no Scenario or main RNG value, detaches nothing and calls no
`DetachAll`. Its one state write is `CombatAlertTimer.Start(Interval)` on an
admitted hit; throttled and rejected hits write nothing. It passes one
`RadarEventRequest` and at most one voice name to the client.

## Residuals

- **`SuppressIfInScreen` is not applied.** Native projects the object's
  coordinate into the live tactical view and suppresses when it is on screen;
  the headless simulation has no view or surface. Trigger: any admitted hit
  whose object is visible; effect: VERA raises a radar ping and voice native
  would suppress; frequency: high with the default `yes` while the camera
  covers the object; risk: extra alert sound/ping only. Applying it belongs to
  the client, which already filters by local owner.
- **`IsControlledByCurrentPlayer` is approximated by "a human controls the
  House".** The simulation has no local player, so it admits every
  human-controlled House and the app keeps only the local owner's event.
  Trigger: a multiplayer client receiving damage on another human House;
  effect: the sim publishes an event the client drops; frequency: multiplayer
  with more than one human House; risk: none for gameplay, no voice/radar
  leak because of the client filter.
- **Non-Ares `Malicious` default.** `warhead_is_malicious` reads Ares'
  warhead extension only under `ares`; without it every warhead is malicious,
  as the Ares constructor defaults. Trigger: a non-Ares Phobos build with a
  warhead authored `Nonprovocative=no` that Ares would mark `Malicious=no`;
  effect: VERA raises an alert native+Ares would suppress via
  `CombatAlert.Suppress`'s `!Malicious || Nonprovocative` fallback; frequency:
  only such mods; risk: an extra alert.
- **Damage multipliers are the receiver's chain, not this one.** The alert
  reads whatever `world_receiver` computed; the surrounding Phobos
  `Damage*Multiplier` block (`Hooks.ReceiveDamage.cpp:49-85`) was unported when
  this page was written and now lives in
  [`docs/phobos-damage-multiplier.md`](phobos-damage-multiplier.md), so the
  `damage > 1` threshold already sees the scaled packet.
- **Unknown `CombatAlert.EVA` names.** Native resolves the name during the
  rules read and retains the previous index for an unknown one; VERA stores
  the authored name and the client plays nothing when its Vox catalog does
  not define it. Trigger: a `CombatAlert.EVA=` name absent from `evamd.ini`;
  effect: silence where native would keep the earlier EVA line or the
  `EVA_UnitsInCombat` fallback; frequency: mod typos only; risk: low.
