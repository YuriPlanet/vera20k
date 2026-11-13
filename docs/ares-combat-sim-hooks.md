# Ares combat and simulation hooks

Build with `--features ares`. These are the Ares 3.0 combat, simulation and
INI hooks of the 2026-11-08 hook pass that no earlier chain had cited. Each
row gives the gamemd hook, the Ares.dll function, and what VERA20K does with
it: ported (the Rust cites both addresses next to the code), equivalent
(the engine already behaves as the hook does), not applicable, or residual
(with the reason it is left).

## Ported

| Hook | Ares.dll | Name | Where |
|---|---|---|---|
| `0x4895B8` | `100531F0` | DamageArea_CellSpread1 | `sim::combat::cell_spread::area_damage_cells` |
| `0x4895C7` | `10053250` | DamageArea_CellSpread2 | same |
| `0x4899BE` | `10053270` | DamageArea_CellSpread3 | same |
| `0x4456E5` | `10053D80` | BuildingClass_UpdateConstructionOptions_ExcludeDisabled | `sim::production::production_tech` |
| `0x6FD438` | `100563A0` | TechnoClass_FireLaser | `sim::combat::laser::create` |
| `0x7187DA` | `10037BC0` | TeleportLocomotionClass_Unwarp_PreventSelfCrush | `sim::movement::teleport_chrono` |
| `0x7188F2` | `10037BE0` | TeleportLocomotionClass_Unwarp_SinkJumpJets | same |
| `0x446AAF` | `10036860` | BuildingClass_Place_SkipFreeUnits | `sim::building_art` (Grand_Opening) |
| `0x4444E2` | `100415E0` | BuildingClass_KickOutUnit_FindAlternateKickout | `sim::production::production_queue` |
| `0x65DBB3` | `1003A340` | TeamTypeClass_CreateInstance_Plane | `sim::team_script_vm::reinforcements` |
| `0x469EBA` | `1001A090` | BulletClass_DetonateAt_Splits | `sim::combat::airburst` (`ares_emit`) |
| `0x468EB9` | `1001A7E0` | BulletClass_Fire_SplitsA | `ProjectileType::airburst_arm` (impact ladder) |
| `0x468FFA` | `1001A810` | BulletClass_Fire_SplitsB | same (`world_receiver` cluster gate) |
| `0x5D7163` | `1001F220` | MPGameMode_SpawnStartingUnits_Types | `sim::scenario_bootstrap::starting_unit_candidates_for_country` |
| `0x5D7337` | `1001F200` | MPGameMode_SpawnStartingUnits_NoInfantry | `sim::scenario_bootstrap` (starting-unit loop) |
| `0x5D6D9A` | `1001EE10` | MPGameModeClass_CreateStartingUnits_UnitCost | `sim::scenario_bootstrap::starting_unit_budget` |
| `0x417E16` | `1004C8B0` | AircraftClass_GetActionOnObject_Dock | `Simulation::entry_click_action` (Aircraft arm) |
| `0x4CDE64` | `1004D930` | FlyLocomotionClass_sub_4CD600_HunterSeeker_Ascent | `sim::movement::fly_height::FlyRuntime::step_height` |
| `0x4CDF54` | `1004DA20` | FlyLocomotionClass_sub_4CD600_HunterSeeker_Descent | same |
| `0x4CE85A` | `1004D8A0` | FlyLocomotionClass_UpdateLanding | `sim::world::fly_landing` (`ares_hunter_seeker_reacquire`) |
| `0x4CCB84` | `1004D7A0` | FlyLocomotionClass_ILocomotion_Process_HunterSeeker | `sim::hunter_seeker` (`ares_hunter_seeker_process`, from `fly_process`) |
| `0x4CD9C8` | `1004DA90` | FlyLocomotionClass_sub_4CD600_HunterSeeker_UpdateTarget | `sim::hunter_seeker` (`ares_hunter_seeker_update_target`) |
| `0x4CF3D0` | `1004DC30` | FlyLocomotionClass_sub_4CEFB0_HunterSeeker | `sim::hunter_seeker` (`ares_hunter_seeker_height`, `_detonate`) |
| `0x4D8D95` | `1004E050` | FootClass_UpdatePosition_HunterSeeker | `sim::movement::per_cell` (`ares_hunter_seeker_parting_strike`) |
| `0x5D7048` | `1001F0B0` | MPGameMode_SpawnBaseUnit_BuildConst | `sim::scenario_bootstrap::ares_place_starting_construction_yard` |
| `0x5D6F61` | `1001ED60` | MPGameModeClass_CreateStartingUnits_BaseCenter | `sim::scenario_bootstrap::ares_settle_starting_construction_yards` |
| `0x5F4FF9` | `1004F470` | ObjectClass_Put_IsFlammable | `TerrainObjectState::joins_logic_on_unlimbo` |
| `0x71B99E` | `1004F4E0` | TerrainClass_ReceiveDamage_ForestFire | `sim::combat::world_receiver::commit_terrain` |
| `0x71C5D2` | `1004F4B0` | TerrainClass_Ignite_IsFlammable | `sim::terrain_fire` (`terrain_catch_fire`) |
| `0x71C7C2` | `1004F530` | TerrainClass_Update_ForestFire | `sim::terrain_fire` (`ares_spread_forest_fire`, from `terrain_spawn`) |
| `0x6FF5F5` | `10056E60` | TechnoClass_Fire_OtherWaves (laser arm) | `sim::combat::world_fire` (FireAt tail); the wave types in `sim::wave` |
| `0x75FA29` | `10057080` | WaveClass_Draw_Colors | `app::presentation::fire_effects::ares_wave_blend`, now for lasers too |
| `0x760BC2` | `10057000` | WaveClass_Draw2 | `render::terrain_surface_lines` (a laser pixel in mode 11) |
| `0x760DE2` | `10057040` | WaveClass_Draw3 | same |
| `0x7609E3` | `10057120` | WaveClass_Draw_NodLaser_Details | `app::presentation::fire_effects::build_weapon_wave_draws` (every laser is drawn) |
| `0x686D7F` | `1002F680` | INIClass_ReadScenario_CacheSP | `CampaignLoadingMetadata::load_text_color` (MISSIONMD `LoadScreenText.Color`, default by the file name's `SOV`/`YUR`/`TUT` prefix); without a MISSIONMD the static keeps an earlier scenario's colour in Ares, the stock one in VERA |
| `0x642B36` | `1002F870` | ProgressScreenClass_GetLoadTextColor | `app::loading::pump`: the campaign title and briefing take that scheme when it exists |
| `0x642B91` | `1002F870` | ProgressScreenClass_GetSideColor | `NativeLoadingScreenState::resolve_player_colors`: the Country's `LoadScreenText.Color` (early `0x6847B7` read) after the observer LightGrey test |

- Area damage walks Ares' `CellSpreadEnumerator` over the stock band
  `ftol(CellSpread + 0.99)`, capped at 0x100. For bands 1..10 the cells are
  the stock table's with `(-1,-1)` visited before `(1,-1)`; band 11 loses the
  stock table's repeated `(-3,11)`; a negative band walks nothing.
- `LaserThickness=` (WeaponTypeExt `+0x64`, -1) is read. SpawnLaser takes it
  above 1. FireAt's Ares arm (`100563D0`, hook `0x6FF4DE`) now sets every
  width: a building's beam is the thickness or 3 (a supported Prism tower
  then adds its Intensity), any other firer's is the thickness (supported
  above 3) or 2, whatever its colour. The Prism support beam (`10016490`)
  takes a non-negative thickness.
