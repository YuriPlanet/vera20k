# Building repair-service overrides

Phobos lets a building type reshape the repair service it offers ground units
(`Ext/BuildingType/Body.cpp:228-231`, hooks in `Ext/Building/Hooks.cpp:579-668`).

```ini
[DEPOT]                       ; BuildingType
Units.RepairRate=             ; double, default [General] IRepairRate/URepairRate
Units.RepairStep=             ; integer, default [General] IRepairStep/RepairStep
Units.RepairPercent=          ; double, default [General] RepairPercent
Units.UseRepairCost=          ; bool, default = the techno is not Infantry
```

## Behavior

The hospital arm (`0x44B8F1`) and the unit-repair arm (`0x44BD38`) use
`Units.RepairRate` instead of `[General] IRepairRate=`/`URepairRate=` for the
`rate * 900`-frame RepairTick threshold. On the repair command (`0x6F4D1A`),
`Units.RepairStep` replaces the techno's repair step (the passed
`IRepairStep`/`RepairStep`), `Units.RepairPercent` scales the cost, and
`Units.UseRepairCost` decides whether the serviced techno pays. The cost is
`int((Cost / (Strength / step)) * percent)`, at least 1, and infantry pay
nothing by default.

A `UnitReload=` building with `Units.RepairRate` set runs its own repair loop
(`0x44C836`): every `int(max(rate * 900, 1))` frames it asks each linked,
non-airborne, damaged unit QUERY_MOVING and, answered ROGER, applies the repair
command's effects. The stock arm's own REPAIR_TICK effects are skipped on such
a building outside that loop. A negative rate disables the separate loop, and
the stock UnitReload arm runs as before.

## Where VERA20K owns it

`rules::phobos_units` reads the four Nullable fields into the ObjectType.
`docking::hospital` and `docking::building_dock` pick the rate for their
thresholds, `radio::receive::techno_repair_tick` applies the step, percent and
cost switch (with the skip-effects latch before the health refusal), and
`docking::airfield_reload::separate_repair_pass` is the UnitReload loop.

## Residuals

- The separate loop calls the repair-tick body directly; VERA models no
  sender-side radio message bookkeeping for that RequestRepair.
- `Units.RepairRate`'s depot-arm threshold change is the same one-line
  selection as the hospital arm and has no separate regression case; the
  hospital arm's period is covered.
- `Units.RepairRate` set on a type that is not `UnitReload=` only changes the
  two mission thresholds, as in Phobos.
