# Rearm and reload holds (`NoRearm.*` / `NoReload.*`)

Phobos can hold a techno's rearm (ROF) and ammo-reload countdowns still while
the techno is EMP'd or deactivated, or while a Temporal weapon is warping it.
Four `Nullable<bool>` TechnoType keys
(`Ext/TechnoType/Body.cpp:1054-1057`, fields `Body.h:286-289`, constructors
`:718-721`) fall back through `Nullable<bool>::Get` to four `[General]`
`Valueable<bool>` globals (`Ext/Rules/Body.cpp:383-386`, fields
`Ext/Rules/Body.h:288-291`, constructors `:804-807`), all default `false`.

```ini
[General]
NoRearm.UnderEMP=false                  ; Valueable<bool>
NoRearm.Temporal=false                  ; Valueable<bool>
NoReload.UnderEMP=false                 ; Valueable<bool>
NoReload.Temporal=false                 ; Valueable<bool>

[SOMETECHNO]
NoRearm.UnderEMP=                       ; Nullable<bool>, falls back to [General]
NoRearm.Temporal=                       ; Nullable<bool>, falls back to [General]
NoReload.UnderEMP=                      ; Nullable<bool>, falls back to [General]
NoReload.Temporal=                      ; Nullable<bool>, falls back to [General]
```

## Native behavior and hooks

`TechnoExt::UpdateRearmInEMPState` (`Ext/Techno/Body.Update.cpp:866-880`)
runs per techno visit from `TechnoExt::OnEarlyUpdate` (`:23`), after
`UpdateAttachEffects`/`UpdateRecountBurst` and before `CheckDeathConditions`.
It returns unless `IsUnderEMP() || Deactivated`; then each running timer is
held with `StartTime++`:

- `RearmTimer.InProgress() && NoRearm.UnderEMP.Get(global)` → `RearmTimer.StartTime++` (`:875-876`);
- `ReloadTimer.InProgress() && NoReload.UnderEMP.Get(global)` → `ReloadTimer.StartTime++` (`:878-879`).

`TechnoExt::UpdateRearmInTemporal` (`:882-895`) holds the same two timers on
a Temporal weapon's target with the `.Temporal` keys and no predicate beyond
them. It is reached from `TechnoExt::UpdateTemporal` (`:853-863`) inside the
`TemporalClass_AI @ 0x71A88D` hook (`Ext/Techno/Hooks.cpp:85-103`) while the
warp runs — the target's own `TechnoClass_AI` is skipped while frozen, which
is why the hold cannot live in the EMP arm.

`StartTime++` moves the timer's frame anchor one frame later on every visit,
so a continuous condition keeps the remaining time still and the countdown
resumes where it stopped when the condition ends. The hold consumes no RNG
draws and touches no other timer (`ForceShield` and the rest are untouched).

## VERA20K port

- The per-visit EMP arm runs in the shared `techno_common_post` step of the
  per-techno visit (`sim::world::techno_ai`), gated
  as part of every build. The Temporal arm runs in the Temporal owner's
  target update (`sim::temporal::temporal_update_head`) on the
  `remaining > 0` path; the `<= 0` path erases the target that frame, whose
  timers then cease, so the hold there would be unobservable.
- Both arms call the same two owner mutations: `RearmTimer::delay_one_frame`
  (`sim/combat/rof.rs`) and `GameEntity::delay_ammo_reload_one_frame`
  (`sim/game_entity.rs`), each delegating to `CdTimer::delay_one_frame`
  (`sim/timer.rs`), the one owner of the `StartTime++` arithmetic.
- Effective values resolve through `rules::phobos_no_rearm` (the type's
  `Option<bool>` per key) against `rules::phobos_globals` (`[General]`
  reads), matching the retail layers: the four globals are read on every
  rules pass from the merged `[General]` state.

## Residuals

None recorded. The hold runs on the techno's own early update, resumes at the stored countdown and
consumes no RNG draw and no other timer.

## Tests

`sim::world::techno_ai` drives `techno_common_post` on an EMP'd and on a
deactivated unit (held vs counting) and through the `[General]` fallback;
`sim::temporal` drives `temporal_update_head` on a chained warp (held vs
counting) for both `.Temporal` keys.
