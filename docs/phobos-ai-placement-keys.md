# AI placement and bug-fix switches

Eleven Phobos switches that rescan found without a reader: six `[AI]` placement
switches, four `[General]` bug-fix switches and one `[CombatDamage]` key, plus
the eight `[General] TeamDelays.Count1..Count8` rows that carry the
`TeamDelays.DynamicType` delays. `AIAutoDeployMCV`, `AISetBaseCenter`, `AIForbidConYard`,
`MissileSpawnAttackCell`, `FallingDownTargetingFix`, `BuildingWaypoints`
(selection half) and `TeamDelays.DynamicType` have consumers ported here; the
remaining consumers are recorded as residuals.

```ini
[AI]
AIAutoDeployMCV=true          ; Valueable<bool>
AISetBaseCenter=true          ; Valueable<bool>
AIBiasSpawnCell=false         ; Valueable<bool>
AIForbidConYard=false         ; Valueable<bool>
AINodeWallsOnly=false         ; Valueable<bool>
AICleanWallNode=false         ; Valueable<bool>

[General]
BuildingWaypoints=false       ; Valueable<bool>
FallingDownTargetingFix=false ; Valueable<bool>
BalloonHoverPathingFix=false  ; Valueable<bool>
TeamDelays.DynamicType=StartingPoint ; Valueable<DynamicTeamDelayType>
TeamDelays.Count1=0,0,0        ; Valueable<Vector3D<int>>
; Count2..Count8, same shape: X,Y,Z for 2..8 counted players

[CombatDamage]
MissileSpawnAttackCell=true   ; Valueable<bool>
```

Readers (`Ext/Rules/Body.cpp`, constructor defaults from `Ext/Rules/Body.h`):

| key | reader | default |
|---|---|---|
| `AIAutoDeployMCV` | `:370` | `true` (`Body.h:275,794`) |
| `AISetBaseCenter` | `:371` | `true` (`Body.h:276,795`) |
| `AIBiasSpawnCell` | `:372` | `false` (`Body.h:277,796`) |
| `AIForbidConYard` | `:373` | `false` (`Body.h:278,797`) |
| `AINodeWallsOnly` | `:374` | `false` (`Body.h:279,798`) |
| `AICleanWallNode` | `:375` | `false` (`Body.h:280,799`) |
| `BuildingWaypoints` | `:323` | `false` (`Body.h:326,835`) |
| `MissileSpawnAttackCell` | `:449` | `true` (`Body.h:359,864`) |
| `FallingDownTargetingFix` | `:451` | `false` (`Body.h:361,866`) |
| `BalloonHoverPathingFix` | `:540` | `false` (`Body.h:455,958`) |
| `TeamDelays.DynamicType` | `:606` | `StartingPoint` (`Body.h:510,1007`) |
| `TeamDelays.Count1..Count8` | `:608-613` | `{0,0,0}` (`Body.h:511,1008`) |

`DynamicTeamDelayType` is `Utilities/Enum.h`'s enum: `StartingPoint`, `PlayerCount`,
`Allies`, `Enemies`, `AliveCount`, `AliveAllies`, `AliveEnemies`, `None`, parsed by
`EnumParser<DynamicTeamDelayType>` (case-insensitive names, unknown keeps the
constructed value). `TeamDelays.CountN` is indexed by the count `1..8` and its
`X`, `Y`, `Z` by the house's AI difficulty `0`, `1`, `2` (Hard, Normal, Easy),
the same hardest-first order as `[General] TeamDelays=`; a row left at
`{0,0,0}` or a non-positive selected entry keeps the stock delay.

## BuildingWaypoints

`BuildingTypeClass::CanUseWaypoint` is a vtable entry (`0x7E4610`) that Phobos
replaces with the key (`Ext/Building/Hooks.cpp:877-883`):

```cpp
static bool __fastcall BuildingTypeClass_CanUseWaypoint(BuildingTypeClass* pThis)
{
	return RulesExt::Global()->BuildingWaypoints;
}
DEFINE_FUNCTION_JUMP(VTABLE, 0x7E4610, BuildingTypeClass_CanUseWaypoint)
```

`TechnoClass::CanUseWaypoint @ 0x0070F070` asks the type through `vt+0xA0`; with
the key on a building answers true, so `0x00639DA0`'s walk (PlanMgr's
`IsWaypointMode` selection check, reached from `0x00693187` and radar input
`0x00653CD1`) no longer refuses a selected building.

