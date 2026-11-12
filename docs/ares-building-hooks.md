# Optional Ares gate, wall, trench, tunnel and foundation hooks

Build with `--features ares`. Default builds keep every stock path named
below. This page covers the hooks of `Ext/Building/Hooks.Gates.cpp`,
`Hooks.Infiltrate.cpp` (the sidebar and AllyWith ones), `Hooks.Sensor.cpp`
(the redraw), `Hooks.Trenches.cpp`, `Hooks.Tunnels.cpp` and
`Ext/BuildingType/Hooks.cpp` (pips, outline, undeploy) that no other page
cited. The table at the end gives each one's disposition.

```ini
[SANDTRCH]                ; BuildingType
IsTrench=Sand             ; a trench kind (case-sensitive name); placement
GuardRange=4              ;   links it to a trench of the same kind

[TunnelTypes]
0=Subway                  ; each value names a tunnel type (any case)

[Subway]
Passengers=8              ; each House's network holds this many (-1: any)
MaxSize=0                 ; read, used by nothing in 3.0

[GATUNNEL]                ; BuildingType
Tunnel=Subway             ; an entrance of the Subway network
SizeLimit=15              ; the largest Size= that may enter
```

## What it does

- **Walls.** The any-wall query of `CellClass::IsWallConnectableInDirection`
  (`0x00480510` with type -1) answers the OverlayType's `Wall=` for any
  index instead of the stock GAWALL, NAWALL and GAFWLL literals (Ares
  `10015740`, hook `0x480534`; `sim::cell_kernel::wall_connection_step`).
  Retail sandbags (GASAND), the civilian fences (CAFNCB, CAFNCW, CAFNCP)
  and the Kremlin wall (CAKRMW) now count: a `Wall=yes` warhead may fire
  at a target behind them (`combat::line_of_fire`), a straight bullet
  stops on them (`world::projectile_collision`) and a bounce stops on them
  (`world::bounce_terrain`). Sparks keep the literals: Ares simulates
  spark systems without LineTrail or AlphaImage itself (ParticleSystemExt),
  which VERA does not port ([ares-anim-particles.md](ares-anim-particles.md)).
- **Gates on walls.** `CellClass::Is_Clear_To_Build` (`0x0047C620`) admits a
  building on a wall overlay only when the placing house owns the wall;
  then any `Gate=` type stands on any `Wall=yes` overlay, and a type stands
  on its own `ToOverlay=` overlay on its damaged stage at any index, not
  only GASAND, GAWALL and NAWALL. Otherwise the `[General]` WallTower and
  gate lists decide as before (Ares `10015780`, hook `0x47C8AB`;
  `sim::build_site`). Retail names GADUMY for every gate; GAGATE_A is a
  `Gate=yes` type with TechLevel -1.
- **Trench lines.** `IsTrench=` (BuildingTypeExt `+0x84`) makes a type
  linkable like a firestorm section: placing it fills the gap up to
  `GuardRange` cells to a same-house or allied trench of the same kind, and
  the placement cursor marks those cells (buildLines `100113D0`,
  IsLinkable `1000D770`; `sim::ares_firestorm`,
  [ares-firestorm.md](ares-firestorm.md)).
- **Trench traversal.** With a trench selected whose occupants could
  fit, the cursor over another building that is `CanBeOccupied=` with
  room, of the same trench kind, and whose Location lies within 256
  leptons (a straight neighbour) shows Enter before any stock action
  (`100171B0`, hook `0x44725F`, canTraverseTo `10011690`); Ares tests no
  owner. The click raises the TrenchRedirectClick event (0x60, ten bytes:
  the target building's Cell and the source building; `10016F60`, hook
  `0x443414`, RaiseTrenchRedirectClick `1006A170`), whose handler
  (`1006A540`) asks canTraverseTo again for the Cell's building and
  moves `min(free slots, occupants)` occupants head first, restarts the
  source's firing index past the count and returns an emptied raided
  source to its former owner (doTraverseTo `100130E0`;
  `sim::ares_garrison`, `app::input::context_order`, `app::input::cursor`).
- **Passenger filters.** `Passengers.Allowed` and `Passengers.Disallowed`
  (TechnoTypeExt `+0x4AC`/`+0x4B8`, Find-only TechnoType lists) gate a
  carrier's CAN_LOAD by the sender's type (IsPassengerAllowed
  `1003DD60`): an empty Allowed list admits any type, and Disallowed
  refuses. Every building answers NEGATORY past its power test
  (`100174E0`, hook `0x43C326`: tunnels, absorbers, bunkers, grinders,
  depots, hospitals alike), and a Unit transport at its ROGER (`1004C270`,
  hook `0x73769E`; `sim::radio::receive`). Garrisons are not gated: their
  occupiers enter through `CanBeOccupiedBy` (`sim::ares_garrison`).
