# Optional Ares TechnoClass hooks: healers, deploy facing, gaps and more

Build with `--features ares`. Default builds keep every stock path named
below. This page covers the hooks of `Ext/Techno/Hooks.cpp`,
`Hooks.Deactivated.cpp`, `Hooks.Gap.cpp`, `Hooks.Mechanical.cpp`,
`Hooks.Spotlights.cpp` and `Ext/TechnoType/Hooks.Weapons.cpp`; the table at
the end gives each one's disposition. Hooks that `ares-techno-types.md`,
`ares-cloak-power.md`, `ares-veterancy.md` and the other pages already
describe show as "ported earlier".

```ini
[MyTank]                     ; TechnoType
TurretROT=4                  ; secondary (turret) turn rate, else ROT
Passengers.BySize=no         ; the hold counts heads, not Size=
ImmuneToBerserk=yes          ; overrides ImmuneToPsionics vs Psychedelic
VeteranAbilities=RADIMMUNE   ; also POISONIMMUNE, PSIONICWEAPONIMMUNE
DeployDir=2                  ; 0..7, the facing a DeployingAnim deployer takes
VoiceRepair=MyRepairVoice    ; attack voice of a repairing weapon
GapRadiusInCells=10          ; read as a whole int
SuperGapRadiusInCells=12     ; read as a whole int
HasSpotlight=yes             ; any Techno, made at Unlimbo
```

## Healers

Every healing weapon (`GetWeaponDamageValue` below zero) scans the
infantry, vehicle and aircraft classes (`or ebx,403Ch`, 1004E4A0, hooks
0x6F8EE3/0x6F8F1F), a healing vehicle accepts any damaged ally in
Evaluate_Candidate (1004E490, 0x6F7FC5), and a computer house's healer keeps
an allied target and lets a non-allied one go (1004E4C0, 0x6FA361). The fire
errors accept any damaged Techno (Unit 1004E540, 0x741113; Infantry
1004E270, 0x51C913) and the ILLEGAL firing state keeps it (1004E6E0,
0x736E8E; 1004E420, 0x520731). A moving turreted Unit holding a
TemporalClass fires unless its SelectWeapon(NULL) weapon is Temporal
(1004BC80, 0x741206), so an IFV with a Chrono gunner still moves and fires
its other modes.

## Combat, transports and voices

ReceiveDamage's retaliation turns on the open-topped transport of a riding
attacker (1004AD10, 0x702A38). The rank abilities RADIMMUNE, POISONIMMUNE,
PSIONICWEAPONIMMUNE and PSIONICSIMMUNE join the type flags in the receiver
(1004A950, 0x701BFE); RADIMMUNE also skips the Foot radiation pulse
(10049490, 0x4DA584); `ImmuneToBerserk` overrides the Psychedelic immunity
in the receiver and the fire error (10049EF0, 0x6FC417). A
`Passengers.BySize=no` Unit hold compares heads with `Passengers=` in its
three room tests, keeping the SizeLimit test (1004C150/1004C1D0/1004C210).
A cloaking carrier's own Detach_All no longer kills its docked spawns
(1004A910, 0x707B19). The Temporal update frees its target's JammerClass
(1004BB60, 0x71A84E). A MindControl pip scale counts the first MindControl
weapon's Damage (1004B910, 0x7162B0). The attack voice takes `VoiceRepair`,
then the case-sensitive `FV` VoiceIFVRepair, then the rank override with an
elite-to-rookie fallback, then a `RandomRanged` Main draw over VoiceAttack
(1004B1D0, 0x7090A8).

## Movement, deploy and spotlights

`TurretROT` sets the secondary facing's rate at Unit and Aircraft
construction (1004BC40, 0x735584; 10048AC0, 0x413FFA). A simple deployer
with a `DeployingAnim=` faces its `DeployDir` (else `[AudioVisual]`) in
eighth turns before the animation starts (1004C2D0, 0x739B8A), a landing
jumpjet turns only with a DeployingAnim (100499F0, 0x54C767), and an airborne
deployer without DeployToLand deploys where it is (1004C380, 0x739ADA).
Every `HasSpotlight=` Techno gets its BuildingLight at TechnoClass::Unlimbo
(1004EDE0, 0x6F6D0E/0x6F6F20), replacing the building's own construction
(1004EA10, 0x441163). Gap radii are the whole TechnoTypeExt ints
(1004D330, 1004D250, 1004D170, 1004D1B0).

## Where VERA20K owns it

- `sim/combat/threat_mask.rs`, `greatest_threat.rs`: healer classes and
  candidates; `sim/world/techno_ai.rs`: the allied-target drop.
- `sim/combat/fire_error.rs` and `fire_error_world.rs`: U6, U10, I2 and
  T26/T27; `sim/combat/world_fire.rs`: the ILLEGAL firing state.
