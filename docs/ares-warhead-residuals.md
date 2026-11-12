# Optional Ares warhead, techno and AI residuals

Build with `--features ares`. Default builds keep their behaviour. These are
the small mechanisms the phase-1 handoff left between the chains.

```ini
[General]
TeamRetaliate=yes          ; a formed AI team takes its attacker as its focus
CanMakeStuffUp=yes         ; April fools: Rules redrawn, infantry panic
[GAPILL]                   ; BuildingType
AIInnerBase=yes            ; the AI places it with the inner-base key
EVA.Lost=EVA_StructureSold ; spoken when the player's building dies
[E1]                       ; TechnoType
EVA.Lost=none              ; a Foot loss stays silent (default EVA_UnitLost)
DontScore=yes              ; ... as does any DontScore/Insignificant type
```

## Where VERA20K owns it

- `sim/game_entity/emp.rs`: `ares_cell_spread_items`, the one port of
  getCellSpreadItems (`10076660`) with includeInAir, which applyIronCurtain
  (`10052890`), applyAttachedEffect (`10052700`) and CreateEMPulse
  (`10062240`) all pass: the cell lists, then every FootClass object whose
  Location is within `CellSpread * 256` and whose GetHeight is above 0; the
  final GetCoords test halves an in-air Aircraft's distance.
- `sim/powered_by.rs`: EnableEMPEffect2 (`10062980`) takes a Foot out of its
  team (`leave_team`, `0x006EA870`), reselects a selected victim after
  Deactivate, and runs updateSpawnManager (`10063070`) and the slave
  manager's SuspendWork; DisableEMPEffect2 (`10062650`) runs
  updateSpawnManager and ResumeWork. updateSpawnManager keys on the EMP
  counter (Techno+0x504), so a PoweredBy deactivation alone resumes, not
  pauses, the spawn timers, as in Ares. TechnoExt::Destroy (`10044300`) is
  `ares_techno_destroy`.
- `sim/team_script_vm/team_ai.rs`: TeamClass_AttackedBy_Retaliate
  (`1003A180`, hook `0x6EB432`) replaces the dead tail of
  `TeamClass::Took_Damage @ 0x006EB380`. IsCloseEnoughToAttackCoords
  (vt+0x3B0, `0x006F7930`) is CanFireAt of the cell at the centre's
  GetCoords with SelectWeapon's choice (`techno_can_fire_at_target`).
- `sim/ai_base_site.rs`: HouseClass_FindPlaceToBuild_Evaluate (`10023860`,
  hook `0x506306`): `AIInnerBase.Get(CloakGenerator)` selects the inner-base
  key (`0x00506329..0x0050640D`, `SiteKey::InnerBase`): the cell centre's
  Sqrt_Approx distance from the base-or-starting cell's coordinate.
- `sim/combat/techno_death.rs`: FootClass_Destroyed (`10049080`, hook
  `0x4D98C0`) in `death_announcement_event`: DontScore, Insignificant and
  Spawned are silent, the line is the type's `EVA.Lost` (a rules set never
  bound to the EVA catalog takes Initialize's `EVA_UnitLost`), and it plays
  whether or not the radar event is admitted
  (`app/match_runtime/sound_dispatch.rs`). BuildingClass_Destroyed_UnitLost
  (`10013F10`, hook `0x44D760`) heads `building_death_announcement`, the one
  port of `BuildingClass::Death_Announcement @ 0x0044D760`, which the
  Temporal erase and, under Ares, BuildingClass_ReceiveDamage_Destroyed
  (`10014520`, hook `0x44266B`, after DestructionEffects) call.
