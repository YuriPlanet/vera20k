# Autosave

A VERA feature; the original game has no autosave. It is off by default.

Set it in the options file (`VERA.INI`):

```ini
[VERA]
AutosaveMinutes=5   ; 0 (default) = off, at most 60
```

In a campaign mission or a skirmish, every that many minutes of play the match is saved to
`autosave.sav` in the saves folder. Each autosave replaces the previous one; if a write fails, the
previous autosave stays. The Load page lists it like any other save, as "Autosave" and the map.

- The minutes count real time from the start of the match or the last autosave.
- A match that has not moved on since the last autosave (paused, or waiting in a menu) is not
  saved again.
- Network matches are never autosaved.