- `sim/combat/world_receiver.rs`: retaliation source and Foot radiation;
  `sim/combat/receiver_call.rs`: the receiver immunities.
- `rules/ares_techno_type.rs`: `ImmuneToBerserk`, `DeployDir`;
  `rules/ares_abilities.rs`: the rank immunities; `rules/object_type.rs`:
  gap radii, pip max, `VoiceRepair`.
- `sim/radio/receive.rs`: BySize; `sim/world/lifecycle.rs`: spotlight
  construction and the spawn-manager forward; `sim/temporal.rs`: the jammer.
- `sim/unit_simple_deploy.rs`, `sim/world/jumpjet_cruise.rs`: DeployDir;
  `sim/world/selection_voice.rs`: the firing voice;
  `sim/world/world_spawn/construction.rs`: TurretROT.

## Residuals

- Deactivated cursor and click actions (`Hooks.Deactivated.cpp`): the cell
  What_Action answers 0 at its one entry (`Simulation::cell_move_action`),
  the ClickedAction refusals drop a Deactivated object from the click's
  receivers (`Simulation::click_order_receivers`), and GetDeactivatedAction
  10044AD0 decides the hover cursor of the object that owns it
  (`capability_cursor_for_hover`). The click paths over an object have no
  single per-class What_Action entry; they keep their stock rungs, whose
  orders the receiver filter already withholds; when the cursor owner is
  Deactivated, its answer decides the whole release, so no object receives
  an order (`ares_deactivated_selection_click`). SelectBestObjectForAction
  ranks an EMPed object 1.
- The heal arms: a Unit's (1004E5A0, hook 0x73FDBD) replaces the stock arm
  in `Simulation::unit_repair_action`, an Infantryman's (1004E2E0, hook
  0x51E710) is `Simulation::ares_infantry_heal_action`. Both heal any damaged
  ally the selected weapon's Verses reach, answer Select under Alt, and set
  InfantryHeal/UnitRepair; the click sends the same Attack
  (`healer_object_action`). The Infantryman over itself still takes the stock
  self arm at 0x51E748, which VERA does not port (NoSelfGuardArea). The arms
  follow the game's Ares switch (`RuleSet::ares`) in every build; their
  cursors go through the Ares cursor owner, still built only with the feature.
- Init_1's capture manager and parasite for any weapon slot and the
  parasite slot hooks (0x629804, 0x62A020, 0x62A7B1); the gunner temporal
  swap (0x74642C, 0x74653C); Hover DeployToLand (0x513EAA, 0x514A21,
  0x514DFE); NoSelfGuardArea (0x51E748: the healer's self arm it skips is
  not ported); the Occupier+VehicleThief expiry
  (0x4D9A83); the infantry DamageSparks parse order (0x5240BD).
- Dormant in retail: flying carriers (0x414338, 0x6B783B).
- Presentation and audio: the radial indicators of non-buildings
  (0x41BE80; the building GetSensorRange override 0x4566B0 is ported), damage
  spark and smoke particle-system lists (0x6FAD49, 0x702894), move sounds
  (0x4DAA68, 0x7101CF). The airstrike voices (0x41D5AE, 0x41D940) are
  ported in `sim/airstrike.rs`.
- The AttachEffect anim hiding of the Temporal hook.
- `SuperGapRadiusInCells` multiples of 256: the stock What_Action
  (`0x004472AA`) still reads the char, VERA the int.

## RulesExt general keys

With `--features ares`, the RulesExt `[General]` and `[AudioVisual]` keys
below are read by `AresGeneralRules` (`rules/native_processing.rs`) in
LoadBeforeTypeData (`1002AB60`) or LoadAfterTypeData (`1002A800`), with the
constructor's (`1002A360`) defaults. Default builds read none of them.

```ini
[General]
TeamRetaliate=no              ; read, not consumed (see below)
AIFriendlyDistance=           ; unset: AISafeDistance
StartInMultiplayerUnitCost=   ; unset: the average cost
EnemyWrench=yes
[AudioVisual]
DiskLaserAnimEnabled=no
InfantryElectrocuted=         ; Find only; unset: ELECTRO, else AnimTypes[1]
```

### Where VERA20K owns them

- `rules/ruleset.rs`: the InfDeath=5 anim (InfantryClass_ReceiveDamage_
  ElectricDeath `1002C930`, hook `0x518744`), resolved against the final
  AnimType registry.
- `sim/team_script_vm/actions.rs`: script action 54 regroups its argument
  plus `AIFriendlyDistance` (else `AISafeDistance`) cells from the base
  (`1003A2A0`, hook `0x6EFB69`); action 53 gathers its argument plus
  `AISafeDistance` from the enemy base (`1003A270`, hook `0x6EF8A1`).
- `sim/scenario_bootstrap.rs`: a set `StartInMultiplayerUnitCost` prices each
  starting unit (`1001EE10`, hook `0x5D6D9A`).
