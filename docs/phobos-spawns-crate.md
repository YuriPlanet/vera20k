# SpawnsCrate warhead keys

A warhead detonation places a fixed powerup crate at the impact.

```ini
[Warhead]
SpawnsCrate0.Type=Money           ; Nullable<Powerup>, numbered
SpawnsCrate0.Weight=1             ; Nullable<int>, numbered
SpawnsCrate.Type=Money            ; entry-0 alias
SpawnsCrate.Weight=1              ; entry-0 alias
```

Reader `WarheadTypeExt::LoadFromINIFile`
(`Ext/WarheadType/Body.cpp:520-560`); consumer
`WarheadTypeExt::Detonate` (`Ext/WarheadType/Detonate.cpp:113-118`).

## Behavior

- Types are `Parser<Powerup>` names from the fixed nineteen-entry table at
  `0x007E523C`; an unknown or blank name, `<default>`, or the first missing
  numbered entry ends the list. Entry 0 also tries the unnumbered aliases.
- The parallel weight list uses `Nullable<int>`; entry 0 also tries
  `SpawnsCrate.Weight`; an unset weight is 1.
- At detonation with a firing House (native's `if (pHouse)` encloses the
  block), one Scenario `RandomDouble` feeds
  `GeneralUtils::ChooseOneWeighted` over the weights; an index inside the type
  list calls `MapClass::PlacePowerupCrate(Cell(Coord2Cell(coords)), type)`.
  The crate's overlay data byte stores the powerup slot, which the pickup
  reads as its selection. A house-less detonation draws nothing.
- Ordering: after the `TransactMoney` block and before `LaunchSW`
  (`Detonate.cpp:102-150`).

## Where VERA20K owns it

`rules::phobos_spawns_crate::PhobosSpawnsCrate` on the warhead feeds
`sim::combat::crit::apply_spawns_crate`, which reuses
`sim::crates::place_crate` (Place_Crate at `0x0056BEC0`, fixed powerup) and
the shared `ChooseOneWeighted` implementation. Rules-layer tests cover the
numbered/alias reader; the consumer test places a weighted crate with the
exact draw order (pick, then the crate-timer draw).

RESIDUAL: with no overlay registry (mapless fixtures only) the pick is still
drawn and the placement is skipped; native always has a registry.
RESIDUAL — cross-layer aliases: VERA reads the merged warhead section once, so
a later layer's `SpawnsCrate.Type` alias is ignored when an earlier layer set
`SpawnsCrate0.Type`, and a later invalid/`<default>` value yields the merged
value native's per-pass reads would retain. Trigger = a mode/map layer that
overrides individual SpawnsCrate keys; frequency = rare; risk = a different
crate list, not a crash.
