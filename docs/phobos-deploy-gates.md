# Deploy gates

Phobos lets a deployable vehicle skip its passenger unload on the deploy path
and the self cursor, skip an empty harvester's tiberium dump, and gate a
simple deployer's own cell by land type and pathfinding. The five UnitType keys
are read in `UnitTypeExt::LoadFromINIFile` (`Ext/UnitType/Body.cpp:55-56,84-86`)
and consumed by `UnitClass_Mi_Unload_Subterranean` (`0x73D63B`,
`Ext/Unit/Hooks.Unload.cpp:12-59`), `UnitClass_Mi_Unload_SkipHarvester`
(`0x73DEEB`, `:61-75`), `UnitClass_MouseOverObject_SkipPassengers` (`0x740015`,
`:77-89`) and `UnitExt::SimpleDeployerAllowedToDeploy` (`Ext/Unit/Body.cpp:258-316`)
through the `TechnoClass_CanShowDeployCursor_UnitsAndAircraft` hook `0x7010C1`
(`Ext/Techno/Hooks.cpp:1452-1474`).

```ini
[DEPLOYER]                          ; UnitType (Vehicle)
Deploy.SkipPassengerUnload=no       ; bool, default no
Deploy.NoPassenger=no               ; bool, default no
Deploy.NoTiberium=no                ; bool, default no
IsSimpleDeployer.ConsiderPathfinding=no             ; bool, default no
IsSimpleDeployer.DisallowedLandTypes=               ; LandType list, unset by default
```

## Behavior

- `Deploy.SkipPassengerUnload` / `Deploy.NoPassenger` make `Mission_Unload`
  skip the transport's passenger unload and continue to its deploy arm. The
  entry hook tests them after the tunnel, tank-bunker and linked/unloading
  harvester arms. `NoPassenger` skips only while the transport holds no live
  passengers AND `MissionStatus == 0` (one status step has run). The self
  cursor (`UnitClass::MouseOverObject`) applies the same two keys without the
  `MissionStatus` test.
- `Deploy.NoTiberium`: at the harvester no-link tail, a harvester that is not
  already unloading (`!Unloading`) skips its dump when `NoTiberium` is unset or
  the hold's total tiberium value is zero. With the key set, an empty hold skips
  and a full hold dumps; without it, every non-unloading pass skips.
- `IsSimpleDeployer.ConsiderPathfinding` opens the own-cell gate.
  `SimpleDeployerAllowedToDeploy(pThis, true, false)` first refuses a cell whose
  land type is in `DisallowedLandTypes` (unset = the stock `Water | Beach` for a
  `DeployToLand` Hover/Jumpjet lander, none otherwise), then requires the cell
  to pass `CellClass::CheckCellPassability @ 0x004834A0` with the deploy type's
  `SpeedType`/`MovementZone` (the `Convert.Deploy` target's under Ares),
  ignoring infantry and vehicles. A missing (`-1`) speed type or movement zone
  admits without the cell test. Without `ConsiderPathfinding` the gate stays
  closed.

## Where VERA20K owns it

`rules::phobos_deploy::PhobosDeploy` reads the five keys for Vehicle types in
`ObjectType::from_section` and owns the gates (`cursor_skips_passengers`,
`mission_skips_passengers`, `disallowed_flags`, `land_type_disallowed`).
`sim::deploy::phobos_unload_skips_passengers` is the one mission passenger-skip
owner: `unit_fires_on_deploy`, `mcv_deploy::is_mcv`,
`unit_simple_deploy::is_simple_deployer`, the transport and fallback-stub arms
of the Unit Unload dispatch, and `can_deploy_slash_unload`.
`sim::miner::refinery_dock::mission_unload` calls
`Simulation::phobos_harvester_skips_tiberium_dump` at its no-radio tail, reading
the hold total through `Miner::cargo_value`. `can_deploy_slash_unload` calls
`phobos_simple_deployer_allowed_to_deploy`, which reuses `check_cell_passability`
with the shared `own_map_cell` and the cell's structural-bridge flag; the
`Convert.Deploy` target is read only in `ares,phobos` builds. The self cursor
skips through `app::input::context_order::phobos_cursor_skips_passengers` and
`world_orders::unit_self_deploy_site_refused`.

## Residuals

- The `LandTypeFlags` reader resolves a comma list to its first token only:
  Phobos calls `GetLandTypeFromName(parser.value())` on the buffer head every
  `strtok` pass (`Utilities/TemplateDef.h:694`), so `Water,Beach` is just
  `Water`. `none` alone parses as the empty set (`Some(0)`, overriding the stock
  lander set); `<none>` and unknown names fail the read and leave the field
  unset.
- The hover/jumpjet deploy-time `SimpleDeployerAllowedToDeploy(pUnit, false,
  true)` callers (`Hooks.SimpleDeployer.cpp:258,323`, hooks `0x54C58E`,
  `0x514A2A`) belong to the separate simple-deployer landing chain; only the
  `CanShowDeployCursor` `(true, false)` caller is wired here.
- `can_deploy_slash_unload` is the single admission owner, so the gate also
  covers AI/scripted deploys that native would reach without the cursor
  function.
- The harvester no-radio skip returns the 450-frame `Mission_Unload` stub
  instead of modeling the native deploy-path `MissionStatus` dispatch; a
  harvester has no deploy arm, so the observable result matches.