- `sim/world/disk_laser.rs`: a disk laser's hit flashes its warhead and, with
  `DiskLaserAnimEnabled`, plays its damage animation (`1004CFF0`, hook
  `0x4A76ED`).
- `app/presentation/ui_overlays.rs`: `EnemyWrench`, the warp tests, frame
  and offset of the repair wrench, and the power-off marker over a building
  whose owner switched it off, for that player and observers (`10049FE0`,
  hook `0x6F526C`; it covers buildings only, not deactivated units). The
  passenger pips (`1004A280`, hook `0x709D38`) take TechnoExt::GetPassengerPips
  (`10044C90`): only a `PipScale=Passengers` type, the last passenger first,
  each clipped to the pips left, one gap step after a Gunner's first pip,
  `Passengers.BySize`, and a tunnel's network; another PipScale draws its own
  scale instead.

### Not covered

- `TeamRetaliate` (`1003A180`, hook `0x6EB432`) is consumed by
  `team_ai::ares_team_retaliate` ([ares-warhead-residuals.md](ares-warhead-residuals.md)).
- `CanMakeStuffUp` (`100677B0`, hook `0x687C16`; `100680F0`, hook `0x55AFB3`): Ares'
  randomized VeteranRatio/BuildSpeed/BuildupTime write the immutable Rules,
  and its 90-frame infantry panic is not ported.
- `Message.SilosNeeded` (`10024AE0`, hook `0x4F8C23`): the silo nag is
  unreachable while VERA has no Ares tiberium storage (hook `0x4F9610`).
- `UnitsUnsellable`: the unit sale on a depot
  ([ares-reverse-engineering.md](ares-reverse-engineering.md)).
- The `HunterSeeker*` defaults: [ares-hunter-seeker.md](ares-hunter-seeker.md).
- `StartInMultiplayerUnitCost` unset: Ares' own average also takes the
  HouseType `StartInMultiplayer.Types`, which VERA does not read; HouseType
  `StartInMultiplayer.WithConst` (`1001F0B0`, hook `0x5D7048`, a starting
  construction yard) is read and not consumed.

## Hook table

Ported: ported in this chain. Equivalent: VERA already behaves so; the
code now cites the hook. Not applicable: no VERA counterpart exists.
Hooks the 3.0 DLL removed (34 in these files) are not listed.

