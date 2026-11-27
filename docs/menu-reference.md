---
layout: default
title: Menu reference
---

# Menu reference

[简体中文](zh-CN/menu-reference.md) · **English**

Every page and menu a player can open, in or out of a match, read from the source code.
Where the code disagrees with this page, the code is right; fix this page in the same change.

**Notes**

- Retail labels (keys starting `GUI:`, `STT:`, `TXT_`) come from the player's `ra2md.csf`. VERA's own labels come from `src/i18n/vera20k.en.ini`.
- 【】 gives where a value is saved. A `[Section] Key` with no file named is in VERA.INI.
- "This run only" means the value lives in memory and resets when the game restarts.

## 1. Outside a match

```
Launch
├─ EA logo / intro movie (only Esc skips them; the intro plays on the first launch only)
├─ Startup splash (at least 6 s, no controls)
├─ Startup failure card (game files not found / menu failed to load / config.toml unreadable / settings folder unusable)
│   ├─ Quit
│   └─ Import Game Files (Android only) → Choose Folder / Cancel / Close
├─ Update offer (desktop only, appears before the main menu at startup, or once it is idle): Update or Download / Skip Version【[VERA] SkipUpdate】/ Later
│
└─ Main Menu
    ├─ Single Player
    │   ├─ New Campaign → campaign selection: Allied / Soviet emblem (click to start), Difficulty slider (Easy/Normal/Hard)【[Options] Difficulty】, Back
    │   ├─ Load Saved Game (greyed with no saves): save list (double-click loads), Load, Back
    │   ├─ Skirmish setup
    │   │   ├─ Player name【[MultiPlayer] Handle】
    │   │   ├─ Each row (8 shown, up to 120 slots): Side, Color, Start, Team; computer rows add AI (None/Easy/Normal/Hard)
    │   │   ├─ Sliders: Game Speed (incl. 90 ticks/s and Unlimited), Credits (5000–10000, plus 30000 and 100000), Unit Count
    │   │   ├─ Checkboxes: Short Game, MCV Repacks, Crates Appear, Super Weapons, Build Off Ally
    │   │   │     【all of the above in [Skirmish]; the rows in SkirmishSetups.ini】
    │   │   ├─ Quick Team Setup list: Team Starts, Team Colors, Balance teams, Players vs AI, AIs in 2/3/4 teams, Free-for-all;
    │   │   │     Bulk Seat Actions list: Fill with AI (Easy/Normal/Hard/random), Open closed seats, Remove all AI (VERA)
    │   │   ├─ Map preview (click enlarges)
    │   │   ├─ Start Game / Back
    │   │   ├─ Game Options box: Superweapon Limit (Off, 2–20)【[VERA] SuperweaponLimit】, Tech Level (1–10), Bases, Fog of War,
    │   │   │     Multi-Engineer, Harvester Truce【[Skirmish] TechLevel / Bases / FogOfWar / MultiEngineer / HarvesterTruce】,
    │   │   │     Terrain Deformation【[VERA] TerrainDeformation】, Allow cheat codes【[VERA] SkirmishCheats】, OK
    │   │   └─ Choose Map
    │   │       ├─ Game type list (incl. Favorites), map list, search field, Add/Remove Favorite【MapFavorites.txt】
    │   │       ├─ Use Map / Cancel
    │   │       └─ Create Random Map
    │   │           ├─ Basic: Environment, Time of Day, Theater, Map Size (incl. VERA's Huge to Colossal, 4 sizes), Resources, Players (2–120), Urban Areas (Ares only)
    │   │           ├─ Advanced (2 pages, 13 sliders 0–100): Water, Hills, Buildings, Vegetation, Ore Fields, Plateau Size, Accessibility;
    │   │           │     Tech Buildings, Derelicts, Damage, Civilians, Animals, Neutral Defenders
    │   │           ├─ Extras (3 pages): Share Code, Use a Code, Previous Map, My Preset【Maps/VeraPreset1..5.vpr】;
    │   │           │     Ore Fields, Towns, Base Room, Sea Access, Fair Derricks, Centre Gems, Roads, Mood, War Damage, Countryside, Landmark, Town Fences
    │   │           ├─ Buttons: Surprise Me, Preview Map, Use Map, Save/Load/Delete Map, Export Map (.map),
    │   │           │         Fair Starts On/Off, Presets (1v1 Duel / 30-Player Brawl / City War / Islands / Team Battle / My Preset),
    │   │           │         Basic/Advanced/Extras switch, Cancel
    │   │           └─ Seed browser: list, description, Load/Save/Delete, Back【*.SED in the Maps folder】
    │   └─ Main Menu
    │
    ├─ Network (LAN)
    │   ├─ Lobby: name, Observe checkbox【[MultiPlayer] ObserverMode】, games list (double-click joins), players list, chat, New, Join, Main Menu
    │   ├─ Create Game box: Max players (2–30), Allow spectators (both this run only), OK/Cancel
    │   └─ Game room (see below)
    │
    ├─ WW Online
    │   ├─ Entry: Play as Guest (a generated GuestNNNNN name, no account), Sign In and Play
    │   │   └─ Sign In and Play → Sign-in: Username【[VERA] OnlineName】, Password, Remember password【Account.json】, Play, Back
    │   │       └─ Popups: Create Account → verification code; Forgot Password → reset password; Delete Account
    │   └─ Online lobby
    │       ├─ Games list, players list (click opens the player card), Name/Ping sort, Refresh, Observe checkbox, chat, Emote
    │       ├─ Create Game: room name, password, max players, allow spectators, limit screen resolution, ranked game, lobby watching (Delayed/Off/Live) (all this run only)
    │       ├─ Join Game (a password-protected room opens the channel password box)
    │       ├─ Under the games: search (game name or map), Hide full, Hide locked (this run only)
    │       ├─ Quick Match: progress and Cancel only, no preferences
    │       ├─ WOL Options: Play Music【[WOnline] LobMusic】, Translate chat into【[VERA] TranslateChat】,
    │       │     Translate in matches【[VERA] TranslateMatches】, Sound Options, OK/Cancel
    │       ├─ Player card: Message, Add/Remove Buddy, Full Profile (opens the web page), Change Country (own card only)【[VERA] OnlineLocale】, Close
    │       └─ Page dialog / Page Received dialog
    │
    │   Game room (shared by LAN and online)
    │   ├─ Right-hand buttons: Start Game (host), Game Options, Choose Map (host), Lobby (online only), Back
    │   ├─ Each roster row: Player (Open/Closed/AI/Kick/Give Seat/Move to Spectators), Side, Color, Start, Team; host can drag rows between teams; Previous/Next page
    │   ├─ Team bar (host): Teams, Room size, Quick Team Setup list (Team Starts, Team Colors, Balance teams online,
    │   │     the quick teams), Bulk Seat Actions list (Fill with AI, Close all, Open closed seats, Remove all AI);
    │   │     each group has a Join/Watch/Join Queue button, and the host also gets Add AI
    │   ├─ Co-op difficulty, chat; with a missing map: Download / Leave
    │   └─ Room Game Options popup, two pages:
    │       ├─ speed, credits, unit count, Tech Level, the five retail checkboxes, Bases, Fog of War,
    │       │     Multi-Engineer, Harvester Truce; More Options
    │       └─ Superweapon Limit, Destroyable Bridges, In-game Allying, No Yuri, No France, No Spy, No Stolen Tech,
    │             Terrain Deformation, Building Production Queue; Main Options
    │       【retail items in [LAN] or [WonlinePref]; VERA's additions this run only】
    │
    ├─ Movies & Credits: Sneak Peeks, Play Movies → movie list (Play Movie/Back), View Credits, Main Menu
    │
    ├─ Options (6 pages, see the table below)
    │   ├─ Keyboard: category, command list, shortcut capture, Assign, Reset All【[Hotkey]】
    │   ├─ Mods (not in the web build): mod list, Use This Mod【config.toml [paths] mod; restarts】,
    │   │   Open Mods Folder, Check Files, New Mod
    │   ├─ Check for Updates → updater dialogs (checking / up to date / new version: Update, Skip Version, Later / download progress)
    │   └─ Main Menu (saves and returns)
    │
    ├─ Exit Game → confirmation (OK saves then quits / Cancel)
    │
    └─ Front-end Esc menu (VERA's own; Esc on the main menu, Single Player, Skirmish, Choose Map and similar pages)
        Options, Exit Game (quits at once, no confirmation), Cancel, Full Screen, Performance Stats, Language, Set Window Size (desktop only)
```

