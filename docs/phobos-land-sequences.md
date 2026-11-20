# Land sequences in water (`OnlyUseLandSequences`)

An infantry type that plays its land animation sequences even while it is in
water.

## Keys

| Key | Type | Default |
|---|---|---|
| `OnlyUseLandSequences=` (art section) | `Nullable<bool>` | unset |
| `[General] OnlyUseLandSequences=` | `Valueable<bool>` | `no` |

Read: `InfantryTypeExt::LoadFromINI` (`Ext/InfantryType/Body.cpp:32`) from the
**art** INI (`exArtINI, pArtSection`), declaration `Ext/InfantryType/Body.h:19`,
constructor `Body.h:44`; the global at `Ext/Rules/Body.cpp:622`, declaration
`Ext/Rules/Body.h:520`, constructor `Body.h:1017`. VERA reads the type key in
`InfantryArt::read_pass` (`src/rules/object_type.rs`, the per-pass fixed-ART
reads at `0x005246A1..0x0052472D` followed by the `0x52473F` hook), from the
same Image section as `Crawls=`, and projects it onto
`ObjectType::phobos_land_sequences`; the global owner is
`RuleSet::phobos_globals.only_use_land_sequences`. The type key is an
**art**-section key: a rules section never supplies it, because
`InfantryTypeExt::LoadFromINI` reads `exArtINI` only. Tests:
`src/rules/phobos_land_sequences_tests.rs` and the water-action regression
`src/sim/movement/infantry_water_action_tests.rs`
(`only_use_land_sequences_skips_the_water_remap`).

## Behaviour

`InfantryClass_DoAction_Water` (`0x51D7E0`,
`Ext/Techno/Hooks.Misc.cpp:883-901`):

```cpp
if (pTypeExt->OnlyUseLandSequences.Get(RulesExt::Global()->OnlyUseLandSequences))
    return SkipWaterSequences;      // 0x51D842
if (sequence == Sequence::Walk || sequence == Sequence::Crawl)
    return UseSwim;                 // 0x51D83D
else if ((sequence == SecondaryFire || sequence == SecondaryProne)
         && pTypeExt->SecondaryFireSequenceLandOnly.Get(Global))
    return UseWetAttack;            // 0x51D82F
return Continue;                    // 0x51D7EC
```

So `OnlyUseLandSequences` short-circuits the whole water-sequence selection:
`Walk`/`Crawl` never become `Swim`, and the wet-attack arm never runs. VERA
consumes it in `src/sim/movement/infantry_action.rs`'s water arm:
`object.phobos_land_sequences.only_land(rules.phobos_globals.only_use_land_sequences)`
keeps every requested action unmapped while the water-state store and the
transition sound still run on the `SkipWaterSequences` path. The sibling
`SecondaryFireSequenceLandOnly=` art key is owned by the same module
(`src/rules/phobos_land_sequences.rs:53-90`) and consumed by the same water
arm (`src/sim/movement/infantry_action.rs:311-348`, `4 | 8 if
secondary_fire_land_only => 22`).

## Residual

None. `SecondaryFireSequenceLandOnly=` (`Ext/Techno/Hooks.Misc.cpp:897-899`)
was previously recorded here as unported; it is read (type art + `[General]`
fallback, `Valueable` default yes) and consumed, so a water-borne
`SecondaryFire`/`SecondaryProne` request maps to `WetAttack` only when the
resolved key is yes.

## Tests

- `src/rules/phobos_land_sequences_tests.rs` — the type's value outranks the
  global, an unset key defers to the global (default `no`), and the
  `<default>` / `none` reset tokens.
- `src/rules/phobos_globals_tests.rs` — the `[General]` key reads alongside
  the other `HoverDrownable`-block globals.
- `src/sim/movement/infantry_water_action_tests.rs`
  `only_use_land_sequences_skips_the_water_remap` — the stock water remap,
  the `[General]` fallback, the type's art key, and an explicit type `no`
  overriding a global `yes`, all through the production ART layer.