### Hooks.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `413FFA` | AircraftClass_Init_TurretROT | `10048AC0` | ported: Aircraft secondary facing TurretROT |
| `414338` | AircraftClass_Put_SpawnHigh | `10048C10` | residual: spawned aircraft keep their launch Z (flying carriers) |
| `415544` | AircraftClass_Mi_Unload_Blocked | `10048B30` | ported: [ares-transports.md](ares-transports.md) |
| `415CA6` | AircraftClass_Paradrop_Units | `10048BA0` | ported: `sim::aircraft::drop_payload` ([ares-country-paradrop.md](ares-country-paradrop.md)) |
| `415DF6` | AircraftClass_Paradrop_Carryall | `10048B80` | ported: [ares-transports.md](ares-transports.md) |
| `41668B` | AircraftClass_ReceiveDamage | `10048C50` | ported earlier |
| `416C3A` | AircraftClass_Carryall_Unload_Facing | `10048920` | ported: [ares-transports.md](ares-transports.md) |
| `416C4D` | AircraftClass_Carryall_Unload_DestroyCargo | `100488C0` | ported with a residual: [ares-transports.md](ares-transports.md) |
| `416C94` | AircraftClass_Carryall_Unload_UpdateCargo | `10048A10` | ported: [ares-transports.md](ares-transports.md) |
| `416CF4` | AircraftClass_Carryall_Unload_Guard | `100489D0` | ported: [ares-transports.md](ares-transports.md) |
| `416E37` | AircraftClass_Mi_MoveCarryall_CanTote | `10048B00` | ported: [ares-transports.md](ares-transports.md) |
| `417D75` | AircraftClass_GetActionOnObject_CanTote | `10048A60` | ported: [ares-transports.md](ares-transports.md) |
| `417DD2` | AircraftClass_GetActionOnObject_NoManualUnload | `10048A90` | ported earlier |
| `41946B` | AircraftClass_ReceivedRadioCommand_QueryEnterAsPassenger_KillDriver | `10048CF0` | ported: [ares-transports.md](ares-transports.md) |
| `41949F` | AircraftClass_ReceivedRadioCommand_SpecificPassengers | `10048D10` | ported: [ares-transports.md](ares-transports.md) |
| `41BE80` | ObjectClass_DrawRadialIndicator | `10049A70` | residual: radial indicators for all technos (presentation) |
| `41D5AE` | AirstrikeClass_PointerGotInvalid_AirstrikeAbortSound | `10048DE0` | ported: `sim::airstrike` |
| `41D940` | AirstrikeClass_Fire_AirstrikeAttackVoice | `10048D60` | ported: `sim::airstrike` |
| `446EE2` | BuildingClass_Place_InitialPayload | `10048EB0` | ported: [ares-transports.md](ares-transports.md) |
| `44A03C` | BuildingClass_Mi_Selling_Transfer | `10048E90` | ported earlier |
| `4566B0` | BuildingClass_GetRangeOfRadial_Radius | `10048E60` | ported: the building radius (`rules::object_type`); other technos' indicators are 0x41BE80 |
| `471C96` | CaptureManagerClass_CanCapture | `10048ED0` | ported earlier |
| `4D718C` | FootClass_Put_InitialPayload | `100491B0` | ported: [ares-transports.md](ares-transports.md) |
| `4D85E4` | FootClass_UpdatePosition_TiberiumDamage | `10049280` | ported earlier |
| `4D98C0` | FootClass_Destroyed | `10049080` | ported: the DontScore/Insignificant gates and `EVA.Lost` ([ares-warhead-residuals.md](ares-warhead-residuals.md)) |
| `4D9920` | FootClass_SelectAutoTarget_Cloaked | `100491E0` | ported earlier |
| `4D9A83` | FootClass_PointerGotInvalid_OccupierVehicleThief | `10049180` | residual: Occupier+VehicleThief destination expiry |
| `4D9EBD` | FootClass_CanBeSold_SellUnit | `10048FD0` | ported earlier |
| `4DA584` | FootClass_Update_RadImmune | `10049490` | ported: RADIMMUNE skips Foot radiation |
| `4DA8B2` | FootClass_Update_AnimRate | `100493D0` | ported earlier |
| `4DAA68` | FootClass_Update_MoveSound | `10049460` | residual: move sound under a Magnetron (audio) |
| `4DECAE` | FootClass_Crash_Spin | `10049050` | ported earlier |
| `4DFE00` | FootClass_GarrisonStructure_TakeVehicle | `10049130` | ported earlier |
| `513EAA` | HoverLocomotionClass_UpdateHover_DeployToLand | `10049670` | residual: Hover DeployToLand landing |
| `514A21` | HoverLocomotionClass_ILocomotion_Process_DeployToLand | `10049550` | residual: Hover DeployToLand landing |
| `514DFE` | HoverLocomotionClass_ILocomotion_MoveTo_DeployToLand | `10049510` | residual: Hover DeployToLand landing |
| `519675` | InfantryClass_UpdatePosition_BeforeInfantrySpecific | `10049820` | ported earlier |
| `5198AD` | InfantryClass_UpdatePosition_EnteredGrinder | `100498D0` | ported: `sim::reverse_engineering` (ares-reverse-engineering.md) |
| `51CE9A` | InfantryClass_Idle | `10049740` | ported earlier |
| `51E748` | InfantryClass_GetActionOnObject_NoSelfGuardArea | `10049710` | residual: NoSelfGuardArea (stock self Area Guard arm unported) |
| `51E7BF` | InfantryClass_GetActionOnObject_CanCapture | `10049690` | ported earlier |
| `51F76D` | InfantryClass_Unload | `10049790` | ported earlier |
| `5202F9` | InfantryClass_UpdateVehicleThief_Check | `10049970` | equivalent: VehicleThief or CanDrive admits the follow |
| `5203F7` | InfantryClass_UpdateVehicleThief_Hijack | `100499A0` | ported earlier |
| `52138C` | InfantryClass_UpdateDeployment_Deso2 | `100497F0` | ported earlier |
| `5215F9` | InfantryClass_UpdateDeployment_Deso1 | `100497C0` | ported earlier |
| `5240BD` | InfantryTypeClass_LoadFromINI_DamageSparks | `100499E0` | residual: infantry DamageSparks parse order |
| `53C450` | TechnoClass_CanBePermaMC | `10049DA0` | ported earlier |
| `54C767` | JumpjetLocomotionClass_State4_54C550_DeployDir | `100499F0` | ported: jumpjet DeployDir only with a DeployingAnim |
| `5F3FB2` | ObjectClass_Update_MaxFallRate | `10049BA0` | ported earlier |
| `629804` | ParasiteClass_UpdateSquiddy | `10049C60` | residual: parasite weapon slot (Init_1 parasite part) |
| `62A020` | ParasiteClass_Update | `10049C30` | residual: parasite weapon slot (Init_1 parasite part) |
| `62A7B1` | Parasite_ExitUnit | `10049C90` | residual: parasite weapon slot (Init_1 parasite part) |
| `6B783B` | SpawnManagerClass_Update_SpawnHigh | `10049CC0` | residual: flying carrier aims spawns at its cell |
| `6F3283` | TechnoClass_CanScatter_KillDriver | `10049F90` | ported earlier |
| `6F407D` | TechnoClass_Init_1 | `1004A530` | ported earlier: Temporal slot; capture manager and parasite of any slot are residual |
| `6F4103` | TechnoClass_Init_2 | `1004A870` | equivalent: skips the stock managers; Init_1 replaces them |
| `6F526C` | TechnoClass_DrawExtras_PowerOff | `10049FE0` | ported: the wrench gate, frame and offset and the power-off marker (`app/presentation/ui_overlays.rs`) |
| `6F6A58` | TechnoClass_DrawHealthBar_HidePips_KillDriver | `1004A260` | ported earlier |
| `6F6AC9` | TechnoClass_Remove | `1004B140` | ported earlier |
| `6F826E` | TechnoClass_CanAutoTargetObject_CivilianEnemy | `10049D00` | ported earlier |
| `6F9E50` | TechnoClass_Update | `1004B320` | ported earlier |
| `6FA743` | TechnoClass_Update_SelfHeal | `1004B820` | ported earlier |
| `6FACD9` | TechnoClass_Update_DamageSparks | `1004B760` | ported earlier |
| `6FAD49` | TechnoClass_Update_SparkParticles | `1004B870` | residual: DamageSparksParticleSystems |
| `6FC0D3` | TechnoClass_CanFire_DisableWeapons | `10049EA0` | ported earlier |
| `6FC417` | TechnoClass_CanFire_PsionicsImmune | `10049EF0` | ported: PSIONICSIMMUNE and ImmuneToBerserk vs Psychedelic |
| `6FE31C` | TechnoClass_Fire_AllowDamage | `1004A370` | ported earlier |
| `6FF28F` | TechnoClass_Fire_BerserkROFMultiplier | `1004A3D0` | ported earlier |
| `700536` | TechnoClass_GetActionOnObject_NoManualFire | `1004A460` | ported earlier |
| `7008D4` | TechnoClass_GetActionOnCell_NoManualFire | `1004A430` | ported earlier |
| `700EEC` | TechnoClass_CanDeploySlashUnload_NoManualUnload | `10049E70` | ported earlier |
| `7014D5` | TechnoClass_ChangeOwnership_RadarJammer | `10049FB0` | ported earlier |
| `701914` | TechnoClass_ReceiveDamage_Damaging | `1004ACF0` | ported earlier: damaging flag read by the Aftermath hook |
| `701BFE` | TechnoClass_ReceiveDamage_Abilities | `1004A950` | ported: rank immunities and ImmuneToBerserk completed here; house gate earlier |
| `702050` | TechnoClass_ReceiveDamage_SuppressUnitLost | `1004B020` | ported earlier |
| `702185` | TechnoClass_ReceiveDamage_OverrideVoiceDie | `1004ADA0` | ported earlier |
| `7021F5` | TechnoClass_ReceiveDamage_OverrideDieSound | `1004AD60` | ported earlier |
| `702200` | TechnoClass_ReceiveDamage_SpillTiberium | `1004AEC0` | ported earlier |
| `702216` | TechnoClass_ReceiveDamage_TiberiumHeal | `1004B050` | ported earlier |
| `702819` | TechnoClass_ReceiveDamage_Aftermath | `1004AB50` | ported earlier |
| `702894` | TechnoClass_ReceiveDamage_SmokeParticles | `1004AE20` | residual: DamageSmokeParticleSystems |
| `702A38` | TechnoClass_ReceiveDamage_OpenTopped | `1004AD10` | ported: retaliation turns on the open-topped transport |
| `702CFE` | TechnoClass_ReceiveDamage_PreventScatter | `1004ADE0` | ported earlier: PreventScatter |
| `702E64` | TechnoClass_RegisterDestruction_Bounty | `1004B100` | ported earlier |
| `707B19` | TechnoClass_PointerGotInvalid_SpawnCloakOwner | `1004A910` | ported: cloaking carrier keeps its spawns |
| `7087EB` | TechnoClass_ShouldRetaliate_KillDriver | `1004B2D0` | ported earlier |
| `7090A8` | TechnoClass_SelectFiringVoice | `1004B1D0` | ported: `sim::world::selection_voice` |
| `7091D6` | TechnoClass_CanPassiveAquire_KillDriver | `10049F70` | ported earlier |
| `709D38` | TechnoClass_DrawPipscale_Passengers | `1004A280` | ported: `scale_pip_draws` with GetPassengerPips `10044C90` (`app/presentation/ui_overlays.rs`) |
| `70BE80` | TechnoClass_ShouldSelfHealOneStep | `1004B2F0` | ported earlier |
| `70DEBA` | TechnoClass_UpdateGattling_Cycle | `1004B6C0` | ported earlier |
| `70E2B0` | TechnoClass_IronCurtain | `1004A880` | ported earlier |
| `7101CF` | FootClass_ImbueLocomotor | `10049160` | residual: move sound under a Magnetron (audio) |
| `7162B0` | TechnoTypeClass_GetPipMax_MindControl | `1004B910` | ported: MindControl pips from the MindControl weapon |
| `718275` | TeleportLocomotionClass_MakeRoom | `1004B980` | ported earlier |
| `71A84E` | TemporalClass_UpdateA | `1004BB60` | ported: Temporal frees the target radar jammer |
| `71A917` | TemporalClass_Update_Erase | `1004BBC0` | ported earlier |
| `732C30` | TechnoClass_IDMatches | `1004A490` | ported earlier |
| `735584` | UnitClass_CTOR_TurretROT | `1004BC40` | ported: Unit secondary facing TurretROT |
| `73758A` | UnitClass_ReceivedRadioCommand_QueryEnterAsPassenger_KillDriver | `1004C250` | ported earlier |
| `73762B` | UnitClass_ReceivedRadioCommand_BySize1 | `1004C150` | ported: Passengers.BySize=no counts heads |
| `73769E` | UnitClass_ReceivedRadioCommand_SpecificPassengers | `1004C270` | ported: `sim::radio::receive::unit_can_enter_radio` (Passengers.Allowed/Disallowed, [ares-building-hooks.md](ares-building-hooks.md)) |
| `73778F` | UnitClass_ReceivedRadioCommand_BySize2 | `1004C190` | not applicable: Unit DockNow (0x15) arm is not represented |
| `73782F` | UnitClass_ReceivedRadioCommand_BySize3 | `1004C1D0` | ported: Passengers.BySize=no counts heads |
| `737994` | UnitClass_ReceivedRadioCommand_BySize4 | `1004C210` | ported: Passengers.BySize=no counts heads |
| `737F97` | UnitClass_ReceiveDamage | `1004C0E0` | ported earlier |
| `738749` | UnitClass_Destroy_TiberiumExplosive | `1004BD90` | ported earlier |
| `739956` | UnitClass_Deploy_Transfer | `10048E90` | ported earlier |
| `739ADA` | UnitClass_SimpleDeploy_Height | `1004C380` | ported: airborne simple deployer deploys; InAir only for DeployToLand |
| `739B8A` | UnitClass_SimpleDeploy_Facing | `1004C2D0` | ported: simple deployer turns to DeployDir first |
| `739F21` | UnitClass_UpdatePosition_Visceroid | `1004C530` | ported: `Simulation::ares_visceroid_merge`, a raw Type store into `[General] LargeVisceroid` ([ares-tiberium.md](ares-tiberium.md)) |
| `73A1BC` | UnitClass_UpdatePosition_EnteredGrinder | `1004C460` | ported: `sim::reverse_engineering`, with the hijacker refund (ares-reverse-engineering.md) |
| `73DE90` | UnitClass_Mi_Unload_SimpleDeployer | `1004C070` | ported: [ares-transports.md](ares-transports.md) |
| `740031` | UnitClass_GetActionOnObject_NoManualUnload | `1004BEF0` | ported earlier |
| `74031A` | UnitClass_GetActionOnObject_NoManualEnter | `1004BEA0` | ported earlier |
| `741206` | UnitClass_CanFire | `1004BC80` | ported: moving IFV fires unless its gunner weapon is Temporal |
| `741613` | UnitClass_ApproachTarget_OmniCrusher | `1004BC10` | ported earlier |
| `7418AA` | UnitClass_CrushCell_CrushDamage | `1004BCE0` | ported earlier |
| `7441B6` | UnitClass_MarkOccupationBits | `1004BFF0` | ported: `track_host::track_raw_mark_at` records the marked plane (`GameEntity::ares_alt_occupation`) |
| `744216` | UnitClass_UnmarkOccupationBits | `1004C3E0` | ported: `track_host::track_raw_mark_at` also clears the recorded plane; VERA's retail bridge crossings and collapses mark and clear one plane, so it matters only for a deck-high Unit over a Cell without the bridge bit |
| `74642C` | UnitClass_ReceiveGunner | `1004C110` | residual: gunner temporal swap |
| `74653C` | UnitClass_RemoveGunner | `1004C2A0` | residual: gunner temporal swap |
| `746B89` | UnitClass_GetUIName | `1004BF20` | ported earlier |
| `746C55` | UnitClass_GetUIName_Space | `1004BF90` | ported earlier |
| `747BCF` | UnitTypeClass_LoadFromINI_Turrets | `1004C620` | ported earlier |

