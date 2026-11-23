# Optional Phobos FlyNoWobbles

Build with `--features phobos` to decide whether technos on `Locomotor=Fly`
wobble, independently of `IsDropship`.

```ini
[AudioVisual]
FlyNoWobbles=          ; boolean, unset keeps the retail rule

[SOMETECHNO]           ; TechnoType, aircraft only
FlyNoWobbles=          ; boolean, defaults to [AudioVisual] FlyNoWobbles
```

In vanilla the wobble is gated on `TechnoType+C95` (`IsDropship`) at
`0x4CF8B1`, inside `FlyLocomotionClass::Draw_Point`, so a mod could only stop
the wobble by also taking every other `IsDropship` behaviour. The TechnoType key
wins, then the global key, then that retail rule: leaving both unset still means
"wobble unless the type is a dropship", and setting both is how a dropship gets
the wobble back.

Source: `FlyLocomotionClass_Draw_Point_NoWobbles` at `0x4CF8B1`
(`src/Ext/Techno/Hooks.cpp`), the `AircraftTypeExt` field in
`src/Ext/AircraftType/Body.cpp`, and the `RulesExt` read from `[AudioVisual]` in
`src/Ext/Rules/Body.cpp`. Phobos holds the key on aircraft types only, so a
repeated key on a ground type is ignored here too.

## Where VERA20K owns it

- `rules/phobos_fly_wobble.rs` — the resolution ladder the hook performs.
- `rules/object_type.rs` — the per-type value, read for `AircraftTypes`.
- `rules/ruleset.rs` — the `[AudioVisual]` value and `fly_no_wobbles_for`.
- `render/locomotor_visual.rs` — `fly_draw_point_y`, whose first gate is that
  branch.
- `app/presentation/instances/units.rs` — resolves the value where the aircraft
  body point adds the `Draw_Point` delta.

## Residuals

- The wobble arithmetic stays the retail `Draw_Point` one; only the gate on it
  is configurable.
