# Optional Ares Building, House, Infantry and SWType hooks

Build with `--features ares`. Default builds keep every stock path named
below. This page covers the hooks of `Ext/Building/Hooks.cpp`,
`Hooks.Foundation.cpp`, `Ext/House/*.cpp`, `Ext/HouseType/Hooks.cpp`,
`Ext/Infantry/*.cpp` and `Ext/SWType/Hooks.cpp`. The table at the end gives
each one's disposition; hooks other pages already describe show as "ported
earlier".

```ini
[MyYard]                     ; BuildingType
MassSelectable=no            ; band boxes skip it (unset: the stock 1x1 UndeploysInto test)

[MyAnim]                     ; artmd.ini, an anim with MakeInfantry=
MakeInfantryOwner=victim     ; now also for the InfDeath table, virus, mutate and PlayAnimAt anims

[GlobalControls]
AllowBypassBuildLimit=no,no,yes  ; also lets a hard AI team ignore BuildLimit
```

## AI base plans, teams and production

The base plan copies the whole priority list (0x505B58, 0x505C34) and, when
it plans defenses, pads a list shorter than four with defense nodes before
any draw (10023B30, 0x505CF1). A TaskForce entry is admitted at a factory
only while its type's `BuildLimit` leaves room for the whole entry, unless
the difficulty's `AllowBypassBuildLimit` is set; otherwise it takes the
recruit path (100240D0, 0x50965E). The unit, infantry and aircraft choosers
draw `RandomRanged(0, 99)` against `FillEarliestTeamProbability` instead of
the stock `RandomRanged(0, 0x7FFFFFFE)` threshold, and the harvester branch
compares TechLevels signed (HouseExt::GetTypeToProduce 100216A0, hooks
0x4FEA60, 0x4FEEE0, 0x4FF210). Anger against a `MultiplayPassive` house is
ignored outside campaigns (10023FB0, 0x504796). A construction yard's queue
update also refreshes the defense strip (10025100, 0x509140).

## Buildings

A FreeUnit without `Harvester=` starts on Area_Guard (10014390, 0x446E9F).
An Armory's trainee takes the Hospital's exit post (10014100, 0x444D26), and
a Hospital or Armory on Mission_Unload kicks out its first passenger
(100145B0, 0x44D8A1). An EMP'd power plant gives no power (10014440,
0x44E855). GoOnline and GoOffline remember the toggle, which a Temporal
release restores instead of switching the building on (0x452287, 0x452393,
0x452210). An unengineered `NeedsEngineer=` building counts as Powered for
its anims (0x4467D6, 0x451A54, 0x454BF7). `MassSelectable` decides band
selection (10013CF0, 0x459C03). A `Deliver.BaseNormal=no` delivery gives no
own-base adjacency but still serves BuildOffAlly (10014890, 0x4A8FF5).

## Infantry

The InfDeath table's anims (InfantryExplode, FlamingInfantry, the
electrocution anim, InfantryHeadPop, InfantryNuked, InfantryBrute) take
SetMakeInfOwner with no invoker; InfantryVirus and InfantryMutate take it
with the killer's house, and an `invoker` virus anim loses its remap
(10025D20, 10025F50, 10025EB0). The paradrop and NotHuman virus anims are
owned the same way and kept when their type has no `SpawnsParticle`
(10025EF0). TAction PlayAnimAt owns a `MakeInfantry=` anim by the trigger's
house (10025C20, 0x6E232E). A rank holding the C4 ability plants and enters
the target building's cell like a `C4=` type (0x51A4D2, 0x51C325).
Panicking infantry do not go prone on Beach or Water (100256F0, 0x5201CC).

## Superweapons

A Super that is still charged after Fire_SW (ClickFire refused) alerts no
computer house (10034C70, 0x4FAF2A). The AI's offensive SW target scan skips
cloaked objects without their draw (10034C50, 0x50CFAA).

## Already equivalent

Custom foundations already use their whole cell lists, exact covered-cell
dedupe, their own width and radar shape and unbounded placement previews;
the country menu has no 0..9 cap and the random pick uses the Ares weights;
a missing shipyard or base unit is handled; FindBuildable is the existing
candidate test; the second SuperWeapon anim passes and the destructor's
Super update match; campaign GameOptions keep SuperWeapons on. The code
cites each hook.

## Residuals

- The storage pips (0x44D755, 0x709B4E; ares-tiberium.md). The storage
  itself is ported in `sim::ares_tiberium_storage`.