### Hooks.Deactivated.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `417CCB` | AircraftClass_GetActionOnObject_Deactivated | `1004CB00` | ported: deactivated rungs (see Residuals) |
| `417F83` | AircraftClass_GetActionOnCell_Deactivated | `1004CAE0` | ported: deactivated rungs (see Residuals) |
| `4436F7` | BuildingClass_ActionOnCell_Deactivated | `1004CB30` | ported: deactivated rungs (see Residuals) |
| `447218` | BuildingClass_GetActionOnObject_Deactivated | `1004CB70` | ported: deactivated rungs (see Residuals) |
| `447548` | BuildingClass_GetActionOnCell_Deactivated | `1004CB50` | ported: deactivated rungs (see Residuals) |
| `4D74EC` | FootClass_ActionOnObject_Deactivated | `1004CBC0` | ported: deactivated rungs (see Residuals) |
| `4D7D58` | FootClass_ActionOnCell_Deactivated | `1004CBA0` | ported: deactivated rungs (see Residuals) |
| `51D0DD` | InfantryClass_Scatter | `1004CC60` | ported earlier |
| `51E440` | InfantryClass_GetActionOnObject_Deactivated | `1004CC30` | ported: deactivated rungs (see Residuals) |
| `51F808` | InfantryClass_GetActionOnCell_Deactivated | `1004CC10` | ported: deactivated rungs (see Residuals) |
| `5200B3` | InfantryClass_UpdatePanic | `1004CC80` | ported earlier |
| `70FBE0` | TechnoClass_Activate | `1004CCB0` | ported: `Simulation::techno_reactivate`, with the factory queue refresh and the sound mutex |
| `70FC90` | TechnoClass_Deactivate | `1004CDC0` | ported: `Simulation::techno_deactivate`, with the factory queue refresh and the sound mutex |
| `736135` | UnitClass_Update_Deactivated | `1004CFD0` | ported earlier |
| `73C143` | UnitClass_DrawVXL_Deactivated | `1004CEA0` | ported earlier |
| `73DBF9` | UnitClass_Mi_Unload_Decactivated | `1004CF80` | ported earlier |
| `73FD5A` | UnitClass_GetActionOnObject_Deactivated | `1004CF50` | ported: deactivated rungs (see Residuals) |
| `7404B9` | UnitClass_GetActionOnCell_Deactivated | `1004CF30` | ported: deactivated rungs (see Residuals) |

