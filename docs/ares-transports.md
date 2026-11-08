# Optional Ares transports, payloads and conversions

Requires an explicit `--features ares` build. Default builds do not read or execute
these extensions; see [build capability](ares.md).

```ini
[APC]
Passengers=5
InitialPayload.Types=E1,ENGINEER
InitialPayload.Nums=4,1

[DEPLOYER]
IsSimpleDeployer=yes
Convert.Deploy=DEPLOYED
```

## Ported

- `InitialPayload.Types` (TechnoTypeExt +0x104, ValueableVector<TechnoTypeClass*>
  1000EDD0, Find only) and `InitialPayload.Nums` (+0x110, ValueableVector<int>
  1000ED00), read after `Operator` (`rules/ares_techno_type.rs`). An object makes its
  payload once, `TechnoExt::ExtData::CreateInitialPayload` 10043C10
  (`sim/ares_transports.rs`):
  - FootClass::Unlimbo makes it for a Unit or Aircraft (FootClass_Put_InitialPayload
    100491B0, hook 0x4D718C, `world/lifecycle.rs`), BuildingClass::Place past the
    free unit for a building (BuildingClass_Place_InitialPayload 10048EB0, hook
    0x446EE2, `building_art.rs`).
  - PayloadCreated (TechnoExt +0x9E) is per-object state (`InitialPayloadState`,
    saved, hashed while pending). TechnoClass_Init_1 (1004A530, hook 0x6F407D) sets
    it for an object constructed inside the A8E7AC scope after frame 0
    (`world/world_spawn/construction.rs`).
  - Free slots, the per-entry count (last count repeats, 1 without any, signed cap),
    the BuildingType/AircraftType skip and the infantry-only building rule follow
    the DLL. A garrison appends occupants and refreshes its threat; an absorber marks
    the infantry absorbed; another building hears HELLO, then takes it through
    CargoClass with AbortMotion; a Foot transport sets the rider's Location,
    OpenTopped rider, Transporter and AddPassenger with VoicesEnabled cleared (no
    EnterTransportSound; a Gunner transport takes its gunner).
- `Convert.Deploy` (+0x60C): UnitClass_Mi_Unload_SimpleDeployer 1004C070 (hook
  0x73DE90) converts a deployed simple deployer in place (ConvertToType 10043650,
  `Simulation::convert_techno_type`) and clears Deployed; every simple deployer's
  Unload then waits (return 1 at 0x73E5B1) while its locomotor is moving
  (`sim/unit_simple_deploy.rs`).
- Hospital/Armory eviction: `BuildingExt::KickOutHospitalArmory` 10012920 has one
  owner (`Simulation::ares_kick_out_hospital_armory`), called from Mission_Unload
  (100145B0, hook 0x44D8A1, ported earlier), every Sell_Back (10016970, hook
  0x447113, `production/production_sell.rs`) and ChangeOwner before the owner
  changes (100163B0, hook 0x448277, `world/mod.rs`). The prism and bomb parts of
  the last two belong to [ares-prism-forwarding.md](ares-prism-forwarding.md).

Tests: `sim::ares_transports::tests` (transport and garrison payload, armory capture
and sale), `sim::unit_simple_deploy::tests::ares_simple_deployer_converts_to_its_convert_deploy_type`,
and the reader test in `rules::ares_techno_type`.

## Hooks and keys left

| Hook | Ares.dll | Name | Disposition |
|---|---|---|---|
| `457DB7` | `10016FD0` | BuildingClass_CanBeOccupied_SpecificAssaulters | residual: `Assaulter.Level` refines the stock `Assaulter=` arm of CanBeOccupiedBy (0x457DAD..0x457DD3), and the assault itself (AddGarrisonOccupant 0x522A09..0x522A45: occupants killed, the assaulter scatters) is not ported in either build; porting them changes default builds. No retail infantry is an assaulter |
| `417D75` | `10048A60` | AircraftClass_GetActionOnObject_CanTote | residual: `Carryall.Allowed`/`Carryall.SizeLimit` gate the Tote action; VERA has no Carryall tote (Mission_Move_Carryall 0x416D50, `aircraft/move_mission.rs`). No retail aircraft is a Carryall |
| `416E37` | `10048B00` | AircraftClass_Mi_MoveCarryall_CanTote | residual: as 0x417D75 |
| `728EF0` | `10047DD0` | TunnelLocomotionClass_ILocomotion_Process_Dig | residual: `DigIn`/`DigOut`/`DigInSound`/`DigOutSound` (PlayDigEffects 100443E0) need the Tunnel locomotor, which VERA does not have (TS legacy, unused by retail YR) |
| `7292CF` | `10047DF0` | TunnelLocomotionClass_sub_7291F0_Dig | residual: as 0x728EF0 (also the dig timer write) |
| `7293DA` | `10047E20` | TunnelLocomotionClass_sub_729370_Dig | residual: as 0x728EF0 |
| `7297C4` | `10047E40` | TunnelLocomotionClass_sub_729580_Dig | residual: as 0x728EF0 |
| `7299A9` | `10047E60` | TunnelLocomotionClass_sub_7298F0_Dig | residual: as 0x728EF0 |

The other Carryall hooks (0x415DF6, 0x416C3A, 0x416C4D, 0x416C94, 0x416CF4) are in
[ares-techno-hooks.md](ares-techno-hooks.md).

Residuals inside the ported mechanisms:
- CreateInitialPayload writes the occupant's TechnoExt `GarrisonedIn`, which VERA
  does not keep (see `sim/ares_garrison.rs`); the occupied building's
  UpdatePlacement(Redraw) is presentation.
- Convert.Deploy goes through the shared conversion owner, which does not refresh
  passenger capacity and the other construction-only state (its RESIDUAL in
  `game_entity/type_conversion.rs`).
