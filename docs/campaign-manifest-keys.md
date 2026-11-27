---
layout: default
title: Campaign manifest keys
---

# Campaign manifest keys

**English** · [简体中文](zh-CN/campaign-manifest-keys.md) · [Deutsch](de/campaign-manifest-keys.md) · [العربية](ar/campaign-manifest-keys.md) · [Русский](ru/campaign-manifest-keys.md) · [Svenska](sv/campaign-manifest-keys.md) · [ไทย](th/campaign-manifest-keys.md) · [Türkçe](tr/campaign-manifest-keys.md)

Read the sections in `campaign.ini`. Replace `<id>` with the stage ID. Boolean keys accept `Yes`, `No`, `True`, `False`, `1` and `0`. The table gives the defaults when a stage comes from this manifest. A dash means the key is absent. Omit an unused optional entry instead of giving it an empty value. [Setup, examples and local checks](campaign-manifest.md)

## `[Campaign]`

| Key or entry | Values | Default | When it takes effect |
| --- | --- | --- | --- |
| `Title` | Text | Series directory | The room’s mode list uses this label when the client builds the installed series list. Example `Title=Broken Line`. |
| `Title.<locale>` | Text | `Title` | The series list uses this translation for the matching interface locale. Example `Title.zh-CN=断线`. It falls back to the base language and then `Title`. |

## `[Stage <id>]`

| Key or entry | Values | Default | When it takes effect |
| --- | --- | --- | --- |
| `Map` | Relative path | Required | The room and loader open this file from the series directory. Use an existing authored map, for example `Map=maps/opening.map`. A path that escapes the series is refused. |
| `MapSha256` | 64 hex digits | — | On map verification, compare the file’s SHA-256 against this value when present. Players compare their map digests even when this key is omitted. |
| `MinPlayers` | 1–128 | `1` | Before starting, require at least this many playing connections. For a two-role mission that needs both players, set `MinPlayers=2`. Watchers do not count. |
| `MaxPlayers` | 1–128 | Role and room limits | During room admission, cap the number of players. The effective maximum is the smallest of this value, the available roles (one for a stage without roles) and room capacity. Use `MaxPlayers=2` for two seats. |
| `Title` | Text | `ID` | The stage picker and result screen show this label. Example `Title=Breakthrough`. |
| `Title.<locale>` | Text | `Title` | The stage picker and result screen use this translation for the matching locale. Example `Title.zh-CN=突破`. Fallback follows `[Campaign] Title.<locale>`. |

## `[Roles <id>]`

| Key or entry | Values | Default | When it takes effect |
| --- | --- | --- | --- |
| `RoleId=HouseId` | Map House ID | `[Basic] Player` for one player | On room setup and start, bind one playing role to an ID in the map’s `[Houses]` list. `Lead=Americans` and `Support=Americans` give two separate roles the same primary House. Assign each role to only one player. |

## `[Grants <id>]`

| Key or entry | Values | Default | When it takes effect |
| --- | --- | --- | --- |
| `RoleId.HouseId` | `FullControl` / `UnitsOnly` | — | At startup, grant an extra scope to a declared role for a different target House. `Lead.Russians=FullControl` adds tactical and full-control orders. `Support.Russians=UnitsOnly` adds tactical orders. Primary-House-only commands continue to use the role’s primary House. |

## `[Routes <id>]`

| Key or entry | Values | Default | When it takes effect |
| --- | --- | --- | --- |
| `BranchId=StageId` | Declared stage ID | — | On manifest read, resolve the destination to an existing stage. The result dialog shows it under Continue, for example `Coast=Finale`. `Retry` is reserved for the current stage. Omit it to use the built-in Retry and its entry record. |

## `[CarryOver <id>]`