- **Tunnels.** A building with `Tunnel=` is an entrance of its owner's
  network of that type (HouseExt `+0x64`, `sim::ares_tunnel`, saved and
  hashed). CAN_LOAD admits a Foot while the network has room under
  `Passengers=` and its `Size=` is within the building's `SizeLimit=`,
  skipping the contact-capacity gate, and refuses a controller or a
  mind-controlled sender with a capture manager (`100174E0`); DOCK_NOW
  answers ROGER (`100176B0`). Infantry may click any entrance
  (`100176E0`); a unit's order parks it as a pending entry and its cell
  entry admits the building like a `UnitAbsorb=` one (`10017830`,
  `100177F0`). On arrival infantry enter the building owner's network and
  units their own House's (`10017760`, `10017870`): focus, gattling and
  captor dropped, `EnterTransportSound=` played, limboed
  (EnterTunnel `10009000`). Self-clicking an entrance whose network holds
  a Foot unloads it (`100173A0`), last in first out, one per Unload
  dispatch (UnloadOne `100098A0`, `sim::transport_unload`). A
  `PipScale=Passengers` entrance's passenger pips show the network, as many
  as GetPipMax and the tunnel's MaxCap allow (GetPassengerPips `10044C90`:
  an infantryman's `Pip=`, else frame 4; `ui_overlays::scale_pip_draws`). When the last
  entrance (no other alive, unlimboed building of the type that is neither
  built nor sold) dies or is erased, the network dies with it, credited
  to the C4 planter or the eraser (`100093C0`); when it is sold, every
  member is put out on its cell and scattered (PlaceOne `10009810`).
- **Custom foundation pips.** A building on a `Foundation=Custom` foundation
  answers its `Foundation.X` width as its pip count, whatever its
  `PipScale=` (Ares `10010150`, hook `0x45ECE0`; `ObjectType::pip_max`).

## Not included

- Ares' wall test in the `Gate=` arm of `BuildingClass::Unlimbo @ 0x00440580`
  (hook `0x440709`). VERA ports the stock arm (a placed gate sells its
  house's walls under its foundation, and the WallTower arm at
  `0x004405BC`): `sim::world::world_spawn::wall_unlimbo`. It tests the
  stock overlay ids 0, 2 and 0x1A; Ares' hook, which admits any `Wall=`
  overlay there, is not ported, so under Ares a mod wall stays under a
  gate. No retail buildable type is a gate or WallTower.
- Trench traversal: the cursor and click rows are those of a selection of
  buildings only; a mixed selection with a trench does not offer them,
  and Ctrl+Alt over a friendly trench keeps its stock empty answer. The
  occupant's TechnoExt `GarrisonedIn` is not kept (nothing in 3.0 reads
  it); VERA moves the occupant's carrier link instead. Online, VERA's
  network gate refuses an event whose source building is another
  House's, where Ares trusts it.
- Tunnels: the building click that radios a clicked unit (`0x10`, hook
  `0x44351A`, `100172F0`) has no host, since VERA does not port the
  absorbers' arm of `BuildingClass::ClickedAction_Object` (no RESERVE_DOCK
  radio, `sim::radio`); EnterTunnel clears OnBridge after Limbo instead of before (as
  `board_passenger` does); UnloadOne keeps a member whose nearest-cell
  search or Unlimbo fails, where native puts a vehicle on cell (0, 0) or
  drops it.
- `Passengers.Allowed`/`.Disallowed` also gate an aircraft's hold
  (`10048D10`, hook `0x41949F`), which VERA does not represent (no retail
  aircraft carries passengers).
- The stock BuildingType override of GetPipMax (`0x0045ECE0`: Tiberium and
  Power pips from `width * 60 / 8`, capped by `Storage=` or
  `WeedCapacity=`) is not in VERA either; buildings take the TechnoType
  answer (`0x00716290`), so a retail refinery's pip row has 5 slots where
  native has 30 (GAREFN, NAREFN) or 15 (YAREFN). Default builds share
  this gap.

## Hooks

