# Harvester area guard

A human-owned harvester normally leaves Area Guard the moment its visit runs:
the Foot body queues Harvest and commences it unconditionally. Phobos lets a
type keep Area Guard, optionally only while a threat is in range.

```ini
[MINER]                                  ; UnitType
Harvester.CanGuardArea=false             ; Valueable<bool>, default false
Harvester.CanGuardArea.RequireTarget=false ; Valueable<bool>, default false
```

Reads: `Ext/UnitType/Body.cpp:34-35`, declarations `Ext/UnitType/Body.h:19-20`,
constructor `false` at `:89-90`.

## Behavior

Native hook `FootClass_MissionAreaGuard_Miner @ 0x4D6D34`
(`Ext/Unit/Hooks.Harvester.cpp:71-90`), inside `FootClass::Mission_AreaGuard`:

- the object must be a Unit whose type sets `Harvester.CanGuardArea` **and**
  whose owner is human-controlled; a computer harvester always takes the
  stock Harvest switch;
- without `RequireTarget` the type keeps Area Guard;
- with it, `TargetAndEstimateDamage(Location, ThreatType::Area)` must find a
  threat. When it does, the handler jumps past the Harvest queue and RNG draw
  (`0x4D6D69`) and continues with the ordinary Area Guard body.

## Where VERA20K owns it

`sim::world::techno_ai::mission_handlers::evaluate_foot_area_guard` owns the
`FootClass::Mission_AreaGuard` visit; the harvester switch is its
`Foot4D6D34..4D6D68` block. The Phobos arm sits immediately before it and
skips that block when the type and owner qualify. `RequireTarget` reuses the
shared threat scanner (`techno_ai::target_scan::scan`) with
`ScanMission::AreaGuard` (mask 2, the same scan the Area Guard body uses) at
the object's `Location`, standing in for `TargetAndEstimateDamage`'s area
query.

The scan entry `0x70982C`'s per-TechnoType targeting delay is part of the
shared scanner (`target_scan::retaliate_and_scan` → `targeting_delay` with
`TargetingMission::AreaGuard`), so the `RequireTarget` scan stamps the same
timer the Area Guard body does. No residual.

Test coverage: the synthetic key/owner/reachability gate, the no-threat
refusal and the scan attempt are covered by
`phobos_harvester_guard_tests`; the retail-rules positive arm
(`require_target_keeps_area_guard_with_a_threat_on_retail_rules`) runs and
passes in this checkout.