### Options window (shared by the main menu and the match)

| Page | Settings【storage】 |
|---|---|
| **Display** | Zoom【[VERA] Zoom】, Display (monitor)【[VERA] Display】, Graphics API【[VERA] GraphicsAPI】*(Windows only, next start)*, Color Vision【[VERA] ColorVisionMode】, Team Color Strength 1–10【[VERA] HouseColorStrength】*(locked in a match)*, Visual Details (Low/High)【[Options] DetailLevel】, Full Screen【[VERA] FullScreen】, VSync【[VERA] VSync】*(not in the web build)*, High refresh rate in battle【[VERA] HighRefreshRateInBattle】*(not in the web build)*, Original 4:3 Shape【[VERA] OriginalShape】, Performance Stats *(not saved)* |
| **Game Options** | Difficulty【[Options] Difficulty】*(locked in a match)*, Shift-Click Batch Queue【[VERA] ShiftQueue】, Building Production Queue【[VERA] BuildingProductionQueue】*(next game)*, Auto Repair【[VERA] AutoRepair】, Ares (Auto/On/Off)【[VERA] Ares】*(next launch)*, Custom End Game Banner / Score / Movie【[VERA] CustomEndGameBanner / Score / Movie】, Commander Clips【[VERA] CommanderFeedback】 |
| **UI Options** | Sidebar size (Normal/Large/XL)【[VERA] Sidebar】, Sidebar Columns 2–5【[VERA] SidebarColumns】, HD Radar【[VERA] HdRadar】, Smooth Sidebar【[VERA] SmoothSidebar】, Health Bars (Original/All Units/All Units Not Full)【[VERA] HealthBars】, Tooltips【[Options] ToolTips】, Target Lines【[Options] UnitActionLines】, See Hidden Objects【[Options] ShowHidden】, Compact Super Timers【[VERA] CompactSuperTimers】, Grouped Super Timers【[VERA] GroupedSuperTimers】, Detailed EVA Text【[VERA] DetailedEvaText】, Convenience Help【[VERA] ConvenienceHelp】; button: Edit Layout *(match only)* |
| **Audio Options** | Music Volume【[Audio] ScoreVolume】, Sound Volume【[Audio] SoundVolume】, Voice Volume【[Audio] VoiceVolume】 |
| **Controls** | Scroll Rate【[Options] ScrollRate】, Right-Click Orders【[VERA] RightClickOrders】, Minimap Beacons【[VERA] MinimapBeacons】, Mouse Wheel Zoom【[VERA] MouseWheelZoom】, Lock Cursor to Window【[VERA] LockCursor】 |
| **General** | Language【[VERA] Language】, Original YR Style【[VERA] YROriginalStyle】, Smooth unit movement【[VERA] RenderInterpolation】, Smooth Online Menus【[VERA] SmoothOnlineMenus】, HD Map Previews【[VERA] HdMapPreviews】*(not in the web build)*, VERA Icon【[VERA] VeraIcon】, Skip Intro【[VERA] SkipIntro】*(next launch)*, Show String Keys【[VERA] ShowStringKeys】; button: Check for Updates *(locked in a match, not in the web build)* |

