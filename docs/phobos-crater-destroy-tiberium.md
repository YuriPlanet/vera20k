# Crater tiberium destroy

`Crater.DestroyTiberium=` controls whether a `Crater=yes` animation still
removes ore from its cell at its middle frame (or at Start for a type whose
middle frame is zero). It is a Phobos AnimType ART key with a global fallback.

```ini
[General]
AnimCraterDestroyTiberium=yes       ; Valueable<bool>, default yes

[S_CLSN22]                          ; art.ini AnimType section
Crater=yes
Crater.DestroyTiberium=no           ; Nullable<bool>, default unset
```

Readers: `Ext/AnimType/Body.cpp:126` (field `Ext/AnimType/Body.h:81`) from the
AnimType ART section, and `Ext/Rules/Body.cpp:418` (constructor
`Ext/Rules/Body.h:844`, default true) for the global. VERA reads the pair into
`AnimTypeRuntimeConfig::crater_destroy_tiberium` (`src/rules/art_data.rs`) and
`PhobosGlobals::anim_crater_destroy_tiberium` (`src/rules/phobos_globals.rs`).

## Native body

`AnimClass_Middle_CraterDestroyTiberium` (`0x4250E1`,
`Ext/Anim/Hooks.cpp:707`) sits in `AnimClass::Middle`'s crater branch
(`0x00424F00`). The stock code at `0x4250E1` loads the anim's cell and calls
`0x00480A80` (`CellClass::Reduce_Tiberium`) with 6; the hook replaces those
six bytes and jumps to `0x4250EC` when the resolved value is false, so the ore
reduction — and any RNG it consumes — is skipped while the crater itself is
still placed. The flag does not affect scorch marks or the building-destruction
and survivor smudges, which never reduce tiberium.

## VERA

`Simulation::anim_middle` (`src/sim/anim_class.rs`) resolves the AnimType ART
value with the `[General]` fallback and stores it in
`AnimMiddleMarks::destroy_tiberium`. `anim_middle_marks`
(`src/sim/combat/smudge_dispatch.rs`) skips `reduce_tiberium(6)` when the flag
is false and then places the crater as before. Builds without the `phobos`
feature set the field to true, keeping the stock call.

## Tests

- `src/sim/anim_class.rs` — a crater animation over a TIB01 overlay: the ART
  key off, the global off and the key-on-over-global-off rows leave or reduce
  the ore through the real middle-frame visit.
- `src/sim/combat/smudge_dispatch.rs` — the dispatcher gate: false records no
  reduction and still places the crater.
- `src/rules/phobos_globals_tests.rs` — the global's default and section, and
  the ART nullable override.

## Residuals

- The key changes only the `AnimClass::Middle` crater branch; other mark
  producers keep their stock ore behavior.
- An unset global is not reachable: both the constructor and the reader keep
  the current value, constructed true.