### Hooks.Gap.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `44E2B0` | BuildingClass_Mi_Unload_LargeGap | `1004D1B0` | ported: int SuperGapRadiusInCells; toggle otherwise stock |
| `454BDC` | BuildingClass_UpdatePowered_LargeGap | `1004D250` | ported: int GapRadiusInCells |
| `4566D5` | BuildingClass_GetRangeOfRadial_LargeGap | `1004D170` | ported: int radius in GetSensorRange |
| `65731F` | RadarClass_UpdateMinimap_Lock | `1004D2C0` | not applicable: radar surface lock (DirectDraw performance) |
| `65757C` | RadarClass_UpdateMinimap_Unlock | `1004D2F0` | not applicable: radar surface unlock |
| `657CF2` | MapClass_MinimapChanged_Lock1 | `1004D270` | not applicable: minimap surface lock |
| `657D35` | MapClass_MinimapChanged_Unlock1 | `1004D2A0` | not applicable: minimap surface unlock |
| `657D3D` | MapClass_MinimapChanged_Lock2 | `1004D270` | not applicable: minimap surface lock |
| `657D8A` | MapClass_MinimapChanged_Unlock2 | `1004D2A0` | not applicable: minimap surface unlock |
| `6FB1B5` | TechnoClass_CreateGap_LargeGap | `1004D330` | ported: int GapRadiusInCells |
| `6FB306` | TechnoClass_CreateGap_Optimize | `1004D360` | equivalent: same cell writes, fewer GetCellAt calls |
| `6FB4A3` | TechnoClass_CreateGap_LargeGap | `1004D330` | ported: int GapRadiusInCells |
| `6FB5F0` | TechnoClass_DeleteGap_Optimize | `1004D3A0` | equivalent: same cell writes, fewer GetCellAt calls |

