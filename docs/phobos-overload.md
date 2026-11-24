# Overload tables for mind-control managers

Phobos lets a Mastermind's TechnoType replace the shared capture-overload
tables and effects (`Ext/TechnoType/Body.cpp:1133-1138`). The mechanism runs in
`CaptureManagerClass_Overload_AI` (`Ext/CaptureManager/Hooks.cpp:111-196`),
reached from `TechnoClass::AI_Update` at `0x006FA730`.

```ini
[MIND]                          ; TechnoType
Overload.Count=                 ; int list, default = [CombatDamage] OverloadCount
Overload.Damage=                ; int list, default = [CombatDamage] OverloadDamage
Overload.Frames=                ; int list, default = [CombatDamage] OverloadFrames
Overload.DeathSound=            ; sound, default [AudioVisual] MasterMindOverloadDeathSound
Overload.ParticleSys=           ; particle system, default [CombatDamage] DefaultSparkSystem
Overload.ParticleSysCount=      ; integer, default [CombatDamage] Overload.ParticleSysCount

[CombatDamage]                  ; rulesmd.ini
Overload.ParticleSysCount=5     ; integer, default 5
```

## Behavior

The captive count selects a row of `Overload.Count`. A present type list
replaces the Rules table as a whole, including a present empty list, which
returns before any check. The same applies to `Overload.Frames` (the delay
until the next check) and `Overload.Damage` (the row's damage). A damaging row
sets the ten-frame overload pip, hits the controller with `C4Warhead=`, plays
the overload voice once per episode and throws `Overload.ParticleSysCount`
spark systems, each offset by two Scenario draws (`(-200..200)` Y, then X). The
voice and particle type each fall back to their stock keys when the type
leaves them unset or names something the catalog does not know, and the spark
count falls back to the `[CombatDamage]` global.

Phobos rewrites three details of the original `0x00471A50` body besides the
type override:

- The row walk selects the last row whose bound the count exceeds (original:
  the first row whose bound the count does not exceed). Both agree on the
  stock tables but diverge for a single-entry table, where Phobos picks row
  one and the original stays on row zero.
- Out-of-range table reads clamp to the list's last entry (`FixIdx`); the
  original reads zero past the end.
- The sideways rocking magnitude is inverted: row one plays `0.03` and rows
  above one play `0.015` (original: `0.015` for row one). The spark coordinate
  draws only run when the particle system resolves; the original draws them
  even when `DefaultSparkSystem=` names nothing.

## Where VERA20K owns it

`rules::phobos_overload` reads the type keys and `PhobosGlobals` carries the
spark-count fallback. `Simulation::capture_manager_update` in
`sim::capture_manager` resolves the lists, voice, particle type and count per
visit, applies the Phobos row walk, clamp and rocking under the `phobos`
feature, and keeps the original walk, out-of-range reads, spark draws and
rocking on the stock build.

## Residuals

- The lists read through the shared `NullableVector<int>` port: absent, empty
  and `<default>` keep the Rules fallback; any other value replaces the list,
  including one whose tokens do not convert (a present empty list disables the
  checks).
- `Overload.DeathSound` resolves through the shared rules sound catalog at
  parse time; a sound registered only after the type binds keeps the stock
  voice.
- The legacy native corpus in `tools/spatial_oracle/capture_overload_update.py`
  pins the original row walk, so its single-row control is skipped under the
  `phobos` feature; the Phobos walk has its own production regression.