- An upgrade's ProduceCash timer is not started at Put (0x4409F4); the
  Update hook starts it one frame longer. The payout and
  `ProduceCashDisplay` are ported in `sim::credit_income`.
- Ares serial AI queues (`AllowParallelAIQueues=no`, 0x4502F4, 0x4CA07A and
  the four KickOutUnit hooks); the default changes nothing.
- Doggie (0x51F628, 0x518CB3, 0x51ABD7,
  0x5200C1), `DeathAnims=` owners (0x51849A), Mission_Guard's SW gate
  (0x449716) and the ICBM crate's Ares Nuke type (0x6CEEB0): dormant on
  retail data.
- Presentation and input: `Message.SilosNeeded`
  (0x4F8C23), the ready-building and hotkey factory lookups (0x6A9822,
  0x6AB312, 0x535DB6, 0x535E76), the Money SW tooltip (0x6A932B),
  the observer strip (0x6AA0CA, 0x6AA164),
  the amphibious water sound (0x51D799), an Ivan entering an allied
  Grinder (0x51EB48) and the bullet anim's MakeInfantryOwner outside Phobos
  (0x469C4E).
- GarrisonedIn (0x51DF38, 0x51DFFD, 0x518434) and map-house `Degrades=`
  (0x50114D).

## Hook table

Ported: ported in this chain. Equivalent: VERA already behaves so; the code
cites the hook. Not applicable: no VERA counterpart exists. Hooks the 3.0
DLL removed are not listed.

### Building/Hooks.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `43CA80` | BuildingClass_ReceivedRadioCommand_DockUnloadCell | `10014540` | ported earlier |
| `43FD2C` | BuildingClass_Update_ProduceCash | `100146E0` | ported: `credit_income::produce_cash_step` |
| `44019D` | BuildingClass_Update_Battery | `10014690` | ported earlier |
| `4409F4` | BuildingClass_Put_ProduceCash | `10014490` | ported in part: the upgrade's factory plans (`building_upgrades`); residual: the upgrade's cash timer is not started at Put (`delay`), so the Update hook starts it (`delay + 1`) |
| `440C08` | BuildingClass_Put_AIBaseNormal | `10014460` | ported earlier |
| `44266B` | BuildingClass_ReceiveDamage_Destroyed | `10014520` | ported: a damage-killed building runs `building_death_announcement` after DestructionEffects ([ares-warhead-residuals.md](ares-warhead-residuals.md)) |
| `444D26` | BuildingClass_KickOutUnit_ArmoryExitBug | `10014100` | ported: an Armory's exit takes the Hospital post (`production_queue::ExitProducer`) |
| `445A72` | BuildingClass_Remove_AIBaseNormal | `10014580` | ported earlier |
| `4467D6` | BuildingClass_Place_NeedsEngineer | `100143C0` | ported: unengineered NeedsEngineer counts as Powered at the opening (`anim_power_policy`) |
| `446E9F` | BuildingClass_Place_FreeUnit_Mission | `10014390` | ported: a non-harvester FreeUnit is born on Area_Guard (`production_refinery`) |
| `447A63` | BuildingClass_QueueImageAnim_Sell | `100144E0` | ported earlier |
| `4482BD` | BuildingClass_ChangeOwnership_ProduceCash | `10013D40` | ported: `credit_income::produce_cash_on_owner_change` |
| `44840B` | BuildingClass_ChangeOwnership_Tech | `10013E40` | ported earlier |
| `44BB1B` | BuildingClass_Mi_Repair_Promote | `10014170` | not applicable: null-trainee guard of the Armory promotion arm, which VERA does not port |
| `44C844` | BuildingClass_MissionRepair_Reload | `10014190` | ported: per-docker reload timers (BuildingExt DockReloadTimers) and TechnoType `ReloadRate` (`sim::docking::airfield_reload`); a slot Ares never wrote reads 0 |
| `44D4CA` | BuildingClass_Mi_Missile_NoReport | `10014130` | ported earlier |
| `44D755` | BuildingClass_GetPipFillLevel_Tiberium | `10013FD0` | residual: storage pips (ares-tiberium.md) |
| `44D760` | BuildingClass_Destroyed_UnitLost | `10013F10` | ported: building `EVA.Lost` (`combat::ares_building_lost_event`) |
| `44D8A1` | BuildingClass_UnloadPassengers_Unload | `100145B0` | ported: Hospital/Armory Mission_Unload kicks out the first passenger (`building_missions::mission_unload`) |
| `44E855` | BuildingClass_PowerProduced_EMP | `10014440` | ported: an EMP'd plant outputs no power (`power_system`) |
| `4509B4` | BuildingClass_UpdateRepair_Funds | `10014630` | ported earlier |
| `451A54` | BuildingClass_PlayAnim_NeedsEngineer | `10014400` | ported: same for a new slot anim (`building_art`) |
| `4521C8` | BuildingClass_Disable_Temporal_Factories | `10013FA0` | ported: `ObjectSubstrate::set_building_online_for_temporal` queues the refresh, run before the factory sweep (`Simulation::ares_refresh_temporal_factories`) |
| `452210` | BuildingClass_Enable_TogglePower | `10013FB0` | ported: a Temporal release restores it instead of forcing the latch on |
| `452218` | BuildingClass_Enable_Temporal_Factories | `10013FA0` | ported: with `4521C8`. LetGo (0x71ABC0) runs from ~15 rule-less call sites, so the refresh waits for the sweep; a queue order in between reads the old hold |
| `452287` | BuildingClass_GoOnline_TogglePower | `10014090` | ported: GoOnline records TogglePower_HasPower (`building_power`) |
| `452393` | BuildingClass_GoOffline_TogglePower | `10014070` | ported: GoOffline records it |
| `454BF7` | BuildingClass_UpdatePowered_NeedsEngineer | `100145F0` | ported: same in the power update (`update_building_anim_power`) |
| `4555D5` | BuildingClass_IsPowerOnline_KeepOnline | `100140B0` | ported earlier |
| `456370` | BuildingClass_UnmarkBaseSpace_AIBaseNormal | `100145C0` | ported earlier |
| `459C03` | BuildingClass_CanBeSelectedNow_MassSelectable | `10013CF0` | ported: `MassSelectable` band selection (`ObjectType::band_selectable`) |
| `4A8FF5` | MapClass_CanBuildingTypeBePlacedHere_Ignore | `10014890` | ported: a NoBaseNormal delivery still serves BuildOffAlly (`is_within_build_area`) |
| `51E4ED` | InfantryClass_GetActionOnObject_EngineerRepairable | `10014850` | ported earlier |
| `51FA82` | InfantryClass_GetActionOnCell_EngineerRepairable | `10014810` | not applicable: the fogged-building arm of What_Action_OnCell (0x51F953..0x51FAAB) is not ported |
| `709B4E` | TechnoClass_DrawPipscale_SkipSkipTiberium | `100148B0` | residual: storage pips (ares-tiberium.md) |
| `7376D9` | UnitClass_ReceivedRadioCommand_DockUnload_Facing | `10014970` | ported earlier |
| `73DF66` | UnitClass_Mi_Unload_DockUnload_Facing | `10014910` | ported earlier |