- A building's free units (`FreeUnit=`) come only from the first Place that
  reaches the block, so a building that Ares' ChronoWarp removes and puts
  back does not deliver a second harvester. The flag is BuildingExt `+0xC`,
  saved and hashed.
- A busy factory hands its product to another building with the producer's
  `Factory=` and `Naval=` whose type the product `CanBeBuiltAt`
  (`1003D9A0`, now one helper with FindFactory's use), instead of the same
  type only.
- A Droppod team's carrier is the head owner's `GetParadropPlane`
  (`1001BCB0`: Country, then Side `ParaDrop.Aircraft=`, then PDPLANE), the
  same helper the ParaDrop superweapon uses.
- TS laser waves: a weapon past FireAt's laser, bolt and both radiation
  arms and without `IsMagBeam=` creates, through CreateWave, a WaveClass
  of type 2 for `Wave.IsBigLaser=` or type 1 for `Wave.IsLaser=`
  (WeaponTypeExt `+0x3A`/`+0x39`), even when the firer links a wave
  already; the new wave takes the link. The constructor's
  Draw_NonMagnetic (`0x761640`) moves both endpoints 5% past each other
  (binary32 1.05, no +50 Z) and takes the type's corners (Laser 34 along
  and 44 across, BigLaser 27 and 34); the laser starts fully drawn
  (dFront 1.0, nLaserIntensity 160) and its one Update_Wave, under the
  Ares range and reversal rules, records the cells. WaveClass_Update
  (`100571A0`) never updates a laser again: each frame its AmbientDamage
  strikes those cells, then the intensity drops by 6 and the wave leaves
  below 32, so it damages on 22 frames. The client fills its six-point
  polygon, brightening red and blue by the intensity (Draw_Laser
  `0x7609E0`, ModifyWaveColor with the weapon's colours in Ares).
- Airburst and Splits: BulletTypeExt `Splits`, `RetargetSelf`,
  `RetargetAccuracy`, `AirburstSpread` and `AroundTarget` (`+0x1C`..`+0x30`)
  are read, and `Airburst= or Splits=` takes the impact ladder's airburst
  arms. The tail sends one `CreateBullet` child (damage, warhead, speed,
  `Bright=`, `ProjectileRange=`) to each map cell within `AirburstSpread=`
  of the burst (the 9 around it at the 1.5 default; retail's only
  `Airburst=` projectile, `[V3AirburstP]`, is fired by no weapon), or with
  `Splits=` `Cluster=` children at nearby Technos (CanAffectHouse, under
  1280 leptons, `AA=` for aircraft) and random cells, retargeted by
  `RetargetAccuracy=`. Each child draws one Scenario `RandomRanged(0, 31)`
  for its drop vector (`launch::ares_airburst_launch_velocity`, the game's
  sine table as YRpp `Math::sin` calls it).
- `StartInMultiplayer.Types` (CountryExt `+0x1C0`, NullableVector over
  TechnoTypeClass::Find, read before `.WithConst`, not inherited): a set list
  of the house's own country is its starting-unit candidates in list order,
  Unit and Infantry types only, with no AllowedToStartInMultiplayer, Owner,
  tech-level or BaseUnit filter. A list of only other types is the stock
  both-empty refusal (`0x5D72AB`); a list that resolved to nothing returns
  success (`0x5D743E`), so the house spawns nothing and is credited its whole
  budget. Without infantry candidates a house always draws a vehicle. The
  unpriced budget averages the set of listed Unit/Infantry types of the
  non-passive countries plus the AllowedToStartInMultiplayer types within
  the game's tech level that the unlisted ones own, BaseUnit erased, in
  unsigned arithmetic.
- The aircraft depot row drops the `UnitRepair=` gate: a human player's
  aircraft over a building its `Dock=` lists answers Enter when the building
  has a free or own contact slot, no first passenger (`+0x118`, TechnoClass
  `Passengers.FirstPassenger`, as `BuildingClass::Receive_Radio 0x43C2D0`
  types it) and its CAN_LOAD is ROGER, else NoEnter; any other building
  keeps Select. A Harrier now enters its own Airforce Command on a click.
- A `HunterSeeker=` Fly climbs at `HunterSeeker.EmergeSpeed` while taking
  off and `.AscentSpeed` otherwise, and (when not a Dropship) descends at
  `.DescentSpeed`, each capped by the remaining distance and falling back to
  the `[General]` values. It never lands: its landing callback acquires a
  target when it has none and, on success, stops landing, climbs back to the
  type's flight level, unlinks its first contact and attacks.
- A Chrono warper under the Iron Curtain no longer crushes itself on its
  landing cell. On a water tile, a Unit offline for want of power does not
  float, and a `SpeedType=Hover` `JumpJet=` Unit sinks without the entry
  test (`0x718A66`).

## Equivalent

| Hook | Ares.dll | Name | Reason |
|---|---|---|---|
| `0x70CEA0` | `10054310` | TechnoClass_EvalThreatRating_Verses1 | the threat score reads `WarheadType::interaction`, the Ares verses table (`sim::combat::greatest_threat`) |
| `0x70CF45` | `100542E0` | TechnoClass_EvalThreatRating | same |
| `0x76110B` | `10057180` | WaveClass_RecalculateAffectedCells_Clear | Ares zeroes the count instead of freeing; `Vec::clear` (`sim::wave`) |
| `0x483226` | `1005F970` | CellClass_CrateBeingCollected_Firepower2 | ported with the crate multipliers (`sim::crates::effects`); storing the magnitude into a 1.0 slot is the stock product |
| `0x523932` | `10041890` | InfantryTypeClass_CTOR_Initialize | zeroes the sequence sound fields the stock constructor left unset; Rust sequences start zeroed |
| `0x72FA1A` | `1002FA50` | Sides_MixFileYuriFiles1 | `Sidebar.YuriFileNames` picks RADARY/BKGD*Y in `render::sidebar_chrome` (`SidebarArtNames::for_yuri`, with siblings `1002FA60`/`1002FA70`) |
| `0x6FFF9E` | `10056D70` | TechnoClass_GetActionOnObject_IvanBombsB | skips stock's self-detonate rung; `Simulation::bomb_detonate_action` answers the Ares predicate in its place |
| `0x643BB9` | `1002F890` | ProgressScreenClass_UpdateSingleProgressBar | the campaign meter row shows no colour scheme: DrawFill `0x643400` draws through the SPLDBR convert at `+0x74`, `+0x71` draws no outline in mode 0, and the label is empty (`app::loading::pump::render`) |
| `0x55403D` | `1002F850` | LoadProgressMgr_Draw_SkipShadowOnNullString3 | the loading text draw adds its backing only behind laid-out glyphs, in every build (`app::loading::pump::render`); stock blends a box under an empty `GUI:LoadingEx` |

## Not applicable

| Hook | Ares.dll | Name | Reason |
|---|---|---|---|
| `0x5BE974` | `100079D0` | MouseClass_GetCursorFirstFrame | the table readers serve only `Tactical::DrawWaypointPaths`, whose YR paths are always empty (`render::cursor_atlas`) |
| `0x5BE994` | `10007A50` | MouseClass_GetCursorFrameCount | same |
| `0x4F8F54` | `1002F610` | HouseClass_Update_SlaveMinerCheck | unreachable in 3.0: HasBeenDefeated (`10024990`, hook `0x4F8EBD`) always returns to `0x4F8F87`, past it |
| `0x474DEE` | `10067700` | INIClass_GetFoundation | debug log of an invalid value only |
| `0x474E8E` | `10067740` | INIClass_GetMovementZone | same |
| `0x477007` | `10067760` | INIClass_GetSpeedType | same |

## Residual

| Hook | Ares.dll | Name | Reason |
|---|---|---|---|
| `0x5227A3` | `1002F9C0` | Sides_Disguise | the PermaDisguise arm of `InfantryClass::ClearDisguise` (`0x522780`) has no Rust caller (the damage reveal admits only non-PermaDisguise types); the construction arm `0x6F422F` of the same function is ported |
| `0x715320` | `10041DC0` | TechnoTypeClass_LoadFromINI_EarlyReader | reads `WaterImage` (unread), whose only consumer swaps a vehicle's draw image on water (presentation) |
| `0x4016F0` | `1005AF20` | IDXContainer_LoadSample | Ares loose-WAV samples and extra audio bags (`Misc/Audio.cpp`); the client audio has neither |

Notes on the mechanisms ported in the 2026-11-08 follow-up pass:
- `.WithConst`: the yard is the first `[AI] BuildConst=` building the house
  can expect to build; it is constructed, ForceMission(Guard)ed (Assign_Mission,
  vt+0x1F0), BaseSpawnCell moves one cell up-left for a foundation over 2,
  and a computer house runs the AI half of a yard deploy, now one helper
  (`Simulation::refresh_ai_base_after_deploy`, `0x00505180` plus node zero,
  BasePlan center and the three latches) shared with `deploy_mcv`, at its
  base cell. The stock placement then puts the yard at that cell, else near
  it. BaseCenter then QueueMission(Construction, 1)s and, in the placement
  scope, Enter_Idle_Mode(0, 1)s every ConYard of the house, Bases on or
  off. Traced build-up: a building's discovery (`0x00440D61`, owner
  `0x00440E5C`) follows the Unlimbo's own Enter_Idle_Mode(1, 1), which
  queued Construction. A human's yard is refused Grand_Opening while
  Construction is queued, so its ready byte (`+0x6DD`) stays clear,
  BaseCenter's QueueMission does not commence, Enter_Idle_Mode replaces the
  queue with Guard and the Idle body, and the yard stands in the forced
  Guard with no build-up and no Grand_Opening. A computer's yard is admitted
  (its current mission is the forced Guard), opens and sets the ready byte;
  BaseCenter's QueueMission commences Construction, the first Update's
  Mission_Construction restarts the build-up, and the leftover ready byte
  lets that Update's second ready check (`0x0043FF91`) commence the queued
  Guard, so Mission_Construction's completion state never runs.
  Worth a side-by-side look once a mod sets the key.