| Hook | Ares.dll | Name | Disposition |
|---|---|---|---|
| `0x440709` | `100156F0` | BuildingClass_Put | residual: the stock gate arm is ported with the stock wall ids; the hook's wall test is not (above) |
| `0x47C8AB` | `10015780` | CellClass_CanThisExistHere_GateOnWall | ported: `sim::build_site::overlay_admits` |
| `0x480534` | `10015740` | CellClass_AttachesToNeighbourOverlay | ported: `sim::cell_kernel::wall_connection_step` |
| `0x4574D2` | `10015E70` | BuildingClass_Infiltrate_Standard | not applicable: a sidebar repaint; VERA redraws the sidebar every frame |
| `0x457533` | `10015E70` | BuildingClass_Infiltrate_Standard | not applicable: as above |
| `0x509303` | `10015EE0` | HouseClass_AllyWith_unused | not applicable: gamemd `0x005092F0` has no caller |
| `0x4557BC` | `10016DF0` | BuildingClass_SensorArray_BuildingRedraw | not applicable: marks a cloaked enemy building for redraw; VERA redraws every frame |
| `0x455923` | `10016DF0` | BuildingClass_SensorArray_BuildingRedraw | not applicable: as above |
| `0x443414` | `10016F60` | BuildingClass_ActionOnObject | ported: `app::input::context_order::building_object_orders` raises event 0x60 (`AresTrenchRedirectRecord`; handler `Simulation::ares_respond_to_trench_redirect`); its Deactivated arm is `Simulation::click_order_receivers` ([ares-techno-hooks.md](ares-techno-hooks.md)) |
| `0x44725F` | `100171B0` | BuildingClass_GetActionOnObject_TargetABuilding | ported: `app::input::cursor::capability_cursor_for_hover` and the click release, through `Simulation::ares_can_traverse_to` |
| `0x4C6CCD` | `1006A540` | Networking_RespondToEvent | ported for 0x60: `decode_native_command_record` keeps the record raw and `apply_command` runs `Simulation::ares_respond_to_trench_redirect`; the 0x61 FirewallToggle arm has no raiser ([ares-firestorm.md](ares-firestorm.md)) |
| `0x64B704` | `1006A5F0` | sub_64B660_PayloadSize | ported: `net::event_packet::event_payload_len` answers 10 for 0x60 (GetEventPayloadSize `1006A130`); 0x61 has no raiser |
| `0x64BE83` | `1006A620` | sub_64BDD0_PayloadSize1 | ported: as above, the one payload table serves both directions |
| `0x64C314` | `1006A660` | sub_64BDD0_PayloadSize2 | ported: as above |
| `0x4586CA` | `10017220` | BuildingClass_KillOccupiers_EachOccupierKilled | ported: `passenger::garrison_eject::kill_building_occupants` runs evalRaidStatus before each assaulted occupant's kill; the vector still holds them all there, so only the all-killed hook `0x458729` can return a raided building |
| `0x43C326` | `100174E0` | BuildingClass_ReceivedRadioCommand_QueryCanEnter_Tunnel | ported: `sim::radio::receive::can_load`, IsPassengerAllowed included |
| `0x43C716` | `100176B0` | BuildingClass_ReceivedRadioCommand_RequestCompleteEnter_Tunnel | ported: `sim::radio::receive` DOCK_NOW |
| `0x44351A` | `100172F0` | BuildingClass_ActionOnObject_Tunnel | residual: the absorbers' click arm it extends is not in VERA |
| `0x44731C` | `100173A0` | BuildingClass_GetActionOnObject_Tunnel | ported: `app::input::context_order` self click, `Command::UnloadPassengers` admission |
| `0x44D8A7` | `100174A0` | BuildingClass_Mi_Unload_Tunnel | ported: `transport_unload::ares_tunnel_mission_unload` |
| `0x442DF2` | `10017350` | BuildingClass_Demolish_Tunnel | ported: `crew_survival::spawn_building_survivors` |
| `0x44A37F` | `100173E0` | BuildingClass_Mi_Selling_Tunnel | ported: `production_sell::sale_survivors` |
| `0x71A995` | `100177B0` | TemporalClass_Update_Tunnel | ported: `temporal` erase |
| `0x73A23F` | `10017870` | UnitClass_UpdatePosition_Tunnel | ported: `movement::per_cell::unit_per_cell_absorber` |
| `0x73F606` | `100177F0` | UnitClass_IsCellOccupied_Tunnel | ported: `world::object_entry` |
| `0x741CE5` | `10017830` | UnitClass_SetDestination_Tunnel | ported: `movement::track_path` |
| `0x51ED8E` | `100176E0` | InfantryClass_GetActionOnObject_Tunnel | ported: `world::world_orders::entry_click_action` |
| `0x51A2AD` | `10017760` | InfantryClass_UpdatePosition_Tunnel | ported: `movement::per_cell::infantry_per_cell_enter` |
| `0x45ECE0` | `10010150` | BuildingTypeClass_GetMaxPips | ported: `ObjectType::pip_max` |
| `0x465550` | `10010120` | BuildingTypeClass_GetFoundationOutline | equivalent: gamemd `0x00465550` has no caller; the outline consumers read `Foundation::custom_outline` (`docking::building_dock`) |
| `0x465D4A` | `100101D0` | BuildingTypeClass_IsUndeployable | equivalent: `ObjectType::is_1x1_with_undeploy` reads `Foundation::dimensions`, the custom width and height |
| `0x483D94` | `10014B70` | CellClass_UpdatePassability | ported: `map::resolved_terrain::recalc_zone_type_with_buildings` in SetFirestormState's zone batch (`Simulation::ares_firestorm_update_pathfinding`); other Recalc writers residual ([ares-firestorm.md](ares-firestorm.md)) |
