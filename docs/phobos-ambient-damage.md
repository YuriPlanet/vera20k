# Ambient damage warhead and target skip

Phobos lets a weapon give its railgun and blast-wave ambient damage a different
warhead, and leave the primary target out of that ambient hit.

```ini
[WEAPON]                            ; WeaponType
AmbientDamage.Warhead=              ; Nullable<WarheadTypeClass*>, unset keeps Warhead=
AmbientDamage.IgnoreTarget=         ; Nullable<bool>, unset uses [General]

[General]
AmbientDamage.IgnoreTarget=no       ; Valueable<bool>
```

Readers: `Ext/WeaponType/Body.cpp:167-168`; the global is
`Ext/Rules/Body.cpp:268` with constructor `false` (`Ext/Rules/Body.h:710`).
Absent weapon keys inherit the weapon's own warhead and the global.

## Railgun

Hooks `0x70C862` and `0x70CA8B` (`Ext/Techno/Hooks.WeaponEffects.cpp:136-151`)
leave the primary target out of the ambient-damage list built by
`TechnoClass::ApplyRailgunLineDamage @ 0x0070C690`, while the line's other
objects keep taking it. Hook `0x70CBDA` (`:162-171`) selects the override
warhead for the per-object `ReceiveDamage` at `0x0070CBB0`.

VERA keeps both in `sim/combat/railgun.rs::line_damage`: the ignore key removes
the target's entity id from `struck` before the damage walk, and the ambient
warhead is `AmbientDamage.Warhead` else `Warhead=`.

## Blast wave

`WaveClass_DamageAI` hooks `0x75F39D` and `0x75F415`
(`Ext/Techno/Hooks.WeaponEffects.cpp:173-186`) select the ambient warhead and
skip the wave's Target object, Techno or Terrain, on each `DamageArea` walk.
VERA's `WaveDamageRequest` now carries the wave's `Target`, and
`commit_logic_wave_damage_request` reads the weapon's override and skips that
object in every recorded cell.

## Residuals

- The railgun target exclusion is applied after the line walk, which is the
  same set the native hooks leave; a terrain-object target remains the
  existing undamaged residual.
- An unresolvable override warhead name is kept as the interned name and the
  receiver drops the hit. Native reads the key with `Read<true>`
  (`Ext/WeaponType/Body.cpp:167`), i.e. `WarheadTypeClass::FindOrAllocate`, so
  an unknown name allocates a blank warhead type; its constructor defaults
  apply to that ambient hit, and a blank value stores a null warhead pointer.
  Trigger: invalid mod data. Frequency: authored mods. Risk: that ambient hit
  does nothing instead of using the blank warhead's defaults.
- Retail rules author none of these keys; the defaults reproduce the stock
  weapon warhead and no target skip.