The display side (`Ext/Building/Hooks.cpp:885-898`, hook `0x4AE95E` -> `0x4AE982`)
still drops a building's non-`Attack` plan click: in planning mode a building's
`MouseOverCell` action only reaches `CellClickedAction` for `Attack`.

## MissileSpawnAttackCell

```cpp
DEFINE_HOOK(0x6B75AC, SpawnManagerClass_AI_SetDestinationForMissiles, 0x5)
{
	if (!RulesExt::Global()->MissileSpawnAttackCell || pTarget->WhatAmI() == AbstractType::Building)
		pSpawnTechno->SetDestination(pTarget, true);
	else
	{
		const CoordStruct coord = pSpawnManager->Target->GetCenterCoords();
		CellClass* pCellDestination = MapClass::Instance.TryGetCellAt(coord);
		pSpawnTechno->SetDestination(pCellDestination, true);
	}
	return 0x6B75BC;
}
```

(`Misc/Hooks.BugFixes.cpp:840-870`.) The key is on in Phobos' own default, so a
missile heads for its target's centre Cell; a Building target keeps the object so
that its footprint stays the aim point.

## Native anchors

- `UnitClass::Mission_Guard` deploy arm `0x007408C7..0x00740A19`, hook
  `0x00740A11` -> `0x00740A50` (`AIAutoDeployMCV`).
- `UnitClass::TryToDeploy @ 0x007393C0`, hook `0x00739889` -> `0x0073992B`
  (`AISetBaseCenter`).
- `HouseClass::AI_Building_Strategy @ 0x004FD500`, hook `0x004FD538`
  (`AIBiasSpawnCell`; nearest conyard of `GetSpawnPosition()`).
- `BuildingClass::Exit_Object @ 0x00443C60`, hook `0x004451F8` (`AIForbidConYard`
  -> `0x00445696`, `AICleanWallNode` -> `0x004454E6`).
- `BaseClass::GetBaseNodeIndex @ 0x0042EB50`, hook `0x0042EB8E` (`AIForbidConYard`
  -> invalid `0x0042EBAE`, else `0x0050CAD0`).
- `MapClass` wall fill `0x00588570` (FirestormWall `0x005885D1` -> `0x00588730`) and
  `0x00588750` (Wall `0x005887C1` -> `0x00588935`) (`AINodeWallsOnly`).
- `TechnoClass::SelectAutoTarget @ 0x006F8FE0`, hook `0x006F9398`
  (`FallingDownTargetingFix`); `ObjectClass::DropAsBomb 0x005F4160`,
  `ObjectClass::SpawnParachuted 0x005F5965`, `ObjectClass::Update 0x005F3F86`
  track and untrack the falling list.
- Balloon hover pathing `Misc/Hooks.BugFixes.cpp:3499` (`BalloonHoverPathingFix`).
- `HouseClass::Update_ResetTeamDelay @ 0x004F8ACC`, hook `0x004F8ACC` ->
  `0x004F8AD5` (`TeamDelays.DynamicType`; `Ext/House/Hooks.cpp:599-674`).

## VERA

`rules::phobos_globals::PhobosGlobals` carries the eleven switches and the eight
`team_delays_count` rows (`src/rules/phobos_globals.rs`), reads them in
`from_ini` by their own sections and defaults them in `Default`.
`src/rules/ini_value.rs` adds `PhobosDynamicTeamDelayType` with
`read_phobos_valueable_dynamic_team_delay_type`.

