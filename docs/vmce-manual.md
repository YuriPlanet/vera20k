---
layout: default
title: VMCE Manual
lang: en
---

# VMCE Manual

[Documentation home](index.md) · [简体中文](zh-CN/vmce-manual.md) · **English**

VERA Multiplayer Campaign Expansion (VMCE) lets players run authored multiplayer campaigns in a VERA20k LAN room. A series consists of a `campaign.ini` and its maps. Each stage can define its own roles, shared units, room rules and continuation.

- **Players and hosts** can start with setup, then look up observation, Diplomacy and results. [Install and start](#install-and-start) · [Player guide](#player-guide)
- **Map authors** can use the examples for Houses and branching triggers and check the manifest reference. [Author the series](#author-the-series) · [Configuration keys](#campaignini-keys)

## How the modes play

| Mode | Entry and players | Starting state and control | Results and continuation |
|---|---|---|---|
| Original single-player campaign | Select a campaign from the main menu and play as one human. `BATTLEMD.INI` defines the entry mission. | The map's `[Basic] Player` selects the current House. The map can grant control of other Houses with `PlayerControl=`. | A victory leads to the campaign score screen and continuation. The campaign can carry globals and funds forward. A defeat offers a retry. [Campaign definitions](../src/rules/campaigns.rs) · [Current House](../src/sim/scenario_bootstrap.rs) · [Victory](../src/app/campaign_victory.rs) · [Defeat](../src/app/campaign_defeat.rs) |
| Original Skirmish | Select Skirmish from the main menu to play against computer seats. Retail modes include Battle, Free For All and Team Alliance. | Map starts and the setup options assign Houses and spawn starting units. Computer opponents use their Skirmish seats and difficulty. | The match settles under Skirmish rules. Players select a map and settings in setup for the next match. [Mode list](../src/skirmish_modes.rs) · [Starting units](../src/sim/scenario_bootstrap.rs) · [Results](../src/app/match_runtime/scenario_exit.rs) |
| Original Cooperative | Select Cooperative in a multiplayer room. Two human seats play together against computer opponents. | `CoopCampMD.ini` defines mission variants and the countries for player and enemy positions. The mission configuration places both partners and the computer forces. | With campaign progress active, the results use dialog `0xBC6`. A victory records the pair's progress in `coopsave.ini` and advances the mission. [Mode and seats](../src/skirmish_modes.rs) · [Scenario configuration](../src/app/frontend/skirmish_session/cooperative.rs) · [Cooperative results](../src/app/cooperative_round.rs) |
| VMCE | Select **Multiplayer Campaign** and a stage in a regular VERA20k LAN room. The stage defines the playing limits, roles and observer policy. | The room binds each player to a primary House and freezes any shared control the author grants. The map can place units and bases at the start or create them with triggers. | Map triggers decide the outcome. `campaign.ini` defines stage order, branches and the finale. Victory opens the `0x103` medal page and the host's return preselects the successor. Defeat opens the `0xBC6` statistics page and offers Retry when eligible. [Manifest and successors](../src/map/vmce.rs) · [Results and room return](../src/app/match_runtime/vmce_result.rs) |

The sidebar button reopens the mission briefing in the original single-player campaign and Cooperative. Cooperative also uses the default `Tab` key for Diplomacy. In Skirmish, the sidebar Diplomacy button and default `Tab` key open Diplomacy directly. VMCE opens the mission briefing first, with **Diplomacy** in the right sidebar. The stage's `AllowAlliesChange` setting and the players' seats determine who can change alliances. [Diplomacy input](../src/app/input/diplomacy.rs) · [Alliance rules](../src/sim/house_diplomacy.rs)

## Install and start

### Requirements

The host, players and observers use the same VERA20k build and gameplay feature configuration. Each needs access to the original Yuri's Revenge assets. With a mod, every client needs matching effective rules. Install identical copies of the series manifest and the current map in each client's settings directory. [Network verification](Networking.md#authored-scenario-setup)

### Build and enable

For a source build, use the regular VERA20k build command. Once a valid series is installed, choose **Multiplayer Campaign** in the LAN map chooser.

```sh
python -m tools.cargo_run -- build -p vera20k --profile fast --bin vera20k
```

### Campaign folders

Put each series in its own folder under `Maps/VMCE/` in the active VERA20k settings directory. If `[paths]` in `config.toml` sets `settings_dir`, the game looks for `Maps` there.

```text
<settings_dir>/Maps/VMCE/
└── Example/
    ├── campaign.ini
    ├── entry.map
    ├── next.map
    ├── branch.map
    ├── ra2md.csf
    └── credits.txt
```

- Map paths in the manifest are relative to the series folder. For a map in a subfolder such as `Stage01/entry.map`, use the section `[Scenario:Stage01/entry.map]`.
- Stages are loose map files in the series folder, such as `.map` or `.yrm`. The room shows the series and stage names on separate lines in the right sidebar. `[Campaign] Name`, the map's `[Basic] Name` and a primary House's `UIName` can reference a CSF label. Lookup tries the series' `ra2md.csf`, then the running game's CSF. The series falls back to its folder name. A stage can use a literal `[Basic] Name` and falls back to its stage number. A role without a resolved name uses its role number.
- `campaign.ini` can be up to 1 MiB and the optional series `ra2md.csf` up to 8 MiB. Each resolved title can be up to 128 characters. Verification and start remain available if the translation table is missing, damaged or oversized.
- The room checks the raw bytes of `campaign.ini` and the selected map along with the effective map and rules. Clients can install the manifest and the current map first. When the host selects another stage, the room checks that map. The chooser marks missing or invalid stages. The optional translation table is for local display.
- The finale can use an optional `credits.txt` in UTF-8, with or without a BOM, up to 32 KiB. Each line is centred and blank lines add spacing. A missing or invalid file falls back to the original rolling credits.

### Minimal campaign.ini

This one-map series has two selectable roles. Place the manifest next to `entry.map`. The map's `[Houses]` must contain `HouseAlpha` and `HouseCharlie`.

```ini
[Campaign]
Entry=entry.map
ScenarioCount=1
Scenario1=entry.map

[Scenario:entry.map]
PlayerCount=2
Player1=HouseAlpha
Player2=HouseCharlie
EndOfGame=yes
```

With the map's player limits left at their defaults, one or two players can start. The chooser lists `Entry` first within its series. The host can also select any other installed, valid stage directly.

### Interface walkthrough

1. From **Network**, the host creates a LAN room, opens **Choose Map** and selects **Multiplayer Campaign** and a valid installed stage. After entering the VMCE room, use **Choose stage** in the right sidebar to switch stages. The host uses a playing seat. Remove any computer seats already added to the ordinary room.
2. Guests join from the LAN list. Use its **Multiplayer campaigns only** filter to find VMCE rooms. To spectate, enable the observer toggle before joining. The stage must allow observation and have an open observer seat.
3. Players select a free role or **Random** below their name. **Details** shows map-configured funds, tech level, counts of placed units and buildings, and shared forces. The host can adjust the opening game speed and any unlocked difficulty using separate sliders. The left column shows the seats, role colours, available flags and room rules. The middle column is for chat. The right column shows the preview and opens the mission briefing. Long text can span multiple pages.
4. Guests select **Ready** after verification. Observers also verify and select **Ready**. The host selects **Start** once the stage's player limit and readiness requirements are met.
5. During a role, rules or roster change on the same stage, an existing Ready choice is restored after a successful recheck. Select **Ready** again after a stage change or when returning for a new match. The loading screen shows campaign art, the briefing and the local loading progress. **Waiting for other players...** appears inside the progress bar while another player is still loading.

### Read the INI examples

Brackets enclose the section name. A key goes to the left of the equals sign and its value to the right. Replace names or paths shown in angle brackets. Paths in `campaign.ini` are relative to the series folder. Merge the map fragments into a complete map saved by the editor.

## Player guide

### Roles, primary Houses and shared Houses

Each role maps to one primary House. Its country, colour, starting units and base funds come from the map. The room reserves explicit picks, then uses the host's seed to shuffle the unclaimed roles and assign them to Random players in seat order. With one playing participant, Random selects the first authored role. Each primary House can go to one player. Unassigned Houses keep their authored units, AI and triggers.

The author can grant control of a supporting House to one or more roles. The House must be listed in the map's `[Houses]` and be outside the stage's primary-role list. Players can click, box-select and select discovered shared mobile units by type, group them with `1–9`, and issue the unit's supported orders. These include move, attack, force-attack, stop (`S`), scatter (`X`), area guard, deploy, enter and unload transports, plant C4 and engineer capture. A building captured by an engineer goes to the engineer's House. A granted building with a rally line can take a cell rally point.

Production, funds, superweapons, repair, sale and MCV repack belong to the primary House. Shroud and vision follow the map and its alliances.

### Mission rules

- **Playing limits** follow `1 <= MinPlayers <= MaxPlayers <= PlayerCount <= 30`. Observer seats come after the playing capacity. `MaxPlayers=30` uses the full connection capacity. Role assignment, gameplay command grants and statistics use the playing seats.
- **Map alliances** come from each House's `Allies=` and the map's triggers. Press the default `Tab` key or the sidebar button to open the briefing, then use **Diplomacy** to open dialog `0x73`. Human rows show the player name and role. Playing rows also show the available flag and faction. Observers appear as chat recipients and have no alliance state. With `AllowAlliesChange=yes`, active human players can change alliances using the Allies checkbox. When the map locks alliances, the text shows reciprocal, outward-only, inward-only and unallied states. Return from Diplomacy to the briefing, then back to the battlefield. The match runs while either page is open.
- **Crates** placed on the map or spawned by triggers use single-player campaign rewards. In LAN, cash goes to the House that owns the collecting unit.
- **Results** come from map triggers. If a primary House loses all its forces, the mission continues and the player can still command surviving granted units.
- **Narrative playback** waits until every connected player and observer finishes a blocking movie. Esc skips local playback. Missing media or a playback failure counts as a skip on that client. Nonblocking radar movies let the mission run. Opening the regular Esc menu also lets the others keep playing.

### Difficulty and game speed

Set `Difficulty` to `0`, `1` or `2` for Easy, Normal or Hard. Normal is the default. These values select the Easy, Medium and Hard trigger columns and apply difficulty modifiers such as firepower, production and starting funds to each playing primary House. `LockDifficulty=yes` fixes the stage setting. Changing an unlocked difficulty makes clients verify again.

The **Opening Speed** slider sets the opening pace and makes clients verify the updated room settings. The room keeps this speed when changing stages, and the host can edit it when the room reopens. During a mission, playing clients can change the shared live speed from **Esc → Game Controls**. A direct Retry uses the opening speed.

### Results and the next stage

- **Victory** opens the `0x103` medal page. The report resolves the map's `[Ranking]` text labels in the series CSF, then in the running game's CSF. The default title and text are **After-action report** and **Mission complete.** The panels below the medal show time taken and par time. Par uses the map's Normal value. The right sidebar offers **Continue**, **Statistics** and **Leave Room**. Players select **Continue** independently.
- **Returning to the room.** A guest can return early and chat. The room shows **No preview available** until the host returns and the preview and Ready control become available. If the guest cannot rejoin within two minutes, the game returns to the LAN list and shows a message. The host can change the preselected stage. An available role carries forward by its primary House. When the host continues after a finale, the credits play and the room reopens with the finale selected. Verify and select Ready again for the next match.
- **Defeat** plays the single-player failure sequence when available, then shows the `0xBC6` statistics page. The right sidebar shows the stage name and available preview and offers **Retry Mission**, **Return to Room** and **Leave Room**. Retry requires a common defeat and every starting player still connected and voting to retry. The host excludes observers whose departure is known before the retry roster is frozen. The remaining observers verify their local files and follow the group. Retry reloads the original stage, playing roles, opening settings, seed and incoming carry-over. A player can use **Return to Room** to cancel a pending retry. The exit controls are temporarily disabled during handoff. If handoff is interrupted, join the host's reopened room for the same stage from the LAN list.
- **Mixed results** give each player their own victory or defeat sequence. The host's result decides the room's next selection. A host victory preselects the successor, and a host defeat keeps the current stage.
- **Departures.** A departing or disconnected player is omitted from the statistics table. Their primary House and units remain and still count for trigger outcomes. If the host leaves, the remaining players can finish and then return to the LAN list.
- **Statistics** show both times and each remaining primary House's income, kills, losses, built count and score. Income totals all positive cash receipts after setup, including resource payments, sales, refunds, transfers and theft. Opening and inherited funds are excluded.

## Author the series

### Define the role Houses

Register each role's House in the map. In this example, `HouseAlpha` and `HouseCharlie` are the primary Houses and `HouseBravo` provides shared units. The three Houses use different existing countries and list one another as allies. Merge these fields into the map's House settings and place the starting units or bases each role needs.

```ini
[Houses]
0=HouseAlpha
1=HouseBravo
2=HouseCharlie

[HouseAlpha]
Country=Americans
Allies=HouseAlpha,HouseBravo,HouseCharlie

[HouseBravo]
Country=Russians
Allies=HouseAlpha,HouseBravo,HouseCharlie

[HouseCharlie]
Country=British
Allies=HouseAlpha,HouseBravo,HouseCharlie
```

### Countries, map owners and triggers

The manifest's `Player1=HouseAlpha` references a House name. Use `Americans` and `British` to distinguish the roles in conditions that select a country. Assign reinforcements to the intended map owner in the editor. Separate Houses can have their own conditions and reuse the same scripts and teams.

In `[Scenario:entry.map:HouseAlpha]` in `campaign.ini`, set `CameraWaypoint=2` to open that role's camera at waypoint `2` in the map's `[Waypoints]`. The supported indices are `0` through `701`. An absent or invalid waypoint uses the primary House's normal opening view.

### Room rules and branching

The map's `[Basic]` supplies initial values, then the manifest's `[Campaign]` and `[Scenario:<MapPath>]` override them. The host can adjust an unlocked difficulty. An omitted key inherits from the layer below. An explicit `no` or difficulty `0` overrides it. `AltNextGlobal` inherits from the series separately and can be overridden for a stage.

This series has an entry stage, a regular successor and a branch. All three stages require two players and allow observation. The entry stage grants role 1 control of `HouseBravo`. Both successors are finales.

```ini
[Campaign]
Entry=entry.map
ScenarioCount=3
Scenario1=entry.map
Scenario2=next.map
Scenario3=branch.map
MinPlayers=2
AllowObservers=yes
Difficulty=1
LockDifficulty=no
AllowAlliesChange=no
AltNextGlobal=22

[Scenario:entry.map]
PlayerCount=2
Player1=HouseAlpha
Player2=HouseCharlie
NextScenario=next.map
AltNextScenario=branch.map

[Scenario:entry.map:HouseBravo]
Player1Control=yes

[Scenario:next.map]
PlayerCount=2
Player1=HouseAlpha
Player2=HouseCharlie
EndOfGame=yes

[Scenario:branch.map]
PlayerCount=2
Player1=HouseAlpha
Player2=HouseCharlie
EndOfGame=yes
```

After a host victory, the room preselects a stage in this order.

1. `EndOfGame=yes` completes the series and keeps the current stage selected.
2. If `AltNextScenario` is set and the terminal `AltNextGlobal` is true, select that branch.
3. Otherwise choose `NextScenario`, then fall back to the next list entry. If the last listed stage has no applicable successor, complete the series and play the credits.

The host can change this selection when the room reopens. List both regular and alternate successors in the manifest. Each stage uses its own Houses, units and triggers.

### Carry over globals, the countdown and funds

Starting the preselected successor after a host victory carries over the previous mission's terminal global variables. If the receiving map sets `TimerInherit=yes` in `[Basic]`, it also inherits any time remaining on the mission countdown and displays the new map's timer label. Selecting a different stage clears the carry-over. Closing the room ends the continuation.

For funds, add a separate mapping for each transition in `campaign.ini`. Put a primary House from the preceding map on the left and a primary House from the receiving map on the right. Each House can appear once on its side of the mapping. Paths and House names in the mapping are case-insensitive. List a House explicitly even if its name stays the same.

```ini
[CarryOver:entry.map:next.map]
HouseAlpha=HouseCharlie
HouseCharlie=HouseAlpha
```

Set `CarryOverMoney` and `CarryOverCap` in the receiving map's `[Basic]` to control the fraction and cap per role. The game adds the carried amount to that role's difficulty-adjusted opening funds. Whoever takes the destination role receives it. A branch needs its own mapping. A direct Retry restores the incoming record from the first start.

### Write triggers

The map's `Intro` and `Action` movies in `[Basic]` play in that order before the first simulation tick. If the movie catalog cannot resolve `Intro`, the game uses `Brief` in its place. In play, action `10` plays a fullscreen movie, `117` plays a blocking radar movie, and `100` plays a nonblocking radar movie. Actions `48/112` move every client's camera to the specified waypoint. Action `110` pauses the scenario. A positive duration lets clients resume together at expiry, and `0` gives an untimed pause. Actions `46/47` lock or unlock input while the simulation runs. Action `108` places a map crate.

In `[AITriggerTypesEnable]`, `yes` enables the AI trigger and `no` disables it. The game checks trigger and team eligibility using single-player campaign rules, including enable status, country, side and difficulty. When a factory is missing, an eligible recruitable unit can satisfy that check. Map Houses with no explicit `TechLevel` use VMCE's starting value of `10`.

Discovery by any playing participant can satisfy the corresponding discovery event. A country-based elimination event uses the first matching House in map registration order. VMCE's `PlayerN` role numbers and the `Player @ A/B/…` trigger selectors have separate meanings. Use the intended country or map owner in triggers.

#### Trigger header

Keep the editor's trigger format and use the country name for `Owner`. If an event selects the first House for a country, give the roles different `Country` values and add the appropriate condition for each role.

```ini
[Triggers]
<ID>=<Owner>,<Linked>,<Name>,<Disabled>,<Easy>,<Medium>,<Hard>,<Transfer>
```

#### Outcome actions

| Action | Name | VMCE behaviour |
|---|---|---|
| `1` | Win by country | All starting players of the specified country and their direct, reciprocal allies win. The rest lose. |
| `2` | Lose by country | All starting players of the specified country and their direct, reciprocal allies lose. The rest win. |
| `67` | Announce victory | Sets a shared victory for the playing participants. |
| `68` | Announce defeat | Sets a shared defeat for the playing participants. |
| `69` | End scenario | Resumes the timer for a pending result. By itself, requests victory. |

With no matching participating House, action `1` makes everyone lose and action `2` makes everyone win. The first accepted match result locks in the verdict. Provide map conditions for victory, defeat and mutual destruction. A map that relied on automatic elimination needs outcome triggers.

VMCE uses the network result timing, which can complete a victory while an `AllowWin` tag is active. Team script actions `7/23` give every playing seat the same result. In single-player campaign play those actions affect the current player.

#### Branch after entering a cell

This entry-stage fragment uses cell `(30,36)` as the destination. `HouseBravo` uses `Russians`, and the Entered By event checks for a unit of that country. When merging the fragment, choose trigger and tag IDs that are free in your map and use them consistently across the sections.

```ini
[Triggers]
01000001=Russians,<none>,Finish with the shared tank,0,1,1,1,0

[Events]
01000001=1,1,0,8

[Actions]
01000001=3,28,0,22,0,0,0,0,A,67,0,0,0,0,0,0,A,69,0,0,0,0,0,0,A

[Tags]
01000003=0,Terminal cell,01000001

[CellTags]
36030=01000003
```

Action `28` sets global `22` before the victory, so the host's Continue preselects `branch.map`. For the regular successor, make another victory trigger run only `67` and `69` and leave global `22` clear. A loss can use `68` and `69` and keeps the entry stage selected. `8` is the `Russians` country index in the retail rules for this example. Use the editor's value if you change the rules or country.

### Convert a single-player map

1. Copy the map in the editor and confirm that every primary and supporting House is registered in `[Houses]`. Place the roles' starting units and bases and set their map alliances.
2. Write `campaign.ini` and check its paths. Point each `PlayerN` to that stage's primary House and add `PlayerNControl=yes` for the supporting Houses to share.
3. Check the original map's ownership conditions, reinforcements and outcomes for assumptions about a single player. When roles need separate conditions, give them the appropriate country checks.
4. From a LAN room, use a primary or shared unit to reach the objective and then check victory, defeat and Retry. For a branch, try the flag set and cleared, and check the finale setting.
5. Distribute copies of the original files. After editing the map or manifest, reselect the stage, verify it and select Ready before starting again.

The repository has a two-stage example with configured roles. [Two-stage manifest](../tests/fixtures/vmce/lan_outcomes/campaign.ini) · [Entry map](../tests/fixtures/vmce/lan_outcomes/outcome_stage01.map)

## Reference

### campaign.ini keys

The `[Campaign]` section defines the series.

Unrecognized keys in `campaign.ini` produce a message in the host's room chat when the host selects a stage. The game logs the full list of keys and can still load the series.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `Name` | CSF label | Folder name when omitted | Series title. Lookup uses the series CSF, then the running game's strings. |
| `Entry` | Relative map path | Required | Entry stage. It must be listed in the manifest and appears first in the chooser. |
| `ScenarioCount` | Positive integer | Required | Number of stages in the manifest. |
| `Scenario1..ScenarioN` | Relative map path | Required | Sequentially numbered from 1. Each path is unique. |

The `[Scenario:<MapPath>]` section defines the stage's roles and successors.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `PlayerCount` | Integer `1..30` | Required | Number of selectable primary roles. The actual player count also depends on room settings and available connection seats. |
| `Player1..PlayerN` | House name | Required | Primary Houses in role order. Each role uses a different House. |
| `NextScenario` | Relative map path | Next list entry when omitted | Regular selection when the host returns after victory. |
| `AltNextScenario` | Relative map path | Omitted | Stage preselected when the branch global is set. |
| `EndOfGame` | Boolean | `no` | With `yes`, completes the series before checking successors. The last listed stage also completes the series when it has no applicable successor. |

Both sections can also set the following options. The defaults apply when lower layers omit the key as well.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `MinPlayers` | Positive integer | `1` | Minimum playing participants needed to start. It must be at most `MaxPlayers`. |
| `MaxPlayers` | Positive integer | `PlayerCount` | Playing capacity, up to the number of roles in the stage. |
| `AllowObservers` | Boolean | `no` | Opens observer seats when connections remain beyond the playing capacity. |
| `AllowAlliesChange` | Boolean | `no` | Lets active human players change alliances with each other. |
| `AutoEnemySelection` | Boolean | `no` | Lets a computer House with no current enemy use the ordinary multiplayer enemy-selection step. |
| `Difficulty` | Integer `0..2` | `1` | Easy, Normal or Hard, used for triggers and AI difficulty. |
| `LockDifficulty` | Boolean | `no` | Fixes difficulty to the stage's configured value. |
| `AltNextGlobal` | Integer `0..49` | `1` | Global variable index used for branch selection. A stage can override the series value. |

The `[Scenario:<MapPath>:<HouseName>]` section configures a primary House's camera or the grants for a supporting House.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `CameraWaypoint` | Integer `0..701` | Primary House's normal opening view | Sets the role's opening camera to a map waypoint. |
| `Player1Control..PlayerNControl` | Boolean | `no` | Grants the numbered role control of the supporting House. It must be in the map's `[Houses]`, outside the primary-role list. The role number must be within `PlayerCount`. |

Fund mappings go in `[CarryOver:<FromStage>:<ToStage>]`, with each entry written as `<FromHouse>=<ToHouse>`. [Carry-over details](#carry-over-globals-the-countdown-and-funds)

### Map INI fields

VMCE uses these original single-player campaign map fields.

| Map section | Key or field | Meaning |
|---|---|---|
| `[Basic]` | `Name` | Stage display name. |
| `[Basic]` | `Intro`, `Brief`, `Action` | Opening movie keys. See [Write triggers](#write-triggers). |
| `[Basic]` | `TimerInherit`, `CarryOverMoney`, `CarryOverCap` | Control the incoming countdown and funds. Funds follow the primary-House mapping for this transition. |
| `[SpecialFlags]` | `FogOfWar` | Enables the original campaign fog-of-war option for this stage. |
| `[Houses]` | `N=<HouseName>` | Register primary and supporting Houses by numeric index `N`, for example `0=HouseAlpha`. |
| `[<HouseName>]` | `Country`, `Allies`, `Credits`, `TechLevel` | The House's country, opening alliances, base funds and tech level. |

VMCE displays the map's `Briefing` in `[Basic]` as the mission briefing. It accepts text or a CSF key. A primary House's `UIName` can supply the role's display label. Both CSF lookups try the series table first and then the running game's strings.

The map's `[Basic]` can also supply the following initial room values. Series and stage settings in `campaign.ini` can override them. Set branch indices and successors in `campaign.ini`.

| Key | Meaning |
|---|---|
| `MinPlayers`, `MaxPlayers` | Minimum and maximum playing participants. |
| `AllowObservers` | Allows observer seats. |
| `AllowAlliesChange` | Lets active human players change alliances during the mission. |
| `Difficulty`, `LockDifficulty` | The stage's initial difficulty and whether the host can adjust it. |
| `AutoEnemySelection` | Lets a computer House pick an enemy when it has none. |

### Trigger record format

| Map section | Key or field | Meaning |
|---|---|---|
| `[Triggers]` | `<TriggerID>=<Owner>,…` | Use the trigger header above, including its owning country and the `Easy`, `Medium`, `Hard` flags. |
| `[Events]`, `[Actions]` | `<TriggerID>=<Count>,<Records>` | The number at the start matches the actual event or action record count on that line. |
| `[Tags]` | `<TagID>=<Repeat>,<Name>,<TriggerID>` | Attach a tag to a trigger. |
| `[CellTags]` | `<PackedCell>=<TagID>` | Attach a tag to the target cell. The example's `36030` maps to `(30,36)`. |

See the map format reference for other map sections and retail keys. [VERA map format](map-format.md)

### Troubleshooting

| Symptom | What to check |
|---|---|
| VMCE or the stage is missing | Check the VERA settings folder, `Maps/VMCE/<Series>/campaign.ini`, `Entry` and the `ScenarioN` entries. Look for that series' read error in the game log. |
| A role is missing or stage selection fails | Check that `[Scenario:<MapPath>]` matches the map and that the Houses in `PlayerN` and grant sections are registered in `[Houses]`. Check `MinPlayers <= MaxPlayers <= PlayerCount`. |
| A guest cannot select Ready | Compare the builds and the raw manifest and current map. After a stage change, wait for verification and select Ready. A same-stage change makes the guest verify again and restores their prior Ready choice after success. |
| An observer cannot join | Check that `AllowObservers=yes` is effective and that the room has a free observer connection. |
| The Diplomacy Allies checkbox is unavailable | Check the stage's effective `AllowAlliesChange` and whether both players are active. Diplomacy shows the alliances authored in the map. |
| Start stalls while waiting | If loading stalls after GO and shows the connection error, acknowledge it. The host reopens the same stage. Healthy guests can rejoin via the LAN list and regain Ready after verifying the same stage. The stalled seat becomes available. A departure before the first frame is certified, including during the opening movies, follows the same recovery path. |
| The mission continues after a House loses everything | Check for an outcome trigger for that situation. Check its country condition and difficulty columns. |
| Victory preselects an unexpected stage | Check `EndOfGame`, `AltNextGlobal`, whether the global is set before the victory action, and whether the successor is listed in the manifest. |
| Retry is unavailable or is canceled | Check that everyone lost, all starting players remain connected and vote to retry, and everyone on the frozen retry roster still has the original manifest and map. Verify and select Ready again when starting a new match from the room. |

### Versions and saved games

This guide uses LAN. Network clients use the same build and stage files. Retry reloads the frozen start, and stage continuation uses the settings confirmed in the reopened room. VMCE currently has no mid-mission save, load or replay recording. [LAN room synchronization](Networking.md#authored-scenario-setup)

### Retail assets and licensing

Players supply their own original game assets. See the project README for VERA20k installation, assets and licensing. [Project README](../README.md)
