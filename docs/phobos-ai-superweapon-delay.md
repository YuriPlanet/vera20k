# AI superweapon delay

`[General] AISuperWeaponDelay=N` (game frames) spaces a computer house's
`AI_TryFireSW` calls instead of letting the strategy tick or the per-frame
call fire as often as it likes. `Nullable<int>`, unset by default, read by
`Ext/Rules/Body.cpp:127` (decl `Ext/Rules/Body.h:45`).

```ini
[General]
AISuperWeaponDelay=60   ; an AI house tries its supers at most once per 60 frames
```

## Native evidence

`Ext/House/Hooks.cpp`:

- `0x4FD77C` (`HouseClass_ExpertAI_Superweapons`, `:508-516`) skips the
  strategy-side `AI_TryFireSW` call whenever the key is set.
- `0x4F9038` (`HouseClass_AI_Superweapons`, `:518-541`) runs the per-frame
  call: an unset key, a human house or a neutral house takes the vanilla path;
  otherwise a per-house `CDTimerClass` starts at `N` when it has no time left,
  and the vanilla call is skipped while it does. `N <= 0` starts nothing and
  tries every frame. The call itself keeps the vanilla
  `!Campaign || IQ >= SuperWeapons` gate.

## Where VERA20K owns it

- `rules::phobos_globals::PhobosGlobals::ai_superweapon_delay` (field
  `rules/phobos_globals.rs:1228`, read `:3206`).
- `sim::house_state::HouseState::ai_superweapon_delay_timer`
  (`sim/house_state.rs:917-919`) with `ai_superweapon_delay_remaining` /
  `start_ai_superweapon_delay` (`:1271-1282`) — the shared `CdTimer`, so it
  persists and hashes like its sibling timers.
- `sim::house_strategy::building_strategy` step 3
  (`sim/house_strategy.rs:240-252`): `strategy_side_sw` is false while the key
  is set, so `0x4FD77C`'s skip leaves the rest of the step intact.
- `sim::house_strategy::update_ai_superweapon_delay` (`:105-151`), called for
  each house from `sim/world/house_defeat.rs:319-328` right after
  `update_strategy`, before `update_production_choices` — native's
  `HouseClass::Update` order.
- Tests: `sim::house_strategy::house_strategy_tests::an_unset_key_keeps_the_vanilla_strategy_timer`
  (the draw, the `112`-frame restart and the unset skip),
  `…a_set_key_runs_ai_sw_on_its_own_timer_and_skips_the_vanilla_step`
  (`40`-frame cadence plus the same strategy frame not calling with the key
  and calling without it), `…a_nonpositive_key_fires_every_frame` (`0`, `-1`),
  and `…ai_superweapon_delay_gates_the_per_frame_try`.

## Residuals

- Stock (unset) cadence: native's per-frame `AI_TryFireSW` at `0x4F9038` runs
  every frame for an AI house with the key unset; VERA's only stock caller is
  the strategy tick (`sim/house_strategy.rs:244-252`). The hook's vanilla
  per-frame call is therefore not introduced here — adding it would move every
  AI superweapon's launch frame, which this package must not do. With the key
  set, the per-frame call and its timer are native's.
- A human or neutral house keeps its existing owner (`update_strategy`'s Ares
  human arm); this function returns before touching it.
- Native's `IsNeutral()` guard is matched with `HouseState::multiplay_passive()`;
  no other VERA state carries that flag.