- Terrain fire: IsOnFire (`+0xCC`) on `TerrainObjectState`, saved and
  hashed when set. The tree owns its fire anims (`AnimObject::owner_terrain`,
  stored absolute since a tree never moves); SetOwnerObject's last-detach
  callback is Finish_Fire_Death, pointer expiry detaches them, and the lethal
  arm clears their loops (ClearLoopsForOwner `0x00422B80`). `Sparky=`
  (WarheadTypeClass+0x14A), `TreeFire=` and `TreeFlammability=` (Rules+0x588,
  constructor 0.1) are read. Residuals: Finish_Fire_Death's crumble of a
  zero-Health tree is unreachable (a lethal hit UnInits the tree in the same
  call); the spread runs after the whole stage step rather than right after
  the IsAnimated draw, which differs only for an animated flammable
  non-spawner tree reaching its midpoint that frame; a missing `TreeFire=`
  entry builds no anim where native indexes unchecked.
- Stock finding: when a house has neither kind of starting-unit candidate,
  gamemd's `GenerateStartingUnits` (`0x5D70F0`) returns false and its
  caller `Generate_Starting_Units` (`0x5D6D80`, the vt+0xCC call after
  `0x5D6F77`) returns at once, so later houses get neither units nor
  credits; VERA skips only that house.

