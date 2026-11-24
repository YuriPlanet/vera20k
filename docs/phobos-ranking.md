# Ranking overrides from MISSIONMD

Phobos lets a campaign scenario override the map's `[Ranking]` values from its
own section in `MISSIONMD.INI` (`Ext/Scenario/Body.cpp:152-166`, in
`ScenarioExt::ExtData::LoadFromINIFile` while `ScenarioClass::LoadFromINI`
runs after `ScenarioClass::Read_INI` read `[Ranking]` at `0x0068A741..812`).

```ini
[C1A01MD.MAP]                    ; the loaded scenario's file name
Ranking.ParTimeEasy=00:10:00     ; clock time, default = the map's [Ranking]
Ranking.ParTimeMedium=00:10:00
Ranking.ParTimeHard=00:10:00
Ranking.UnderParTitle=           ; string, default = the map's [Ranking]
Ranking.UnderParMessage=
Ranking.OverParTitle=
Ranking.OverParMessage=
```

## Behavior

Only a campaign launch reads the file. Each key replaces the corresponding
ranking field read from the map's `[Ranking]` section; an absent key keeps the
map value, an invalid time reads as `0`, and a string is truncated to the
native 0x1F field. A skirmish or multiplayer launch ignores the section. A
loaded save keeps the ranking fields the save carries, because native applies
the override while the scenario is loaded, not when a save is restored.

## Where VERA20K owns it

`map::ranking::RankingSection` holds the projection the campaign victory page
consumes; `apply_missionmd_overrides` applies the per-scenario keys under the
`phobos` feature. `app::loading::transitions::install_world_presentation`
selects `MISSIONMD.INI` through the shared retail source finder for a fresh
campaign startup and passes the session's map name (Scenario+0x125C) as the
section; the save-restore route keeps its saved ranking.

## Residuals

- The override only runs on the fresh-start route; a save restored under a
  changed `MISSIONMD.INI` keeps the saved fields, matching native.
- The string fields use the ScenarioClass 0x1F capacity that the map `[Ranking]`
  parser already uses; the YRpp `ReadString(section, key, buffer, default)`
  overload's own copy limit is not reconstructible without the retail binary.