### Building/Hooks.Foundation.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `45EC90` | BuildingTypeClass_GetFoundationWidth | `10015310` | equivalent: custom width (`Foundation::dimensions`) |
| `45ECA0` | BuildingTypeClass_GetFoundationHeight | `100152D0` | ported earlier |
| `4A8C77` | DisplayClass_ProcessFoundation1_UnlimitBuffer | `10015340` | equivalent: no fixed foundation buffer (`selection_overlay`) |
| `4A8DD7` | DisplayClass_ProcessFoundation2_UnlimitBuffer | `100153B0` | equivalent: no fixed foundation buffer |
| `568411` | MapClass_AddContentAt_Foundation_P1 | `10015490` | equivalent: full foundation list (`occupancy::hidden_diagonal_cells`) |
| `568565` | MapClass_AddContentAt_Foundation_OccupyHeight | `10015420` | equivalent: exact covered-cell dedupe (`hidden_diagonal_cells`) |
| `568841` | MapClass_RemoveContentAt_Foundation_P1 | `10015520` | equivalent: full foundation list on removal |
| `568997` | MapClass_RemoveContentAt_Foundation_OccupyHeight | `100154B0` | equivalent: exact dedupe, signed guard on removal (`decrement_guarded`) |
| `6563B0` | RadarClass_UpdateFoundationShapes_Custom | `10015580` | equivalent: same (`radar_foundation_brush`) |
| `656584` | RadarClass_GetFoundationShape | `10015540` | equivalent: custom radar shape from width/height (`radar_foundation_brush`) |
| `6D50FB` | sub_6D5030_CustomFoundation | `100155C0` | ported earlier |
| `6D5573` | sub_6D5030_CustomFoundation | `100155C0` | ported earlier |