## Tests

`StartInMultiplayer.WithConst` has
`ares_start_with_const_places_a_construction_yard_for_the_base_unit`
(`sim::scenario_bootstrap`); the terrain fire
`ares_a_sparky_hit_sets_a_flammable_tree_alight_until_its_fire_anim_ends` and
`ares_a_burning_tree_spreads_to_its_flammable_neighbours`
(`sim::terrain_fire`); the HunterSeeker step
`ares_hunter_seeker_detonates_and_descends_inside_its_proximities`
(`sim::hunter_seeker`). `StartInMultiplayer.Types` has
`ares_start_in_multiplayer_types_replace_the_starting_candidates` and
`ares_start_in_multiplayer_types_enter_the_unit_cost_average`
(`sim::scenario_bootstrap`); the Dock row
`ares_a_harrier_enters_its_listed_dock_and_keeps_select_elsewhere`
(`sim::world::passenger_entry_tests`, retail), which replaces the stock
retail check that a jet over the Service Depot answers NoEnter; the
HunterSeeker flight `ares_hunter_seeker_climbs_and_descends_at_its_speeds`
(`sim::movement::fly_height`) and
`ares_hunter_seeker_skips_the_landing_callback` (`sim::world::fly_landing`).

The laser waves have
`ares_laser_wave_damages_its_cells_on_each_update_until_it_fades`
(`sim::world::world_tests`, retail) and
`laser_wave_constructor_extends_its_beam_and_takes_its_type_size`
(`sim::wave`).

The Ares airburst has `ares_airburst_sends_one_child_to_each_cell_within_the_spread`
and `ares_splits_send_cluster_children_to_nearby_technos_or_random_cells`
(`sim::combat::airburst`), replacing the old check that Ares builds run no
tail. Ares builds replace two stock native comparisons: the band-11 cell-spread
oracle and the busy-factory redirect rows run only in stock builds, and the
laser-birth corpus skips its width and support checks under Ares. The Ares
behaviour has its own tests next to them (`ares_area_damage_walks_the_cell_spread_enumerator_ring_order`,
`ares_busy_factory_exit_takes_another_type_that_can_build_the_product`,
`ares_laser_thickness_sets_the_fire_at_and_spawn_laser_widths`).

