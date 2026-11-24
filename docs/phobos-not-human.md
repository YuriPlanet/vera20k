# NotHuman death sequences

`NotHuman=yes` infantrymen can pick which `Die1`..`Die5` sequence a death
plays.

```ini
[WH]                                  ; Warhead
NotHuman.DeathSequence=-1             ; Valueable<int>, 1..5 selects Die1..Die5

[InfantryType]
NotHuman.RandomDeathSequence=         ; Nullable<bool>, unset -> [General]

[General]
NotHuman.RandomDeathSequence=no       ; Valueable<bool>, default no
```

Reader `Ext/WarheadType/Body.cpp:289` (constructor default -1,
`Body.h:417`); InfantryType `Ext/InfantryType/Body.cpp:20` (constructor
unset, `Body.h:16`); global `Ext/Rules/Body.cpp:621` (default false,
`Body.h:1016`).

## Behavior

Phobos `InfantryClass_ReceiveDamage_NotHuman` (hook `0x518505`,
`Ext/Techno/Hooks.ReceiveDamage.cpp:308-345`) replaces the `NotHuman=` death
path's hardcoded `Die1`:

- a dying infantryman whose current sequence is `Paradrop` and who is falling
  spawns the `[General] InfantryExplode=` anim at its location and jumps to the
  shared death tail (`0x5185F1`) instead of playing a sequence;
- otherwise the sequence is `Die1`; with the InfantryType's
  `NotHuman.RandomDeathSequence` (else the `[General]` fallback) it becomes one
  `ScenarioClass::Instance->Random.RandomRanged(Die(1), Die(5))` draw;
- a killing warhead with `NotHuman.DeathSequence > 0` overrides it with
  `Die(min(value, 5))` after that draw.

## Where VERA20K owns it

`WarheadType::phobos_not_human_death_sequence`,
`ObjectType::phobos_not_human_random_death_sequence` (Infantry only) and the
`phobos_globals` `[General]` fallback feed the `InfantryDeathArm::NotHuman`
arm in `sim/world/infantry_death.rs`; `InfantryDeathSequence` gained
`Die3`..`Die5`; the warhead value reaches the arm as the extra
`begin_infantry_receiver_death` argument from the `ReceiveDamage` kill site.

RESIDUAL: the `RandomRanged(11, 15)` draw and the warhead override run in the
same order as native; the paradrop branch reuses the shared
`InfantryExplode` anim recipe. Without Ares that recipe writes no explicit
owner, like native's bare `GameCreate<AnimClass>`; under Ares it attaches the
Ares `MakeInfantryOwner` handling (a possible random-owner draw) that the
native branch does not.

RESIDUAL: VERA's arm order keeps `NotHuman=` ahead of the stock Paradrop
rung (`infantry_death_arm`), which is what makes the hook's own
`SequenceAnim == Paradrop && IsFallingDown` check reachable; native's
`0x51836F` rung ordering is not re-verified here.

RESIDUAL: the type key is a `Nullable<bool>` read from the merged section
once; native re-reads each INI pass and keeps the previous value when a later
pass omits it. A later pass that clears the key (blank or `<default>`) would
differ.
