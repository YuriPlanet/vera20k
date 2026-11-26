# Optional Phobos timer blink colour

Build with `--features phobos` to choose the colour a superweapon timer blinks
in once its countdown has finished.

```ini
[AudioVisual]                  ; rulesmd.ini
TimerBlinkColorScheme=LightGrey ; a name from [Colors]
```

The value is a `[Colors]` entry name, matched case-insensitively as
`ColorScheme::FindIndex` does. The default is runtime scheme 5, the third entry
listed in `[Colors]`: runtime scheme `R` addresses `[Colors]` entry `R / 2`.

| Value | Effect |
| --- | --- |
| a name in `[Colors]` | the blinking time prints in that entry's colour |
| absent, empty, or a name no entry carries | the earlier layer's choice, else the third entry |
| `none` or `<none>` | scheme -1 (see Residuals) |

Each rules layer resolves the name when it is read, against the `[Colors]`
entries loaded so far, including that layer's own. A later layer naming no
entry keeps the earlier choice.

## Where VERA20K owns it

- `rules/native_processing.rs`: each rules pass reads the key where Phobos's
  `RulesExt::ExtData::LoadBeforeTypeData` does (hook `0x679A15`, after
  `RulesClass::Process` has read that pass's `[Colors]`), through the shared
  `ValueableIdx<ColorScheme>` reader
  (`IniSection::read_ares_color_scheme_index`, 2048-byte buffer) and the
  pass's `[Colors]` lookup.
- `rules/ruleset.rs`: `GeneralRules::timer_blink_color_scheme`, the runtime
  scheme index those passes leave.
- `app/presentation/super_timers.rs`: `blink_rgb` prints entry `R / 2` instead
  of the native `BLINK_SCHEME` (`PrintTimerOnTactical_BlinkColor`, `0x6D4CD9`).

## Residuals

- Only the superweapon timer lines step a blink in VERA20K, so the scenario
  timer and the blackout lines never take this colour. Phobos describes the
  key as covering "superweapon, mission etc" timers, but the unhardcoded hook
  replaces the colour where the game already blinks.
- `none` stores scheme -1, which Phobos then reads as `ColorScheme::Array[-1]`,
  memory before the array. VERA20K prints the native third entry instead, as
  it does for an entry past the 256th.
- A build without the capability keeps the native third entry unconditionally.
- The colour is the entry's base H,S,V through `HSV_To_RGB`, exactly as
  `Fancy_Text_Print_Wide` prints a house's text. The doubled runtime scheme
  pairs share one H,S,V, so either half of an entry prints the same colour.