High refresh rate in battle is off by default. Turn it on in Display or set `HighRefreshRateInBattle=yes` under `[VERA]` in `VERA.INI`. With VSync on or off, the battlefield can draw at the monitor's refresh rate when drawing is fast enough. This can make unit movement smoother and use more GPU time.

On Windows, the Display page's Graphics API list, or `GraphicsAPI=` under `[VERA]` in `VERA.INI`, picks the renderer for the next start: `opengl` starts in OpenGL compatibility mode, and `vulkan` or `dx12` asks for that API alone. If the API you name cannot start, the game says so in a box and starts on another, skipping one that failed at an earlier start. The default, `auto`, lets the game choose. If a Vulkan or DirectX 12 startup hangs or loses the graphics device, the next automatic start tries the other API and records the failure in `GraphicsLostAtStart.txt` beside `VERA.INI`. If neither API finds a hardware GPU, the game tries OpenGL. The `WGPU_BACKEND` environment variable takes precedence. The Language list above writes `[VERA] Language=`, which you can also edit by hand.

## 2. In a match

```
In a match
├─ In-game menu (Esc or the sidebar Options tab; pauses the local game while open)
│   ├─ Game Controls page (always shown first)
│   │   Game Speed【[Options] GameSpeed】, Scroll Rate, the three volumes, Target Lines, See Hidden Objects, Tooltips
│   │   Buttons: Set Window Size (desktop only), Keyboard, Sound
│   ├─ Options column: Keyboard + the 6 Options pages above (no Mods) + Back
│   │   Online matches add "Internet Game Controls": Connection, Game Speed (this run only, sent on Resume)
│   ├─ Sound page: the three volumes, Shuffle / Repeat【[Audio] IsScoreShuffle / IsScoreRepeat】, track list, Play, Stop, Back
│   ├─ Keyboard page (same as the front end)
│   ├─ Load / Save / Delete Game (single player only): save list, save name, confirmation
│   ├─ Abort Mission → Leave / Restart (campaign) or Observe (network) / Resume Mission
│   │      In a WOL tournament this is Surrender instead: OK / Cancel
│   ├─ Pause All Players / Resume All Players (relay matches only)
│   └─ Resume Mission (shows "Return to Game" with shared pause)
├─ Diplomacy (sidebar Diplomacy tab or Tab): Allies and Chat checkboxes per player;
│     Taunts On【[MultiPlayer] LANTaunts / WOLTaunts】; Return
│   └─ In campaign and co-op the tab opens the Mission Briefing instead (Return only)
├─ Page dialog (U, WOL only)
├─ Reconnect / kick dialog (opens when a player stalls): propose removing a player, Quit Game
├─ Layout edit (Ctrl+Alt+E or Edit Layout on UI Options): drag interface parts, Save【veraui.ini】/ Reset; Esc cancels
├─ Command bar: Beacon, Planning Mode, hide/show
├─ Hotkeys that change a setting:
│   Ctrl+Alt+F Performance Stats, Alt+Enter Full Screen, Ctrl+Alt+L Lock Cursor, Ctrl+Alt+K Show String Keys,
│   Ctrl+Alt+H Convenience Help,
│   Ctrl+Alt+T superweapon building timers【[VERA] SuperweaponBuildingTimers; no Options control】,
│   Alt+A relation colours, Alt+T team colours; /pause and /resume in chat drive the shared pause
└─ Result screens
    ├─ Skirmish / multiplayer score, campaign score: Continue only
    ├─ Co-op campaign results: Continue / Quit, chat
    └─ Campaign defeat: replay? (Yes / No)
```

