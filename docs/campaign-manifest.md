# Author a multiplayer campaign manifest

Place `campaign.ini` in `campaigns/<series>/` with the maps for that series.
A single authored stage can use the map's `[Basic] Player` House and the stock
campaign policy.

```ini
[Stage Opening]
Map=maps/opening.map
```

Use UTF-8 and keep the manifest within 64 KiB. Section names, keys, stage IDs and
role IDs are ASCII case-insensitive. Stage, role and branch IDs can contain
letters, digits, underscores and hyphens. Semicolons introduce comments.
Errors identify the file, section, key and 1-based line, and give a fix hint.

The map path uses forward slashes and stays inside the series directory.
An optional `MapSha256` is the 64-digit SHA-256 of that authored map file.
The manifest has a separate SHA-256 over its exact bytes, including comments
and line endings.

## Roles, rules and branches

The remaining sections are optional. Add them to the stage by its ID.

```ini
[Campaign]
Title=Broken Line
Title.zh-CN=断线

[Stage Opening]
Map=maps/opening.map
Title=Breakthrough
Title.zh-CN=突破

[Roles Opening]
Lead=Americans
Support=Americans

[Grants Opening]
Lead.Russians=FullControl
Support.Russians=UnitsOnly
Lead.Confederation=UnitsOnly

[Rules Opening]
UsesMatchCrates=Yes
AllowWinTags=RequireClear

[Routes Opening]
Win=Finale
Retry=Opening

[CarryOver Opening]
Globals=Yes
Countdown=Yes
House.Americans=Credits,Difficulty
House.Russians=Credits

[Stage Finale]
Map=maps/finale.map
```

- `Title.<locale>` takes precedence over the base language and then `Title`.
  Stage lists sort by this resolved title, using a stable Unicode case-insensitive
  sort key. The series directory name and stage ID are the fallback titles.
- Roles use the exact House IDs from the map's `[Houses]` list. Source indices
  follow role declaration order. Roles may share a primary House, and a grant
  applies to one role and one other House.
- Routes resolve author-named branches to stage IDs in the series. A route may
  point back to the same stage.
- Carry-over selects what to capture when leaving the stage. `Globals`,
  `Countdown`, and `MapStage` are booleans. Each `House.<id>` takes
  `Credits`, `Difficulty`, or both. The receiving map controls the credit
  multiplier and cap using `[Basic] CarryOverMoney` and `CarryOverCap`.
  Set `CarryOverMoney=100%` and `CarryOverCap=-1` for uncapped 1:1 credits.
  The receiving map's `TimerInherit=Yes` admits the countdown.

## Rule overrides

Only declared keys override the stock campaign preset. Boolean keys accept
`Yes` or `No` (also `True` or `False`, and `1` or `0`).

| Selector | Values |
| --- | --- |
| `Bootstrap` | `Campaign` for the authored House roster |
| `AllowWinTags` | `RequireClear`, `Ignore` |
| `ComputerParanoid` | `Disabled`, `React` |
| `Crates` | `Keep`, `Skip` |
| `AiBase` | `RetainAuthoredNodes`, `ReplaceUnavailableNodes` |
| `DoubleOwned` | `OwnerMask`, `AllCountries` |
| `TeamCreationLimitScope` | `AllHouses`, `PerHouse` |

The boolean keys are
`AllowsConstructionYardRedeploy`, `AllowsIdleObjectSleep`,
`AllowsOwnerlessTerrainLogic`, `CountsPlayerControlAsHuman`,
`UsesCountryDifficultyBiases`, `SkipsLocalPlayerAuthoredBuildings`,
`UsesManagedAiBase`, `UsesAiEconomy`, `RunsAiStrategy`, `UsesHouseTeamRules`,
`UsesCampaignDiscovery`, `UsesCampaignBaseDefense`,
`UsesDirectedHunterSeekerTargeting`, `UsesMatchCrates`,
`UsesEliminationResults`, `UsesRoomOptions`, `UsesCampaignDiplomacy`,
`AllowsManualAlliances`, `ModMapAdditions`, `SharedAlliedVictory`,
`SkipsNetworkScoreDialog`, `ParanoidIgnoresAlliesAllowed`,
`NetworkDiplomacyBriefing`, `StartingHouseRoleAlliances`,
`AresPsychicRevealExclusions`, and `DissolveUnfilledTeams`.

## Validate a stage

```text
asset campaign-check campaigns/broken-line/campaign.ini Opening 120 --ra2-dir /path/to/retail
```

The command validates the manifest and selected map, runs the stage headlessly
with a fixed seed, then reconstructs it from the captured replay blueprint.
It compares the simulation hash at each frame and prints JSON with the
manifest and map SHA-256, checked frame count, and final hash.