### House/Hooks.BasePlan.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `4F65BF` | HouseClass_CanAffordBase | `10023840` | equivalent: no base unit is handled (`ai_build_need`) |
| `5051E0` | HouseClass_FirstBuildableFromArray | `100238E0` | equivalent: FindBuildable = `candidate_allowed` (`ai_buildable`) |
| `5054B0` | HouseClass_GenerateAIBuildList_EnsureSanity | `10023AF0` | not applicable: base plan sanity log only |
| `5055D8` | HouseClass_GenerateAIBuildList_SWAllowed | `10023B90` | equivalent: campaign GameOptions keep SuperWeapons on (`base_plan_generation`) |
| `505B58` | HouseClass_GenerateAIBuildList_SkipManualCopy | `10023BD0` | ported: whole-list copy (`recalc_base_plan`) |
| `505C34` | HouseClass_GenerateAIBuildList_FullAutoCopy | `10023B10` | ported: whole-list copy from index 0 |
| `505C95` | HouseClass_GenerateAIBuildList_CountExtra | `10023940` | ported earlier |
| `505CF1` | HouseClass_GenerateAIBuildList_PadWithN1 | `10023B30` | ported: short plans padded with defense nodes, no draw |
| `50610E` | HouseClass_FindPositionForBuilding_FixShipyard | `100238A0` | equivalent: no shipyard answers the empty cell (`naval_base_placement`) |
| `506306` | HouseClass_FindPlaceToBuild_Evaluate | `10023860` | ported: `AIInnerBase` picks the inner-base key (`ai_base_site::SiteKey::InnerBase`) |
| `5D705E` | MPGameMode_SpawnBaseUnit_BaseUnit | `10023C80` | equivalent: no base unit skips construction (`scenario_bootstrap`) |
| `688B37` | MPGameModeClass_CreateStartingUnits_B | `10023BF0` | not applicable: Generate_Random_Units runs only without a game mode object (0x686937); VERA starts skirmish houses through the mode |

### House/Hooks.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `441B30` | BuildingClass_Destroy_Refinery | `10023DA0` | ported: `sim::ares_tiberium_storage` (ares-tiberium.md) |
| `4F62FF` | HouseClass_CTOR_FixNameOverflow | `10023FF0` | equivalent: no fixed UIName buffer (`HouseUiName`) |
| `4F645F` | HouseClass_CTOR_FixSideIndices | `10024030` | equivalent: houses take their Country's side for any index (`resolve_house_side_index`); map houses take it again in ReadINI (0x500B40) |
| `4F7870` | HouseClass_CanBuild | `10024090` | ported earlier |
| `4F8440` | HouseClass_Update_TogglePower | `10024B40` | ported earlier |
| `4F8B08` | HouseClass_Update_DamageDelay | `100247A0` | ported earlier |
| `4F8C23` | HouseClass_Update_SilosNeededEVA | `10024AE0` | ported: the EVA line (`sim::house_eva`); residual: the `Message.SilosNeeded` text (presentation) |
| `4F8EBD` | HouseClass_Update_HasBeenDefeated | `10024990` | ported earlier |
| `4F9610` | HouseClass_GiveTiberium_Storage | `10024290` | ported: `sim::ares_tiberium_storage` (ares-tiberium.md) |
| `4FC731` | HouseClass_DestroyAll_ReturnStructures | `10024150` | ported earlier |
| `4FF563` | HouseClass_RegisterTechnoLoss_StatCounters_KeepAlive | `100245D0` | ported earlier |
| `4FF71B` | HouseClass_RegisterTechnoGain_StatCounters_KeepAlive | `10024580` | ported earlier |
| `500CC5` | HouseClass_InitFromINI_FixBufferLimits | `10024480` | ported earlier |
| `504796` | HouseClass_AddAnger_MultiplayPassive | `10023FB0` | ported: no anger against a MultiplayPassive house outside a campaign (`update_anger_nodes`) |
| `505360` | HouseClass_PrerequisitesForTechnoTypeAreListed | `10024520` | ported earlier |
| `508C7F` | HouseClass_UpdatePower_Auxiliary | `10024650` | ported earlier |
| `508D32` | HouseClass_UpdatePower_LocalDrain1 | `10024690` | ported earlier |
| `508D4A` | HouseClass_UpdatePower_LocalDrain2 | `10024760` | ported earlier |
| `508E66` | HouseClass_UpdateRadar_Battery | `10024780` | ported earlier |
| `508EBC` | HouseClass_Radar_Update_CheckEligible | `10024550` | ported earlier |
| `508F91` | HouseClass_SpySat_Update_CheckEligible | `10024620` | ported earlier |
| `50965E` | HouseClass_CanInstantiateTeam | `100240D0` | ported: a factory admits a TaskForce entry only within BuildLimit unless AllowBypassBuildLimit (`ai_team_creation`) |
| `522D75` | InfantryClass_Slave_UnloadAt_Storage | `10024B60` | ported: `sim::ares_tiberium_storage` (ares-tiberium.md) |
| `73E4A2` | UnitClass_Mi_Unload_Storage | `10024BC0` | ported: `sim::ares_tiberium_storage` (ares-tiberium.md) |

