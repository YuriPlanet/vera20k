# StartFacing

A building's initial facing becomes configurable instead of due north.

```ini
[General]
BuildingStartFacing=0             ; Valueable<int>, 256-point circle
BuildingStartFacing.Random=false  ; Valueable<bool>

[SOMEBUILDING]                    ; BuildingType
StartFacing=                      ; Nullable<int>, defaults to [General]
StartFacing.Random=               ; Nullable<bool>, defaults to [General]
```

Readers `Ext/BuildingType/Body.cpp:267-268`, declarations
`Ext/BuildingType/Body.h:123-124`; globals `Ext/Rules/Body.cpp:626-627`,
constructor `Ext/Rules/Body.h:1024-1025`.

## Behavior

`GetBuildingStartFacing` (`Ext/Building/Hooks.cpp:1491-1504`): with random
mode on (type, else `[General]`) it draws
`ScenarioClass::Instance->Random.RandomRanged(0, 255)` once and caches it in
`BuildingExt::ConstructionStartFacing` (constructed `-1`,
`Ext/Building/Body.h:31,50`); otherwise it returns the type's `StartFacing`
(else `[General] BuildingStartFacing`). Only the low byte is used.

Three hooks read it:

- `TechnoClass_Unlimbo_BuildingStartFacing` (`0x6F6D9E`, 7 bytes,
  `Ext/Building/Hooks.cpp:1530-1543`): a building not created from a map file
  and not a LaserFence replaces the constructor's facing byte with the start
  facing before Unlimbo's facing set (`0x4C9300`).
- `BuildingClass_Mission_Construction_StartFacing` (`0x449AFE`, 6 bytes,
  `:1506-1513`): the construction turn's facing byte replaces the stock
  BuildingType+`0xED8` zero (skipped when type+`0x16BF` is set).
- `BuildingClass_Mission_Selling_StartFacing_Compare` / `_Set`
  (`0x449DAA` / `0x449DE9`, `:1514-1529`): the selling rotation compares and
  sets the same facing.

## Where VERA20K owns it

`rules::object_type::ObjectType::phobos_start_facing` /
`phobos_start_facing_random` (Building types only), the `[General]` fallbacks
in `rules::phobos_globals`, and the one-time random value in
`GameEntity::phobos_construction_start_facing` (serialized, hashed when set).
`sim::phobos_start_facing` holds `GetBuildingStartFacing` (the Scenario draw
and cache) and the Unlimbo application; `Simulation::unlimbo_constructed_parent`
calls it for runtime-created non-LaserFence buildings, and
`techno_ai::building_missions::mission_construction` uses the same value for
the construction turn.

RESIDUAL — the selling rotation (`0x449DAA`/`0x449DE9`): VERA's sell path
models the deconstruction stages but no facing turn, so a sold building
reaches the start facing only through Unlimbo. The native block is gated on
`BuildingType+0x16CA` `Artillary=` (`0x00460F56`); no retail building sets it
(`object_type.rs` records the same dormancy for stage 0), so retail sales are
unaffected. Trigger = selling a custom `Artillary=yes` building; impact =
presentation facing only; risk = visual, the sale completes normally.
