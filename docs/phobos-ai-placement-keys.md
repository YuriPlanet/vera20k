# AI placement and bug-fix switches

Eleven Phobos keys that rescan found without a reader: six `[AI]` placement
switches, four `[General]` bug-fix switches and one `[CombatDamage]` key. Two of
them (`BuildingWaypoints`, `MissileSpawnAttackCell`) have their consumer ported
here; the rest are read and recorded as residuals.

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

`DynamicTeamDelayType` is `Utilities/Enum.h`'s enum: `StartingPoint`, `PlayerCount`,
`Allies`, `Enemies`, `AliveCount`, `AliveAllies`, `AliveEnemies`, `None`, parsed by
`EnumParser<DynamicTeamDelayType>` (case-insensitive names, unknown keeps the
constructed value).

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
- `HouseClass::ResetTeamDelay @ 0x004F8ACC` (`TeamDelays.DynamicType`).

## VERA

`rules::phobos_globals::PhobosGlobals` carries all eleven fields
(`src/rules/phobos_globals.rs:529-600`), reads them in `from_ini` by their own
sections (`:2169-2196`) and defaults them in `Default` (`:1676-1690`).
`src/rules/ini_value.rs` adds `PhobosDynamicTeamDelayType` with
`read_phobos_valueable_dynamic_team_delay_type` (`:1069-1080`, `:2218-2250`).

- `building_waypoints`: `sim::world::planning`'s `planning_selection_refusal`
  (`0x00639DA0`'s owner) admits a selected building when the key is on;
  `planning_press_refusal` and the radar caller pass `RuleSet` in, the
  display-side drop is not ported (see the residual below).
- `missile_spawn_attack_cell`: `sim::spawn_manager`'s `set_destination` sends a
  missile to the target's Cell when the key is on and the target is not a
  `EntityCategory::Structure`. The native oracle host opts out through
  `phobos_hooks()`, so its recorded vanilla calls stay comparable.

## Residuals

- `AIAutoDeployMCV` (`0x00740A11`): VERA has no con-yard count owner
  (`HouseClass::NumConYards`); its MCV deploy path (`sim::mcv_deploy`,
  `0x00738D30`) decides from the deployment site alone. Trigger: a computer house
  guarding an MCV while it already holds a Construction Yard. Effect: stock
  skips the deploy attempt (`0x00740A50`) and Phobos' default lets it deploy;
  VERA behaves as if the key were on always, which is Phobos' default but wrong
  for a mod that clears it. Frequency: every AI MCV, once per house. Risk: a
  second Construction Yard in an AI base that retail stock would not build.
- `AISetBaseCenter` (`0x00739889`): same missing `NumConYards`; VERA's
  `house_state::set_base_center` is called unconditionally on deploy. Trigger:
  deploy with more than one conyard. Effect/frequency/risk as above.
- `AIBiasSpawnCell` (`0x004FD538`): VERA's `sim::house_strategy` has no
  `GetSpawnPosition()` reader and no conyard enumeration, so the nearest-conyard
  base-centre rewrite does not run. Trigger: an AI house with two or more
  conyards in a non-campaign game. Effect: the base centre and spawn cell stay
  on the scenario waypoint instead of the conyard nearest it. Frequency: once per
  house AI update. Risk: AI production concentrates on the far conyard; player
  visible in long AI games.
- `AIForbidConYard` (`0x004451F8`, `0x0042EB8E`): VERA's `sim::ai_base_building`
  and `sim::base_plan` place nodes without the `Placed`/`Attempts` pair the hooks
  write, and `BaseClass::Find_Node`'s two refusals are not ported. Trigger: a
  key-enabled mod where an AI base plan still holds a Construction Yard node.
  Effect: the AI rebuilds/buys a conyard the key forbids. Frequency: base-plan
  rebuild only. Risk: none in retail (the key is absent, so the branch is
  dormant).
- `AINodeWallsOnly` (`0x005885D1`, `0x005887C1`) and `AICleanWallNode`
  (`0x004451F8`): VERA's wall fill (`sim::production::wall_placement`,
  `0x00588750`'s owner) expands every cardinal direction from the origin and has
  no base-node list to test the visited cell against, and its placement loop has
  no eight-neighbour `ProtectWithWall` test. Trigger: an AI house filling walls.
  Effect: walls grow past non-node cells, and a wall node with no protecting
  neighbour is still placed. Frequency: every wall placement an AI makes when its
  key is set. Risk: dormant in retail; visible wall spam in a mod that sets it.
- `FallingDownTargetingFix` (`0x006F9398`): VERA tracks the falling flag per
  object (`GameEntity::is_falling_down`, `ObjectClass +0x8D`) but has no
  scenario-level falling list, and `sim::combat::greatest_threat` does not add
  falling objects as candidates. Trigger: an object falling from `DropAsBomb`
  (`0x005F4160`) or a paradrop (`0x005F5965`) while an AA scanner looks for
  targets. Effect: the falling object is never scored, so AA units ignore it
  until it lands. Frequency: every shot-down aircraft and paradropped unit.
  Risk: falling units pass through an AA screen unharmed in mods that set the key.
- `BalloonHoverPathingFix` (`Misc/Hooks.BugFixes.cpp:3499`): VERA's hover
  locomotor pathing does not implement the stock shortcut the hook removes.
  Trigger: a balloon/hover object pathing beyond its hover range. Effect: VERA
  takes the uncorrected path in both key states. Frequency: hover movement only.
  Risk: a mod relying on the stock shortcut sees the corrected route; no retail
  object is player-visible here.
- `TeamDelays.DynamicType` (`0x004F8ACC`): `HouseClass::ResetTeamDelay` itself is
  not ported; VERA keeps Ares' `TeamDelayPercent` scaling in
  `rules::personality::scale_team_delay` and starts teams from the retail delay.
  Trigger: any AI team launch after a reset. Effect: the per-type refill (player
  count, allies, enemies, alive counts) never replaces the flat delay. Frequency:
  every team the AI launches. Risk: AI attack pacing differs from a mod that sets
  `PlayerCount` or `AliveEnemies`.
- `BuildingWaypoints`' display half (`0x4AE95E`): VERA's planning mode does not
  run a building's planning click, so no building receives a non-attack plan.
  Trigger: a building selected in planning mode with the key on. Effect: native
  skips the click (`0x4AE982`) except for `Attack`; VERA never issues one.
  Frequency: player action. Risk: a `BuildingWaypoints` mod expects a building to
  accept a move-plan icon, which VERA still refuses at the click.
- Retail rules author none of the eleven keys except where noted, so the
  constructed defaults reproduce stock; `AIAutoDeployMCV`,
  `AISetBaseCenter` and `MissileSpawnAttackCell` are on by default and are the
  three whose Phobos behaviour retail already sees.
