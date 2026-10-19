# Optional Ares superweapon building gates

Original implementation by **baixiong443**, who attributes the availability
logic and regression scenarios to their earlier ra2-mono work and contributes
this change under VERA's GPLv3. That donor repository was unavailable during
review; its source and history have not been independently verified.
The gates compose with existing superweapon types, including GenericWarhead.

```ini
[BlackWidowSpecial]
Type=SpyPlane
SW.AuxBuildings=GASCEA,GASCPF
SW.NegBuildings=GAMERC

[BlackWidowAlphaSpecial]
Type=SpyPlane
SW.AuxBuildings=GAMERC
```

The normal weapon needs either GASCEA or GASCPF and is unavailable while its
owner has GAMERC. The alternative needs GAMERC. Both still need a building
providing their `SuperWeapon=` / `SuperWeapon2=` grant. Native `AuxBuilding=`
remains a separate AND requirement. An omitted extension list retains its
earlier rules-layer value; `none` clears it. Each nonempty authored list replaces
the prior list and binds only BuildingTypes registered at that read point.
Unknown names are skipped and never create a type. A later type registration
does not retroactively resolve an earlier miss.

## Port provenance and adaptation

Contributor-reported source files (unavailable for independent inspection):

- `src/Ra2.Logic/GameLogicManager.SuperweaponAvailability.cs`:
  `IsSuperweaponBlockedByMissingAuxBuildings` and
  `IsSuperweaponBlockedByNegativeBuildings`.
- `src/Ra2.Tests/GameLogicManagerTriggerSuperweaponTests.cs`:
  `UnitDeliverySuperweapon_AuxBuildings_BlockUseBeforeCostAndCharge` and
  `BlackWidowStructureBoundSuperweapons_FilterMutualExclusiveUpgradeState`.

The test's original UnitDelivery payload and
extended building `SuperWeapons` list are not ported here: synthetic
PsychicReveal types and the two existing provider slots exercise the same
availability/charge transaction and mutual-exclusion mechanism without those
unimplemented dependencies.

The contributor describes the C# source as enumerating ready/live owned structures. This adaptation
uses VERA's existing authoritative `HouseTracking::active_count` (the same
owner as native `AuxBuilding`), matching Ares's owned-and-present query. No
new building index, periodic world scan, persistent availability cache, or
power/online requirement is introduced. Constructed limbo objects do not count;
ordinary buildings on the map do, including ones without power. Building
upgrades are not independent prerequisite buildings.

Compatibility reference: [Ares availability documentation](https://ares-developers.github.io/Ares-docs/new/superweapons/availability.html)
and the auxiliary/negative predicates in
[`SWTypeExt::ExtData::IsAvailable`](https://github.com/Ares-Developers/Ares/blob/master/src/Ext/SWType/Body.cpp).
These establish the tag meaning. The Ares pointer-vector reader uses `Find`,
not `FindOrAllocate` (`Utilities/TemplateDef.h` and `Utilities/Parser.h` at Ares
commit `4f1d929920aca31924c6cd4d3dfa849daa65252a`). No C++ implementation is imported.

## Integration and boundaries

The shared building predicate feeds the existing provider grant/revoke/hold
passes and detection EVA eligibility. Building placement, limbo/removal and
ownership changes continue through their existing House dirty-tail owner.
The extension also rejects final fire commands before paired-Super mutation,
charge consumption, launch effects or defense alerts, and rejects AI target
search before any target-search RNG. This command preflight is an explicit
ra2-mono safety adaptation, including for trigger-granted instances; it is not
a claim of executable Ares parity for forced/scripted launches. Trigger grants
retain VERA's existing non-losable/one-time flags and sidebar lifecycle.

When the extension is absent, these new command/AI checks are skipped and no
extension hash input is added. Native stock firing,
charge, power and provider behavior is otherwise unchanged. Resolved extension
lists participate in the simulation configuration hash and persisted processed
rules; availability itself is recomputed from the restored House counts.
The snapshot format advances from 438 to 439 because persisted processed type
records have a new optional field; older binary records must be rejected rather
than silently decoded with the new positional layout.

Not included: `SW.RequiredHouses`, `SW.ForbiddenHouses`, `SW.AllowPlayer`,
`SW.AllowAI`, `SW.AlwaysGranted`, shot limits, provider upgrades, UnitDelivery,
or changes to the existing payload implementations and building animations.

Focused regressions are in `sim::superweapon::building_requirements_tests`.
They require no retail assets and exercise production rules processing, House
grant/revoke, ownership/limbo, command dispatch, AI preflight and save/load.
They do not establish native executable parity, a full-suite pass, or a live
Mental Omega/VERA playtest.