### House/Hooks.100.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `4FEA60` | HouseClass_AI_UnitProduction | `10023410` | ported: harvester branch with signed TechLevel, then GetTypeToProduce's `RandomRanged(0, 99)` (`ai_unit_choice`) |
| `4FEEE0` | HouseClass_AI_InfantryProduction | `100233B0` | ported: GetTypeToProduce draw (`ai_unit_choice::pick`) |
| `4FF210` | HouseClass_AI_AircraftProduction | `10023350` | ported: GetTypeToProduce draw |

### House/Hooks.Queue.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `443CCA` | BuildingClass_KickOutUnit_AircraftType | `10024D60` | residual: serial AI queue bookkeeping |
| `444119` | BuildingClass_KickOutUnit_UnitType | `10024DC0` | residual: serial AI queue bookkeeping |
| `444131` | BuildingClass_KickOutUnit_InfantryType | `10024DA0` | residual: serial AI queue bookkeeping |
| `44531F` | BuildingClass_KickOutUnit_BuildingType | `10024D80` | residual: serial AI queue bookkeeping |
| `4502F4` | BuildingClass_Update_Factory | `10024E00` | residual: Ares serial AI queues (`AllowParallelAIQueues=no`); with the default the hook does nothing (`phobos_parallel_ai_queues`) |
| `4CA07A` | FactoryClass_AbandonProduction | `10024EE0` | residual: serial AI queue bookkeeping (as 0x4502F4) |
| `509140` | HouseClass_Update_Factories_Queues | `10025100` | ported: the BuildingType update also refreshes the Combat (defense) queue (`update_factory_queue`); the sidebar redraw flag is presentation |
| `50B370` | HouseClass_ShouldDisableCameo | `10024F80` | ported: replaces CheckBuildLimit for every caller (BuildLimitRemaining less the queue, the held object counted once, the AirportBound limit going on to BuildLimit, a usable factory) |
| `535DB6` | SetStructureTabCommandClass_Execute_Power | `10025150` | residual: structure-tab hotkey asks a powered factory (input) |
| `535E76` | SetDefenseTabCommandClass_Execute_Power | `10025130` | residual: defense-tab hotkey asks a powered factory (input) |
| `6A9822` | StripClass_Draw_Power | `100251D0` | residual: a ready building shows ready only with a factory to place from (sidebar) |
| `6AB312` | SidebarClass_ProcessCameoClick_Power | `10025170` | residual: cameo click factory lookup through HouseExt::FindFactory (input) |

### House/Hooks.Academy.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `413FD2` | AircraftClass_Init_Academy | `10023600` | ported earlier |
| `442D1B` | BuildingClass_Init_Academy | `100236D0` | ported earlier |
| `445905` | BuildingClass_Remove_Academy | `10023690` | ported earlier |
| `446366` | BuildingClass_Place_Academy | `10023750` | ported earlier |
| `448AB2` | BuildingClass_ChangeOwnership_Remove_Academy | `10023690` | ported earlier |
| `4491D5` | BuildingClass_ChangeOwnership_Add_Academy | `10023650` | ported earlier |
| `517D51` | InfantryClass_Init_Academy | `10023790` | ported earlier |
| `735678` | UnitClass_Init_Academy | `100237C0` | ported earlier |
| `74689B` | UnitClass_Init_Academy | `100237C0` | ported earlier |

### House/Hooks.Firewall.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `6FF008` | TechnoClass_Fire_FSW | `10024C00` | ported earlier (firestorm chain) |
| `6FF860` | TechnoClass_Fire_FSW | `10024C00` | ported earlier (firestorm chain) |