### Hooks.Mechanical.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `51C913` | InfantryClass_CanFire_Heal | `1004E270` | ported: medic fires on any damaged Techno |
| `51E710` | InfantryClass_GetActionOnObject_Heal | `1004E2E0` | ported: `Simulation::ares_infantry_heal_action` (see Residuals for the self arm) |
| `520731` | InfantryClass_UpdateFiringState_Heal | `1004E420` | ported: medic keeps any damaged Techno target |
| `6F7FC5` | TechnoClass_CanAutoTargetObject_Heal | `1004E490` | ported: healing vehicle takes any damaged ally |
| `6F8EE3` | TechnoClass_FindTargetType_Heal | `1004E4A0` | ported: healer target classes 0x403C |
| `6F8F1F` | TechnoClass_FindTargetType_Heal | `1004E4A0` | ported: healer target classes 0x403C |
| `6FA361` | TechnoClass_Update_LoseTarget | `1004E4C0` | ported: computer healer keeps allies, drops enemies |
| `736E8E` | UnitClass_UpdateFiringState_Heal | `1004E6E0` | ported: healing Unit keeps any damaged Techno target |
| `73FDBD` | UnitClass_GetActionOnObject_Heal | `1004E5A0` | ported: `Simulation::unit_repair_action` (Ares arm) |
| `741113` | UnitClass_CanFire_Heal | `1004E540` | ported: healing Unit fires on any damaged Techno |

