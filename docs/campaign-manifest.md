---
layout: default
title: Campaign series
---

# Campaign series

**English** · [简体中文](zh-CN/campaign-manifest.md) · [Deutsch](de/campaign-manifest.md) · [العربية](ar/campaign-manifest.md) · [Русский](ru/campaign-manifest.md) · [Svenska](sv/campaign-manifest.md) · [ไทย](th/campaign-manifest.md) · [Türkçe](tr/campaign-manifest.md)

A series lives in `campaigns/<series>/campaign.ini`. Each stage names a map and can set rules, roles, routes and the state to carry into the next stage. The guide starts with one map. Add roles when the stage needs more players.

Use `asset campaign-check` to run a stage without opening a room. For a playable session, an authored stage can run in a room with one player or with one player per assigned role. Internet rooms also need lobby support. The ordinary **Campaign** menu uses the game’s `battlemd.ini` campaign list.

[Key reference](campaign-manifest-keys.md) lists the accepted values, defaults and read points for every section.

## Start with one stage

Start with this series layout.

```text
campaigns/broken-line/
  campaign.ini
  maps/opening.map
```

```ini
[Stage Opening]
Map=maps/opening.map
```

With this file, the authored stage uses the House named by the map’s `[Basic] Player`. The author check runs it with one local source. A room with the same stage has one playing seat.

Save `campaign.ini` as UTF-8 and keep it within 64 KiB. Semicolons start comments. Section names, keys, stage IDs, role IDs and branch IDs compare without regard to ASCII case. IDs use ASCII letters, digits, underscores and hyphens. The map path is relative to the series directory and uses forward slashes. Both the manifest and its maps must resolve inside the series. Use ordinary file names, without drive letters, parent-directory segments or reserved Windows device names such as `CON` and `COM1`.

Title lookup uses the exact locale, the base language, then `Title`. For example, `Title.zh-CN` precedes `Title.zh`. The setup lists sort by resolved title. Ship one manifest with all translations. The manifest digest includes comments and line endings, and the room compares it along with the exact map SHA-256 and the digest of the expanded map after includes.

## Run the author check

```text
asset campaign-check campaigns/broken-line/campaign.ini Opening 120 --ra2-dir "/path/to/Yuri's Revenge"
```

Use the stage ID and a positive frame count. The tool loads the authored map and checks each referenced House. It runs the stage at Normal difficulty with a fixed seed, rebuilds it from the diagnostic recording, and checks every requested frame. The result is JSON with `stage`, `manifest_sha256`, `map_sha256`, `checked_frames` and `final_state_hash`. Run the check for each destination, using a frame count the stage can reach.

A manifest error gives the file, section, key, line number and a suggested fix. For **grant names an undeclared role**, check the part before the dot against `[Roles <id>]`. For an unknown House, check the map’s actual `[Houses]` IDs. For a file mismatch in the room, compare the two digests and check any included map INIs. Run a LAN session to check actual role selection, cutscenes and route votes.

## Add roles and a second stage

The opening and finale maps in this example each need `Americans` and `Russians` in `[Houses]`. Both stages seat two players. Add matching roles to every destination that those players can reach.

```ini
[Campaign]
Title=Broken Line
Title.zh-CN=断线

[Stage Opening]
Map=maps/opening.map
Title=Breakthrough
Title.zh-CN=突破
MinPlayers=2
MaxPlayers=2

[Roles Opening]
Lead=Americans
Support=Americans

[Grants Opening]
Lead.Russians=FullControl
Support.Russians=UnitsOnly

[Rules Opening]
NetworkDiplomacyBriefing=Yes

[Routes Opening]
Coast=Finale

[CarryOver Opening]
Globals=Yes
Countdown=Yes
House.Americans=Credits,Difficulty
House.Russians=Credits

[Stage Finale]
Map=maps/finale.map
MinPlayers=2
MaxPlayers=2

[Roles Finale]
Lead=Americans
Support=Americans
```

`Lead` and `Support` have full authority over their primary House, `Americans`. `Lead` can also issue tactical orders and commands that require full control over `Russians`. `Support` can issue tactical orders for `Russians`. An extra grant belongs to its role for that stage. Declare it again in the finale if the role needs it there.

The result screen labels each branch with its destination stage title. The built-in **Retry** uses the current stage and its incoming carry-over. The name Retry is reserved for the current stage. Omit it to use the built-in action. Use a different branch ID for a route shown under **Continue**. A closed loop can revisit a stage and apply the outgoing record.

`Globals`, `Countdown` and `MapStage` accept booleans and default to `No`. `House.<id>` takes `Credits`, `Difficulty`, or both as a comma-separated list. Each receiving map must contain the carried House IDs. Its `[Basic] CarryOverMoney` scales the credit addition, and `CarryOverCap` caps it. Use `CarryOverMoney=100%` and `CarryOverCap=-1` to add the full carried amount to the receiving House’s budget. Set `TimerInherit=Yes` in the receiving map to admit a saved countdown.

### Rule overrides

Set `NetworkDiplomacyBriefing=Yes` to show the mission and read-only role and authority summary under **Diplomacy**. Put the briefing in `[Basic] Briefing` or the scenario’s briefing in `MISSIONMD.INI`. Use `@` for a line break in map text.

## Install and open a room

Put the series under `campaigns/<series>/` in the game folder or the active mod. Every player needs the same game build, rules and campaign files. Keep the directory layout, including any files included by the maps. If both locations contain a series with the same name, the active mod supplies it.

1. Host a LAN room. In the mode list, choose the campaign by its title. **Standard multiplayer** returns to the regular game modes. The campaign list appears when the host has at least one readable `campaign.ini`.
2. Pick a stage and difficulty. If the stage lists roles, assign one per player in the **Role** column. Two different roles can use the same House. A stage without a role list uses `[Basic] Player` and seats one player.
3. Start when the room is ready. A missing role, mismatched files or a player count outside the stage limits keeps the match from starting.

Internet rooms require a WOL lobby advertising `campaign_rooms` and forwarding the campaign options at admission, at start and to watchers. Use LAN if the Internet lobby reports that campaign rooms are unavailable. [Networking reference](Networking.md#multiplayer-campaign-rooms)

A role can acquire tactical authority over other Houses. An author can add a read-only briefing and role overview to the in-game **Diplomacy** button. The **Role** column is for players. Connection-level watchers have no role and do not count against stage capacity. If the host has seated spectators in the regular lobby rows, clear those rows before starting the campaign. Use the lobby’s **Watch** action where it is available.

An in-game cutscene can hold the match until the players reach the same point. At the result screen, **Retry** starts the stage from its entry record. **Other route** cycles through the available destinations. **Continue** votes for the displayed destination and carries the outgoing stage’s selected state. Every player in the result vote must choose the same action. Watchers can leave during the vote.

## Replay a played stage

Use the game build and assets matching the recording. Keep the map at its recorded path under `campaigns/<series>/`. A diagnostic recording contains the admitted roles, grants and carry-over, as well as the simulation inputs used during cutscene holds.

```text
vera20k --replay replays/session.json --ra2-dir "/path/to/Yuri's Revenge"
vera20k --replay-verify replays/session.json --ra2-dir "/path/to/Yuri's Revenge"
```

The first command opens playback. The second verifies it without a window or a lobby connection and prints the final state hash. The headless verifier uses retail rules. Keep a new diagnostic recording if the game reports an incompatible version. [Diagnostic replay format](Networking.md#diagnostic-replays)