- `building_waypoints`: `sim::world::planning`'s `planning_selection_refusal`
  (`0x00639DA0`'s owner) admits a selected building when the key is on;
  `planning_press_refusal` and the radar caller pass `RuleSet` in, the
  display-side drop is not ported (see the residual below).
- `ai_auto_deploy_mcv`: `sim::mcv_deploy::guard_queues_unload` is
  `0x007408C7..0x00740A19`'s arm. Off with a non-empty
  `HouseState::build_const_order()` — the `NumConYards` owner, appended by
  `BuildingClass::Unlimbo`'s port and removed on expiry or owner change —
  refuses the Unload queue (`0x00740A11` -> `0x00740A50`); on or with no yard
  it keeps the current queue. Test
  `ai_auto_deploy_mcv_off_keeps_the_second_yard_out_of_the_guard_arm` covers
  both states.
- `ai_set_base_center`: `Simulation::deploy_mcv` samples the owner's
  `build_const_order()` before the new yard spawns, then runs the
  `set_base_center` / `refresh_ai_base_after_deploy` / `MoveGroundForcesToBase`
  block only when the key is on or the sampled count is at most one; the
  key-off two-yard case skips the whole `0x00739889..0x0073992B` range, as the
  hook does. Test
  `ai_set_base_center_off_skips_the_rewrite_only_above_one_yard` covers the
  boundary.
- `missile_spawn_attack_cell`: `sim::spawn_manager`'s `set_destination` sends a
  missile to the target's Cell when the key is on and the target is not a
  `EntityCategory::Structure`. The native oracle host opts out through
  `phobos_hooks()`, so its recorded vanilla calls stay comparable.
- `falling_down_targeting_fix`: `Simulation::phobos_falling_down` is the
  scenario tracker. `Simulation::drop_as_bomb`
  (`src/sim/world/lifecycle.rs`, `ObjectClass_DropAsBomb_Track 0x005F4160`) and
  `spawn_parachuted` (`src/sim/aircraft/drop_payload.rs`,
  `ObjectClass_SpawnParachuted_Track 0x005F5965`) add the Techno when the key
  is on; `greatest_threat` (`0x006F8DF0`'s owner) appends the tracked falling
  objects after the airborne pre-pass and before the AA-only `mask == 5` return
  (`TechnoClass_SelectAutoTarget_Scan_FallingDown 0x006F9398`,
  `Ext/Techno/Hooks.cpp:1704-1756`). The scan applies the native ally /
  `OnlyTargetHouseEnemy` gates, the `evaluate_candidate` receiver with
  `WalkArgs::flags | 0x8006` (Infantry/Unit/Aircraft class bits), appends
  `DistributedFireCandidate` entries and replaces the best score. Test
  `falling_down_targeting_fix_offers_a_tracked_fall_to_an_air_capable_guard`
  covers key-off invisibility and the key-on pick.
- `ai_forbid_conyard`: `sim::ai_base_building::exit_building` refuses a
  `ConstructionYard=` product when the key is on, after `find_node` and before
  any site or flush: `BasePlanState::mark_placed` writes the node's
  `filled = true` (`Placed`) and `retry_count = 0` (`Attempts`), and the exit
  answers `Failed` with the product left in limbo (`0x004451F8` ->
  `0x00445696`). `find_node` (`BaseClass::GetBaseNodeIndex 0x0042EB50`) skips
  a `filled` node whose type is a `ConstructionYard=` in both its `-1` and
  concrete-type branches (`0x0042EB8E` -> invalid), so such a node is never
  chosen again. Tests
  `ai_forbid_conyard_marks_the_open_node_placed_and_fails_the_exit`,
  `ai_forbid_conyard_off_still_places_the_second_yard` and
  `a_placed_conyard_node_is_not_open_only_with_the_key` cover both hooks and
  the key-off path.
- `ai_node_walls_only`: `sim::production::wall_placement::autofill_walls`
  (`0x00588750`'s owner) passes a `require_base_node` gate to
  `scan_autofill_direction` for each cardinal direction until one filler has
  been revealed. The gate compares the direction's first cell with the owner's
  `BasePlanState` nodes (`pack_base_plan_cell` + `type_or_control ==
  base_plan_type_index`, `0x005887C1` / `0x005885D1`); `autofill_cells` (the
  human preview) never gates. `ai_clean_wall_node`:
  `sim::ai_base_building::exit_building` (`0x00443C60`) refuses a `Wall=`
  product whose top-left eight-neighbourhood has no `protect_with_wall`
  building, forgetting the site and answering `Failed`
  (`0x004451F8` -> `0x004454E6`). Tests
  `ai_node_walls_only_gates_each_first_cell_until_one_wall_lands`,
  `ai_clean_wall_node_refuses_a_wall_without_a_protected_neighbour` and
  `ai_clean_wall_node_requires_a_protected_neighbour` cover the gate, key-off
  fill and both neighbour answers.
- `ai_bias_spawn_cell`: `sim::house_strategy` (`0x004FD500`'s owner) calls
  `bias_base_center_to_nearest_conyard` before its enemy/superweapon steps. With
  the key on outside a campaign and more than one `ConstructionYard=` in the
  house, it keeps the nearest alive, healthy, out-of-limbo yard by cell squared
  distance against the retained launch waypoint center, then writes
  `HouseState::base_center` (VERA's one published center; native's
  `BaseSpawnCell` and `Base.Center` collapse to it). Tests
  `ai_bias_spawn_cell_moves_the_center_to_the_nearest_live_yard`,
  `ai_bias_spawn_cell_skips_limbo_and_dead_yards` and
  `ai_bias_spawn_cell_needs_two_yards_and_a_non_campaign` cover the nearest
  pick, the live filter and the key/campaign/one-yard boundaries.
- `team_delays_dynamic_type`: `sim::ai_team_creation::dynamic_team_delay` is
  `HouseClass_Update_ResetTeamDelay 0x004F8ACC`'s owner. It leaves the stock
  path when the session is a campaign, the type is `None`, or
  `ScenarioSession::multiplayer_start_count` (the retained non-zero
  multiplayer start waypoints, `0..=7`) is below two. `StartingPoint` uses that
  count; every other type iterates houses and applies the `Alive*` `Defeated`
  gate, the observer/`MultiplayPassive` skip, and the `Allies`/`Enemies`
  alliance filters. A count outside `1..=8` or a non-positive X/Y/Z entry
  returns `None`; otherwise the house's difficulty selects the column.
  `update_team_creation` computes the stock `[General] TeamDelays=` fallback
  with the commander `TeamDelayPercent` scaling, then restarts the timer with
  the dynamic value when it returns one. Tests
  `team_delays_count_rows_select_the_ai_difficulty_column`,
  `player_count_reads_the_count_row_for_the_houses_difficulty`,
  `an_absent_count_row_leaves_the_stock_team_delay`,
  `one_start_waypoint_leaves_the_stock_team_delay`,
  `alive_enemies_counts_only_living_non_allied_houses` and
  `none_and_campaign_keep_the_stock_team_delay` cover the column, house-count,
  filter and fallback boundaries.

## Residuals

- `FallingDownTargetingFix` boundaries: the native hook's `skip` reads the
  `ThreatType::Air | ThreatType::Range` argument, which VERA's scan does not
  carry; the port gates on an air-capable non-AA-only mask (`mask & 4 != 0`,
  `mask != 5`). `ObjectClass_Update_Track 0x005F3F86`'s eager remove is a scan
  filter on `is_falling_down` instead of a `finish_fall` hook, and the tracker
  is `serde(skip)`/unhashed: after a save load it rebuilds in entity-store order.
  `CanAutoTargetObject`'s `wantedDist` term is only `evaluate_candidate`'s
  existing gates, so a tracked fall is considered globally rather than
  ring-local. Trigger: a scan with the key on. Effect: some fall picks and tie
  order may differ from native; no retail key means dormant.
- `BalloonHoverPathingFix` (`Misc/Hooks.BugFixes.cpp:3499`): VERA's hover
  locomotor pathing does not implement the stock shortcut the hook removes.
  Trigger: a balloon/hover object pathing beyond its hover range. Effect: VERA
  takes the uncorrected path in both key states. Frequency: hover movement only.
  Risk: a mod relying on the stock shortcut sees the corrected route; no retail
  object is player-visible here.
- `TeamDelays.DynamicType` boundaries: the hook's first branch is the per-house
  `HouseExt::TeamDelay >= 0` override, whose only writer in Phobos is trigger
  action `SetTeamDelay` (`Ext/TAction/Body.cpp:759-765`,
  `HouseExt::Fetch(pHouse)->TeamDelay = value`). VERA does not port that action
  yet, so no house carries an override and the dynamic or stock path always
  runs. The hook's neutral test is `HouseClass::IsNeutral`; VERA substitutes the
  `MultiplayPassive=` house flag because it has no separate neutral flag. A
  `SetTeamDelay` mod therefore sees the dynamic pacing instead of its fixed
  override, and a map marking a house neutral without `MultiplayPassive=` counts
  it. Trigger: a mod using `SetTeamDelay`, or a non-retail neutral house.
  Frequency: per AI team reset in such a mod. Risk: pacing differs there; retail
  authors neither key.
- `BuildingWaypoints`' display half (`0x4AE95E`): VERA's planning mode does not
  run a building's planning click, so no building receives a non-attack plan.
  Trigger: a building selected in planning mode with the key on. Effect: native
  skips the click (`0x4AE982`) except for `Attack`; VERA never issues one.
  Frequency: player action. Risk: a `BuildingWaypoints` mod expects a building to
  accept a move-plan icon, which VERA still refuses at the click.
- Retail rules author none of these switches or `Count` rows, so the constructed
  defaults reproduce stock; `AIAutoDeployMCV`, `AISetBaseCenter` and
  `MissileSpawnAttackCell` are on by default and are the three whose Phobos
  behaviour retail already sees. `TeamDelays.DynamicType` defaults to
  `StartingPoint`, but `Count1..8` all stay `{0,0,0}`, so retail keeps
  `[General] TeamDelays=` unless a map or mod supplies a row.