### House/Body.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `4F6532` | HouseClass_CTOR | `100230E0` | not applicable: HouseExt allocation; Ares house state lives in `HouseState` |
| `4F7371` | HouseClass_DTOR | `10023100` | not applicable: HouseExt release |
| `50114D` | HouseClass_InitFromINI | `10023140` | residual: a map house section's `Degrades=` (ares-building-types.md) |
| `503040` | HouseClass_SaveLoad_Prefix | `10023230` | not applicable: ExtMap stream prefix; VERA serializes `HouseState` |
| `504069` | HouseClass_Load_Suffix | `10023160` | not applicable: ExtMap load suffix |
| `504080` | HouseClass_SaveLoad_Prefix | `10023230` | not applicable: ExtMap stream prefix |
| `5046DE` | HouseClass_Save_Suffix | `10023270` | not applicable: ExtMap save suffix |

### HouseType/Hooks.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `48DA3B` | sub_48D1E0_PlayTaunt | `1001E400` | ported earlier (File.Taunt) |
| `4E3562` | Game_GetFlagSurface | `1001DCE0` | ported earlier |
| `4E3792` | HTExt_Unlimit1 | `1001DD70` | equivalent: the country menu has no 0..9 cap (`country_choices`) |
| `4E38D8` | LoadPlayerCountryString | `1001DF10` | equivalent: `MenuText.Status` (country presentation) |
| `4E3A6A` | hWnd_PopulateWithCountryNames | `1001E2C0` | ported earlier |
| `4E3A9C` | HTExt_Unlimit2 | `1001DD80` | equivalent: no cap |
| `4E3F31` | HTExt_Unlimit3 | `1001DD90` | equivalent: no cap |
| `4E412C` | HTExt_Unlimit4 | `1001DDA0` | equivalent: no cap |
| `4E41A7` | HTExt_Unlimit5 | `1001DDB0` | equivalent: no cap |
| `4FE782` | HouseClass_AI_BaseConstructionUpdate_PickPowerplant | `1001DDC0` | ported earlier |
| `536438` | TauntCommandClass_Execute | `1001E290` | ported earlier (country presentation: File.Taunt) |
| `553412` | LoadProgressMgr_Draw_LSFile | `1001DFD0` | ported earlier |
| `5536DA` | LoadProgressMgr_Draw_LSName | `1001E010` | ported earlier |
| `553A05` | LoadProgressMgr_Draw_LSSpecialName | `1001E070` | ported earlier |
| `553D06` | LoadProgressMgr_Draw_LSBrief | `1001DF70` | ported earlier |
| `69B670` | HTExt_PickRandom_AI | `1001DD30` | equivalent: weighted AI random country |
| `69B774` | HTExt_PickRandom_Human | `1001DD50` | equivalent: weighted human random country (`skirmish_session`) |
| `6AA0CA` | StripClass_Draw_DrawObserverBackground | `1001E160` | residual: observer strip art (ares-country-presentation.md: no observer strip) |
| `6AA164` | StripClass_Draw_DrawObserverFlag | `1001E1E0` | residual: observer strip flag (same) |
| `72B690` | LoadScreenPal_Load | `1001E0E0` | ported earlier |
| `752B70` | PlayTaunt | `1001E130` | ported earlier (File.Taunt) |

### Infantry/Hooks.MakeInf.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `469C4E` | BulletClass_DetonateAt_DamageAnimSelected | `10025C90` | residual: bullet damage anim MakeInfantryOwner outside Phobos (dormant: no retail AnimList anim has MakeInfantry); its -15 ZAdjust equals Explosion_Z_Adjust |
| `5183DE` | InfantryClass_ReceiveDamage_InfantryVirus1 | `10025EF0` | ported: paradrop InfantryVirus owner, kept without SpawnsParticle (`emit_inline_virus_particle`) |
| `51849A` | InfantryClass_ReceiveDamage_DeathAnim | `10025D60` | residual: `DeathAnims=` owner: the stock DeathAnims rung is not ported (dormant on retail) |
| `518575` | InfantryClass_ReceiveDamage_InfantryVirus1 | `10025EF0` | ported: NotHuman InfantryVirus owner, same |
| `5185C8` | InfantryClass_ReceiveDamage_InfDeath | `10025DE0` | ported earlier |
| `518698` | InfantryClass_ReceiveDamage_Anims | `10025D20` | ported: InfantryExplode owner (`InfantryDeathPostlude::commit`) |
| `5186F2` | InfantryClass_ReceiveDamage_Anims | `10025D20` | ported: FlamingInfantry owner |
| `518755` | InfantryClass_ReceiveDamage_Anims | `10025D20` | ported: electrocution anim owner |
| `5187BB` | InfantryClass_ReceiveDamage_Anims | `10025D20` | ported: InfantryHeadPop owner |
| `518821` | InfantryClass_ReceiveDamage_Anims | `10025D20` | ported: InfantryNuked owner |
| `51887B` | InfantryClass_ReceiveDamage_InfantryVirus2 | `10025F50` | ported: InfantryVirus owner; an invoker kind clears the Convert |
| `518A96` | InfantryClass_ReceiveDamage_InfantryMutate | `10025EB0` | ported: InfantryMutate owner by MakeInfantryOwner |
| `518B93` | InfantryClass_ReceiveDamage_Anims | `10025D20` | ported: InfantryBrute owner |
| `6E232E` | ActionClass_PlayAnimAt | `10025C20` | ported: TAction PlayAnimAt MakeInfantry owner (`trigger_play_anim_at_waypoint`) |