| Key or entry | Values | Default | When it takes effect |
| --- | --- | --- | --- |
| `Globals` | `Yes` / `No` | `No` | On continuation, keep the departing stage’s global trigger variables. `Globals=Yes` forwards their current values to the next map before its first frame. |
| `Countdown` | `Yes` / `No` | `No` | Capture the remaining campaign countdown at stage end. Set `Countdown=Yes` here and `[Basic] TimerInherit=Yes` in the destination map. |
| `MapStage` | `Yes` / `No` | `No` | Capture the outgoing map’s numeric campaign stage word. With `MapStage=Yes`, the next map receives that value. Stage IDs and routes are resolved separately. |
| `House.<id>` | `Credits` / `Difficulty` / `Credits,Difficulty` | — | At stage end, capture the named House’s available money, difficulty, or both. `House.Americans=Credits,Difficulty` requires `Americans` on both maps. The destination’s `CarryOverMoney` and `CarryOverCap` control the credit addition. |

## `[Rules <id>]`

| Key or entry | Values | Default | When it takes effect |
| --- | --- | --- | --- |
| `Bootstrap` | `Campaign` | `Campaign` | Select map House construction at startup. Write `Bootstrap=Campaign` explicitly if desired. |
| `AllowWinTags` | `RequireClear` / `Ignore` | `RequireClear` | At the victory check, `RequireClear` waits until the House’s AllowWin tag count clears. `Ignore` skips that count check. The outcome timer still applies. |
| `ComputerParanoid` | `Disabled` / `React` | `Disabled` | When the AI handles requests to reconsider enemies or alliances, `React` lets it enter Computer_Paranoid logic. Its normal House and rules checks still apply. |
| `Crates` | `Keep` / `Skip` | `Keep` | When loading overlays, `Keep` preserves map-placed crates and `Skip` drops those crates. Use `UsesMatchCrates` for timed match crates. |
| `AiBase` | `RetainAuthoredNodes` / `ReplaceUnavailableNodes` | `RetainAuthoredNodes` | When the AI processes its base plan, `RetainAuthoredNodes` keeps the authored placement nodes. `ReplaceUnavailableNodes` allows replacement of unavailable or repeatedly failed nodes under the normal AI rules. |
| `DoubleOwned` | `OwnerMask` / `AllCountries` | `OwnerMask` | During factory selection, `OwnerMask` uses the normal country owner mask. `AllCountries` broadens that mask for types with `DoubleOwned=Yes`. |
| `TeamCreationLimitScope` | `AllHouses` / `PerHouse` | `AllHouses` | When creating a team, count existing instances toward the TeamType’s `Max=` across all Houses or only the creating House. |
| `CountsPlayerControlAsHuman` | `No` | `No` | Campaign startup identifies human Houses from admitted roles. `CountsPlayerControlAsHuman=No` makes that choice explicit. |
| `AllowsConstructionYardRedeploy` | `Yes` / `No` | `No` | When a player attempts to pack up a Construction Yard, permit the redeploy path for an eligible yard. The yard and game-option checks still apply. |
| `AllowsIdleObjectSleep` | `Yes` / `No` | `No` | During frame updates, let eligible idle objects sleep under the usual trigger and repair guards. Use this when a map has many inert objects. |
| `AllowsOwnerlessTerrainLogic` | `Yes` / `No` | `Yes` | When updating terrain objects, include ownerless entries that the native identity check would otherwise skip. |
| `UsesCountryDifficultyBiases` | `Yes` / `No` | `No` | When applying House difficulty, combine its difficulty factors with the selected country’s modifiers. |
| `SkipsLocalPlayerAuthoredBuildings` | `Yes` / `No` | `No` | The structure loader checks this flag only for local execution. Authored lockstep loads the map’s placed buildings. Remove a building from the map to omit it in this mode. |
| `UsesManagedAiBase` | `Yes` / `No` | `No` | During play, opt eligible AI Houses into automatic construction, base placement and deployment. This is separate from how failed nodes are replaced. |
| `UsesAiEconomy` | `Yes` / `No` | `No` | During AI economy updates, apply the multiplayer ore-payment rules and eligible AI building sale behavior. The map and rules still supply the rates and eligibility. |
| `RunsAiStrategy` | `Yes` / `No` | `No` | During strategy updates, allow periodic AI enemy searches and autonomous build and strategy decisions. |
| `UsesHouseTeamRules` | `Yes` / `No` | `No` | On AI team load and activation, select House-based trigger admission. Difficulty and TechLevel checks follow the House/team path rather than the normal campaign trigger rules. |
| `UsesCampaignDiscovery` | `Yes` / `No` | `Yes` | Use campaign discovery checks in sight and targeting logic. The single-client discovery path also requires local execution. Use the map’s alliance settings to control inter-House vision. |
| `UsesCampaignBaseDefense` | `Yes` / `No` | `Yes` | During a base attack, choose campaign rules for the attacked object’s response and which defenders react. |
| `UsesDirectedHunterSeekerTargeting` | `Yes` / `No` | `No` | For an Ares Hunter Seeker launch, allow an AI House to use its designated enemy. A human House can prefer nonpassive targets when `RandomOnly` permits it. |
| `UsesMatchCrates` | `Yes` / `No` | `No` | Use match crate processing during play, including timed slots and validated pickups. Automatic spawning also requires the match’s Crates game option. |
| `UsesEliminationResults` | `Yes` / `No` | `No` | At the result check, use House elimination to decide the match and perform its cleanup. With the default, the stage can use authored victory and defeat triggers. |
| `UsesRoomOptions` | `Yes` / `No` | `No` | With `Yes`, the room supplies harvester immunity and both ore flags. With `No`, the stage reads its map’s `[SpecialFlags]`. Omitted keys use shared scenario defaults, including `TiberiumGrows=No` and `TiberiumSpreads=Yes`. |
| `UsesCampaignDiplomacy` | `Yes` / `No` | `Yes` | At the Diplomacy button, select campaign briefing behavior. This policy also selects campaign handling of passive or civilian Houses in diplomacy. See `NetworkDiplomacyBriefing` for the authored role panel. |
| `AllowsManualAlliances` | `Yes` / `No` | `No` | When players issue an Ally order, allow the order admission path. The independent alliance and control checks still apply. |
| `ModMapAdditions` | `Yes` / `No` | `No` | Before graphical map loading, allow the selected mod’s additions to be applied to the map. Ship the same mod data to each player. For a standalone map and the headless author check, keep required map settings in the authored files. |
| `SharedAlliedVictory` | `Yes` / `No` | `No` | When used with lockstep elimination results, apply the shared-win check for eligible allied survivors. Scripted stage results use their own trigger path. |
| `SkipsNetworkScoreDialog` | `Yes` / `No` | `No` | When a lockstep mode reaches the normal multiplayer score dialog, skip it. Authored campaign continuation has its own result dialog. |
| `ParanoidIgnoresAlliesAllowed` | `Yes` / `No` | `No` | When `ComputerParanoid=React` reaches its alliance check, let it pass the `AlliesAllowed` gate. This does not grant players permission to change alliances. |
| `NetworkDiplomacyBriefing` | `Yes` / `No` | `No` | At the in-game Diplomacy button, open the authored briefing and read-only role/House authority view. Set `NetworkDiplomacyBriefing=Yes` and write `[Basic] Briefing` on the map. |
| `StartingHouseRoleAlliances` | `Yes` / `No` | `No` | When the slot-based setup constructs Houses, ally starting Houses with matching human or AI classifications. For authored MapHouses, declare starting alliances in the map. |
| `AresPsychicRevealExclusions` | `Yes` / `No` | `No` | While an Ares psychic reveal runs in lockstep, preserve the three map-relative exclusion cells used by the competitive path. |
| `DissolveUnfilledTeams` | `Yes` / `No` | `No` | During team AI updates, let an empty team that has never filled expire after `DissolveUnfilledTeamDelay`. The delay is set in the game rules. |
