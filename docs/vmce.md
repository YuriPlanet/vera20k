---
layout: default
title: VMCE LAN manual
lang: en
---

# VMCE LAN manual

VMCE plays authored multiplayer campaign stages in a VERA20k LAN room. Use the same VERA20k build, gameplay features and effective rules on each client. The host and guests install identical copies of `campaign.ini` and the selected stage file. [简体中文](zh-CN/vmce-manual.md) · [Interface walkthrough](#interface-walkthrough)

## How the modes play

| Mode | Players and starting state | Results and progression |
|---|---|---|
| Original single-player campaign | One local player selects a campaign listed by `BATTLEMD.INI`. Its map identifies the starting House in `[Basic] Player` and places the starting forces. A campaign can also mark support Houses with `PlayerControl`. | The campaign's victory and defeat dialogs follow its map and campaign progression, including campaign carryover where configured. |
| Original Skirmish | One local human sets up a standalone match with computer opponents. Battle, Free For All and Team Alliance use the ordinary map starts and setup options. | The match has its own outcome and score. A new match takes its settings from setup. |
| Original Cooperative | The Cooperative multiplayer mode has two human positions. `CoopCampMD.ini` supplies the mission variants and the countries for player and enemy positions. The sidebar button restates the mission briefing and `Tab` opens Diplomacy. | Cooperative campaign results track the pair's stage progression and record their wins in `coopsave.ini`. |
| VMCE | A LAN host selects an installed authored stage. Each playing role maps to its own named House. The stage controls participant limits, optional observer seats, units, AI, triggers and shared grants. `Tab` and the sidebar button open the mission briefing with an entry to Diplomacy. | Victory opens the campaign-style `0x103` medal page, with statistics available from its right sidebar. Defeat uses the VMCE statistics and Retry actions over `0xBC6`. The host can reopen the same room for the next stage or a new selection. |

[Campaign definitions](../src/rules/campaigns.rs) · [Original mode policies](../src/skirmish_modes.rs) · [Cooperative setup](../src/app/frontend/skirmish_session/cooperative.rs) · [Diplomacy entry](../src/app/input/diplomacy.rs)

## Install and start

Put each series in its own folder under `<settings_dir>/Maps/VMCE/`. The VERA20k settings directory is the directory selected by `[paths] settings_dir` in `config.toml` or the default user settings location.

```text
Maps/VMCE/Example/
├── campaign.ini
├── entry.map
└── ra2md.csf
```

The host creates a LAN room as a playing participant, opens **Choose Map**, chooses **Multiplayer Campaign** and selects a verified stage within its series. The host can then switch stages with **Choose stage**. A guest joins through the LAN list, which has a mode column and a multiplayer campaign filter. Pick a free role or **Random** below your name. The host reserves explicit roles and assigns the remaining roles to Random seats using the host seed. A solo Random pick takes the first authored role.

The room verifies the selected map and manifest before a guest can click Ready. The verifier restores an existing Ready choice after a role, roster or settings change in the same stage. Guests click Ready again when changing stages or returning for a new match. An observer joins using Watch when the stage allows it and an extra connection row is free.

If a peer stalls after GO, the healthy clients can be stopped at frame zero. Acknowledge the connection error to reopen the same stage. The stalled seat opens, and a healthy returning guest regains Ready after verifying the stage. [LAN recovery](Networking.md#go-loading-and-departures)

A verified guest can play the current stage even if a later map has yet to be installed. Restore both selected files and ask the host to reselect the stage after a missing-file or identity refusal. [Networking](Networking.md#authored-scenario-setup)

## Author the series

```ini
[Campaign]
Name=VMCE:Example
Entry=entry.map
ScenarioCount=1
Scenario1=entry.map

[Scenario:entry.map]
MinPlayers=2
PlayerCount=2
Player1=HouseAlpha
Player2=HouseCharlie
EndOfGame=yes

[Scenario:entry.map:HouseBravo]
Player1Control=yes
```

Declare the three Houses in the map's `[Houses]` section. The map initializes their units, diplomacy and resources. Every playing role has one distinct primary House. Unassigned Houses keep their authored units, AI and triggers. Each `PlayerNControl=yes` grants that role control over the named attached House for the whole match. An attached House must be outside the stage's primary-role list.

The host may choose any valid installed stage. The chooser presents `Entry` first in its series. Use an optional series `ra2md.csf` for `[Campaign] Name`, the map's `[Basic] Name` and a primary House's `UIName`. Resolution tries the series, then the running game's CSF. The series falls back to its directory name. `[Basic] Name` accepts a literal title, with the stage number as a fallback. A missing role title uses the role number.

Keep `campaign.ini` within 1 MiB, the optional `ra2md.csf` within 8 MiB and each translated title within 128 characters. A missing, damaged or oversized translation table leaves the stage playable.

Set `CameraWaypoint=2` in `[Scenario:entry.map:HouseAlpha]` to start that role at index `2` in the map's `[Waypoints]`. Supported indices are `0` through `701`. An absent or invalid waypoint uses the ordinary primary-House opening view. Add an optional series `credits.txt` in UTF-8, with one centred line per text line and blank lines for spacing. Files may include a BOM and can be up to 32 KiB. The final stage uses the original rolling credits when the file is missing or invalid.

Room options resolve from the map's `[Basic]`, then `[Campaign]`, then the stage, then the host's permitted changes. `Difficulty` is `0` for Easy, `1` for Normal and `2` for Hard. It selects the difficulty attributes and starting-fund adjustments for every playing primary House. `LockDifficulty` fixes the configured difficulty. Keep `1 <= MinPlayers <= MaxPlayers <= PlayerCount <= 30`. `AllowObservers` enables extra connection seats after the playing capacity when space remains.

VMCE uses campaign AI rules. `AutoEnemySelection=yes` gives computer Houses the ordinary multiplayer step that picks an enemy when they lack one. It defaults to `no` and follows the same map, series and stage precedence.

Crates placed on the map or spawned by triggers use single-player campaign reward rules. In LAN, cash rewards go to the House that owns the collecting unit.

Before the first tick, clients play `[Basic] Intro`, or `Brief` if Intro does not resolve in the movie catalog, and then `Action`. In play, trigger action `10` plays a fullscreen movie, `117` plays a blocking radar movie, and `100` plays a radar movie during normal gameplay. A blocking sequence waits for every connected player and observer to finish or skip. A missing or failed movie counts as skipped on that machine. The map's camera actions move each player's view. Action `110` pauses the shared scenario for its configured duration. Actions `46/47` control gameplay input separately.

Players can click, box-select, select by type and group discovered granted mobile units with `1–9`. Their unit type determines move, attack, force attack, deploy and transport actions. A granted factory with a rally line can take a cell rally. Production, funds, superweapons, repair, sale and MCV repack use the primary House. Shroud and vision follow the map and ordinary alliances.

## Interface walkthrough

| Step | Screen |
|---|---|
| Create or join | Start at **Network**, then use the LAN list to create or join a room. Watch uses an available observer seat on a stage that allows it. |
| Choose a stage | **Choose stage** opens the grouped series list. Verified stages can be used directly. Unavailable stages show their status. |
| Prepare | The left column shows each seat's role in its assigned color, a full country flag and the room rules. The middle column holds chat. The right column shows the map and actions. Briefing and role details support pages for long text. Guests click Ready after verification, then the host clicks Start. |
| Load and play | The layout uses campaign art, briefing and the single-player progress bar for the local client. A centred message appears in the bar while another player is loading. The battle uses the ordinary sidebar, map and selection controls. The primary House owns the production sidebar and funds. |
| Briefing and diplomacy | The default `Tab` key and the sidebar button open the mission briefing. Its first sidebar button opens Diplomacy `0x73`. Return from Diplomacy goes to the briefing, then back to the battlefield. The LAN match runs while either page is open. |
| Settle and continue | Victory opens `0x103` with Continue, Statistics and Leave Room. Defeat opens `0xBC6` with the statistics table and Retry, Return to Room and Leave Room. |

The medal page always shows both times in the original metal panels and an after-action report above the medal. The report uses the stage's `[Ranking]` text when available, with a localized completion message as the default. Report labels can use the series' `ra2md.csf`, with the installed string table as a fallback. Use **Statistics** on the right to see both times and each remaining player's counters, including kills.

**Diplomacy.** The table lists human players with separate name, role and faction columns. The full country flag appears next to the faction. The chat column selects recipients. With `AllowAlliesChange=yes`, active human players may use the original Allies checkbox to send the synchronized `ALLY` order. Locked relations show reciprocal, outward-only, inward-only and unallied states in text. [Diplomacy input](../src/app/input/diplomacy.rs)

## Results and the next stage

Map triggers decide every VMCE outcome. A player whose primary House has lost its units and buildings can still control granted units while the mission runs. Actions `1` and `2` give the named result to all starting players with the specified country and all players directly allied with a matching player in both directions. Other players receive the opposite result. Actions `67` and `68` give everyone the same result. The first outcome commits the result for the whole match.

After victory, each player clicks Continue independently. A guest who arrives first can chat in the same room while waiting for the host. The preview and Ready control become available when the host returns. The host's own result determines the preselection. On a win, `EndOfGame=yes` plays the credits and keeps the final stage selected. Otherwise an `AltNextScenario` with a set terminal flag takes priority, then `NextScenario` or the next listed stage. On a loss the current stage stays selected. A role selection carries forward by its primary House when that House is available.

A player who leaves during the mission keeps their House and units on the map, and their House still participates in trigger outcomes. The result statistics omit the departed player. Income shows a dash where the snapshot has no value. If the host leaves during the mission, the remaining players can finish and then return to the LAN list.

Defeat plays the stage's campaign failure scene when available before showing Retry and Return to Room. Retry can reload the original stage, assignments, settings, seed and incoming carry-over when every starting connection is still present. Each playing participant chooses Retry and all peers recheck their files. An observer follows the reload after verification. Return to Room cancels a pending retry and restores the stage, roles and settings. Guests click Ready again for the next match.

If the restart is interrupted during handoff, return to LAN and join the host's reopened room for the same stage.

### Carry-over

The host's room carries scenario globals into the preselected successor. The successor's map controls the countdown with `[Basic] TimerInherit=yes`. Changing the selection clears carry-over. Closing the room ends that continuation.

To carry funds, add a one-to-one mapping for the specific transition in `campaign.ini`. Each key is a primary House on the preceding stage and its value is a primary House on the receiving stage. Identical House names still need an entry.

```ini
[CarryOver:entry.map:next.map]
HouseAlpha=HouseNewAlpha
HouseCharlie=HouseCharlie
```

The receiving map applies its `[Basic] CarryOverMoney` and `CarryOverCap` to each mapped player's available terminal funds. Any player who takes the destination role gets that amount in addition to its new starting funds. Each direct Retry starts from the same incoming record.

[Configuration keys and trigger examples](zh-CN/vmce-manual.md#campaignini-键值)