### Hooks.Spotlights.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `435820` | BuildingLightClass_CTOR | `1004EA20` | equivalent: TechnoExt::Spotlight is the owned pointer |
| `435BFA` | BuildingLightClass_Draw_Start | `1004EB30` | ported earlier |
| `435CD3` | BuildingLightClass_Draw_Spotlight | `1004EAC0` | ported earlier |
| `436072` | BuildingLightClass_Draw_430 | `1004EAA0` | equivalent: StartHeight-based beam heights |
| `4360D8` | BuildingLightClass_Draw_400 | `1004EA70` | equivalent: StartHeight-based beam heights |
| `4360FF` | BuildingLightClass_Draw_250 | `1004EA40` | equivalent: StartHeight-based beam heights |
| `436459` | BuildingLightClass_Update | `1004EC10` | ported earlier |
| `436A2D` | BuildingLightClass_PointerGotInvalid_OwnerCloak | `1004EBC0` | ported earlier |
| `4370C0` | BuildingLightClass_SDDTOR | `1004EBE0` | equivalent: destructor clears the owned pointer |
| `441163` | BuildingClass_Put_DontSpawnSpotlight | `1004EA10` | ported: building light made by the Techno hook |
| `6F6D0E` | TechnoClass_Put_1 | `1004EDE0` | ported: HasSpotlight Techno gets its light at Unlimbo |
| `6F6F20` | TechnoClass_Put_2 | `1004EDE0` | ported: HasSpotlight Techno gets its light at Unlimbo |

### Hooks.Weapons.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `5F8084` | ObjectTypeClass_UnloadTurretArt | `100423B0` | equivalent: no 18-slot voxel bound to free |
| `5F8277` | ObjectTypeClass_Load3DArt_NoSpawnAlt1 | `10041280` | ported: a unit's NoSpawnAlt `<Image>WO` pair is required, whatever its `Turret=` (`rules::voxel_model_plan`) |
| `5F848C` | ObjectTypeClass_Load3DArt_NoSpawnAlt2 | `100413A0` | ported: skips the stock WO load into the turret slot (`rules::voxel_model_plan`) |
| `73B6E3` | UnitClass_DrawVXL_NoSpawnAlt | `10041460` | equivalent: the unit draw reads the `{type}WO` pair by name in every build (`voxel_frame_catalog::unit_atlas_variants`); residual: a `Turret=yes` NoSpawnAlt unit draws that body without its turret, where Ares draws the turret over it (retail and Brute Force have no such type) |
| `5F9634` | ObjectTypeClass_LoadFromINI | `100413B0` | residual: reads `AlternateTheaterArt` (unread), whose only consumer is Load2DArt (`10041040`, hook `0x5F9070`, not ported: VERA runs the stock SHP image load); retail and Brute Force never set it |
| `5F865F` | ObjectTypeClass_Load3DArt_Turrets | `10042220` | equivalent: turret voxel enumeration has no 18 bound |
| `5F887B` | ObjectTypeClass_Load3DArt_Barrels | `10042090` | equivalent: barrel voxel enumeration has no 18 bound |
| `70DC70` | TechnoClass_SwitchGunner | `10042480` | ported earlier |
| `7128C0` | TechnoTypeClass_LoadFromINI_Weapons1 | `10042570` | equivalent: LoadWeapons reader (WeaponSlots) |
| `715B1F` | TechnoTypeClass_LoadFromINI_Weapons2 | `10042590` | equivalent: art FLH block read by LoadWeapons |
| `7177C0` | TechnoTypeClass_GetWeapon | `10042510` | equivalent: extension weapon records |
| `7177E0` | TechnoTypeClass_GetEliteWeapon | `100424E0` | equivalent: extension weapon records |
| `717890` | TechnoTypeClass_SetWeaponTurretIndex | `100425A0` | equivalent: extension turret table |
| `7178B0` | TechnoTypeClass_GetWeaponTurretIndex | `10042540` | equivalent: extension turret table |
| `73B90E` | UnitClass_DrawVXL_Barrels1 | `100425D0` | equivalent: draw reads indexed voxels unbounded |
| `73BCCD` | UnitClass_DrawVXL_Barrels2 | `10042610` | equivalent: draw reads indexed voxels unbounded |
| `73BD15` | UnitClass_DrawVXL_Turrets | `10042650` | equivalent: draw reads indexed voxels unbounded |
| `73BD6A` | UnitClass_DrawVXL_Barrels3 | `10042630` | equivalent: draw reads indexed voxels unbounded |