- `sim/ares_make_stuff_up.rs`: CanMakeStuffUp. ValidateThings
  (`100677B0`, hook `0x687C16`) runs from `finalize_scenario_post_map`
  after the Secret Lab pass, in every mode: its 25 to 28 Rules draws plus
  the ten draws of the log's "Gremlins" shuffle, in the DLL's order, kept
  as `AresMadeUpRules` (saved with the Simulation and in its state hash).
  LogicClass_Update_1000 (`100680F0`, hook `0x55AFB3`) heads the master
  frame's Logic: every 90th frame each standing InfantryClass row draws
  `RandomRanged(0, 99)` (below 8 Panic `0x00521C10`, below 12 UnPanic
  `0x00521C40`), and a paradropping one above 415 leptons drops its canopy
  and falls as a bomb with FallRate 0.
- `sim/game_entity/emp.rs`: the EMP delivery (`10062320`) now takes
  Fly and Jumpjet victims and spawners; enableEMPEffect (`10062B40`) runs
  updateSpawnManager with the firer, so the rocket kill is live
  ([ares-emp.md](ares-emp.md)).

## Hooks

| Hook | Ares.dll | Name | Disposition |
|---|---|---|---|
| (helper) | `10076660` | Helpers::Alex::getCellSpreadItems | ported: the in-air FootClass pass and half distance |
| (helper) | `10062980` | EMPulse::EnableEMPEffect2 | ported: team, reselection, spawn and slave updates |
| (helper) | `10062650` | EMPulse::DisableEMPEffect2 | ported: spawn and slave updates; its Foot recovery is the EMP tail's owner |
| (helper) | `10063070` | EMPulse::updateSpawnManager | ported |
| `6EB432` | `1003A180` | TeamClass_AttackedBy_Retaliate | ported |
| `506306` | `10023860` | HouseClass_FindPlaceToBuild_Evaluate | ported (default builds keep the ordinary key for `CloakGenerator=`) |
| `4D98C0` | `10049080` | FootClass_Destroyed | ported |
| `44D760` | `10013F10` | BuildingClass_Destroyed_UnitLost | ported |
| `44266B` | `10014520` | BuildingClass_ReceiveDamage_Destroyed | ported |
| `51E748` | `10049710` | InfantryClass_GetActionOnObject_NoSelfGuardArea | residual, below |
| `687C16` | `100677B0` | CanMakeStuffUp's Rules randomisation | ported: draws and saved values; consumers residual, below |
| `55AFB3` | `100680F0` | LogicClass_Update_1000 | ported |
| `6FFEC0` | `10056D30` | TechnoClass_GetActionOnObject_IvanBombsA | ported: `Simulation::bomb_detonate_action`, asked ahead of the berserk and warp exits ([ares-ivan-bomb-abductor.md](ares-ivan-bomb-abductor.md)) |

## Residuals

- `NoSelfGuardArea`: the stock arm it skips, a player's healer infantryman
  over itself answering GuardArea (0x1A, the self test between hooks
  `0x51E710` and `0x51E748` of `InfantryClass::What_Action_OnObject @
  0x0051E3B0`), is in
  neither build's input ladder: a lone medic clicked on itself answers
  NoMove. Porting the arm changes default-build input, so both wait for it.
- `CanMakeStuffUp`'s Rules readers: `AresMadeUpRules` answers each value
  the DLL wrote (VeteranRatio, BuildSpeed, BuildupTime, RefundPercent,
  GrowthRate, Stray, FlightLevel, both fall rates, the three crews,
  HoverHeight, WindDirection, MaximumQueuedObjects, MaxWaypointPathLength,
  CruiseHeight, four colours, PoseDir, DeployDir, Gravity, IvanTimedDelay,
  PlayerAutoCrush, PlayerReturnFire, PlayerScatter), but the ~110 Rust read
  sites, many in free functions without the Simulation, still read the
  authored Rules. Trigger: `CanMakeStuffUp=yes`, set by no retail INI.
  Effect: the authored values stay in force; the draws, their order and the
  infantry pass match the DLL.
- EMP: a Tunnel mover stays unsupported; every other Foot and every
  EMP-prone building is covered ([ares-emp.md](ares-emp.md)).