### Infantry/Hooks.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `518434` | InfantryClass_ReceiveDamage_SkipDeathAnim | `10025640` | residual: a garrisoned infantryman skips its death anim (GarrisonedIn is not kept) |
| `519D9C` | InfantryClass_UpdatePosition_MultiEngineer | `10025880` | ported earlier |
| `519FAF` | InfantryClass_UpdatePosition_EngineerRepairsFriendly | `100257A0` | ported earlier |
| `51A4D2` | InfantryClass_UpdatePosition_C4Ability | `10025730` | ported: a C4-ability rank plants (`c4_plant`) |
| `51C325` | InfantryClass_IsCellOccupied_C4Ability | `10025580` | ported: a C4-ability rank enters a building cell on Sabotage (`object_entry`) |
| `51D799` | InfantryClass_PlayAnim_WaterSound | `100255F0` | residual: water sound only for AmphibiousDestroyer outside transports (audio) |
| `51DF38` | InfantryClass_Remove | `10025660` | residual: GarrisonedIn release on Limbo (Ares-only garrison back-pointer; VERA garrisons own their occupant list) |
| `51DFFD` | InfantryClass_Put | `10025620` | residual: GarrisonedIn release on Unlimbo (same) |
| `51E5BB` | InfantryClass_GetActionOnObject_MultiEngineerA | `10025540` | ported earlier |
| `51E5E1` | InfantryClass_GetActionOnObject_MultiEngineerB | `10025550` | ported earlier |
| `51E635` | InfantryClass_GetActionOnObject_EngineerOverFriendlyBuilding | `10025420` | ported earlier |
| `51EB48` | InfantryClass_GetActionOnObject_IvanGrinder | `100254B0` | residual: an Ivan enters an allied Grinder without Ctrl (input; VERA answers IvanBomb) |
| `5201CC` | InfantryClass_UpdatePanic_ProneWater | `100256F0` | ported: no panic prone on Beach/Water (`infantry_fear_turn`) |

### Infantry/Hooks.Doggie.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `518CB3` | InfantryClass_ReceiveDamage_Doggie | `10025AD0` | residual: Doggie panic when hurt (dormant) |
| `51ABD7` | InfantryClass_SetDestination_Doggie | `10025B00` | residual: Doggie stands up to move (dormant) |
| `51F628` | InfantryClass_Guard_Doggie | `10025960` | residual: Doggie sit-down on tiberium (dormant: no retail `Doggie=yes`) |
| `5200C1` | InfantryClass_UpdatePanic_Doggie | `10025B50` | residual: Doggie panic on tiberium (dormant) |

### Infantry/Hooks.Saboteur.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `519FF8` | InfantryClass_UpdatePosition_Saboteur | `100260D0` | ported earlier |
| `51B2CB` | InfantryClass_SetTarget_Saboteur | `10026080` | ported earlier |
| `51EE6B` | InfantryClass_GetActionOnObject_Saboteur | `10025FC0` | ported earlier |
| `7004AD` | TechnoClass_GetActionOnObject_Saboteur | `100261F0` | ported earlier |

### SWType/Hooks.cpp

