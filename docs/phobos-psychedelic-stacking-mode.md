# Psychedelic.StackingMode

A `Psychedelic=yes` warhead normally overwrites a victim's berserk time with
the hit's damage. Phobos lets a mode combine the new damage with the time the
victim has left, per warhead over a global default.

```ini
[CombatDamage]
Psychedelic.StackingMode=override  ; StackingMode, default override

[PSYCHWARHEAD]                     ; Warhead section
Psychedelic.StackingMode=add       ; StackingMode, unset -> the global key
```

Modes: `override` (replace), `setifzero` (write only while the time is zero),
`min`, `max`, `add`, `subtract`, `multiply` (clamped) and `divide` (no change
when the new value is zero).

Readers: the global key at `Ext/Rules/Body.cpp:551` (`Valueable<StackingMode>`,
constructor `Override` at `Ext/Rules/Body.h:462,967`); the warhead key at
`Ext/WarheadType/Body.cpp:446` (`Nullable<StackingMode>`, unset at
`Ext/WarheadType/Body.h:267,569`).

## Native body

- `TechnoClass_ReceiveDamage_Psychedelic` (hook `0x701D6B`,
  `Ext/WarheadType/Hooks.cpp:678-696`) replaces the stock
  `mov [esi+0x29C], eax` (the time) with
  `EnumFunctions::CalcValueWithStackingMode(pThis->BerzerkDurationLeft, damage, mode)`.
  The mode is the warhead's `Nullable` value else `RulesExt::Global()`'s.
- The stock block after the hook: `0x701D71` reads the berserk byte; when set
  it jumps to the result (`0x701DBA`). Otherwise `0x701D7D` sets the byte,
  runs the idle order (`0x006EA870`), clears the target (`vt+0x3C8(0)`) and
  queues Hunt (`vt+0x1E8(0xF, 0)`). A non-positive combined time jumps
  straight to `StopBerzerk` (`0x701DBA`, `mov eax, 1`), so none of that runs;
  the time field still holds the combined value.
- `CalcValueWithStackingMode` (`Utilities/EnumFunctions.cpp:119-162`,
  `StackingMode` at `Utilities/Enum.h:265-275`): `multiply` goes through
  `GeneralUtils::SafeMultiply` (`Utilities/GeneralUtils.cpp:309-319`), which
  clamps the 64-bit product to the `int` range; `divide` by zero keeps the old
  value; `add`/`subtract` are plain signed 32-bit arithmetic.
- `detail::read<StackingMode>` (`Utilities/TemplateDef.h:1180-1224`) matches
  the eight names case-insensitively. Its `override` arm is a standalone `if`
  before the `setifzero`/`else` chain, so it assigns the value but the reader
  still answers false. `Valueable` ignores that answer and keeps the value;
  `Nullable` sets presence only on true, so a lone warhead-level `override`
  stays unset and takes the global key (an explicit `override` after an
  earlier accepted layer value still replaces it, because the template wrote
  the field).

## VERA

`rules::phobos_stacking_mode::StackingMode` holds the enum, the
`combine(old, new)` port of `CalcValueWithStackingMode`, and the two readers
(`Valueable` via `read_string_with`, `Nullable` with the `override` presence
quirk). `CombatDamageDefaults::phobos_psychedelic_stacking_mode` reads
`[CombatDamage]`; `WarheadType::phobos_psychedelic_stacking_mode` reads the
warhead key.

`receiver_health::psychedelic_berserk_time` resolves the key pair and combines
the incoming damage with `BerserkState::timer`; the Psychedelic arm writes the
result with `BerserkState::psychedelic_stack`, which sets the flag only for a
positive time. `psychedelic_starts_berserk` gives the same answer to the
caller's team removal, target clear and Hunt arm
(`world_receiver.rs`, `0x00701D71..0x00701DB4`).

## Tests

- `src/rules/phobos_stacking_mode.rs` — `every_mode_combines_like_calc_value_with_stacking_mode`
  (all modes plus the wrap/clamp/divide-by-zero boundaries),
  `the_global_key_reads_case_insensitively_and_keeps_on_rejection`,
  `the_warhead_key_is_nullable_and_override_needs_a_prior_value` and
  `ordered_layers_fold_like_the_native_readers` (add → rejected keeps add;
  add → `override` replaces it; `<default>` clears only the nullable key).
- `src/sim/combat/combat_aoe.rs` —
  `psychedelic_stacking_combines_the_time_and_stops_a_non_positive_one`: two
  gas hits stack `100 + 100` under the global `add`; a `subtract` warhead
  overrides it and StopBerzerk leaves a fresh target's flag clear, its target
  set and no Hunt; on an already-berserk target the flag stays and the next
  countdown clears the expired time.
- `src/sim/combat/combat_tests.rs` —
  `stop_berzerk_leaves_a_team_member_where_a_positive_stack_liberates_it`: a
  team-member victim is liberated by a positive stack and stays in its team
  when StopBerzerk skips the removal.

The flag write, the warhead-over-global resolution, the team liberation and
the caller's `starts_berserk` gate were each mutated red and restored.

## Residuals

- `CalcValueWithStackingMode`'s bool return is unused by the hook; VERA's
  `combine` returns only the value.
- The `override` presence quirk above is reproduced as Phobos implements it;
  the documentation's per-warhead `override` only works when the global mode
  is not the target value and no earlier pass set the warhead value.
- A Psychedelic hit whose resulting damage is zero or negative no longer
  starts a berserk even under `override`, because the hook's `StopBerzerk`
  arm runs for every non-positive result. Stock would set the flag with a
  non-positive time. Phobos reaches this only through a kernel that yields
  zero or negative damage.
- Team removal still happens before the receiver's timer/flag write in VERA,
  but it reads only the pre-hit flag and the combined time, as it did before
  this key existed.
