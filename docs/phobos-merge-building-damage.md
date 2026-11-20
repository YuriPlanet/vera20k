# Merge area damage on a building (`MergeBuildingDamage`)

A `WarheadType` key that makes one area detonation damage a building **once**,
instead of once per foundation cell the sweep reaches.

## Key

| Key | Type | Default |
|---|---|---|
| `MergeBuildingDamage=` | `Nullable<bool>` | unset |

Read: `WarheadTypeExt::LoadFromINI` (`Ext/WarheadType/Body.cpp:339`),
declaration `Ext/WarheadType/Body.h:184`, constructor `Body.h:463`. VERA reads
it in `src/rules/phobos_warhead_extras.rs`, owner `WarheadType::phobos_extras`.
The fallback is `[CombatDamage] MergeBuildingDamage=` (`Ext/Rules/Body.cpp:503`,
constructor `Ext/Rules/Body.h:918`, default `no`), held on
`RuleSet::phobos_globals.merge_building_damage`.

## Behaviour

`MapClass::DamageArea` collects one `(target, distance)` group per object per
spread cell, so a 3x3 building inside a 2-cell spread is reached nine times.
Phobos hooks that list twice (`Ext/WarheadType/Hooks.cpp:410-487`):

1. `0x4899DA` builds `std::unordered_map<BuildingClass*, double>` and, per
   building group with `Distance <= F2I(CellSpread * 256)` (`:426-445`),
   accumulates

   ```
   multiplier = (cellSpread && percentDifference)
       ? 1.0 - (percentDifference * Distance / cellSpread) : 1.0
   sum += max(multiplier, 0)
   ```

   with `percentDifference = 1.0 - PercentAtMax`.
2. It then walks the same list again (`:447-478`) and, for the first group of
   each still-listed building, issues a **single**
   `ReceiveDamage(&F2I(baseDamage * sum), 0, pWH, ...)`, erasing the entry; a
   product that truncates to zero keeps `Math::sgn(baseDamage)`.
3. `0x489A1B` jumps over the vanilla per-building damage so nothing is applied
   twice.

VERA collects the same per-cell records (`AoECollector`) and, at the end of
collection, `sim::combat::combat_aoe::merge_building_records` rewrites the first
record of each building to `F2I(baseDamage * sum)` at distance 0 and drops the
rest. The merge threshold is its own
`Game::F2I(CellSpread * 256)` — deliberately not the collection's
`SplashThreshold`, which native also computes separately.

## Residuals

- **Record order.** Native issues the merged calls in its own loop after the
  object sweep; VERA rewrites each building's first record in place, so a
  building's damage keeps its position among the other objects' records.
  Nothing in the receiver observes another record's ordering.
- **The re-checked gates.** Native re-tests `IsAlive`, `InvisibleInGame`,
  `Health > 0`, `IsOnMap`, `!InLimbo` and the Iron Curtain condition
  `invincibleWithoutPenetrateAndCloseTo` before the merged call. VERA leaves
  them to the receiver, which refuses a dead or limboed building anyway; a
  building that fails them only costs the walk, and its records were already
  being dropped there.
- **Nested detonations.** Phobos uses a fresh map per hook entry precisely
  because shrapnel can re-enter `DamageArea`. VERA's merge runs once over one
  collector, so a nested collection merges on its own — the same isolation.

## Tests

- `src/sim/combat/aoe_merge_building_tests.rs` —
  `four_building_records_become_one`,
  `the_distance_term_is_summed_inside_the_spread_only`,
  `a_single_record_is_kept_and_zeroed`.
- `src/rules/phobos_warhead_extras_tests.rs` and
  `src/rules/phobos_globals_tests.rs` — the parse and fallback cases.