| Hook | Name | Ares.dll | Disposition |
|---|---|---|---|
| `43BE50` | BuildingClass_DTOR_HasAnySW | `10034980` | equivalent: the destructor's Update_Owned_Supers already uses `provided_supers` (`world::lifecycle`) |
| `4463F0` | BuildingClass_Place_SuperWeaponAnimsA | `10034A60` | ported earlier |
| `44656D` | BuildingClass_Place_SuperWeaponAnimsB | `10034AE0` | equivalent: skips the stock second loop (`open_super_weapon_anims`) |
| `4468F4` | BuildingClass_Place_AnnounceSW | `100349D0` | ported earlier |
| `449716` | BuildingClass_Mi_Guard_HasFirstSW | `100349B0` | residual: Mission_Guard's SuperWeapon gate is not ported (dormant: no armed SW building in retail) |
| `450F9E` | BuildingClass_ProcessAnims_SuperWeaponsA | `10034AF0` | ported earlier |
| `451132` | BuildingClass_ProcessAnims_SuperWeaponsB | `10034B60` | equivalent: skips the stock second pass |
| `457630` | BuildingClass_SWAvailable | `10034BB0` | ported earlier |
| `457690` | BuildingClass_SW2Available | `10034B70` | ported earlier |
| `4AC20C` | DisplayClass_LeftMouseButtonUp | `10034BF0` | equivalent: VERA fires the selected Super type itself (`commands::launch_super_weapon_at_cell`) |
| `4F9004` | HouseClass_Update_TrySWFire | `10034CD0` | ported earlier |
| `4FAE72` | HouseClass_SWFire_PreDependent | `10034C90` | ported earlier |
| `4FAF2A` | HouseClass_SWDefendAgainst_Aborted | `10034C70` | ported: a still-charged Super alerts no house (`superweapon::fire`) |
| `50CFAA` | HouseClass_PickOffensiveSWTarget | `10034C50` | ported: a cloaked rally target is skipped without its draw (`ai_fire::pick_best_rally_target`) |
| `653B3A` | RadarClass_GetMouseAction_CustomSWAction | `10034D00` | ported earlier |
| `6A932B` | StripClass_GetTip_MoneySW | `10034F80` | residual: Money SW tooltip text (presentation) |
| `6A99B7` | StripClass_Draw_SuperDarken | `10034EF0` | ported earlier |
| `6AAEDF` | SidebarClass_ProcessCameoClick_SuperWeapons | `10034DA0` | ported earlier |
| `6AAF9D` | SidebarClass_ProcessCameoClick_SelectTarget | `10034D60` | ported earlier |
| `6CB4D0` | SuperClass_SetOnHold | `10035750` | ported earlier |
| `6CB70C` | SuperClass_Grant_InitialReady | `100354E0` | ported earlier |
| `6CB7B0` | SuperClass_Lose | `10035610` | ported earlier |
| `6CB920` | SuperClass_ClickFire | `10035130` | ported earlier |
| `6CBCDE` | SuperClass_Update_Animation | `10035870` | ported earlier |
| `6CBD6B` | SuperClass_Update_DrainMoney | `100358A0` | ported earlier |
| `6CBD86` | SuperClass_Progress_Charged | `10035730` | ported earlier |
| `6CBDD7` | SuperClass_AnnounceReady | `100350D0` | ported earlier |
| `6CBF5B` | SuperClass_GetCameoChargeStage_ChargeDrainRatio | `10035420` | ported earlier |
| `6CC053` | SuperClass_GetCameoChargeStage_FixFullyCharged | `100354C0` | ported earlier |
| `6CC0EA` | SuperClass_AnnounceQuantity | `10035070` | ported earlier |
| `6CC2B0` | SuperClass_NameReadiness | `100356A0` | ported earlier |
| `6CC390` | SuperClass_Launch | `100355C0` | ported earlier |
| `6CEE96` | SuperWeaponTypeClass_FindIndex | `10035A20` | equivalent: custom SW `Type=` names resolve in the SuperWeaponType reader |
| `6CEEB0` | SuperWeaponTypeClass_FindFirstOfAction | `100359A0` | residual: the ICBM crate also takes an Ares Nuke-type SW (dormant: retail NukeSpecial has Action=Nuke) |
| `6CEF84` | SuperWeaponTypeClass_GetAction | `10035A40` | ported earlier |
| `6D49D1` | TacticalClass_Draw_TimerVisibility | `10035A80` | ported: `SW.TimerVisibility` (`AresSuperWeaponPresentation`) through IsTimerVisible `10032E80` (`sim::superweapon::ares_timer_visible`), asked before the timer lines (`app/presentation/super_timers.rs`) |