## 3. Legacy pages players cannot reach

These retail pages open only in the diagnostics capture mode (`native_shell_ui`), so they are left out of the lists above:

- WOL welcome page, legacy sign-in, My Information, Buddy List, locale page
- The legacy lobby's Clan, Tournament, News and Page buttons and channel list
- The retail LAN and online room pages
- The 8 retail WOL Options checkboxes other than Play Music

## 4. Known issues

Remove an item here once it is fixed.

**One setting, several entry points**

- The three volumes appear in 4 places: the Audio Options page, the in-match Game Controls page, the Sound page, and Sound Options under WOL Options.
- Scroll Rate, Target Lines, See Hidden Objects and Tooltips appear on both Game Controls and an Options page.
- In a match, the Keyboard page has 3 entry points.
- Full Screen, Performance Stats and Language are in both the front-end Esc menu and Options.
- Online matches have two Game Speed sliders that apply differently and have different ranges.

**One rule set, saved three times**

- Skirmish, LAN rooms and online rooms each keep their own Short Game, Crates, Credits and so on: in `[Skirmish]`, `[LAN]` and `[WonlinePref]`.
- The room's own additions (country bans, bridges, allying) and its Superweapon Limit, Terrain Deformation and Building Production Queue are never saved when changed in the room.
- Room name, ranked game, lobby watching and max players reset when the game restarts.

**Changes not saved, or shown state out of date**

- Changes on the in-match Sound page are not written to VERA.INI right away; they wait for a later full save.
- After Ctrl+Alt+L locks the cursor, the Options checkbox does not follow.
- The online lobby's Observe checkbox is saved only by WOL Options OK.
- Going from Options to Keyboard and back does not write earlier Options changes to the file.

**No effect, or developer features shown to players**

- Ares, Skip Intro, Original YR Style, HD Map Previews and Smooth Online Menus can be changed in a match but do nothing there.
- Difficulty is always greyed in a match.
- Commander Clips only matters in skirmish but shows in the LAN in-game menu too.
- Performance Stats, Show String Keys and Skip Intro are developer-oriented but sit in player menus.
- The main menu's 4 hidden typed codes (PENGO, THETEAM, HIRES, TOGGLE) play a sound but have no effect.

**Inconsistent behaviour**

- The front-end Esc menu's Exit Game quits without confirmation; the main menu's Exit Game asks first.
- The in-game menu's title "Game Options" has the same name as the "Game Options" page in the Options column, which is a different page.
- Esc opens the in-game menu but never closes it; on the Keyboard page Esc silently drops unsaved key assignments.

**Missing or ineffective features**

- Quick Match has no preferences; the My Information page that set them is unreachable.
- Buddies can only be added from the player card; there is no buddy list page.
- Check for Updates always fails on Android.
- Skipping the intro movies, ending the credits and opening the Esc menu all need the Esc key, which a touch-only player may not have.
