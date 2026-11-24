# Crate goodie reroll chance

`CrateGoodie.RerollChance=` turns a crate unit candidate into a probabilistic
one: a type with `CrateGoodie=yes` rerolls and lets the pick draw again with
the configured probability.

```ini
[SOMEVEHICLE]                       ; UnitType
CrateGoodie=yes
CrateGoodie.RerollChance=0.0        ; Valueable<double>, default 0.0
```

Reader: `Ext/UnitType/Body.cpp:76` (field `Ext/UnitType/Body.h:61`, constructor
`:123`). Phobos parses doubles through `Parser<double>`, which stores
`static_cast<float>(value)` widened back to double
(`Utilities/Parser.h:261-283`), so VERA keeps the value as `f64`.

## Native body

`CellClass_GoodieCheck_CrateGoodie` (`0x4821BD`,
`Misc/Hooks.Crates.cpp:80-92`): when the drawn type's `CrateGoodie=` is set and
its reroll chance is above zero, the boolean becomes
`chance < Scenario.Random.RandomDouble()`. A true boolean keeps the type; a
false one rerolls it, so the selection loop draws another candidate. The draw
precedes the base-unit gates, so a rejected base unit still spends it.

## VERA

`ObjectType::phobos_crate_goodie_reroll_chance` carries the key (read for
vehicle types, the UnitType family). In
`sim/crates/effects.rs::choose_unit_crate_type` the pick loop applies the same
draw after the type's `CrateGoodie=` and before the base-unit gates: a type
passes only while `chance < SimRng::random_double()`. The draw-free
no-candidate guard keeps its previous behaviour.

## Residuals

- The retry loop never terminates when every candidate rerolls (for example a
  single `CrateGoodie=yes` type with `CrateGoodie.RerollChance=1.0`); VERA's
  draw-free guard only rejects a pool with no static candidate, so it spins
  like native. Trigger: authored data where every goodie rerolls. Frequency:
  invalid mod data. Risk: the simulation freezes in that pickup.
- Retail rules author no `CrateGoodie.RerollChance`; the default 0 disables
  the draw and keeps the stock pick.
