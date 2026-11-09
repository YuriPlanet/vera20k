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

- Carryall totes over the stock Carryall chain (`sim/aircraft/carryall.rs`):
  `Carryall.Allowed` (TechnoTypeExt +0x2F8, Nullable<bool>) and
  `Carryall.SizeLimit` (+0x2FC, Nullable<int>), read after `ClonedAs`, decide
  `TechnoTypeExt::ExtData::CarryallCanLift` 1003D9E0
  (`Simulation::ares_carryall_can_lift`): no parasite on the Unit, its
  `Carryall.Allowed` else neither `Organic=` nor `NonVehicle=`, and its `Size=`
  within a set, non-negative limit. A refused Unit keeps the base action
  (AircraftClass_GetActionOnObject_CanTote 10048A60, hook 0x417D75) and is flown
  to without the radio exchange (AircraftClass_Mi_MoveCarryall_CanTote 10048B00,
  hook 0x416E37).
- The 3.0 hooks on `Carryall_Unload` 0x416AF0: the cargo comes down facing the
  Carryall's turret with its type's `ROT=`/`TurretROT=` back (Facing 10048920,
  0x416C3A), runs its arrival and powers off when Deactivated (UpdateCargo
  10048A10, 0x416C94), loses its Transporter, takes Guard and rejoins its team
  (Guard 100489D0, 0x416CF4); a refused drop destroys the Carryall
  (DestroyCargo 100488C0, 0x416C4D). A paradropped passenger loses Transporter
  and IsOnCarryall (Paradrop_Carryall 10048B80, 0x415DF6).
- Aircraft passengers over the stock CAN_LOAD arm (`aircraft_can_enter_radio`,
  `sim/radio/receive.rs`): a driverless transport answers 0
  (QueryEnterAsPassenger_KillDriver 10048CF0, 0x41946B) and one whose
  `Passengers.Allowed`/`.Disallowed` refuses the sender NEGATORY
  (SpecificPassengers 10048D10, 0x41949F). A passenger the aircraft unloads
  loses its Transporter (Mi_Unload_Blocked 10048B30, 0x415544, whose refusal arm
  VERA's unload already takes).

Tests: `sim::ares_transports::tests` (transport and garrison payload, armory capture
and sale), `sim::unit_simple_deploy::tests::ares_simple_deployer_converts_to_its_convert_deploy_type`,
`sim::aircraft::carryall::tests::ares_carryall_keys_gate_the_tote`,
`sim::radio::receive::tests::ares_aircraft_transport_refuses_a_disallowed_passenger`
and the reader test in `rules::ares_techno_type`.

## Hooks and keys left

| Hook | Ares.dll | Name | Disposition |
|---|---|---|---|
| `457DB7` | `10016FD0` | BuildingClass_CanBeOccupied_SpecificAssaulters | residual: `Assaulter.Level` refines the stock `Assaulter=` arm of CanBeOccupiedBy (0x457DAD..0x457DD3), and the assault itself (AddGarrisonOccupant 0x522A09..0x522A45: occupants killed, the assaulter scatters) is not ported in either build; porting them changes default builds. No retail infantry is an assaulter |
| `728EF0` | `10047DD0` | TunnelLocomotionClass_ILocomotion_Process_Dig | residual: `DigIn`/`DigOut`/`DigInSound`/`DigOutSound` (PlayDigEffects 100443E0) need the Tunnel locomotor, which VERA does not have (TS legacy, unused by retail YR) |
| `7292CF` | `10047DF0` | TunnelLocomotionClass_sub_7291F0_Dig | residual: as 0x728EF0 (also the dig timer write) |
| `7293DA` | `10047E20` | TunnelLocomotionClass_sub_729370_Dig | residual: as 0x728EF0 |
| `7297C4` | `10047E40` | TunnelLocomotionClass_sub_729580_Dig | residual: as 0x728EF0 |
| `7299A9` | `10047E60` | TunnelLocomotionClass_sub_7298F0_Dig | residual: as 0x728EF0 |

The Carryall and aircraft-passenger hooks are listed in
[ares-techno-hooks.md](ares-techno-hooks.md).

Residuals inside the ported mechanisms:
- CreateInitialPayload writes the occupant's TechnoExt `GarrisonedIn`, which VERA
  does not keep (see `sim/ares_garrison.rs`); the occupied building's
  UpdatePlacement(Redraw) is presentation.
- DestroyCargo damages the Carryall alone; its death kills the cargo it put back
  in the hold, so the cargo takes no receiver of its own (the RESIDUAL on
  `ares_destroy_refused_carryall`).
- Convert.Deploy goes through the shared conversion owner, which does not refresh
  passenger capacity and the other construction-only state (its RESIDUAL in
  `game_entity/type_conversion.rs`).
