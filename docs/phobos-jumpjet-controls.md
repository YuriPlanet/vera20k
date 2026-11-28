# Phobos `[JumpjetControls]` techno-type seeds

Stock YR reads `[JumpjetControls]` into seven `RulesClass` fields
(`ReadJumpjetControls @ 0x006743D0`, the section gate `0x526810`) but the
`TechnoTypeClass` constructor at `0x007115AE` ignores them and pushes its own
literals. That is why `rulesmd.ini` can say *"These are now merely defaults"*
and why a modded `[JumpjetControls]` has no effect in stock YR.

Phobos replaces the constructor seeds (`TechnoTypeClass_CTOR_JumpjetControls`,
`Ext/Rules/Body.cpp:1325`, hook `0x7115AE`, returns `0x711601`) and adds two
keys of its own (`Ext/Rules/Body.cpp:1353-1354`):

| Field (`+0xD70`…) | Source | Rules offset | Constructor |
|---|---|---|---|
| `JumpjetTurnRate` | `[JumpjetControls] TurnRate=` | `+0x40C` | 3 |
| `JumpjetSpeed` | `Speed=` | `+0x410` | 30 |
| `JumpjetClimb` | `Climb=` (double → `float`) | `+0x418` | 5.0 |
| `JumpjetCrash` | Phobos `Crash=` | — | 5.0 |
| `JumpjetHeight` | `CruiseHeight=` | `+0x420` | 400 |
| `JumpjetAccel` | `Acceleration=` (double → `float`) | `+0x428` | 0.25 |
| `JumpjetWobbles` | `WobblesPerSecond=` (double → `float`) | `+0x430` | 0.25 |
| `JumpjetNoWobbles` | Phobos `NoWobbles=` | — | no |
| `JumpjetDeviation` | `WobbleDeviation=` | `+0x438` | 40 |

The two Phobos keys are `Valueable<double>` / `Valueable<bool>`
(`Ext/Rules/Body.h:54-55,594-595`). The other seven are the stock Rules fields;
their constructor values come from `RulesClass`'s constructor at
`0x00665C16`-`0x00665C69`, and the stock read uses gamemd's `ReadDouble`, i.e.
the authored text lands as the nearest binary32 widened back.

The TechnoType's own `Jumpjet*=` keys still win, because the native
`ReadINI` run (`0x00715020`-`0x0071520F`) passes the just-seeded field as each
reader's default. Retail `rulesmd.ini` carries the same numbers as the old
literals, so retail behaviour is unchanged; a mod that edits the section now
moves every unit that does not author the key.

## VERA owners

- `src/rules/jumpjet_params.rs` — `JumpjetControls` (the one reader of
  `[JumpjetControls]`, seven keys) and
  `JumpjetParams::from_ini_section_with(section, ctor)`; `from_ini_section`
  delegates with the literal constructor for callers without a rules context.
  `JumpjetParams::from_controls` is the Phobos hook's seed block.
- `src/rules/phobos_globals.rs` — `jumpjet_crash` (`NativeF64Bits`) and
  `jumpjet_no_wobbles`, read in the same `[JumpjetControls]` pass as
  `RotateOnCrash`.
- `src/rules/ruleset.rs` — `GeneralRules::from_ini` takes `CruiseHeight` from
  the shared reader; `RuleSet::from_ini` builds the Phobos constructor before
  the type registries (as native does) and re-reads each type's block over it.

## Tests

`rules::jumpjet_params` pins the seven-key reader, its constructor values, the
Phobos re-seed through `RuleSet::from_ini` (including `Crash`/`NoWobbles`), and
the stock build's literals. `rules::phobos_globals` pins the two Phobos keys to
`[JumpjetControls]`. `sim::movement::` keeps the locomotor consumers green.

Ordering: the hook reads `RulesClass::Instance`, so the section must already be
read when a type is constructed — Phobos depends on that, and VERA mirrors it by
building the constructor before the type registries. No separate native capture
pins the moment; the feature working is the evidence that the read precedes the
types.
